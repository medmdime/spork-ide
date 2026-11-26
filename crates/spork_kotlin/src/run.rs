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
use gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Entity, IntoElement as _, ParentElement as _,
    Styled as _, Task, div, px,
};
use gpui_kit::component::h_flex;
use gpui_kit::component::menu::PopupMenuItem;
use language::{Buffer, ContextLocation, ContextProvider, LanguageName, LanguageToolchainStore};
use serde_json::{Value, json};
use spork_java::{JavaProject, Tool};
use spork_run::configurations::{self, Configuration, Edit, Field, Runner};
use task::{
    DebugScenario, TaskTemplate, TaskTemplates, TaskVariables, VariableName, ZedDebugConfig,
};
use workspace::Workspace;

use crate::kotlin_project::KotlinProject;

/// The exec plugin that runs `java` on Maven's runtime classpath.
const EXEC_PLUGIN: &str = "org.codehaus.mojo:exec-maven-plugin:3.5.0:exec";

/// The configuration's keys of IntelliJ's Kotlin form beyond the Java parameters: "Use classpath
/// of module:" (IntelliJ's module name, `fixture.app.main`), "JRE:" (the Application's
/// `javaExec`, which the server's launch takes too) and "Shorten command line:".
const MODULE: &str = "module";
const JAVA_EXEC: &str = "javaExec";
const SHORTEN: &str = "shortenCommandLine";

/// IntelliJ's `ShortenCommandLine` keys.
const MANIFEST: &str = "MANIFEST";
const ARGS_FILE: &str = "ARGS_FILE";

/// The ways of shortening the command line, as IntelliJ's combo box lists them for a JDK 9 or
/// newer: the key, the name and the command line it makes.
// shortcut: a Java 8 JRE gets "classpath file" in IntelliJ in place of @argfile, which needs
// IntelliJ's own launcher class; list it once a Java 8 project can run
const SHORTEN_METHODS: [(&str, &str, &str); 3] = [
    ("NONE", "none", "java [options] className [args]"),
    (
        MANIFEST,
        "JAR manifest",
        "java -cp classpath.jar className [args]",
    ),
    (
        ARGS_FILE,
        "@argfile (Java 9+)",
        "java @argfile className [args]",
    ),
];

/// A module of the build as IntelliJ names it (Gradle's `fixture`, `fixture.main`,
/// `fixture.app.test`; Maven's artifact), and where it runs: its Gradle project and source set.
#[derive(Clone, Debug, PartialEq)]
struct Module {
    name: String,
    gradle_project: String,
    source_set: &'static str,
}

