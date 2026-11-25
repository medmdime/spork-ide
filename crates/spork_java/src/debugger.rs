//! Java's debugger as IntelliJ's, over Eclipse's java-debug. Spork IDE reaches java-debug through
//! a proxy of the debug adapter protocol on this machine's loopback, which makes java-debug behave
//! as IntelliJ's debugger where the protocol leaves it free:
//! - Suspend: All, IntelliJ's default: when a thread stops, the others are paused too; Resume
//!   Program resumes them all, Pause Program pauses the whole VM, and a step runs the other threads
//!   while it steps ("Resume only the current thread" ☐). A Suspend: Thread breakpoint (the
//!   `thread` breakpoint mode the proxy offers) stops its thread alone, as java-debug does.
//! - A logging breakpoint keeps its condition (java-debug drops it) and says IntelliJ's "Breakpoint
//!   reached at …": the proxy evaluates the log message where the breakpoint stops, prints it and
//!   resumes; a condition that fails asks IntelliJ's question at each stop.
//! - Threads, frames and values read as IntelliJ writes them: `main`, `hello:9, Greeting
//!   (com.example)`, `{ArrayList@23}  size = 3`.
//! - java-debug's own events reach the app: a breakpoint condition's error, hot code replace.

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow};
use futures::channel::{mpsc, oneshot};
use futures::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, StreamExt as _};
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Context, Entity, Global,
    InteractiveElement as _, SharedString, Task, TaskExt as _, WeakEntity,
};
use lsp::LanguageServer;
use project::Project;
use project::debugger::session::{Session, SessionEvent, ThreadId};
use serde_json::{Value, json};
use smol::net::{TcpListener, TcpStream};
use spork_settings::debugger::{DebuggerSettings, Reload};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::notifications::{NotificationId, simple_message_notification::MessageNotification};

use crate::java_project::{JavaProject, execute_command};

/// What java-debug tells the app beyond the protocol.
#[derive(Debug)]
pub(crate) enum Notice {
    /// A breakpoint's condition failed to evaluate: the breakpoint, as IntelliJ names it, and
    /// the error. java-debug then stops there.
    ConditionError { breakpoint: String, error: String },
    /// `hotcodereplace`: its change type (`BUILD_COMPLETE`, `STARTING`, `END`, `ERROR`,
    /// `WARNING`) and message.
    HotCodeReplace { change: String, message: String },
    /// `usernotification`: its type (`ERROR`, `WARNING`, `INFORMATION`) and message.
    Message { kind: String, message: String },
    /// A logging breakpoint's text, for the program's console; the answer says whether it was
    /// written there, the stopped thread going on once it is.
    Log {
        text: String,
        written: oneshot::Sender<bool>,
    },
}

/// A request the app sends java-debug through the proxy, answered with its body.
pub(crate) struct AppRequest {
    command: String,
    arguments: Value,
    answer: oneshot::Sender<Result<Value>>,
}

/// The app's end of a proxy: requests to java-debug.
#[derive(Clone)]
pub(crate) struct ProxyHandle {
    requests: mpsc::UnboundedSender<AppRequest>,
    /// Reload Changed Classes builds the project, then reloads: the build's "classes changed"
    /// isn't a question for the user then.
    pub(crate) reloading: Arc<AtomicBool>,
    /// Stepping's "Resume only the current thread": a step leaves the other threads suspended.
    resume_only_current: Arc<AtomicBool>,
}

impl ProxyHandle {
    pub(crate) async fn request(&self, command: &str, arguments: Value) -> Result<Value> {
        let (answer, answered) = oneshot::channel();
        self.requests
            .unbounded_send(AppRequest {
                command: command.into(),
                arguments,
                answer,
            })
            .map_err(|_| anyhow!("The debug session has ended"))?;
        answered
            .await
            .map_err(|_| anyhow!("The debug session has ended"))?
    }
}

/// Opens a proxy to java-debug's `adapter_port`: the port Zed's debugger connects to, the app's
/// handle, and the notices java-debug sends.
pub(crate) async fn start(
    adapter_port: u16,
) -> Result<(
    u16,
    ProxyHandle,
    mpsc::UnboundedReceiver<Notice>,
    impl std::future::Future<Output = ()>,
)> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let (requests, app) = mpsc::unbounded();
    let (notices, noticed) = mpsc::unbounded();
    let resume_only_current = Arc::<AtomicBool>::default();
    let run = {
        let resume_only_current = resume_only_current.clone();
        async move {
            if let Err(error) =
                serve(listener, adapter_port, app, notices, resume_only_current).await
            {
                log::error!("Java debugger proxy: {error:#}");
            }
        }
    };
    let handle = ProxyHandle {
        requests,
        reloading: Arc::default(),
        resume_only_current,
    };
    Ok((port, handle, noticed, run))
}

enum Input {
    Client(Value),
    Adapter(Value),
    App(AppRequest),
    Closed,
}

