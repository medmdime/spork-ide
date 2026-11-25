//! IntelliJ's Application run configuration, kept as a debug scenario of the "Java" adapter
//! (java-debug's launch arguments: `mainClass`, `args`, `vmArgs`, `cwd`, `env`), run without the
//! debugger as the `java` command line: JDT LS builds the workspace, then gives the module's
//! runtime classpath, and the JDK found runs the main class in the Run tool window. Also the
//! gutter's run icon on a `main` method (the runnables query's `java-main` tag).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use collections::HashMap;
use dap::adapters::{
    DapDelegate, DebugAdapter, DebugAdapterBinary, DebugAdapterName, DebugTaskDefinition,
};
use gpui::{AnyElement, IntoElement as _, ParentElement as _, Styled as _, div, px};
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::PopupMenuItem;
use language::{Buffer, ContextLocation, ContextProvider, LanguageName, LanguageToolchainStore};
use serde_json::Value;
use spork_run::configurations::{self, Configuration, Edit, Field, ModifyOption, Runner};
use task::{
    DebugScenario, TaskTemplate, TaskTemplates, TaskVariables, VariableName, ZedDebugConfig,
};
use util::ResultExt as _;
use workspace::Workspace;

use crate::java_project::{JavaProject, execute_command};
use crate::jdk::Jdk;

/// The `java` command line that runs `main` on `classpath`.
fn java_command(
    vm_options: &[String],
    classpath: &[String],
    main: &str,
    args: &[String],
) -> Vec<String> {
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut command: Vec<String> = vm_options.to_vec();
    if !classpath.is_empty() {
        command.push("-classpath".into());
        command.push(classpath.join(separator));
    }
    command.push(main.to_owned());
    command.extend(args.iter().cloned());
    command
}

/// The JRE the configuration runs on (IntelliJ's JRE field): java-debug's `javaExec`, the
/// module's JDK when it names none.
const JAVA_EXEC: &str = "javaExec";

fn java_executable(entry: &serde_json::Map<String, Value>, jdk: &Jdk) -> String {
    entry
        .get(JAVA_EXEC)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map_or_else(|| jdk.java().to_string_lossy().into_owned(), str::to_owned)
}

