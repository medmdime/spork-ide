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
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use language::{Buffer, ContextLocation, ContextProvider, LanguageName, LanguageToolchainStore};
use serde_json::Value;
use spork_run::configurations::{self, Configuration, Field, ModifyOption, Runner};
use task::{
    DebugScenario, TaskTemplate, TaskTemplates, TaskVariables, VariableName, ZedDebugConfig,
};
use workspace::Workspace;

use crate::java_project::{JavaProject, execute_command};

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
                command: jdk.java().to_string_lossy().into_owned(),
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

/// Java's debug adapter, by which the Application configurations are debug scenarios.
pub(crate) struct JavaDebugAdapter;

#[async_trait(?Send)]
impl DebugAdapter for JavaDebugAdapter {
    fn name(&self) -> DebugAdapterName {
        DebugAdapterName(configurations::JAVA.into())
    }

    async fn config_from_zed_format(&self, zed_scenario: ZedDebugConfig) -> Result<DebugScenario> {
        let program = match zed_scenario.request {
            task::DebugRequest::Launch(launch) => launch.program,
            task::DebugRequest::Attach(_) => {
                anyhow::bail!("Spork IDE doesn't attach to a Java process yet")
            }
        };
        Ok(DebugScenario {
            adapter: zed_scenario.adapter,
            label: zed_scenario.label,
            build: None,
            config: serde_json::json!({"request": "launch", "mainClass": program}),
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
        let (build, server, jdk) = cx.update(|cx| {
            let java =
                JavaProject::containing(&root, cx).context("The project isn't a Java project")?;
            let java = java.read(cx);
            let server = java
                .language_server(cx)
                .context("The Java language server isn't running")?;
            let jdk = java.jdk.clone().context(crate::jdtls::NO_JDK)?;
            anyhow::Ok((java.build(cx), server, jdk))
        })?;
        build.await?;
        let mut configuration = definition.config.clone();
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
        let project = mains
            .as_array()
            .and_then(|mains| {
                mains
                    .iter()
                    .find(|found| found["mainClass"] == main.as_str())
            })
            .and_then(|found| found["projectName"].as_str())
            .with_context(|| format!("Could not find or load main class {main}"))?
            .to_owned();
        let paths = execute_command(
            &server,
            "vscode.java.resolveClasspath",
            vec![main.clone().into(), project.clone().into()],
        )
        .await?;
        let port = execute_command(&server, "vscode.java.startDebugSession", Vec::new())
            .await?
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .context("java-debug opened no port")?;
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
        object.insert("mainClass".into(), main.into());
        object.insert("projectName".into(), project.into());
        object.insert("modulePaths".into(), paths[0].clone());
        object.insert("classPaths".into(), paths[1].clone());
        object.insert(
            "javaExec".into(),
            jdk.java().to_string_lossy().into_owned().into(),
        );
        object.insert("console".into(), "internalConsole".into());
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
                "request": {"type": "string", "enum": ["launch"]},
                "mainClass": {"type": "string", "description": "The main class to run"},
                "args": {"type": ["string", "array"], "description": "The program's arguments"},
                "vmArgs": {"type": "string", "description": "The options for the JVM"},
                "cwd": {"type": "string", "description": "The working directory"},
                "env": {"type": "object", "description": "The environment variables"},
            },
            "required": ["request", "mainClass"],
        })
    }
}

/// The gutter's run icon on a `main` method: the class, its package, the JDK and the classpath
/// JDT LS gives, which the `java-main` task runs.
pub(crate) struct JavaContextProvider;

const JAVA_COMMAND: &str = "SPORK_JAVA";
const MAIN_CLASS: &str = "SPORK_JAVA_MAIN";
const CLASSPATH: &str = "SPORK_JAVA_CLASSPATH";

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
        // the runnable's capture, or the file's package line when the match has none
        let package = custom("java_package_name")
            .filter(|package| !package.is_empty())
            .map(str::to_owned)
            .or_else(|| package_of(&location.file_location.buffer.read(cx).text()));
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
            ]))
        })
    }

    fn associated_tasks(&self, _: Option<Entity<Buffer>>, _: &App) -> Task<Option<TaskTemplates>> {
        Task::ready(Some(TaskTemplates(vec![TaskTemplate {
            // IntelliJ names the configuration it makes after the class
            label: "$ZED_CUSTOM_java_class_name".into(),
            command: format!("${{ZED_CUSTOM_{JAVA_COMMAND}}}"),
            args: vec![
                "-classpath".into(),
                format!("${{ZED_CUSTOM_{CLASSPATH}}}"),
                format!("${{ZED_CUSTOM_{MAIN_CLASS}}}"),
            ],
            cwd: Some("$ZED_WORKTREE_ROOT".into()),
            tags: vec!["java-main".into()],
            ..TaskTemplate::default()
        }])))
    }
}

/// The package a Java file declares.
fn package_of(text: &str) -> Option<String> {
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
    dap::DapRegistry::global(cx).add_adapter(Arc::new(JavaDebugAdapter));
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