async fn serve(
    listener: TcpListener,
    adapter_port: u16,
    app: mpsc::UnboundedReceiver<AppRequest>,
    notices: mpsc::UnboundedSender<Notice>,
    resume_only_current: Arc<AtomicBool>,
) -> Result<()> {
    let (client, _) = listener.accept().await?;
    drop(listener);
    let adapter = TcpStream::connect((Ipv4Addr::LOCALHOST, adapter_port))
        .await
        .context("connecting to java-debug")?;
    let messages = |stream: TcpStream, wrap: fn(Value) -> Input| {
        futures::stream::unfold(
            Some(smol::io::BufReader::new(stream)),
            move |reader| async move {
                let mut reader = reader?;
                match read_message(&mut reader).await {
                    Ok(Some(message)) => Some((wrap(message), Some(reader))),
                    Ok(None) => Some((Input::Closed, None)),
                    Err(error) => {
                        log::error!("Java debugger proxy: {error:#}");
                        Some((Input::Closed, None))
                    }
                }
            },
        )
        .boxed()
    };
    let mut inputs = futures::stream::select(
        futures::stream::select(
            messages(client.clone(), Input::Client),
            messages(adapter.clone(), Input::Adapter),
        ),
        app.map(Input::App),
    );
    let mut proxy = Proxy {
        client,
        adapter,
        notices,
        next_seq: FIRST_SEQ,
        pending: HashMap::new(),
        suspended_all: false,
        log_points: HashMap::new(),
        thread_points: HashSet::new(),
        condition_error: None,
        failed_conditions: HashMap::new(),
        packages: HashMap::new(),
        resume_only_current,
    };
    while let Some(input) = inputs.next().await {
        match input {
            Input::Client(message) => proxy.from_client(message).await?,
            Input::Adapter(message) => proxy.from_adapter(message).await?,
            Input::App(request) => {
                let seq = proxy
                    .send_adapter(&request.command, request.arguments)
                    .await?;
                proxy.pending.insert(seq, Pending::App(request.answer));
            }
            Input::Closed => break,
        }
    }
    Ok(())
}

/// The breakpoint mode of a Suspend: Thread breakpoint.
const SUSPEND_THREAD: &str = project::debugger::breakpoint_store::SUSPEND_THREAD_MODE;

/// The proxy's own requests are numbered apart from Zed's (java-debug reads a `seq` as an `int`).
const FIRST_SEQ: i64 = 1_000_000_000;

/// What the proxy does with java-debug's answer to a request of its own.
enum Pending {
    App(oneshot::Sender<Result<Value>>),
    /// The other threads paused: the stop goes to Zed, all threads stopped.
    PauseOthers(Value),
    /// The stopped thread's top frame, to see which breakpoint stopped it.
    TopFrame(Value),
    /// A logging breakpoint's message, evaluated: printed, then the thread resumes.
    LogMessage {
        thread: i64,
    },
    Ignore,
}

struct Proxy {
    client: TcpStream,
    adapter: TcpStream,
    notices: mpsc::UnboundedSender<Notice>,
    next_seq: i64,
    pending: HashMap<i64, Pending>,
    /// Every thread is suspended by a stop: the other threads' "pause" stops that `pauseOthers`
    /// sends, whenever they come, aren't Zed's.
    suspended_all: bool,
    /// The logging breakpoints, by their file and line: their log message.
    log_points: HashMap<(String, u64), String>,
    /// The Suspend: Thread breakpoints, by their file and line.
    thread_points: HashSet<(String, u64)>,
    /// A breakpoint's condition failed: the stop java-debug sends next is that breakpoint's.
    condition_error: Option<String>,
    /// The breakpoints whose condition failed, by their file and line, with the error: java-debug
    /// stops at them from then on without a word, where IntelliJ asks each time.
    failed_conditions: HashMap<(String, u64), String>,
    /// The package each source file declares.
    packages: HashMap<String, Option<String>>,
    resume_only_current: Arc<AtomicBool>,
}

impl Proxy {
    async fn send_adapter(&mut self, command: &str, arguments: Value) -> Result<i64> {
        let seq = self.next_seq;
        self.next_seq += 1;
        let request =
            json!({"seq": seq, "type": "request", "command": command, "arguments": arguments});
        write_message(&mut self.adapter, &request).await?;
        Ok(seq)
    }

    async fn send_client(&mut self, message: &Value) -> Result<()> {
        write_message(&mut self.client, message).await
    }

    async fn output(&mut self, text: String) -> Result<()> {
        let event = json!({"seq": 0, "type": "event", "event": "output",
            "body": {"category": "console", "output": text}});
        self.send_client(&event).await
    }

    async fn from_client(&mut self, mut message: Value) -> Result<()> {
        let command = message["command"].as_str().unwrap_or_default().to_owned();
        if matches!(
            command.as_str(),
            "continue" | "next" | "stepIn" | "stepOut" | "disconnect"
        ) {
            self.suspended_all = false;
        }
        match command.as_str() {
            // Resume Program resumes every thread
            "continue" => message["arguments"]["threadId"] = 0.into(),
            // Pause Program suspends the VM
            "pause" => message["command"] = "pauseAll".into(),
            // the other threads run while one steps
            "next" | "stepIn" | "stepOut" if !self.resume_only_current.load(Ordering::Relaxed) => {
                let thread = message["arguments"]["threadId"].clone();
                let seq = self
                    .send_adapter("continueOthers", json!({"threadId": thread}))
                    .await?;
                self.pending.insert(seq, Pending::Ignore);
            }
            "setBreakpoints" => self.keep_log_points(&mut message),
            _ => {}
        }
        write_message(&mut self.adapter, &message).await
    }

