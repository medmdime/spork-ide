//! IntelliJ's Kotlin run configuration and the gutter's run icon on `main`, built and run by the
//! project's own tool, as IntelliJ's "Build and run using: Gradle" does: on Gradle, a `JavaExec`
//! task named after the configuration (`MainKt.main()`) that an init script adds on the source
//! set's runtime classpath; on Maven, `compile`, then `java` on the runtime classpath (the exec
//! plugin's `%classpath`). Debugging is the Kotlin language server's own debug adapter, which
//! builds and launches the program itself (`start_debug_server`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use collections::HashMap;
use dap::adapters::{
    DapDelegate, DebugAdapter, DebugAdapterBinary, DebugAdapterName, DebugTaskDefinition,
};
use gpui::{App, AppContext as _, AsyncApp, Entity, Task};
use language::{Buffer, ContextLocation, ContextProvider, LanguageName, LanguageToolchainStore};
use serde_json::{Value, json};
use spork_java::{JavaProject, Tool};
use spork_run::configurations::{self, Configuration, Field, Runner};
use task::{
    DebugScenario, TaskTemplate, TaskTemplates, TaskVariables, VariableName, ZedDebugConfig,
};
use workspace::Workspace;

use crate::kotlin_project::KotlinProject;

/// The exec plugin that runs `java` on Maven's runtime classpath.
const EXEC_PLUGIN: &str = "org.codehaus.mojo:exec-maven-plugin:3.5.0:exec";

/// The package a Kotlin file declares.
fn package_of(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let package = line.trim().strip_prefix("package ")?;
        Some(package.trim().trim_end_matches(';').trim().to_owned())
    })
}

/// The class a file's top-level functions compile into: `@file:JvmName("…")`'s, or the file's
/// name with `Kt` (`Main.kt` is `MainKt`).
fn facade(file_name: &str, text: &str) -> String {
    let named = text.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("@file:JvmName(")?;
        Some(rest.split('"').nth(1)?.to_owned())
    });
    if let Some(named) = named {
        return named;
    }
    let stem = file_name.strip_suffix(".kt").unwrap_or(file_name);
    let mut chars = stem.chars();
    let first: String = chars
        .next()
        .into_iter()
        .flat_map(char::to_uppercase)
        .collect();
    let name: String = format!("{first}{}", chars.as_str())
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{name}Kt")
}

fn qualified(package: Option<&str>, class: &str) -> String {
    match package {
        Some(package) if !package.is_empty() => format!("{package}.{class}"),
        _ => class.to_owned(),
    }
}

