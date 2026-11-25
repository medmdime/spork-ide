//! Attach to Process… (⌥⇧F5, `XDebugger.AttachToProcess`): IntelliJ's popup of the local
//! processes ("Attach To", `AttachToProcessAction`'s list popup: the four attached last under
//! Recent, then a group per debugger, each row "pid name", the speed search) and the debug session
//! attached to the one picked. A process goes to the adapter that attaches to it by its process
//! id: Python's to Debugpy, Node's to JavaScript, a JVM whose JDWP agent listens for a debugger
//! to Java's (`com.example.Main (5005)`), the others to CodeLLDB (or GDB without it).
// shortcut: IntelliJ 2025.2 shows a dialog by default (`debugger.attach.dialog.enabled`), with
// remote hosts, columns and a debugger filter; this is its popup, which the registry key brings
// back

use dap::DapRegistry;
use gpui::{AppContext as _, Context, Global, SharedString, TaskExt as _, Window};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};
use task::{AttachRequest, DebugRequest, SharedTaskContext, ZedDebugConfig};
use util::ResultExt as _;
use workspace::Workspace;

use crate::list_popup::{ListPopup, Row};
use crate::run;

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &run::AttachToProcess, window, cx| {
        show(workspace, window, cx)
    });
}

#[derive(Clone, Debug, PartialEq)]
struct Process {
    pid: u32,
    name: SharedString,
    /// A JVM's main class and debugger port, when its JDWP agent listens for a debugger.
    jvm: Option<(SharedString, u16)>,
}

impl Process {
    /// The row's text: `pid name`, a JVM's as IntelliJ names it, `com.example.Main (5005)`.
    fn label(&self) -> String {
        match &self.jvm {
            Some((main, port)) => format!("{} {main} ({port})", self.pid),
            None => format!("{} {}", self.pid, self.name),
        }
    }
}

/// The JDWP agent's listening address in a JVM's command line: `*` and no host are this machine.
pub fn jdwp_address(command: &[String]) -> Option<(String, u16)> {
    let options = command.iter().find_map(|arg| {
        arg.strip_prefix("-agentlib:jdwp=")
            .or_else(|| arg.strip_prefix("-Xrunjdwp:"))
    })?;
    let option = |name: &str| {
        options
            .split(',')
            .find_map(|option| option.strip_prefix(name)?.strip_prefix('='))
    };
    if option("server") != Some("y") {
        return None;
    }
    let address = option("address")?;
    let (host, port) = address.rsplit_once(':').unwrap_or(("", address));
    let host = match host {
        "" | "*" | "0.0.0.0" => "localhost",
        host => host,
    };
    Some((host.to_owned(), port.parse().ok()?))
}

/// A JVM's main class and the port its JDWP agent listens on, from its command line.
fn jvm(command: &[String]) -> Option<(SharedString, u16)> {
    let (_, port) = jdwp_address(command)?;
    // the first word after the JVM's options and their values: the main class, or `-jar`'s jar
    let mut words = command.iter().skip(1);
    let mut main = None;
    while let Some(word) = words.next() {
        match word.as_str() {
            "-cp" | "-classpath" | "--class-path" | "-p" | "--module-path" => {
                words.next();
            }
            "-jar" | "-m" | "--module" => {
                main = words.next();
                break;
            }
            word if word.starts_with('-') => {}
            _ => {
                main = Some(word);
                break;
            }
        }
    }
    Some((main?.clone().into(), port))
}

/// The processes attached to last, newest first, with their debuggers: IntelliJ's Recent group
/// keeps four.
#[derive(Default)]
struct Recent(Vec<(SharedString, Process)>);

impl Global for Recent {}

/// The adapter that attaches to a process by its name, of those registered: a JVM listening for
/// a debugger to Java's.
fn debugger_for(process: &Process, registered: &[SharedString]) -> Option<SharedString> {
    let name = process.name.to_lowercase();
    let wanted: &[&str] = if process.jvm.is_some() {
        &[crate::configurations::JAVA]
    } else if name == "java" {
        // a JVM without a debugger agent: IntelliJ's "Java Read Only", which Spork IDE hasn't
        &[]
    } else if name.starts_with("python") {
        &["Debugpy"]
    } else if name == "node" || name.starts_with("node.") {
        &["JavaScript"]
    } else {
        &["CodeLLDB", "GDB"]
    };
    wanted
        .iter()
        .find_map(|wanted| registered.iter().find(|name| name.as_ref() == *wanted))
        .cloned()
}

fn processes() -> Vec<Process> {
    let refresh = RefreshKind::nothing().with_processes(
        ProcessRefreshKind::nothing()
            .without_tasks()
            .with_cmd(UpdateKind::Always),
    );
    let own = std::process::id();
    let mut processes: Vec<Process> = System::new_with_specifics(refresh)
        .processes()
        .values()
        .filter(|process| process.pid().as_u32() != own)
        .map(|process| {
            let command: Vec<String> = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            Process {
                pid: process.pid().as_u32(),
                name: process.name().to_string_lossy().into_owned().into(),
                jvm: jvm(&command),
            }
        })
        .collect();
    processes.sort_by_key(|process| process.pid);
    processes
}