    /// A breakpoint that logs is the proxy's: java-debug would drop its condition, and its
    /// "Breakpoint hit" message is IntelliJ's for the frame it stops in. java-debug gets a plain
    /// breakpoint, which the proxy logs and resumes when it stops.
    fn keep_log_points(&mut self, message: &mut Value) {
        let path = message["arguments"]["source"]["path"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.log_points.retain(|(file, _), _| *file != path);
        self.thread_points.retain(|(file, _)| *file != path);
        self.failed_conditions.retain(|(file, _), _| *file != path);
        let Some(breakpoints) = message["arguments"]["breakpoints"].as_array_mut() else {
            return;
        };
        for breakpoint in breakpoints {
            let Some(object) = breakpoint.as_object_mut() else {
                continue;
            };
            if let (Some(Value::String(mode)), Some(line)) = (
                object.remove("mode"),
                object.get("line").and_then(Value::as_u64),
            ) && mode == SUSPEND_THREAD
            {
                self.thread_points.insert((path.clone(), line));
            }
            let (Some(Value::String(log)), Some(line)) = (
                object.remove("logMessage"),
                object.get("line").and_then(Value::as_u64),
            ) else {
                continue;
            };
            self.log_points.insert((path.clone(), line), log);
        }
    }

    async fn from_adapter(&mut self, mut message: Value) -> Result<()> {
        match message["type"].as_str() {
            Some("response") => {
                let seq = message["request_seq"].as_i64().unwrap_or_default();
                if let Some(pending) = self.pending.remove(&seq) {
                    return self.answered(pending, message).await;
                }
                self.present(&mut message).await;
                self.send_client(&message).await
            }
            Some("event") => self.event(message).await,
            _ => self.send_client(&message).await,
        }
    }

    async fn event(&mut self, message: Value) -> Result<()> {
        let body = &message["body"];
        let text = |key: &str| body[key].as_str().unwrap_or_default().to_owned();
        match message["event"].as_str().unwrap_or_default() {
            "stopped" => self.stopped(message).await,
            "usernotification" => {
                let (kind, text) = (text("notificationType"), text("message"));
                match text
                    .strip_prefix("Breakpoint condition '")
                    .and_then(|rest| rest.split_once("' error: "))
                {
                    Some((_, error)) if kind == "ERROR" => {
                        self.condition_error = Some(error.to_owned())
                    }
                    _ => {
                        self.notices
                            .unbounded_send(Notice::Message {
                                kind,
                                message: text,
                            })
                            .ok();
                    }
                }
                Ok(())
            }
            "hotcodereplace" => {
                self.notices
                    .unbounded_send(Notice::HotCodeReplace {
                        change: text("changeType"),
                        message: text("message"),
                    })
                    .ok();
                Ok(())
            }
            "telemetry" => Ok(()),
            _ => self.send_client(&message).await,
        }
    }

    async fn stopped(&mut self, message: Value) -> Result<()> {
        let body = &message["body"];
        let reason = body["reason"].as_str().unwrap_or_default();
        if reason == "pause" {
            // the threads `pauseOthers` paused stop with the thread that stopped first
            if self.suspended_all && body["allThreadsStopped"] != true {
                return Ok(());
            }
            return self.send_client(&message).await;
        }
        let thread = body["threadId"].clone();
        if self.condition_error.is_some()
            || !self.log_points.is_empty()
            || !self.thread_points.is_empty()
            || !self.failed_conditions.is_empty()
        {
            let seq = self
                .send_adapter(
                    "stackTrace",
                    json!({"threadId": thread, "startFrame": 0, "levels": 1}),
                )
                .await?;
            self.pending.insert(seq, Pending::TopFrame(message));
            return Ok(());
        }
        self.stop_all(message).await
    }

    /// Suspend: All. The thread that stopped holds the others until they are paused too.
    async fn stop_all(&mut self, stopped: Value) -> Result<()> {
        let thread = stopped["body"]["threadId"].clone();
        let seq = self
            .send_adapter("pauseOthers", json!({"threadId": thread}))
            .await?;
        self.suspended_all = true;
        self.pending.insert(seq, Pending::PauseOthers(stopped));
        Ok(())
    }

    async fn answered(&mut self, pending: Pending, response: Value) -> Result<()> {
        let succeeded = response["success"] == true;
        match pending {
            Pending::App(answer) => {
                let result = match succeeded {
                    true => Ok(response["body"].clone()),
                    false => Err(anyhow!(
                        "{}",
                        response["message"].as_str().unwrap_or("The request failed")
                    )),
                };
                answer.send(result).ok();
                Ok(())
            }
            Pending::PauseOthers(mut stopped) => {
                stopped["body"]["allThreadsStopped"] = true.into();
                self.send_client(&stopped).await
            }
            Pending::TopFrame(stopped) => {
                let frame = response["body"]["stackFrames"][0].clone();
                let path = frame["source"]["path"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let line = frame["line"].as_u64().unwrap_or_default();
                let failed = match self.condition_error.take() {
                    Some(error) => {
                        self.failed_conditions
                            .insert((path.clone(), line), error.clone());
                        Some(error)
                    }
                    None if stopped["body"]["reason"] == "breakpoint" => {
                        self.failed_conditions.get(&(path.clone(), line)).cloned()
                    }
                    None => None,
                };
                if let Some(error) = failed {
                    let breakpoint = self.breakpoint_name(&frame).await;
                    self.notices
                        .unbounded_send(Notice::ConditionError { breakpoint, error })
                        .ok();
                    return self.stop_all(stopped).await;
                }
                let thread = stopped["body"]["threadId"].as_i64().unwrap_or_default();
                let log = self.log_points.get(&(path.clone(), line)).cloned();
                match log {
                    Some(log) if stopped["body"]["reason"] == "breakpoint" => {
                        // `status.line.breakpoint.reached`: the frame's method, file and line
                        let package = self.package_of(&path).await;
                        let method = qualified_frame_name(
                            frame["name"].as_str().unwrap_or_default(),
                            package.as_deref(),
                        );
                        let method = method.split('(').next().unwrap_or_default();
                        let file = frame["source"]["name"].as_str().unwrap_or_default();
                        let hit = format!("Breakpoint reached at {method}({file}:{line})");
                        let log = log
                            .lines()
                            .map(|text| match text.starts_with("Breakpoint reached at ") {
                                true => hit.as_str(),
                                false => text,
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                        let seq = self
                            .send_adapter(
                                "evaluate",
                                json!({"expression": java_string(&log), "frameId": frame["id"],
                                    "context": "repl"}),
                            )
                            .await?;
                        self.pending.insert(seq, Pending::LogMessage { thread });
                        Ok(())
                    }
                    // Suspend: Thread, java-debug's own suspend policy
                    _ if stopped["body"]["reason"] == "breakpoint"
                        && self.thread_points.contains(&(path, line)) =>
                    {
                        self.send_client(&stopped).await
                    }
                    _ => self.stop_all(stopped).await,
                }
            }
            Pending::LogMessage { thread } => {
                let text = match succeeded {
                    true => unquote(response["body"]["result"].as_str().unwrap_or_default()),
                    false => response["message"].as_str().unwrap_or_default().to_owned(),
                };
                // in the program's console before the program prints anything more; the
                // debugger's console otherwise (an attached JVM's)
                let (written, answer) = oneshot::channel();
                self.notices
                    .unbounded_send(Notice::Log {
                        text: format!("{text}\n"),
                        written,
                    })
                    .ok();
                if !answer.await.unwrap_or(false) {
                    self.output(format!("{text}\n")).await?;
                }
                let seq = self
                    .send_adapter("continue", json!({"threadId": thread}))
                    .await?;
                self.pending.insert(seq, Pending::Ignore);
                Ok(())
            }
            Pending::Ignore => Ok(()),
        }
    }

    /// A response as IntelliJ shows its contents: thread names, frames, values.
    async fn present(&mut self, response: &mut Value) {
        let command = response["command"].as_str().unwrap_or_default().to_owned();
        let body = &mut response["body"];
        match command.as_str() {
            // the breakpoint modes the proxy makes of java-debug's own: Suspend: Thread
            "initialize" => {
                body["breakpointModes"] = json!([{"mode": SUSPEND_THREAD, "label": "Thread",
                    "appliesTo": ["source"]}]);
            }
            "threads" => {
                for thread in body["threads"].as_array_mut().into_iter().flatten() {
                    if let Some(name) = thread["name"].as_str() {
                        thread["name"] = thread_name(name).into();
                    }
                }
            }
            "stackTrace" => {
                let mut frames = std::mem::take(&mut body["stackFrames"]);
                for frame in frames.as_array_mut().into_iter().flatten() {
                    let path = frame["source"]["path"].as_str().map(str::to_owned);
                    let package = match path {
                        Some(path) => self.package_of(&path).await,
                        None => None,
                    };
                    if let Some(name) = frame["name"].as_str() {
                        frame["name"] = qualified_frame_name(name, package.as_deref()).into();
                    }
                    // java-debug writes -1 where a frame has no line
                    if frame["line"].as_i64().is_some_and(|line| line < 0) {
                        frame["line"] = 0.into();
                    }
                }
                body["stackFrames"] = frames;
            }
            "variables" => {
                for variable in body["variables"].as_array_mut().into_iter().flatten() {
                    if let Some(value) = variable["value"].as_str() {
                        variable["value"] = value_text(value).into();
                    }
                }
            }
            "evaluate" => {
                if let Some(result) = body["result"].as_str() {
                    body["result"] = value_text(result).into();
                }
            }
            "setVariable" => {
                if let Some(value) = body["value"].as_str() {
                    body["value"] = value_text(value).into();
                }
            }
            _ => {}
        }
    }

    /// The package of a frame's source: a `jdt://` class's, or the one its file declares.
    async fn package_of(&mut self, path: &str) -> Option<String> {
        if let Some(rest) = path.strip_prefix("jdt://contents/") {
            let rest = rest.split('?').next()?;
            let mut segments = rest.rsplit('/');
            segments.next()?;
            return segments.next().map(str::to_owned);
        }
        if let Some(package) = self.packages.get(path) {
            return package.clone();
        }
        let package = smol::fs::read_to_string(Path::new(path))
            .await
            .ok()
            .and_then(|text| crate::application::package_of(&text));
        self.packages.insert(path.to_owned(), package.clone());
        package
    }

    /// IntelliJ's name of the line breakpoint at a frame: `Line 44 in Debuggee.measure()
    /// (com.example)`.
    async fn breakpoint_name(&mut self, frame: &Value) -> String {
        let line = frame["line"].as_u64().unwrap_or_default();
        let package = match frame["source"]["path"].as_str() {
            Some(path) => self.package_of(path).await,
            None => None,
        };
        let name = frame["name"].as_str().unwrap_or_default();
        let method = name.split('(').next().unwrap_or_default();
        match package {
            Some(package) => format!("Line {line} in {method}() ({package})"),
            None => format!("Line {line} in {method}()"),
        }
    }
}

/// The proxies of the running Java debug sessions, by the port Zed's debugger connects to.
#[derive(Default)]
struct Proxies(HashMap<u16, ProxyHandle>);

impl Global for Proxies {}

/// The proxy a Java debug session talks through.
pub(crate) fn proxy_of(session: &Session, cx: &App) -> Option<ProxyHandle> {
    let port = session.binary()?.connection.as_ref()?.port;
    cx.try_global::<Proxies>()?.0.get(&port).cloned()
}

/// Opens the proxy for a session of the project's `workspace`, the app answering its notices.
pub(crate) async fn connect(
    adapter_port: u16,
    workspace: WeakEntity<Workspace>,
    session: SharedString,
    cx: &mut AsyncApp,
) -> Result<u16> {
    let (port, handle, mut notices, run) = start(adapter_port).await?;
    cx.update(|cx| {
        if let Some(workspace) = workspace.upgrade() {
            let project = workspace.read(cx).project().clone();
            crate::hotswap::mark_unchanged(&project, false, cx);
        }
        handle.resume_only_current.store(
            spork_settings::debugger::settings(cx).resume_only_current_thread,
            Ordering::Relaxed,
        );
        cx.default_global::<Proxies>()
            .0
            .insert(port, handle.clone());
        cx.background_spawn(run).detach();
        cx.spawn(async move |cx| {
            while let Some(notice) = notices.next().await {
                cx.update(|cx| notify(&workspace, &handle, port, &session, notice, cx));
            }
            cx.update(|cx| cx.default_global::<Proxies>().0.remove(&port));
        })
        .detach();
    });
    Ok(port)
}

/// java-debug's settings (`vscode.java.updateDebugSettings`) from IntelliJ's Data Views › Java and
/// Stepping options: values with their `toString()` and collections' views, the stepping filters
/// ("Do not step into the classes") and "Skip synthetic methods" unless `stepping_filters` is off
/// (Force Step Into). java-debug's settings are JDT LS's, its sessions share them.
fn settings(options: &DebuggerSettings, stepping_filters: bool) -> String {
    let filters = stepping_filters && options.step_filters_enabled;
    let skip: Vec<&str> = options
        .step_filters
        .iter()
        .filter(|(_, on)| filters && *on)
        .map(|(filter, _)| filter.as_str())
        .collect();
    json!({
        "showStaticVariables": options.show_static_fields,
        "showQualifiedNames": options.show_qualified_names,
        "showHex": options.show_hex,
        "showLogicalStructure": options.collection_views,
        "showToString": options.to_string_view,
        "hotCodeReplace": "manual",
        // java-debug parses it on each update
        "logLevel": "WARNING",
        "stepFilters": {
            "skipClasses": skip,
            "skipSynthetics": stepping_filters && options.skip_synthetic,
            "skipStaticInitializers": false,
            "skipConstructors": stepping_filters && options.skip_constructors,
        },
    })
    .to_string()
}

pub(crate) async fn update_settings(
    server: &LanguageServer,
    stepping_filters: bool,
    cx: &mut AsyncApp,
) -> Result<()> {
    let options = cx.update(|cx| spork_settings::debugger::settings(cx));
    execute_command(
        server,
        "vscode.java.updateDebugSettings",
        vec![settings(&options, stepping_filters).into()],
    )
    .await
    .map(drop)
}

/// Force Step Into (⌥⇧F7): a step into with java-debug's stepping filters off until it stops.
pub(crate) fn force_step_into(session: &Entity<Session>, thread: ThreadId, cx: &mut App) {
    let server = session
        .read(cx)
        .worktree()
        .and_then(|worktree| JavaProject::containing(&worktree.read(cx).abs_path(), cx))
        .and_then(|java| java.read(cx).language_server(cx));
    let Some(server) = server else {
        session.update(cx, |session, cx| {
            session.step_in(thread, dap::SteppingGranularity::Statement, cx)
        });
        return;
    };
    let session = session.clone();
    cx.spawn(async move |cx| {
        update_settings(&server, false, cx).await?;
        let (stopped, stop) = oneshot::channel();
        let mut stopped = Some(stopped);
        let _subscription = cx.update(|cx| {
            cx.subscribe(&session, move |_, event: &SessionEvent, _| {
                if let SessionEvent::Stopped(_) = event
                    && let Some(stopped) = stopped.take()
                {
                    stopped.send(()).ok();
                }
            })
        });
        session.update(cx, |session, cx| {
            session.step_in(thread, dap::SteppingGranularity::Statement, cx)
        });
        stop.await.ok();
        update_settings(&server, true, cx).await
    })
    .detach_and_log_err(cx);
}

/// What java-debug's notices show: IntelliJ's "Breakpoint Condition Error" question, its other
/// messages as notifications.
fn notify(
    workspace: &WeakEntity<Workspace>,
    proxy: &ProxyHandle,
    port: u16,
    session: &SharedString,
    notice: Notice,
    cx: &mut App,
) {
    let Some(workspace) = workspace.upgrade() else {
        if let Notice::Log { written, .. } = notice {
            written.send(false).ok();
        }
        return;
    };
    match notice {
        Notice::Log { text, written } => {
            let dap_store = workspace.read(cx).project().read(cx).dap_store();
            let debugged = dap_store.read(cx).sessions().find(|debugged| {
                let binary = debugged.read(cx).binary();
                binary
                    .and_then(|binary| binary.connection.as_ref())
                    .map(|c| c.port)
                    == Some(port)
            });
            let shown = debugged
                .cloned()
                .is_some_and(|debugged| spork_run::write_program_console(&debugged, &text, cx));
            written.send(shown).ok();
        }
        Notice::ConditionError { breakpoint, error } => {
            let Some(window) = window_of(&workspace, cx) else {
                return;
            };
            // `error.evaluating.breakpoint.condition.or.action`, a yes/no question: no resumes
            let message = format!(
                "Problem processing VM event:\nBreakpoint: '{breakpoint}'\nError: {error}\n\
                 Would you like to stop at the breakpoint?"
            );
            let weak = workspace.downgrade();
            window
                .update(cx, |_, window, cx| {
                    let window_handle = window.window_handle();
                    spork_run::Confirm::ask(
                        &weak,
                        [
                            "Breakpoint Condition Error".into(),
                            message.into(),
                            "Yes".into(),
                            "No".into(),
                        ],
                        move |stop, cx| {
                            // once the question is gone and the focus back where it was
                            if !stop {
                                cx.defer(move |cx| {
                                    window_handle
                                        .update(cx, |_, window, cx| {
                                            window.dispatch_action(
                                                Box::new(debugger_ui::Continue),
                                                cx,
                                            )
                                        })
                                        .log_err();
                                });
                            }
                        },
                        window,
                        cx,
                    );
                })
                .log_err();
        }
        Notice::Message { kind, message } => {
            workspace.update(cx, |workspace, cx| {
                let id = NotificationId::composite::<Notice>(SharedString::from(message.clone()));
                workspace.show_notification(id, cx, |cx| {
                    cx.new(|cx| {
                        let notification = MessageNotification::new(message, cx);
                        match kind.as_str() {
                            "ERROR" => notification.with_title("Debugger"),
                            _ => notification,
                        }
                    })
                });
            });
        }
        // HotSwap's "Reload classes after compilation": JDT LS built changed classes
        Notice::HotCodeReplace { change, .. }
            if change == "BUILD_COMPLETE" && !proxy.reloading.load(Ordering::Relaxed) =>
        {
            match spork_settings::debugger::settings(cx).reload_after_compilation {
                Reload::Never => return,
                Reload::Always => {
                    proxy.reloading.store(true, Ordering::Relaxed);
                    reload_classes(workspace.downgrade(), proxy.clone(), cx).detach();
                    return;
                }
                Reload::Ask => {}
            }
            let Some(window) = window_of(&workspace, cx) else {
                return;
            };
            let (weak, proxy) = (workspace.downgrade(), proxy.clone());
            let title = format!("Reload Changed Classes for {session}");
            window
                .update(cx, |_, window, cx| {
                    spork_run::Confirm::ask(
                        &weak.clone(),
                        [
                            title.into(),
                            "Some classes have been changed. Reload changed classes now?".into(),
                            "Reload".into(),
                            "Cancel".into(),
                        ],
                        move |reload, cx| {
                            if reload {
                                proxy.reloading.store(true, Ordering::Relaxed);
                                reload_classes(weak, proxy, cx).detach();
                            }
                        },
                        window,
                        cx,
                    );
                })
                .log_err();
        }
        Notice::HotCodeReplace { change, message } => {
            log::info!("hot code replace {change}: {message}");
        }
    }
}

/// JDT LS's `java/classFileContents`: a `jdt://` class's source, attached or decompiled.
enum ClassFileContents {}

impl lsp::request::Request for ClassFileContents {
    type Params = Value;
    type Result = Option<String>;
    const METHOD: &'static str = "java/classFileContents";
}

/// A library's or the JDK's frame (java-debug's `jdt://` class): its source as JDT LS gives it,
/// written to the data folder and opened read-only, as IntelliJ shows a library's source.
pub(crate) fn class_source(
    uri: &str,
    project: &Entity<Project>,
    cx: &mut App,
) -> Option<Task<Result<Arc<Path>>>> {
    let class = uri.strip_prefix("jdt://contents/")?.split('?').next()?;
    let mut segments = class.rsplit('/');
    let file = segments.next()?.strip_suffix(".class")?;
    // a nested class's source is its top class's file
    let file = file.split('$').next().unwrap_or(file);
    let package = segments.next().unwrap_or_default();
    let path = paths::data_dir()
        .join("java")
        .join("sources")
        .join(package.replace('.', "/"))
        .join(format!("{file}.java"));
    let server = crate::java_project::language_server(project, cx)?;
    let (uri, project) = (uri.to_owned(), project.clone());
    Some(cx.spawn(async move |cx| {
        let text = server
            .request::<ClassFileContents>(json!({"uri": uri}), Duration::from_secs(60))
            .await
            .into_response()?
            .context("JDT LS has no source for the class")?;
        let written = path.clone();
        cx.background_spawn(async move {
            if let Some(folder) = written.parent() {
                std::fs::create_dir_all(folder)?;
            }
            if written.exists() {
                let mut permissions = std::fs::metadata(&written)?.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                std::fs::set_permissions(&written, permissions)?;
            }
            std::fs::write(&written, text)?;
            let mut permissions = std::fs::metadata(&written)?.permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(&written, permissions)?;
            anyhow::Ok(())
        })
        .await?;
        let buffer = project
            .update(cx, |project, cx| project.open_local_buffer(&path, cx))
            .await?;
        buffer.update(cx, |buffer, cx| {
            buffer.set_capability(language::Capability::Read, cx)
        });
        Ok(Arc::from(path.as_path()))
    }))
}

/// Reload Changed Classes, for a Java debug session; the debugger's options applied to the
/// running sessions and JDT LS as they change.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action_renderer(|div, workspace, _, cx| {
            let Some(session) = spork_run::active_debug_session(workspace, cx) else {
                return div;
            };
            let Some(proxy) = proxy_of(session.read(cx), cx) else {
                return div;
            };
            div.on_action(cx.listener(
                move |workspace, _: &crate::java::ReloadChangedClasses, _, cx| {
                    reload_changed_classes(workspace, proxy.clone(), cx)
                },
            ))
        });
    })
    .detach();
    cx.observe_global::<spork_settings::debugger::DebuggerSettingsGlobal>(|cx| {
        let options = spork_settings::debugger::settings(cx);
        if let Some(proxies) = cx.try_global::<Proxies>() {
            for proxy in proxies.0.values() {
                proxy
                    .resume_only_current
                    .store(options.resume_only_current_thread, Ordering::Relaxed);
            }
        }
        let servers: Vec<_> = cx
            .try_global::<crate::java_project::Projects>()
            .map(|projects| {
                projects
                    .0
                    .values()
                    .filter_map(|java| java.read(cx).language_server(cx))
                    .collect()
            })
            .unwrap_or_default();
        for server in servers {
            cx.spawn(async move |cx| update_settings(&server, true, cx).await)
                .detach_and_log_err(cx);
        }
    })
    .detach();
}