/// The Gradle project of a file: the nearest folder above it, up to the root, with a build
/// script (`:` the root's, `:app` a subproject's).
fn gradle_project_path(root: &Path, file: &Path) -> String {
    let scripts = ["build.gradle.kts", "build.gradle"];
    let project = file
        .ancestors()
        .skip(1)
        .take_while(|folder| folder.starts_with(root))
        .find(|folder| scripts.iter().any(|script| folder.join(script).is_file()))
        .unwrap_or(root);
    let parts: Vec<String> = project
        .strip_prefix(root)
        .map(|relative| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    format!(":{}", parts.join(":"))
}

fn groovy_string(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn groovy_list(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|item| groovy_string(item)).collect();
    format!("[{}]", items.join(", "))
}

/// What a run starts: the main class with its arguments, options, folder and environment.
#[derive(Clone, Debug, Default, PartialEq)]
struct Launch {
    /// The configuration's name, which names the Run tool window's tab and Gradle's task.
    name: String,
    main: String,
    args: Vec<String>,
    vm_args: Vec<String>,
    cwd: Option<String>,
    env: HashMap<String, String>,
    /// The Gradle project that holds the main class (`:`, `:app`).
    gradle_project: String,
}

impl Launch {
    /// Gradle's task, as IntelliJ names it: `MainKt.main()`.
    fn gradle_task(&self) -> String {
        format!("{}.main()", self.name)
    }

    /// The init script that adds the task to the project: a `JavaExec` on the main source
    /// set's runtime classpath.
    fn init_script(&self) -> String {
        let mut env: Vec<(&String, &String)> = self.env.iter().collect();
        env.sort();
        let env: Vec<String> = env
            .into_iter()
            .map(|(name, value)| format!("{}: {}", groovy_string(name), groovy_string(value)))
            .collect();
        let env = match env.is_empty() {
            true => "[:]".to_owned(),
            false => format!("[{}]", env.join(", ")),
        };
        let cwd = self
            .cwd
            .as_ref()
            .map(|cwd| format!("            workingDir = file({})\n", groovy_string(cwd)))
            .unwrap_or_default();
        format!(
            "// Spork IDE's run of {name}\n\
             allprojects {{\n\
             \x20   if (path == {project}) {{\n\
             \x20       afterEvaluate {{\n\
             \x20           tasks.register({task}, JavaExec) {{\n\
             \x20               mainClass.set({main})\n\
             \x20               classpath = project.sourceSets.main.runtimeClasspath\n\
             \x20               args({args})\n\
             \x20               jvmArgs({vm_args})\n\
             \x20               environment({env})\n\
             \x20               standardInput = System.in\n\
             {cwd}\
             \x20           }}\n\
             \x20       }}\n\
             \x20   }}\n\
             }}\n",
            name = self.name,
            project = groovy_string(&self.gradle_project),
            task = groovy_string(&self.gradle_task()),
            main = groovy_string(&self.main),
            args = groovy_list(&self.args),
            vm_args = groovy_list(&self.vm_args),
        )
    }

    /// The exec plugin's arguments: the JVM's options, the classpath, the main class and the
    /// program's arguments, each quoted when it holds a space.
    fn exec_args(&self) -> String {
        let quote = |item: &String| match item.contains(char::is_whitespace) {
            true => format!("\"{item}\""),
            false => item.clone(),
        };
        let mut line: Vec<String> = self.vm_args.iter().map(quote).collect();
        line.extend(["-classpath".into(), "%classpath".into(), self.main.clone()]);
        line.extend(self.args.iter().map(quote));
        line.join(" ")
    }
}

/// The Kotlin file a main class compiles from, which the debugger's launch needs to find the
/// module: `com.example.MainKt` is a `Main.kt` in the package's folder, an object or class `App`
/// the `App.kt` there, or the file there that declares it or names its facade so.
fn source_of(root: &Path, main: &str) -> Option<PathBuf> {
    let (package, class) = main.rsplit_once('.').unwrap_or(("", main));
    let class = class.split('$').next()?;
    let folder: PathBuf = package.split('.').filter(|part| !part.is_empty()).collect();
    let stem = class.strip_suffix("Kt").unwrap_or(class);
    let mut files = Vec::new();
    kotlin_files(root, &mut files);
    let in_package: Vec<PathBuf> = files
        .into_iter()
        .filter(|file| {
            file.parent()
                .is_some_and(|parent| parent.ends_with(&folder))
        })
        .collect();
    if let Some(file) = in_package.iter().find(|file| {
        file.file_stem()
            .is_some_and(|name| name == stem || name == class)
    }) {
        return Some(file.clone());
    }
    in_package.into_iter().find(|file| {
        let text = std::fs::read_to_string(file).unwrap_or_default();
        [
            format!("object {class}"),
            format!("class {class}"),
            format!("@file:JvmName(\"{class}\")"),
        ]
        .iter()
        .any(|declaration| text.contains(declaration.as_str()))
    })
}

/// The project's Kotlin files, its build tools' output and hidden folders left out.
fn kotlin_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.')
            || ["build", "target", "out", "node_modules"].contains(&name.as_ref())
        {
            continue;
        }
        if path.is_dir() {
            kotlin_files(&path, files);
        } else if name.ends_with(".kt") {
            files.push(path);
        }
    }
}

/// Where a run's Gradle init script is written: the data folder, one per configuration.
fn init_script_path(root: &Path, name: &str) -> PathBuf {
    let file: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    spork_java::jdtls::project_data(root, "kotlin-run").join(format!("{file}.gradle"))
}

/// The project's build tool, wrapper and environment a run needs.
struct Build {
    root: PathBuf,
    tool: Tool,
    wrapper: PathBuf,
    env: HashMap<String, String>,
    /// The tool's own arguments (offline, profiles).
    tool_args: Vec<String>,
    java: PathBuf,
}

impl Build {
    fn of(java: &Entity<JavaProject>, cx: &App) -> Result<Self> {
        let java = java.read(cx);
        let root = java.root.clone().context("The project has no folder")?;
        let tool = java
            .tool
            .context("The project has neither a Gradle build nor a Maven one")?;
        let wrapper = java
            .wrapper()
            .with_context(|| format!("The project has no {} wrapper", tool.name()))?;
        let jdk = java.jdk.clone().context(spork_java::jdtls::NO_JDK)?;
        Ok(Self {
            root,
            tool,
            wrapper,
            env: java.tool_env(),
            tool_args: java.tool_args(&[]),
            java: jdk.java(),
        })
    }