/// The texts quoted on a line of a build script (`include("app", ":lib")`).
fn quoted(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find(['"', '\'']) {
        let quote = rest[start..].chars().next().unwrap_or('"');
        let after = &rest[start + 1..];
        let Some(end) = after.find(quote) else { break };
        found.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    found
}

/// The build's modules, sorted by name as IntelliJ's combo box sorts them: on Gradle each
/// project (the root and the ones `settings.gradle` includes) and, where it has sources, its
/// `main` and `test` source sets; on Maven the POM's artifact.
// shortcut: Maven's own modules (`<modules>`) aren't listed, as the Maven run builds the root;
// list them once it builds one module with its dependencies
fn modules(root: &Path, tool: Tool) -> Vec<Module> {
    let mut modules = Vec::new();
    match tool {
        Tool::Maven => {
            let pom = std::fs::read_to_string(root.join("pom.xml")).unwrap_or_default();
            if let Ok(pom) = spork_java::pom::Pom::parse(&pom) {
                modules.push(Module {
                    name: pom.artifact_id,
                    gradle_project: ":".into(),
                    source_set: "main",
                });
            }
        }
        Tool::Gradle => {
            let settings = ["settings.gradle.kts", "settings.gradle"]
                .iter()
                .find_map(|name| std::fs::read_to_string(root.join(name)).ok())
                .unwrap_or_default();
            let root_name = settings
                .lines()
                .filter(|line| line.contains("rootProject.name"))
                .find_map(|line| quoted(line).into_iter().next())
                .or_else(|| {
                    root.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            let mut projects = vec![":".to_owned()];
            for line in settings.lines() {
                if line.trim_start().starts_with("include") {
                    projects.extend(
                        quoted(line)
                            .into_iter()
                            .map(|path| format!(":{}", path.trim_start_matches(':'))),
                    );
                }
            }
            for project in projects {
                let parts: Vec<&str> = project.split(':').filter(|part| !part.is_empty()).collect();
                let name = std::iter::once(root_name.as_str())
                    .chain(parts.iter().copied())
                    .collect::<Vec<_>>()
                    .join(".");
                let folder: PathBuf = parts.iter().collect();
                modules.push(Module {
                    name: name.clone(),
                    gradle_project: project.clone(),
                    source_set: "main",
                });
                if root.join(folder).join("src").is_dir() {
                    for source_set in ["main", "test"] {
                        modules.push(Module {
                            name: format!("{name}.{source_set}"),
                            gradle_project: project.clone(),
                            source_set,
                        });
                    }
                }
            }
        }
    }
    modules.sort_by_key(|module| module.name.to_lowercase());
    modules
}

/// Where a configuration runs: its module's Gradle project and source set, or, without one,
/// those of the file its main class compiles from.
fn module_of(root: &Path, tool: Tool, module: Option<&str>, main: &str) -> Module {
    if let Some(found) = module.and_then(|name| {
        modules(root, tool)
            .into_iter()
            .find(|module| module.name == name)
    }) {
        return found;
    }
    match source_of(root, main) {
        Some(file) => module_of_file(root, &file),
        None => Module {
            name: String::new(),
            gradle_project: ":".into(),
            source_set: "main",
        },
    }
}

/// The Gradle project and source set of a source file.
fn module_of_file(root: &Path, file: &Path) -> Module {
    let test = file
        .strip_prefix(root)
        .is_ok_and(|relative| relative.components().any(|part| part.as_os_str() == "test"));
    Module {
        name: String::new(),
        gradle_project: gradle_project_path(root, file),
        source_set: if test { "test" } else { "main" },
    }
}

/// The package a Kotlin file declares.
fn package_of(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let package = line.trim().strip_prefix("package ")?;
        Some(package.trim().trim_end_matches(';').trim().to_owned())
    })
}

/// The class a file's top-level functions compile into: `@file:JvmName("…")`'s, or the file's
/// name with `Kt` (`Main.kt` is `MainKt`).
pub(crate) fn facade(file_name: &str, text: &str) -> String {
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
    /// The source set whose runtime classpath it runs on (`main`, `test`).
    source_set: String,
    /// The JRE's `java`, the build's own when `None`.
    java_exec: Option<String>,
    /// IntelliJ's Shorten command line (`ShortenCommandLine`'s key), `NONE` when empty.
    shorten: String,
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
            .map(|cwd| {
                format!(
                    "                workingDir = file({})\n",
                    groovy_string(cwd)
                )
            })
            .unwrap_or_default();
        let executable = self
            .java_exec
            .as_ref()
            .map(|java| format!("                executable = {}\n", groovy_string(java)))
            .unwrap_or_default();
        // IntelliJ's init script shortens it as the task starts, when its classpath is known
        let shorten = match self.shorten.as_str() {
            MANIFEST => {
                "                doFirst {\n\
                 \x20                   def jar = new File(temporaryDir, 'classpath.jar')\n\
                 \x20                   def manifest = new java.util.jar.Manifest()\n\
                 \x20                   manifest.mainAttributes.putValue('Manifest-Version', '1.0')\n\
                 \x20                   manifest.mainAttributes.putValue('Class-Path', classpath.files.collect { it.toURI().toString() }.join(' '))\n\
                 \x20                   jar.withOutputStream { new java.util.jar.JarOutputStream(it, manifest).close() }\n\
                 \x20                   classpath = files(jar)\n\
                 \x20               }\n"
            }
            ARGS_FILE => {
                "                doFirst {\n\
                 \x20                   def argfile = new File(temporaryDir, 'argfile')\n\
                 \x20                   argfile.text = '-classpath\\n\"' + classpath.asPath.replace('\\\\', '\\\\\\\\') + '\"\\n'\n\
                 \x20                   classpath = files()\n\
                 \x20                   jvmArgs('@' + argfile.absolutePath)\n\
                 \x20               }\n"
            }
            _ => "",
        };
        format!(
            "// Spork IDE's run of {name}\n\
             allprojects {{\n\
             \x20   if (path == {project}) {{\n\
             \x20       afterEvaluate {{\n\
             \x20           tasks.register({task}, JavaExec) {{\n\
             \x20               mainClass.set({main})\n\
             \x20               classpath = project.sourceSets[{source_set}].runtimeClasspath\n\
             \x20               args({args})\n\
             \x20               jvmArgs({vm_args})\n\
             \x20               environment({env})\n\
             \x20               standardInput = System.in\n\
             {executable}\
             {cwd}\
             {shorten}\
             \x20           }}\n\
             \x20       }}\n\
             \x20   }}\n\
             }}\n",
            name = self.name,
            project = groovy_string(&self.gradle_project),
            task = groovy_string(&self.gradle_task()),
            main = groovy_string(&self.main),
            source_set = groovy_string(&self.source_set),
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
                let java = launch
                    .java_exec
                    .clone()
                    .unwrap_or_else(|| self.java.to_string_lossy().into_owned());
                let mut args = vec![
                    "-q".to_owned(),
                    "compile".into(),
                    EXEC_PLUGIN.into(),
                    format!("-Dexec.executable={java}"),
                    format!("-Dexec.args={}", launch.exec_args()),
                ];
                // the exec plugin's own shortening: the classpath in a jar's manifest
                // shortcut: @argfile shortens through the manifest too, as the exec plugin
                // writes no argument file for the classpath
                if !launch.shorten.is_empty() && launch.shorten != "NONE" {
                    args.push("-Dexec.longClasspath=true".into());
                }
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
        let entry = configuration.entry.clone();
        let name = configuration.name().to_string();
        let coverage = spork_java::coverage::prepare(configuration, &build.root, cx);
        cx.background_spawn(async move {
            let module = module_of(&build.root, build.tool, text(&entry, MODULE), &main);
            let coverage = match coverage {
                Some(coverage) => Some(coverage.await?),
                None => None,
            };
            let mut vm_args = arguments(&entry, "vmArgs");
            if let Some(coverage) = &coverage {
                vm_args.splice(0..0, coverage.vm_options.iter().cloned());
            }
            let launch = Launch {
                name,
                main,
                args: arguments(&entry, "args"),
                vm_args,
                cwd: text(&entry, "cwd").map(str::to_owned),
                env: environment(&entry),
                gradle_project: module.gradle_project,
                source_set: module.source_set.into(),
                java_exec: text(&entry, JAVA_EXEC).map(str::to_owned),
                shorten: text(&entry, SHORTEN).unwrap_or_default().to_owned(),
            };
            let allow_concurrent_runs = entry
                .get("allow_concurrent_runs")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut template = build.task(launch, allow_concurrent_runs)?;
            if let Some(coverage) = coverage {
                template.env.extend([coverage.env()]);
            }
            Ok(template)
        })
    }

    fn can_cover(&self) -> bool {
        true
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

    /// IntelliJ's Kotlin form, after the Java parameters: "Use classpath of module:", "JRE:" and
    /// "Shorten command line:".
    fn controls(&self, configuration: &Configuration, edit: Edit, cx: &mut App) -> Vec<AnyElement> {
        let entry = &configuration.entry;
        let module = text(entry, MODULE).map(str::to_owned);
        // the project of its working directory, or the window's
        let kotlin = text(entry, "cwd")
            .and_then(|cwd| KotlinProject::containing(Path::new(cwd), cx))
            .or_else(|| KotlinProject::first(cx));
        let modules: Vec<String> = kotlin
            .and_then(|kotlin| {
                let kotlin = kotlin.read(cx);
                Some((kotlin.root(cx)?, kotlin.tool(cx)?))
            })
            .map(|(root, tool)| {
                modules(&root, tool)
                    .into_iter()
                    .map(|module| module.name)
                    .collect()
            })
            .unwrap_or_default();
        vec![
            module_field(module.clone(), modules, edit.clone()),
            form_row(
                "JRE:",
                spork_java::jre_combo(configuration, module, edit.clone(), cx),
            ),
            shorten_field(text(entry, SHORTEN).unwrap_or("NONE"), edit),
        ]
    }

    fn controls_last(&self) -> bool {
        true
    }
}

fn text<'a>(entry: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a str> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// A row of the form: its label in the label column, then the control.
fn form_row(label: &'static str, control: AnyElement) -> AnyElement {
    h_flex()
        .gap(px(8.))
        .child(
            div()
                .w(px(configurations::KOTLIN_LABEL_WIDTH))
                .flex_none()
                .child(spork_workbench::controls::form_label(label)),
        )
        .child(div().flex_1().min_w_0().child(control))
        .into_any_element()
}

/// Sets a key of the configuration the form edits, or removes it with `None`.
fn set_key(edit: &Edit, key: &'static str, value: Option<String>, cx: &mut App) {
    edit(
        &move |configuration: &mut Configuration| match &value {
            Some(value) => {
                configuration
                    .entry
                    .insert(key.into(), Value::String(value.clone()));
            }
            None => {
                configuration.entry.remove(key);
            }
        },
        cx,
    )
}

/// "Use classpath of module:" (IntelliJ's `ModuleDescriptionsComboBox`): `<no module>`, then the
/// build's modules by name.
fn module_field(module: Option<String>, modules: Vec<String>, edit: Edit) -> AnyElement {
    // `JavaPsiBundle`'s `list.item.no.module`
    const NO_MODULE: &str = "<no module>";
    let label = "Use classpath of module:";
    let combo = spork_workbench::controls::dropdown(
        spork_workbench::controls::ComboBox::new(
            "kotlin-module",
            module.unwrap_or_else(|| NO_MODULE.to_owned()),
        )
        .aria_label(label)
        .w_full(),
        false,
        move |menu, _, _| {
            let menu = menu.item(PopupMenuItem::new(NO_MODULE).on_click({
                let edit = edit.clone();
                move |_, _, cx| set_key(&edit, MODULE, None, cx)
            }));
            modules.iter().fold(menu, |menu, name| {
                let (edit, name) = (edit.clone(), name.clone());
                menu.item(
                    PopupMenuItem::new(name.clone())
                        .on_click(move |_, _, cx| set_key(&edit, MODULE, Some(name.clone()), cx)),
                )
            })
        },
    );
    form_row(label, combo)
}

/// "Shorten command line:" (IntelliJ's `ShortenCommandLineModeCombo`): each way's name, its
/// command line greyed after it.
fn shorten_field(current: &str, edit: Edit) -> AnyElement {
    let label = "Shorten command line:";
    let (_, name, line) = SHORTEN_METHODS
        .iter()
        .find(|(key, _, _)| *key == current)
        .unwrap_or(&SHORTEN_METHODS[0]);
    let combo = spork_workbench::controls::dropdown(
        spork_workbench::controls::ComboBox::new("kotlin-shorten", *name)
            .aria_label(label)
            .detail(format!("- {line}"))
            .w_full(),
        false,
        move |menu, _, _| {
            SHORTEN_METHODS
                .iter()
                .fold(menu, |menu, (key, name, line)| {
                    let edit = edit.clone();
                    // none is the default, which the configuration doesn't store
                    let value = (*key != "NONE").then(|| key.to_string());
                    menu.item(
                        PopupMenuItem::new(format!("{name} - {line}"))
                            .on_click(move |_, _, cx| set_key(&edit, SHORTEN, value.clone(), cx)),
                    )
                })
        },
    );
    form_row(label, combo)
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
        // a test's Debug: the tests run with their JVM waiting, then the debugger attaches
        if let Some(entry) = configuration
            .get(configurations::TEST_CONFIGURATION)
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            return attach_to_tests(&server, &root, &entry, configuration, cx).await;
        }
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
        object.insert("mainClass".into(), main.clone().into());
        match tool {
            Some(Tool::Gradle) => {
                // its module's Gradle project and source set
                let module = match object.get(MODULE).and_then(Value::as_str) {
                    Some(name) => module_of(&root, Tool::Gradle, Some(name), &main),
                    None => object
                        .get("file")
                        .and_then(Value::as_str)
                        .map(|file| module_of_file(&root, Path::new(file)))
                        .unwrap_or_else(|| module_of(&root, Tool::Gradle, None, &main)),
                };
                let folder: PathBuf = module
                    .gradle_project
                    .split(':')
                    .filter(|part| !part.is_empty())
                    .collect();
                object.insert("tool".into(), "gradle".into());
                object
                    .entry("projectPath")
                    .or_insert_with(|| root.join(folder).to_string_lossy().into_owned().into());
                object
                    .entry("sourceSet")
                    .or_insert_with(|| module.source_set.into());
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

/// A test's Debug (IntelliJ's: the tests run by the project's tool, the debugger attached to the
/// test JVM): the configuration run with its JVM waiting, then the server's debugger attached
/// once it listens.
async fn attach_to_tests(
    server: &Arc<lsp::LanguageServer>,
    root: &Path,
    entry: &str,
    mut configuration: Value,
    cx: &mut AsyncApp,
) -> Result<DebugAdapterBinary> {
    let entry: serde_json::Map<String, Value> =
        serde_json::from_str(entry).context("the test configuration")?;
    let test = Configuration::from_entry(entry);
    let workspace = cx.update(|cx| {
        let java = JavaProject::containing(root, cx).context("The project isn't a Java project")?;
        java.read(cx)
            .workspace
            .upgrade()
            .context("The project's window is closed")
    })?;
    let jvm_port =
        spork_run::run_for_debugger(&workspace, test, configurations::KOTLIN, cx).await?;
    let root_uri = lsp::Uri::from_file_path(root)
        .map_err(|()| anyhow::anyhow!("{root:?} isn't a file path"))?
        .to_string();
    let port =
        crate::kotlin_project::execute_command(server, "start_debug_server", vec![root_uri.into()])
            .await?
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .context("The Kotlin language server opened no debug port")?;
    let object = configuration
        .as_object_mut()
        .context("a Kotlin debug configuration is a JSON object")?;
    object.remove(configurations::TEST_CONFIGURATION);
    object.insert("request".into(), "attach".into());
    object.insert("hostName".into(), "127.0.0.1".into());
    object.insert("port".into(), jvm_port.into());
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

/// A Gradle script's task: its configuration's name and command line.
const GRADLE_NAME: &str = "SPORK_GRADLE_NAME";
const GRADLE_COMMAND: &str = "SPORK_GRADLE_COMMAND";

/// The variables that run the task a Gradle Kotlin script declares: `fixture [hello]`, by the
/// project's wrapper, in the script's project (`:app:hello` in a subproject's).
fn gradle_task_variables(
    task: &str,
    script: &Path,
    root: Option<&Path>,
    cx: &App,
) -> Option<TaskVariables> {
    if !script.to_string_lossy().ends_with(".gradle.kts") {
        return None;
    }
    let java = JavaProject::containing(root?, cx)?;
    let java = java.read(cx);
    let (root, wrapper) = (java.root.clone()?, java.wrapper()?);
    let path = match gradle_project_path(&root, script).as_str() {
        ":" => task.to_owned(),
        project => format!("{project}:{task}"),
    };
    let args = java.tool_args(std::slice::from_ref(&path));
    let mut variables = TaskVariables::from_iter([
        (
            VariableName::Custom(GRADLE_NAME.into()),
            format!("{} [{path}]", java.name()),
        ),
        (
            VariableName::Custom(GRADLE_COMMAND.into()),
            command_line(&wrapper.to_string_lossy(), &args),
        ),
    ]);
    let env = java.tool_env();
    for (name, variable) in ENV {
        if let Some(value) = env.get(name) {
            variables.insert(VariableName::Custom(variable.into()), value.clone());
        }
    }
    Some(variables)
}

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
        if let Some(task) = variables.get(&VariableName::Custom("kotlin_gradle_task".into())) {
            return Task::ready(Ok(gradle_task_variables(
                task,
                &file,
                location.worktree_root.as_deref(),
                cx,
            )
            .unwrap_or_default()));
        }
        let text = location.file_location.buffer.read(cx).text();
        // a test class or function: the JUnit or TestNG configuration its run makes
        let custom = |name: &str| variables.get(&VariableName::Custom(name.to_owned().into()));
        if let Some(class) = custom("kotlin_test_class") {
            let class = qualified(custom("kotlin_package_name"), class);
            return Task::ready(Ok(spork_java::test_variables(
                spork_java::testing::configuration_type(&text),
                &class,
                custom("kotlin_test_method"),
            )));
        }
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
        let module = module_of_file(&build.root, &file);
        let launch = Launch {
            name: class,
            main: main.clone(),
            gradle_project: module.gradle_project,
            source_set: module.source_set.into(),
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
        Task::ready(Some(TaskTemplates(vec![
            spork_java::test_template(&["kotlin-test-class", "kotlin-test-method"]),
            // a Gradle script's task, run by the project's wrapper as IntelliJ's Gradle
            // configuration `fixture [hello]`
            TaskTemplate {
                label: format!("${{ZED_CUSTOM_{GRADLE_NAME}}}"),
                command: format!("${{ZED_CUSTOM_{GRADLE_COMMAND}}}"),
                env: ENV
                    .iter()
                    .map(|(name, variable)| {
                        (name.to_string(), format!("${{ZED_CUSTOM_{variable}}}"))
                    })
                    .collect(),
                cwd: Some("$ZED_WORKTREE_ROOT".into()),
                tags: vec!["gradle-task".into()],
                ..TaskTemplate::default()
            },
            TaskTemplate {
                // IntelliJ names the configuration it makes after the class
                label: format!("${{ZED_CUSTOM_{NAME}}}"),
                command: format!("${{ZED_CUSTOM_{COMMAND}}}"),
                // and the configuration Run 'MainKt' with Coverage makes (`spork_run`'s gutter)
                env: ENV
                    .iter()
                    .map(|(name, variable)| {
                        (name.to_string(), format!("${{ZED_CUSTOM_{variable}}}"))
                    })
                    .chain([
                        (
                            configurations::GUTTER_MAIN_ENV.to_owned(),
                            format!("${{ZED_CUSTOM_{MAIN}}}"),
                        ),
                        (
                            configurations::GUTTER_TYPE_ENV.to_owned(),
                            configurations::KOTLIN.to_owned(),
                        ),
                    ])
                    .collect(),
                cwd: Some("$ZED_WORKTREE_ROOT".into()),
                tags: vec!["kotlin-main".into()],
                ..TaskTemplate::default()
            },
        ])))
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
        if adapter.0.as_ref() != configurations::KOTLIN {
            return None;
        }
        if build.env.contains_key(configurations::CONFIGURATION_ENV) {
            return Some(DebugScenario {
                adapter: adapter.0.clone(),
                label: resolved_label.to_owned().into(),
                build: None,
                config: json!({
                    "request": "attach",
                    configurations::TEST_CONFIGURATION: format!("${{ZED_CUSTOM_{}}}", spork_java::TEST_CONFIGURATION),
                }),
                tcp_connection: None,
            });
        }
        if !build.tags.iter().any(|tag| tag == "kotlin-main") {
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
        Launch, command_line, facade, gradle_project_path, module_of, module_of_file, modules,
        package_of, qualified, source_of,
    };
    use spork_java::Tool;
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
    fn lists_the_builds_modules_as_intellij_names_them() {
        let root = tempfile::tempdir().expect("a temporary folder");
        std::fs::write(
            root.path().join("settings.gradle.kts"),
            "rootProject.name = \"fixture\"\ninclude(\"app\", \":lib:core\")\n",
        )
        .expect("settings");
        std::fs::create_dir_all(root.path().join("app/src/main/kotlin")).expect("app");
        let names: Vec<String> = modules(root.path(), Tool::Gradle)
            .into_iter()
            .map(|module| module.name)
            .collect();
        assert_eq!(
            names,
            [
                "fixture",
                "fixture.app",
                "fixture.app.main",
                "fixture.app.test",
                "fixture.lib.core"
            ]
        );
        let app = module_of(root.path(), Tool::Gradle, Some("fixture.app.test"), "AppKt");
        assert_eq!(
            (app.gradle_project.as_str(), app.source_set),
            (":app", "test")
        );
        let file = root.path().join("app/src/main/kotlin/App.kt");
        std::fs::write(root.path().join("app/build.gradle.kts"), "").expect("app script");
        let app = module_of_file(root.path(), &file);
        assert_eq!(
            (app.gradle_project.as_str(), app.source_set),
            (":app", "main")
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
            source_set: "main".into(),
            ..Launch::default()
        };
        let script = launch.init_script();
        assert!(script.contains("classpath = project.sourceSets['main'].runtimeClasspath"));
        assert!(!script.contains("executable") && !script.contains("doFirst"));
        let shortened = Launch {
            java_exec: Some("/jdk/bin/java".into()),
            shorten: "MANIFEST".into(),
            ..launch.clone()
        }
        .init_script();
        assert!(shortened.contains("executable = '/jdk/bin/java'"));
        assert!(shortened.contains("classpath = files(jar)"));
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