/// Reload Changed Classes: the project built ("Build project before reloading classes"), then its
/// classes reloaded.
pub(crate) fn reload_changed_classes(
    workspace: &mut Workspace,
    proxy: ProxyHandle,
    cx: &mut Context<Workspace>,
) {
    let build = spork_settings::debugger::settings(cx)
        .build_before_reload
        .then(|| JavaProject::of(workspace, cx).map(|java| java.read(cx).build(cx)))
        .flatten();
    let weak = workspace.weak_handle();
    proxy.reloading.store(true, Ordering::Relaxed);
    cx.spawn(async move |_, cx| {
        let built = match build {
            Some(build) => build.await,
            None => Ok(()),
        };
        cx.update(|cx| reload_classes(weak, proxy, cx)).await;
        built
    })
    .detach_and_log_err(cx);
}

/// Reload Changed Classes (HotSwap): java-debug redefines the classes JDT LS built since the
/// last reload, then IntelliJ's notification says how many, or why HotSwap failed; whether it
/// reloaded without failing.
pub(crate) fn reload_classes(
    workspace: WeakEntity<Workspace>,
    proxy: ProxyHandle,
    cx: &mut App,
) -> Task<bool> {
    cx.spawn(async move |cx| {
        let answer = proxy.request("redefineClasses", json!({})).await;
        proxy.reloading.store(false, Ordering::Relaxed);
        let changed = answer
            .as_ref()
            .ok()
            .and_then(|body| body["changedClasses"].as_array())
            .map_or(0, Vec::len);
        let error = match &answer {
            Ok(body) => body["errorMessage"].as_str().map(str::to_owned),
            Err(error) => Some(format!("{error:#}")),
        };
        let failed = error.is_some();
        // `status.classes.reloaded`, `status.hotswap.uptodate`, `status.hot.swap.completed.with.errors`
        let (title, text) = match (error, changed) {
            (Some(error), _) => (Some("HotSwap failed"), error),
            (None, 0) => (
                None,
                "Loaded classes are up to date. Nothing to reload.".to_owned(),
            ),
            (None, 1) => (None, "1 class reloaded".to_owned()),
            (None, changed) => (None, format!("{changed} classes reloaded")),
        };
        workspace
            .update(cx, |workspace, cx| {
                // the floating toolbar's "Code has been reloaded"
                if !failed {
                    crate::hotswap::mark_unchanged(workspace.project(), true, cx);
                }
                let id = NotificationId::unique::<HotSwap>();
                workspace.dismiss_notification(&id, cx);
                workspace.show_notification(id, cx, |cx| {
                    cx.new(|cx| {
                        let notification = MessageNotification::new(text, cx);
                        match title {
                            Some(title) => notification.with_title(title),
                            None => notification,
                        }
                    })
                });
            })
            .log_err();
        !failed
    })
}