    /// The task that builds and runs `launch` with the project's tool; a Gradle run writes its
    /// init script first.
    fn task(self, launch: Launch, allow_concurrent_runs: bool) -> Result<TaskTemplate> {
        let args = match self.tool {
            Tool::Gradle => {
                let script = init_script_path(&self.root, &launch.name);
                if let Some(folder) = script.parent() {
                    std::fs::create_dir_all(folder)
                        .with_context(|| format!("creating {folder:?}"))?;
                }
                std::fs::write(&script, launch.init_script())
                    .with_context(|| format!("writing {script:?}"))?;
                let task = match launch.gradle_project.as_str() {
                    ":" => format!(":{}", launch.gradle_task()),
                    project => format!("{project}:{}", launch.gradle_task()),
                };
                let mut args = vec!["-I".to_owned(), script.to_string_lossy().into_owned(), task];
                args.extend(self.tool_args);
                args
            }
            // IntelliJ's Run tool window shows the program, its build is elsewhere: Maven quiet
            Tool::Maven => {
                let mut args = vec![
                    "-q".to_owned(),
                    "compile".into(),
                    EXEC_PLUGIN.into(),
                    format!("-Dexec.executable={}", self.java.to_string_lossy()),
                    format!("-Dexec.args={}", launch.exec_args()),
                ];
                if let Some(cwd) = &launch.cwd {
                    args.push(format!("-Dexec.workingdir={cwd}"));
                }
                args.extend(self.tool_args);
                args
            }
        };
        let mut env = self.env;
        // Gradle's task sets the program's environment; Maven's exec inherits Maven's
        if self.tool == Tool::Maven {
            env.extend(launch.env.clone());
        }
        // the command line as one, quoted for the shell: the exec plugin's arguments hold spaces
        let wrapper = self.wrapper.to_string_lossy().into_owned();
        Ok(TaskTemplate {
            label: launch.name,
            command: command_line(&wrapper, &args),
            cwd: Some(self.root.to_string_lossy().into_owned()),
            env: env.into_iter().collect(),
            allow_concurrent_runs,
            ..TaskTemplate::default()
        })
    }
}

/// A configuration's list of arguments: a string or a list.
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