/// The JRE field's combo box (IntelliJ's `JrePathEditor`): Default (the module's SDK), the JDKs,
/// Download JDK… and Select alternative JRE….
fn jre_field(configuration: &Configuration, edit: Edit, cx: &mut App) -> AnyElement {
    let env = crate::jdk::process_env();
    let module_jdk = JavaProject::containing(
        &configuration
            .entry
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_default(),
        cx,
    )
    .or_else(|| {
        cx.try_global::<crate::java_project::Projects>()
            .and_then(|projects| projects.0.values().next().cloned())
    });
    let (module_name, default_jdk) = module_jdk
        .map(|java| {
            let java = java.read(cx);
            (java.name(), java.jdk.clone())
        })
        .unwrap_or_default();
    let jdks = crate::jdk::find(&env);
    let chosen = configuration
        .entry
        .get(JAVA_EXEC)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .map(str::to_owned);
    let (label, detail) = match &chosen {
        Some(path) => match jdks
            .iter()
            .find(|jdk| jdk.java().to_string_lossy() == *path)
        {
            Some(jdk) => (jdk.name(), jdk.version_string()),
            None => (path.clone(), String::new()),
        },
        None => (
            "Default".to_owned(),
            format!(
                "({} - SDK of '{module_name}' module)",
                default_jdk
                    .as_ref()
                    .map_or_else(|| "<No JRE>".to_owned(), Jdk::name)
            ),
        ),
    };
    let set = move |edit: &Edit, java: Option<PathBuf>, cx: &mut App| {
        let java = java.map(|java| java.to_string_lossy().into_owned());
        edit(
            &move |configuration: &mut Configuration| match &java {
                Some(java) => {
                    configuration
                        .entry
                        .insert(JAVA_EXEC.into(), Value::String(java.clone()));
                }
                None => {
                    configuration.entry.remove(JAVA_EXEC);
                }
            },
            cx,
        )
    };
    let combo = spork_workbench::controls::dropdown(
        spork_workbench::controls::ComboBox::new("jre", label)
            .aria_label("JRE:")
            .detail(detail)
            .w_full(),
        false,
        move |menu, _, _| {
            let mut menu = menu.item(PopupMenuItem::new("Default").on_click({
                let edit = edit.clone();
                move |_, _, cx| set(&edit, None, cx)
            }));
            for jdk in &jdks {
                let (edit, java) = (edit.clone(), jdk.java());
                menu = menu.item(
                    PopupMenuItem::new(format!("{} {}", jdk.name(), jdk.version_string()))
                        .on_click(move |_, _, cx| set(&edit, Some(java.clone()), cx)),
                );
            }
            let download = edit.clone();
            let browse = edit.clone();
            menu.item(
                PopupMenuItem::new("Download JDK…").on_click(move |_, window, cx| {
                    let edit = download.clone();
                    crate::jdk_download_dialog::open(
                        std::rc::Rc::new(move |package, target, _, cx| {
                            let edit = edit.clone();
                            crate::jdk_download::start(
                                package,
                                target,
                                Box::new(move |jdk, cx| set(&edit, Some(jdk.java()), cx)),
                                None,
                                cx,
                            )
                        }),
                        window,
                        cx,
                    )
                }),
            )
            .item(
                PopupMenuItem::new("Select alternative JRE…").on_click(move |_, window, cx| {
                    let picked = spork_workbench::prompt_for_paths(
                        gpui::PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                            prompt: Some("Select Alternative JRE".into()),
                        },
                        cx,
                    );
                    let edit = browse.clone();
                    window
                        .spawn(cx, async move |cx| {
                            let Ok(Ok(Some(mut paths))) = picked.await else {
                                return;
                            };
                            let Some(home) = paths.pop() else { return };
                            let java = crate::jdk::at_folder(&home).map(|jdk| jdk.java());
                            cx.update(|_, cx| set(&edit, java, cx)).log_err();
                        })
                        .detach();
                }),
            )
        },
    );
    h_flex()
        .gap(px(8.))
        .child(
            div()
                .w(px(160.))
                .flex_none()
                .child(spork_workbench::controls::form_label("JRE:")),
        )
        .child(div().flex_1().min_w_0().child(combo))
        .into_any_element()
}

/// The configuration's keys of its Modify options (java-debug ignores them).
const SKIP_BUILD: &str = "skipBuild";
const ALLOW_MULTIPLE: &str = "allowMultipleInstances";