struct HotSwap;

/// The window a workspace is in.
fn window_of(workspace: &Entity<Workspace>, cx: &mut App) -> Option<AnyWindowHandle> {
    cx.windows().into_iter().find(|window| {
        window
            .update(cx, |_, window, cx| {
                Workspace::for_window(window, cx).as_ref() == Some(workspace)
            })
            .unwrap_or(false)
    })
}

async fn read_message(reader: &mut (impl futures::AsyncBufRead + Unpin)) -> Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = Some(value.trim().parse::<usize>()?);
        }
    }
    let mut body = vec![0; length.unwrap_or_default()];
    reader.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

async fn write_message(writer: &mut TcpStream, message: &Value) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(&body).await?;
    writer.flush().await?;
    Ok(())
}

/// IntelliJ names a thread by its name alone; java-debug writes `Thread [main]`.
fn thread_name(name: &str) -> String {
    name.strip_prefix("Thread [")
        .and_then(|name| name.strip_suffix(']'))
        .unwrap_or(name)
        .to_owned()
}

/// java-debug's frame name with its class's package: `com.example.Greeting.hello(String)`;
/// a native method's mark dropped.
fn qualified_frame_name(name: &str, package: Option<&str>) -> String {
    let name = name.trim_end_matches("[native method]");
    match package {
        Some(package) if !package.is_empty() => format!("{package}.{name}"),
        _ => name.to_owned(),
    }
}