fn environment(entry: &serde_json::Map<String, Value>) -> HashMap<String, String> {
    entry
        .get("env")
        .and_then(Value::as_object)
        .map(|env| {
            env.iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) struct KotlinRunner;

impl Runner for KotlinRunner {
    // shortcut: the configuration runs in the root Gradle project; pick the main class's
    // project once "Use classpath of module:" is in the form
    fn task(
        &self,
        configuration: &Configuration,
        workspace: &Workspace,
        cx: &mut gpui::App,
    ) -> Task<Result<TaskTemplate>> {
        let main = configuration.text(Field::Command).trim().to_owned();
        if main.is_empty() {
            // IntelliJ's `run.configuration.error.no.main.class`
            return Task::ready(Err(anyhow::anyhow!("No main class specified")));
        }
        let build = JavaProject::of(workspace, cx)
            .context("The project isn't a Kotlin project")
            .and_then(|java| Build::of(&java, cx));
        let build = match build {
            Ok(build) => build,
            Err(error) => return Task::ready(Err(error)),
        };
        let entry = &configuration.entry;
        let launch = Launch {
            name: configuration.name().to_string(),
            main,
            args: arguments(entry, "args"),
            vm_args: arguments(entry, "vmArgs"),
            cwd: entry.get("cwd").and_then(Value::as_str).map(str::to_owned),
            env: environment(entry),
            gradle_project: ":".into(),
        };
        let allow_concurrent_runs = entry
            .get("allow_concurrent_runs")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        cx.background_spawn(async move { build.task(launch, allow_concurrent_runs) })
    }

    /// Once the Kotlin language server, which debugs, is downloaded.
    fn can_debug(&self, _: &App) -> bool {
        // the run widget asks on each frame: the disk is read until a build is there
        static FETCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if FETCHED.load(std::sync::atomic::Ordering::Relaxed) {
            return true;
        }
        let fetched =
            crate::kotlin_lsp::newest_installed(&crate::kotlin_lsp::container()).is_some();
        FETCHED.store(fetched, std::sync::atomic::Ordering::Relaxed);
        fetched
    }
}

/// The Kotlin language server's debug adapter, by which the Kotlin configurations are debug
/// scenarios.
pub(crate) struct KotlinDebugAdapter;

#[async_trait(?Send)]
impl DebugAdapter for KotlinDebugAdapter {
    fn name(&self) -> DebugAdapterName {
        DebugAdapterName(configurations::KOTLIN.into())
    }

    async fn config_from_zed_format(&self, zed_scenario: ZedDebugConfig) -> Result<DebugScenario> {
        let program = match zed_scenario.request {
            task::DebugRequest::Launch(launch) => launch.program,
            task::DebugRequest::Attach(_) => {
                anyhow::bail!("Spork IDE doesn't attach to a Kotlin process yet")
            }
        };
        Ok(DebugScenario {
            adapter: zed_scenario.adapter,
            label: zed_scenario.label,
            build: None,
            config: json!({"request": "launch", "mainClass": program}),
            tcp_connection: None,
        })
    }

    /// The project's Kotlin language server opens a debug server; its launch builds the module
    /// with the project's tool and starts the program under the debugger.
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
        // the launch finds the main class's module in the server's import: wait for it
        loop {
            let imported = cx.update(|cx| {
                KotlinProject::containing(&root, cx).is_none_or(|kotlin| kotlin.read(cx).imported)
            });
            if imported {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;
        }
        let (server, tool) = cx.update(|cx| {
            let kotlin = KotlinProject::containing(&root, cx)
                .context("The project isn't a Kotlin project")?;
            let kotlin = kotlin.read(cx);
            let server = kotlin.language_server(cx).with_context(|| {
                format!("The {} isn't running", crate::kotlin_lsp::DISPLAY_NAME)
            })?;
            anyhow::Ok((server, kotlin.tool(cx)))
        })?;
        let mut configuration = definition.config.clone();
        let main = configuration["mainClass"]
            .as_str()
            .map(str::trim)
            .filter(|main| !main.is_empty())
            .context("No main class specified")?
            .to_owned();
        let root_uri = lsp::Uri::from_file_path(&root)
            .map_err(|()| anyhow::anyhow!("{root:?} isn't a file path"))?
            .to_string();
        let port = crate::kotlin_project::execute_command(
            &server,
            "start_debug_server",
            vec![root_uri.into()],
        )
        .await?
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .context("The Kotlin language server opened no debug port")?;
        let object = configuration
            .as_object_mut()
            .context("a Kotlin debug configuration is a JSON object")?;
        // its launch takes lists of arguments
        for key in ["args", "vmArgs"] {
            if let Some(Value::String(line)) = object.get(key) {
                let items = configurations::split_arguments(line);
                object.insert(key.into(), items.into());
            }
        }
        object.insert("request".into(), "launch".into());
        if !object.contains_key("file") {
            // IntelliJ's `run.configuration.error.class.not.found`
            let file = source_of(&root, &main).with_context(|| {
                let module = root.file_name().unwrap_or_default().to_string_lossy();
                format!("Class '{main}' not found in module '{module}'")
            })?;
            object.insert("file".into(), file.to_string_lossy().into_owned().into());
        }
        object.insert("mainClass".into(), main.into());
        match tool {
            Some(Tool::Gradle) => {
                object.insert("tool".into(), "gradle".into());
                object
                    .entry("projectPath")
                    .or_insert_with(|| root.to_string_lossy().into_owned().into());
                object.entry("sourceSet").or_insert_with(|| "main".into());
            }
            _ => {
                object.insert("tool".into(), "java".into());
            }
        }
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
        Some(LanguageName::new_static("Kotlin"))
    }

    /// The server's debugger answers to its JVM adapter; the launch's `tool` says which tool runs
    /// the program.
    fn dap_adapter_id(&self) -> Option<String> {
        Some("intellij_jvm".into())
    }

    fn dap_schema(&self) -> Value {
        json!({
            "properties": {
                "request": {"type": "string", "enum": ["launch"]},
                "mainClass": {"type": "string", "description": "The main class to run"},
                "args": {"type": ["string", "array"], "description": "The program's arguments"},
                "vmArgs": {"type": ["string", "array"], "description": "The options for the JVM"},
                "cwd": {"type": "string", "description": "The working directory"},
                "env": {"type": "object", "description": "The environment variables"},
            },
            "required": ["request", "mainClass"],
        })
    }
}

/// The gutter's run icon on `main`: the main class (the file's facade, or the object or class
/// the runnables query captures) run by the project's tool.
pub(crate) struct KotlinContextProvider;

const NAME: &str = "SPORK_KOTLIN_NAME";
/// The main class, which the gutter's Debug launches.
const MAIN: &str = "SPORK_KOTLIN_MAIN";
/// The command line, quoted for the shell.
const COMMAND: &str = "SPORK_KOTLIN_COMMAND";
const ENV: [(&str, &str); 3] = [
    ("JAVA_HOME", "SPORK_KOTLIN_JAVA_HOME"),
    ("MAVEN_OPTS", "SPORK_KOTLIN_MAVEN_OPTS"),
    ("GRADLE_USER_HOME", "SPORK_KOTLIN_GRADLE_USER_HOME"),
];

/// A command line as a shell reads it: a task without arguments runs its command as typed.
fn command_line(command: &str, args: &[String]) -> String {
    std::iter::once(command)
        .chain(args.iter().map(String::as_str))
        .map(|arg| match util::shell::ShellKind::Posix.try_quote(arg) {
            Some(quoted) => quoted.into_owned(),
            None => arg.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl ContextProvider for KotlinContextProvider {
    fn build_context(
        &self,
        variables: &TaskVariables,
        location: ContextLocation<'_>,
        _: Option<HashMap<String, String>>,
        _: Arc<dyn LanguageToolchainStore>,
        cx: &mut App,
    ) -> Task<Result<TaskVariables>> {
        let Some(file) = variables.get(&VariableName::File).map(PathBuf::from) else {
            return Task::ready(Ok(TaskVariables::default()));
        };
        let text = location.file_location.buffer.read(cx).text();
        let package = package_of(&text);
        let class = match variables.get(&VariableName::Custom("kotlin_main_class".into())) {
            Some(class) => class.to_owned(),
            None => {
                let file_name = file
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                facade(&file_name, &text)
            }
        };
        let Some(java) = location
            .worktree_root
            .as_deref()
            .and_then(|root| JavaProject::containing(root, cx))
        else {
            return Task::ready(Ok(TaskVariables::default()));
        };
        let build = match Build::of(&java, cx) {
            Ok(build) => build,
            Err(error) => return Task::ready(Err(error)),
        };
        let main = qualified(package.as_deref(), &class);
        let launch = Launch {
            name: class,
            main: main.clone(),
            gradle_project: gradle_project_path(&build.root, &file),
            ..Launch::default()
        };
        cx.background_spawn(async move {
            let env = build.env.clone();
            let template = build.task(launch, false)?;
            let mut variables = TaskVariables::from_iter([
                (VariableName::Custom(COMMAND.into()), template.command),
                (VariableName::Custom(NAME.into()), template.label),
                (VariableName::Custom(MAIN.into()), main),
            ]);
            for (name, variable) in ENV {
                if let Some(value) = env.get(name) {
                    variables.insert(VariableName::Custom(variable.into()), value.clone());
                }
            }
            Ok(variables)
        })
    }

    fn associated_tasks(&self, _: Option<Entity<Buffer>>, _: &App) -> Task<Option<TaskTemplates>> {
        Task::ready(Some(TaskTemplates(vec![TaskTemplate {
            // IntelliJ names the configuration it makes after the class
            label: format!("${{ZED_CUSTOM_{NAME}}}"),
            command: format!("${{ZED_CUSTOM_{COMMAND}}}"),
            env: ENV
                .iter()
                .map(|(name, variable)| (name.to_string(), format!("${{ZED_CUSTOM_{variable}}}")))
                .collect(),
            cwd: Some("$ZED_WORKTREE_ROOT".into()),
            tags: vec!["kotlin-main".into()],
            ..TaskTemplate::default()
        }])))
    }
}

/// The gutter's Debug: the `main` task as a launch of the Kotlin debug adapter, its variables
/// the task's context.
struct KotlinLocator;

#[async_trait]
impl dap::DapLocator for KotlinLocator {
    fn name(&self) -> gpui::SharedString {
        gpui::SharedString::new_static("Kotlin")
    }

    async fn create_scenario(
        &self,
        build: &TaskTemplate,
        resolved_label: &str,
        adapter: &DebugAdapterName,
    ) -> Option<DebugScenario> {
        if adapter.0.as_ref() != configurations::KOTLIN
            || !build.tags.iter().any(|tag| tag == "kotlin-main")
        {
            return None;
        }
        Some(DebugScenario {
            adapter: adapter.0.clone(),
            label: resolved_label.to_owned().into(),
            build: None,
            config: json!({
                "request": "launch",
                "mainClass": format!("${{ZED_CUSTOM_{MAIN}}}"),
                "file": "$ZED_FILE",
            }),
            tcp_connection: None,
        })
    }

    async fn run(
        &self,
        _: task::SpawnInTerminal,
        _: gpui::BackgroundExecutor,
    ) -> Result<task::DebugRequest> {
        anyhow::bail!("Kotlin's launch builds the program itself")
    }
}

/// Registers the Kotlin type: its runner, its debug adapter and the gutter's Debug.
pub(crate) fn init(cx: &mut App) {
    configurations::register_runner(configurations::KOTLIN, std::rc::Rc::new(KotlinRunner), cx);
    let registry = dap::DapRegistry::global(cx);
    registry.add_adapter(Arc::new(KotlinDebugAdapter));
    registry.add_locator(Arc::new(KotlinLocator));
}

#[cfg(test)]
mod tests {
    use super::{
        Launch, command_line, facade, gradle_project_path, package_of, qualified, source_of,
    };
    use std::path::Path;

    #[test]
    fn names_the_main_class() {
        let text = "package com.example\n\nfun main() {}\n";
        assert_eq!(package_of(text).as_deref(), Some("com.example"));
        assert_eq!(facade("Main.kt", text), "MainKt");
        assert_eq!(facade("hello-world.kt", text), "Hello_worldKt");
        assert_eq!(
            facade("Main.kt", "@file:JvmName(\"App\")\npackage a\n"),
            "App"
        );
        assert_eq!(
            qualified(Some("com.example"), "MainKt"),
            "com.example.MainKt"
        );
        assert_eq!(qualified(None, "MainKt"), "MainKt");
    }

    #[test]
    fn finds_the_gradle_project_of_a_file() {
        let root = tempfile::tempdir().expect("a temporary folder");
        let app = root.path().join("app/src/main/kotlin");
        std::fs::create_dir_all(&app).expect("folders");
        std::fs::write(root.path().join("build.gradle.kts"), "").expect("root script");
        assert_eq!(gradle_project_path(root.path(), &app.join("Main.kt")), ":");
        std::fs::write(root.path().join("app/build.gradle.kts"), "").expect("app script");
        assert_eq!(
            gradle_project_path(root.path(), &app.join("Main.kt")),
            ":app"
        );
        assert_eq!(
            gradle_project_path(Path::new("/a"), Path::new("/b/c.kt")),
            ":"
        );
    }

    #[test]
    fn finds_a_main_classs_source() {
        let root = tempfile::tempdir().expect("a temporary folder");
        let package = root.path().join("src/main/kotlin/com/example");
        std::fs::create_dir_all(&package).expect("folders");
        std::fs::create_dir_all(root.path().join("build/com/example")).expect("output");
        for (file, text) in [
            ("Main.kt", "package com.example\nfun main() {}\n"),
            (
                "Tools.kt",
                "package com.example\nobject Runner { @JvmStatic fun main() {} }\n",
            ),
        ] {
            std::fs::write(package.join(file), text).expect("a file");
        }
        std::fs::write(root.path().join("build/com/example/Main.kt"), "").expect("an output");
        assert_eq!(
            source_of(root.path(), "com.example.MainKt"),
            Some(package.join("Main.kt"))
        );
        assert_eq!(
            source_of(root.path(), "com.example.Runner"),
            Some(package.join("Tools.kt"))
        );
        assert_eq!(source_of(root.path(), "com.other.MainKt"), None);
    }

    #[test]
    fn writes_the_gradle_task_and_the_exec_arguments() {
        let launch = Launch {
            name: "MainKt".into(),
            main: "com.example.MainKt".into(),
            args: vec!["one".into(), "two words".into()],
            vm_args: vec!["-Xmx64m".into()],
            gradle_project: ":".into(),
            ..Launch::default()
        };
        let script = launch.init_script();
        assert!(script.contains("tasks.register('MainKt.main()', JavaExec)"));
        assert!(script.contains("mainClass.set('com.example.MainKt')"));
        assert!(script.contains("args(['one', 'two words'])"));
        assert!(script.contains("if (path == ':')"));
        assert_eq!(
            launch.exec_args(),
            "-Xmx64m -classpath %classpath com.example.MainKt one \"two words\""
        );
        assert_eq!(
            command_line(
                "/p/mvnw",
                &[
                    "-q".into(),
                    "-Dexec.args=a b".into(),
                    ":MainKt.main()".into()
                ]
            ),
            "/p/mvnw -q '-Dexec.args=a b' ':MainKt.main()'"
        );
    }
}