fn show(_: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let listing = cx.background_spawn(async { processes() });
    cx.spawn_in(window, async move |workspace, cx| {
        let processes = listing.await;
        workspace.update_in(cx, |workspace, window, cx| {
            let registered: Vec<SharedString> = DapRegistry::global(cx)
                .enumerate_adapters::<Vec<_>>()
                .into_iter()
                .map(|name| name.0)
                .collect();
            let mut attachable: Vec<(SharedString, Process)> = processes
                .iter()
                .filter_map(|process| Some((debugger_for(process, &registered)?, process.clone())))
                .collect();
            // grouped by debugger, each group by process id
            attachable.sort_by(|(a, x), (b, y)| a.cmp(b).then(x.pid.cmp(&y.pid)));
            let recent: Vec<(SharedString, Process)> = cx
                .try_global::<Recent>()
                .map(|recent| recent.0.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, process)| processes.contains(process))
                .collect();
            let mut rows = Vec::new();
            let mut group = None;
            for (ix, (debugger, process)) in recent.iter().chain(&attachable).enumerate() {
                let mut row = Row::new(process.label()).image("run/debug");
                let title = match ix < recent.len() {
                    true => SharedString::from("Recent"),
                    false => debugger.clone(),
                };
                if group.as_ref() != Some(&title) {
                    row = row.group(title.clone());
                    group = Some(title);
                }
                rows.push(row);
            }
            let attachable: Vec<_> = recent.into_iter().chain(attachable).collect();
            let workspace_handle = cx.weak_entity();
            ListPopup::show(
                workspace,
                "Attach To",
                rows,
                "No processes to attach to",
                420.,
                move |ix, window, cx| {
                    if let Some((debugger, process)) = attachable.get(ix).cloned() {
                        workspace_handle
                            .update(cx, |workspace, cx| {
                                attach(workspace, debugger, process, window, cx)
                            })
                            .log_err();
                    }
                },
                window,
                cx,
            );
        })
    })
    .detach_and_log_err(cx);
}

/// Starts a debug session attached to `process`, named after it as IntelliJ's.
fn attach(
    workspace: &mut Workspace,
    debugger: SharedString,
    process: Process,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(adapter) = DapRegistry::global(cx).adapter(&debugger) else {
        return;
    };
    let recent = &mut cx.default_global::<Recent>().0;
    recent.retain(|(_, old)| old != &process);
    recent.insert(0, (debugger.clone(), process.clone()));
    recent.truncate(4);
    // a JVM's session is named by its address (`localhost:5005`), as IntelliJ's
    let label = match &process.jvm {
        Some((_, port)) => format!("localhost:{port}"),
        None => format!("{} {}", process.pid, process.name),
    };
    let config = ZedDebugConfig {
        label: label.into(),
        adapter: debugger,
        request: DebugRequest::Attach(AttachRequest {
            process_id: Some(process.pid),
        }),
        stop_on_entry: None,
    };
    let worktree = workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).id());
    cx.spawn_in(window, async move |workspace, cx| {
        let scenario = adapter.config_from_zed_format(config).await?;
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.start_debug_session(
                scenario,
                SharedTaskContext::default(),
                None,
                worktree,
                window,
                cx,
            )
        })
    })
    .detach_and_log_err(cx);
}

/// Whether a process can be attached to, for the tests.
#[cfg(test)]
fn attaches(name: &str, registered: &[&str]) -> Option<String> {
    let registered: Vec<SharedString> = registered.iter().map(|n| (*n).into()).collect();
    let process = Process {
        pid: 1,
        name: name.to_owned().into(),
        jvm: None,
    };
    debugger_for(&process, &registered).map(|name| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::attaches;

    #[test]
    fn each_process_goes_to_the_debugger_of_its_kind() {
        let all = ["CodeLLDB", "Debugpy", "JavaScript", "GDB"];
        assert_eq!(attaches("python3.12", &all).as_deref(), Some("Debugpy"));
        assert_eq!(attaches("node", &all).as_deref(), Some("JavaScript"));
        assert_eq!(attaches("fixture", &all).as_deref(), Some("CodeLLDB"));
        assert_eq!(attaches("fixture", &["GDB"]).as_deref(), Some("GDB"));
        assert_eq!(attaches("python3", &["CodeLLDB"]), None);
        assert_eq!(attaches("java", &all), None);
    }

    #[test]
    fn reads_where_a_jvm_listens_for_a_debugger() {
        let line = |arg: &str| vec!["java".to_owned(), arg.to_owned(), "Main".to_owned()];
        assert_eq!(
            super::jdwp_address(&line(
                "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005"
            )),
            Some(("localhost".into(), 5005))
        );
        assert_eq!(
            super::jdwp_address(&line("-Xrunjdwp:transport=dt_socket,server=y,address=8000")),
            Some(("localhost".into(), 8000))
        );
        assert_eq!(
            super::jdwp_address(&line(
                "-agentlib:jdwp=transport=dt_socket,server=n,address=localhost:5005"
            )),
            None
        );
    }

    #[test]
    fn names_a_jvm_by_its_main_class_and_port() {
        let command: Vec<String> = [
            "/jdk/bin/java",
            "-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005",
            "-cp",
            "target/classes",
            "com.example.Main",
            "--ask",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(
            super::jvm(&command),
            Some(("com.example.Main".into(), 5005))
        );
    }
}