/// IntelliJ's frame label from a qualified frame name: `hello:9, Greeting (com.example)`; a
/// frame without a line shows -1, as IntelliJ's.
pub(crate) fn frame_label(name: &str, line: u64) -> String {
    let signature = name.split('(').next().unwrap_or(name);
    let (class, method) = signature.rsplit_once('.').unwrap_or(("", signature));
    let line = match line {
        0 => "-1".to_owned(),
        line => line.to_string(),
    };
    match class.rsplit_once('.') {
        Some((package, class)) => format!("{method}:{line}, {class} ({package})"),
        None if class.is_empty() => format!("{method}:{line}"),
        None => format!("{method}:{line}, {class}"),
    }
}

/// A value as IntelliJ shows it: an object `{Type@id}` and what java-debug says of it after,
/// a collection's size as `  size = 3`.
fn value_text(value: &str) -> String {
    let Some((reference, rest)) = object_reference(value) else {
        return value.to_owned();
    };
    let rest = match rest.trim_start().strip_prefix("size=") {
        Some(size) => format!("  size = {size}"),
        None => rest.to_owned(),
    };
    format!("{{{reference}}}{rest}")
}

/// `ArrayList@23 size=3` → (`ArrayList@23`, ` size=3`), the type's package left out.
fn object_reference(value: &str) -> Option<(String, &str)> {
    let at = value.find('@')?;
    let ty = &value[..at];
    let is_type = !ty.is_empty()
        && ty
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '[' | ']'));
    let digits = value[at + 1..]
        .find(|c: char| !c.is_ascii_digit())
        .map_or(value.len(), |end| at + 1 + end);
    if !is_type || digits == at + 1 {
        return None;
    }
    let short = match ty.split('[').next().and_then(|base| base.rsplit_once('.')) {
        Some((package, _)) => &ty[package.len() + 1..],
        None => ty,
    };
    Some((
        format!("{short}@{}", &value[at + 1..digits]),
        &value[digits..],
    ))
}