fn flag(entry: &serde_json::Map<String, Value>, key: &str) -> bool {
    entry.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn toggle_flag(configuration: &mut Configuration, key: &str) {
    if configuration.entry.remove(key).is_none() {
        configuration.entry.insert(key.into(), Value::Bool(true));
    }
}

/// A configuration's list of arguments, as java-debug takes them: a string or a list.
fn arguments(entry: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
    match entry.get(key) {
        Some(Value::String(text)) => configurations::split_arguments(text),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) struct ApplicationRunner;

impl Runner for ApplicationRunner {
    fn task(
        &self,
        configuration: &Configuration,
        workspace: &Workspace,
        cx: &mut App,
    ) -> Task<Result<TaskTemplate>> {
        let Some(java) = JavaProject::of(workspace, cx) else {
            return Task::ready(Err(anyhow::anyhow!("The project isn't a Java project")));
        };
        let (Some(jdk), Some(root)) = (java.read(cx).jdk.clone(), java.read(cx).root.clone())
        else {
            return Task::ready(Err(anyhow::anyhow!(crate::jdtls::NO_JDK)));
        };
        let main = configuration.text(Field::Command);
        if main.trim().is_empty() {
            return Task::ready(Err(anyhow::anyhow!("Main class is not specified")));
        }
        let entry = configuration.entry.clone();
        let label = configuration.name().to_string();
        // Do not build before run: the classes as they are
        let build = match flag(&entry, SKIP_BUILD) {
            true => Task::ready(Ok(())),
            false => java.read(cx).build(cx),
        };
        let allow_concurrent_runs = flag(&entry, ALLOW_MULTIPLE);
        let classpath = java.read(cx).classpath(&root, cx);
        cx.background_spawn(async move {
            build.await?;
            let classpath = classpath.await?;
            let cwd = entry
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| root.to_string_lossy().into_owned());
            let env: HashMap<String, String> = entry
                .get("env")
                .and_then(Value::as_object)
                .map(|env| {
                    env.iter()
                        .filter_map(|(name, value)| {
                            Some((name.clone(), value.as_str()?.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(TaskTemplate {
                label,
                command: java_executable(&entry, &jdk),
                args: java_command(
                    &arguments(&entry, "vmArgs"),
                    &classpath,
                    main.trim(),
                    &arguments(&entry, "args"),
                ),
                cwd: Some(cwd),
                env: env.into_iter().collect(),
                allow_concurrent_runs,
                ..TaskTemplate::default()
            })
        })
    }

    // IntelliJ's Application options it can follow; the rest are greyed
    // shortcut: Redirect input, Modify classpath, Shorten command line and the "provided" scope
    // are greyed: the java command line has no stdin from the Run tool window, and the classpath
    // is JDT LS's; add them with a classpath editor
    fn modify_options(&self, configuration: &Configuration) -> Vec<ModifyOption> {
        let entry = &configuration.entry;
        vec![
            ModifyOption {
                group: "Operating System",
                label: "Allow multiple instances",
                checked: flag(entry, ALLOW_MULTIPLE),
                toggle: Some(|configuration| toggle_flag(configuration, ALLOW_MULTIPLE)),
            },
            ModifyOption {
                group: "Operating System",
                label: "Redirect input",
                checked: false,
                toggle: None,
            },
            ModifyOption {
                group: "Java",
                label: "Do not build before run",
                checked: flag(entry, SKIP_BUILD),
                toggle: Some(|configuration| toggle_flag(configuration, SKIP_BUILD)),
            },
            ModifyOption {
                group: "Java",
                label: "Add VM options",
                checked: entry.contains_key("vmArgs"),
                toggle: Some(|configuration| {
                    if configuration.entry.remove("vmArgs").is_none() {
                        configuration.entry.insert("vmArgs".into(), "".into());
                    }
                }),
            },
            ModifyOption {
                group: "Java",
                label: "Modify classpath",
                checked: false,
                toggle: None,
            },
            ModifyOption {
                group: "Java",
                label: "Shorten command line",
                checked: false,
                toggle: None,
            },
            ModifyOption {
                group: "Java",
                label: "Add dependencies with \u{201c}provided\u{201d} scope to classpath",
                checked: false,
                toggle: None,
            },
        ]
    }

    fn controls(&self, configuration: &Configuration, edit: Edit, cx: &mut App) -> Vec<AnyElement> {
        vec![jre_field(configuration, edit, cx)]
    }

    /// The VM options field once Add VM options is on, as IntelliJ's fragment.
    fn shows(&self, configuration: &Configuration, field: Field) -> bool {
        field != Field::VmOptions || configuration.entry.contains_key("vmArgs")
    }

    /// Once JDT LS has java-debug (it fetches it the first time it starts).
    fn can_debug(&self, _: &App) -> bool {
        // the run widget asks on each frame: the disk is read until the plugin is there
        static FETCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if FETCHED.load(std::sync::atomic::Ordering::Relaxed) {
            return true;
        }
        let container = paths::languages_dir().join(crate::jdtls::SERVER_NAME.0.as_ref());
        let fetched = crate::jdtls::java_debug_jar(&container).is_file();
        FETCHED.store(fetched, std::sync::atomic::Ordering::Relaxed);
        fetched
    }
}

/// IntelliJ's JUnit configuration, run: JUnit 4's runner on the test class, on the project's test
/// classpath, after JDT LS's build.
// shortcut: JUnit 4's console runner and its text in the Run tool window, where IntelliJ has its
// test runner and the test results tree; JUnit 5 and TestNG need a runner of their own
pub(crate) struct JUnitRunner;

impl Runner for JUnitRunner {
    fn task(
        &self,
        configuration: &Configuration,
        workspace: &Workspace,
        cx: &mut App,
    ) -> Task<Result<TaskTemplate>> {
        let Some(java) = JavaProject::of(workspace, cx) else {
            return Task::ready(Err(anyhow::anyhow!("The project isn't a Java project")));
        };
        let (Some(jdk), Some(root)) = (java.read(cx).jdk.clone(), java.read(cx).root.clone())
        else {
            return Task::ready(Err(anyhow::anyhow!(crate::jdtls::NO_JDK)));
        };
        let class = configuration.text(Field::Command).trim().to_owned();
        if class.is_empty() {
            return Task::ready(Err(anyhow::anyhow!("Class not specified")));
        }
        let (entry, label) = (
            configuration.entry.clone(),
            configuration.name().to_string(),
        );
        let build = java.read(cx).build(cx);
        let classpath = java.read(cx).test_classpath(&root, cx);
        cx.background_spawn(async move {
            build.await?;
            let classpath = classpath.await?;
            Ok(TaskTemplate {
                label,
                command: jdk.java().to_string_lossy().into_owned(),
                args: java_command(
                    &arguments(&entry, "vmArgs"),
                    &classpath,
                    configurations::JUNIT_RUNNER,
                    &[class],
                ),
                cwd: Some(
                    entry
                        .get("cwd")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| root.to_string_lossy().into_owned()),
                ),
                ..TaskTemplate::default()
            })
        })
    }

    fn can_debug(&self, cx: &App) -> bool {
        ApplicationRunner.can_debug(cx)
    }
}

/// Java's debug adapter, by which the Application configurations are debug scenarios.
pub(crate) struct JavaDebugAdapter;

#[async_trait(?Send)]
impl DebugAdapter for JavaDebugAdapter {
    fn name(&self) -> DebugAdapterName {
        DebugAdapterName(configurations::JAVA.into())
    }

    /// A launch of a main class, or an attach to a local JVM by its process (Attach to Process),
    /// at the address its JDWP agent listens on.
    async fn config_from_zed_format(&self, zed_scenario: ZedDebugConfig) -> Result<DebugScenario> {
        let config = match zed_scenario.request {
            task::DebugRequest::Launch(launch) => {
                serde_json::json!({"request": "launch", "mainClass": launch.program})
            }
            task::DebugRequest::Attach(attach) => {
                let pid = attach.process_id.context("No process to attach to")?;
                let (host, port) = jdwp_address_of(pid).with_context(|| {
                    format!("Process {pid} isn't a JVM listening for a debugger")
                })?;
                serde_json::json!({"request": "attach", "hostName": host, "port": port})
            }
        };
        Ok(DebugScenario {
            adapter: zed_scenario.adapter,
            label: zed_scenario.label,
            build: None,
            config,
            tcp_connection: None,
        })
    }

    /// java-debug in the project's JDT LS: the workspace built, the main class's project and
    /// classpath resolved, then a debug session started on a port JDT LS opens.
    async fn get_binary(
        &self,
        delegate: &Arc<dyn DapDelegate>,
        definition: &DebugTaskDefinition,
        _: Option<PathBuf>,
        _: Option<Vec<String>>,
        _: Option<HashMap<String, String>>,
        cx: &mut AsyncApp,
    ) -> Result<DebugAdapterBinary> {
        let root = delegate.worktree_root_path().to_owned();
        let (build, server, jdk, workspace) = cx.update(|cx| {
            let java =
                JavaProject::containing(&root, cx).context("The project isn't a Java project")?;
            let java = java.read(cx);
            let server = java
                .language_server(cx)
                .context("The Java language server isn't running")?;
            let jdk = java.jdk.clone().context(crate::jdtls::NO_JDK)?;
            anyhow::Ok((java.build(cx), server, jdk, java.workspace.clone()))
        })?;
        let mut configuration = definition.config.clone();
        if configuration["request"] == "attach" {
            return attach(
                &server,
                &root,
                configuration,
                workspace,
                definition.label.clone(),
                cx,
            )
            .await;
        }
        build.await?;
        crate::debugger::update_settings(&server, true).await?;
        let main = configuration["mainClass"]
            .as_str()
            .map(str::trim)
            .filter(|main| !main.is_empty())
            .context("Main class is not specified")?
            .to_owned();
        let root_uri = lsp::Uri::from_file_path(&root)
            .map_err(|()| anyhow::anyhow!("{root:?} isn't a file path"))?
            .to_string();
        let mains = execute_command(
            &server,
            "vscode.java.resolveMainClass",
            vec![root_uri.into()],
        )
        .await?;
        // a JUnit configuration's runner is in a jar: the project is the one with main classes
        let test = configuration["classpathScope"] == "test";
        let project = mains
            .as_array()
            .and_then(|mains| {
                mains
                    .iter()
                    .find(|found| test || found["mainClass"] == main.as_str())
            })
            .and_then(|found| found["projectName"].as_str())
            .with_context(|| format!("Could not find or load main class {main}"))?
            .to_owned();
        let mut classpath_arguments = vec![main.clone().into(), project.clone().into()];
        if test {
            classpath_arguments.push("test".into());
        }
        let paths =
            execute_command(&server, "vscode.java.resolveClasspath", classpath_arguments).await?;
        let port = execute_command(&server, "vscode.java.startDebugSession", Vec::new())
            .await?
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .context("java-debug opened no port")?;
        let port = crate::debugger::connect(port, workspace, definition.label.clone(), cx).await?;
        let object = configuration
            .as_object_mut()
            .context("a Java debug configuration is a JSON object")?;
        // java-debug takes the arguments as one line each
        for key in ["args", "vmArgs"] {
            if let Some(Value::Array(items)) = object.get(key) {
                let line = items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|item| match item.contains(char::is_whitespace) {
                        true => format!("\"{item}\""),
                        false => item.to_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                object.insert(key.into(), line.into());
            }
        }
        object.insert("request".into(), "launch".into());
        object.remove("classpathScope");
        object.insert("mainClass".into(), main.into());
        object.insert("projectName".into(), project.into());
        object.insert("modulePaths".into(), paths[0].clone());
        object.insert("classPaths".into(), paths[1].clone());
        // the configuration's JRE, the module's JDK by default
        let java = java_executable(object, &jdk);
        object.insert(JAVA_EXEC.into(), java.into());
        // the program runs in a terminal of its own, its input the console's as IntelliJ's
        object.insert("console".into(), "integratedTerminal".into());
        object
            .entry("cwd")
            .or_insert_with(|| root.to_string_lossy().into_owned().into());
        Ok(DebugAdapterBinary {
            command: None,
            arguments: Vec::new(),
            envs: HashMap::default(),
            cwd: None,
            connection: Some(dap::adapters::TcpArguments {
                host: std::net::Ipv4Addr::LOCALHOST.into(),
                port,
                timeout: None,
            }),
            request_args: dap::StartDebuggingRequestArguments {
                configuration,
                request: dap::StartDebuggingRequestArgumentsRequest::Launch,
            },
        })
    }

    fn adapter_language_name(&self) -> Option<LanguageName> {
        Some(LanguageName::new_static("Java"))
    }

    fn dap_schema(&self) -> Value {
        serde_json::json!({
            "properties": {
                "request": {"type": "string", "enum": ["launch", "attach"]},
                "mainClass": {"type": "string", "description": "The main class to run"},
                "hostName": {"type": "string", "description": "The host of the JVM to attach to"},
                "port": {"type": "integer", "description": "The port of the JVM's debugger agent"},
                "args": {"type": ["string", "array"], "description": "The program's arguments"},
                "vmArgs": {"type": "string", "description": "The options for the JVM"},
                "cwd": {"type": "string", "description": "The working directory"},
                "env": {"type": "object", "description": "The environment variables"},
            },
            "required": ["request"],
        })
    }
}

/// IntelliJ's Remote JVM Debug and Attach to Process: java-debug attaches to a JVM's JDWP agent
/// at `hostName`:`port` (IntelliJ's defaults `localhost`, `5005`), for the project's sources;
/// stopping detaches and the JVM goes on.
async fn attach(
    server: &lsp::LanguageServer,
    root: &std::path::Path,
    mut configuration: Value,
    workspace: gpui::WeakEntity<Workspace>,
    label: gpui::SharedString,
    cx: &mut AsyncApp,
) -> Result<DebugAdapterBinary> {
    crate::debugger::update_settings(server, true).await?;
    let object = configuration
        .as_object_mut()
        .context("a Java debug configuration is a JSON object")?;
    object
        .entry("hostName")
        .or_insert_with(|| "localhost".into());
    object.entry("port").or_insert_with(|| 5005.into());
    // the project whose sources and classpath evaluate expressions (IntelliJ's "Use module
    // classpath: <whole project>")
    if !object.contains_key("projectName") {
        let root_uri = lsp::Uri::from_file_path(root)
            .map_err(|()| anyhow::anyhow!("{root:?} isn't a file path"))?
            .to_string();
        let mains = execute_command(
            server,
            "vscode.java.resolveMainClass",
            vec![root_uri.into()],
        )
        .await?;
        if let Some(project) = mains[0]["projectName"].as_str() {
            object.insert("projectName".into(), project.into());
        }
    }
    let port = execute_command(server, "vscode.java.startDebugSession", Vec::new())
        .await?
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .context("java-debug opened no port")?;
    let port = crate::debugger::connect(port, workspace, label, cx).await?;
    Ok(DebugAdapterBinary {
        command: None,
        arguments: Vec::new(),
        envs: HashMap::default(),
        cwd: None,
        connection: Some(dap::adapters::TcpArguments {
            host: std::net::Ipv4Addr::LOCALHOST.into(),
            port,
            timeout: None,
        }),
        request_args: dap::StartDebuggingRequestArguments {
            configuration,
            request: dap::StartDebuggingRequestArgumentsRequest::Attach,
        },
    })
}

/// The host and port a local JVM's JDWP agent listens on, from its command line
/// (`-agentlib:jdwp=…,server=y,…,address=[host:]port` or `-Xrunjdwp:…`).
pub(crate) fn jdwp_address_of(pid: u32) -> Option<(String, u16)> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    let command: Vec<String> = system
        .process(pid)?
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    spork_run::jdwp_address(&command)
}

/// The gutter's run icon on a `main` method: the class, its package, the JDK and the classpath
/// JDT LS gives, which the `java-main` task runs.
pub(crate) struct JavaContextProvider;

const JAVA_COMMAND: &str = "SPORK_JAVA";
const MAIN_CLASS: &str = "SPORK_JAVA_MAIN";
const CLASSPATH: &str = "SPORK_JAVA_CLASSPATH";
/// A Spring Boot application's VM options and program arguments, its configuration's; empty for
/// another class.
const VM_OPTIONS: &str = "SPORK_JAVA_VM_OPTIONS";
const PROGRAM_ARGUMENTS: &str = "SPORK_JAVA_ARGUMENTS";

impl ContextProvider for JavaContextProvider {
    fn build_context(
        &self,
        variables: &TaskVariables,
        location: ContextLocation<'_>,
        _: Option<collections::HashMap<String, String>>,
        _: Arc<dyn LanguageToolchainStore>,
        cx: &mut App,
    ) -> Task<Result<TaskVariables>> {
        let custom = |name: &str| variables.get(&VariableName::Custom(name.to_owned().into()));
        let Some(class) = custom("java_class_name").map(str::to_owned) else {
            return Task::ready(Ok(TaskVariables::default()));
        };
        let text = location.file_location.buffer.read(cx).text();
        // the runnable's capture, or the file's package line when the match has none
        let package = custom("java_package_name")
            .filter(|package| !package.is_empty())
            .map(str::to_owned)
            .or_else(|| package_of(&text));
        let main = match package {
            Some(package) => format!("{package}.{class}"),
            None => class,
        };
        let Some(java) = location
            .worktree_root
            .as_deref()
            .and_then(|root| JavaProject::containing(root, cx))
        else {
            return Task::ready(Ok(TaskVariables::default()));
        };
        let (jdk, root) = (java.read(cx).jdk.clone(), java.read(cx).root.clone());
        let (Some(jdk), Some(root)) = (jdk, root) else {
            return Task::ready(Ok(TaskVariables::default()));
        };
        let (vm_options, arguments) =
            crate::spring_boot::gutter_options(&java, &main, &text, cx).unwrap_or_default();
        let classpath = java.read(cx).classpath(&root, cx);
        cx.background_spawn(async move {
            let separator = if cfg!(windows) { ";" } else { ":" };
            let classpath = classpath.await.context("the classpath of the main class")?;
            Ok(TaskVariables::from_iter([
                (
                    VariableName::Custom(JAVA_COMMAND.into()),
                    jdk.java().to_string_lossy().into_owned(),
                ),
                (VariableName::Custom(MAIN_CLASS.into()), main),
                (
                    VariableName::Custom(CLASSPATH.into()),
                    classpath.join(separator),
                ),
                (
                    VariableName::Custom(VM_OPTIONS.into()),
                    vm_options.join(" "),
                ),
                (
                    VariableName::Custom(PROGRAM_ARGUMENTS.into()),
                    arguments.join(" "),
                ),
            ]))
        })
    }

    fn associated_tasks(&self, _: Option<Entity<Buffer>>, _: &App) -> Task<Option<TaskTemplates>> {
        Task::ready(Some(TaskTemplates(vec![TaskTemplate {
            // IntelliJ names the configuration it makes after the class
            label: "$ZED_CUSTOM_java_class_name".into(),
            command: format!("${{ZED_CUSTOM_{JAVA_COMMAND}}}"),
            // the shell reads the VM options and arguments as words: a Spring Boot class's are
            // its configuration's
            args: vec![
                format!("${{ZED_CUSTOM_{VM_OPTIONS}}}"),
                "-classpath".into(),
                format!("${{ZED_CUSTOM_{CLASSPATH}}}"),
                format!("${{ZED_CUSTOM_{MAIN_CLASS}}}"),
                format!("${{ZED_CUSTOM_{PROGRAM_ARGUMENTS}}}"),
            ],
            cwd: Some("$ZED_WORKTREE_ROOT".into()),
            tags: vec!["java-main".into()],
            ..TaskTemplate::default()
        }])))
    }
}

/// The package a Java file declares.
pub(crate) fn package_of(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let package = line.trim().strip_prefix("package ")?;
        Some(package.trim().trim_end_matches(';').trim().to_owned())
    })
}

/// Registers the Application type: its runner and Java's debug adapter.
pub(crate) fn init(cx: &mut App) {
    configurations::register_runner(
        configurations::JAVA,
        std::rc::Rc::new(ApplicationRunner),
        cx,
    );
    configurations::register_runner(configurations::JUNIT, std::rc::Rc::new(JUnitRunner), cx);
    dap::DapRegistry::global(cx).add_adapter(Arc::new(JavaDebugAdapter));
    // java-debug's filters, IntelliJ's "Any exception" before the first session
    let filter = |filter: &str, label: &str| dap::ExceptionBreakpointsFilter {
        filter: filter.into(),
        label: label.into(),
        description: None,
        default: Some(false),
        supports_condition: None,
        condition_description: None,
    };
    spork_run::register_exception_filters(
        configurations::JAVA,
        vec![
            filter("uncaught", "Uncaught Exceptions"),
            filter("caught", "Caught Exceptions"),
        ],
        Some(("caught", "uncaught")),
        cx,
    );
    cx.set_global(debugger_ui::FrameSourceResolver(std::rc::Rc::new(
        crate::debugger::class_source,
    )));
    // Spring Boot's sessions are java-debug's too
    for adapter in [configurations::JAVA, configurations::SPRING_BOOT] {
        cx.default_global::<debugger_ui::FlatScopeAdapters>()
            .0
            .insert(adapter.into());
        spork_run::register_adapter_hooks(
            adapter,
            spork_run::AdapterHooks {
                frame_label: |frame| crate::debugger::frame_label(&frame.name, frame.line),
                force_step_into: crate::debugger::force_step_into,
            },
            cx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{java_command, package_of};

    #[test]
    fn reads_the_package_a_file_declares() {
        assert_eq!(
            package_of("/** x */\npackage com.example ;\n\npublic class Main {}"),
            Some("com.example".into())
        );
        assert_eq!(package_of("public class Main {}"), None);
    }

    #[test]
    fn writes_the_java_command_line() {
        let command = java_command(
            &["-Xmx512m".into()],
            &["/p/target/classes".into(), "/m2/a.jar".into()],
            "com.example.Main",
            &["one".into()],
        );
        assert_eq!(
            command,
            [
                "-Xmx512m",
                "-classpath",
                "/p/target/classes:/m2/a.jar",
                "com.example.Main",
                "one"
            ]
        );
    }
}