/// A log message, which IntelliJ's breakpoints write as text with `{expression}`s, as the Java
/// expression that makes its text.
fn java_string(message: &str) -> String {
    let mut parts = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        parts.push(java_literal(&rest[..open]));
        let expression = rest[open + 1..open + close].trim();
        if !expression.is_empty() {
            parts.push(format!("({expression})"));
        }
        rest = &rest[open + close + 1..];
    }
    parts.push(java_literal(rest));
    format!("\"\" + {}", parts.join(" + "))
}

fn java_literal(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// A string's value without java-debug's quotes.
fn unquote(result: &str) -> String {
    result
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(result)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{frame_label, java_string, qualified_frame_name, thread_name, value_text};

    #[test]
    fn writes_threads_frames_and_values_as_intellij() {
        assert_eq!(thread_name("Thread [main]"), "main");
        let name = qualified_frame_name("Greeting.hello(String)", Some("com.example"));
        assert_eq!(frame_label(&name, 9), "hello:9, Greeting (com.example)");
        let native = qualified_frame_name("Thread.sleep(long)[native method]", Some("java.lang"));
        assert_eq!(frame_label(&native, 0), "sleep:-1, Thread (java.lang)");
        assert_eq!(
            value_text("ArrayList@23 size=3"),
            "{ArrayList@23}  size = 3"
        );
        assert_eq!(
            value_text("Debuggee$Point@12 \"Point[x=1, y=2]\""),
            "{Debuggee$Point@12} \"Point[x=1, y=2]\""
        );
        assert_eq!(value_text("java.lang.String[0]@463"), "{String[0]@463}");
        assert_eq!(value_text("\"a@1\""), "\"a@1\"");
        assert_eq!(value_text("42"), "42");
    }

    #[test]
    fn a_log_message_is_a_java_expression() {
        assert_eq!(
            java_string("len {name.length()}\"!"),
            "\"\" + \"len \" + (name.length()) + \"\\\"!\""
        );
    }
}
