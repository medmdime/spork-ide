//! Run configurations: Zed's tasks (`tasks.json`) and debug scenarios (`debug.json`) as IntelliJ's
//! run configurations. A task is a Shell Script configuration, a debug scenario one of its debug
//! adapter's type. "Store as project file" keeps a configuration in the project's `.zed` folder,
//! otherwise in the user's files; the dialog writes them back there.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use fs::Fs;
use gpui::SharedString;
use serde_json::{Map, Value};

/// The configuration types Spork IDE runs, by the debug adapter backing them; a task's is
/// `SHELL_SCRIPT`.
pub const SHELL_SCRIPT: &str = "Shell Script";

/// Java's debug adapter (`spork_java`'s), whose configurations are IntelliJ's Application ones.
pub const JAVA: &str = "Java";

/// IntelliJ's Remote JVM Debug: Java's debugger attached to a JVM's JDWP agent at a host and
/// port, its sessions Java's.
pub const REMOTE_JVM_DEBUG: &str = "Remote JVM Debug";

/// IntelliJ's JUnit: a test class run by JUnit 4's runner (`org.junit.runner.JUnitCore`) on the
/// project's test classpath, debugged by Java's debugger.
pub const JUNIT: &str = "JUnit";

/// JUnit 4's console runner, the main class a JUnit configuration runs.
pub const JUNIT_RUNNER: &str = "org.junit.runner.JUnitCore";
/// Kotlin's debug adapter (`spork_kotlin`'s), whose configurations are IntelliJ's Kotlin ones.
pub const KOTLIN: &str = "Kotlin";

/// The Spring Boot type (`spork_java`'s): an Application with IntelliJ's Spring options.
pub const SPRING_BOOT: &str = "Spring Boot";

/// A Spring Boot configuration's Override configuration properties: `[{"name", "value"}]`, each
/// a `-Dname=value` option.
pub const OVERRIDE_PROPERTIES: &str = "properties";

/// The Override configuration properties of a configuration, each name with its value.
pub fn override_properties(configuration: &Configuration) -> Vec<(String, String)> {
    let Some(Value::Array(rows)) = configuration.entry.get(OVERRIDE_PROPERTIES) else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| {
            let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
            (text("name").to_owned(), text("value").to_owned())
        })
        .collect()
}

/// Sets the Override configuration properties, kept while the option is on even when empty.
pub fn set_override_properties(configuration: &mut Configuration, rows: &[(String, String)]) {
    let rows = rows
        .iter()
        .map(|(name, value)| serde_json::json!({"name": name, "value": value}))
        .collect();
    configuration
        .entry
        .insert(OVERRIDE_PROPERTIES.into(), Value::Array(rows));
}

/// The path a configuration's Services link opens after its port (IntelliJ's `URL_PATH`).
pub const URL_PATH: &str = "urlPath";

/// Whether a type runs a Java main class: the Application and Spring Boot types.
pub fn is_java(adapter: Option<&str>) -> bool {
    matches!(adapter, Some(JAVA | SPRING_BOOT))
}

/// IntelliJ's name and icon for a configuration type: Shell Script for tasks, the debug
/// adapter's language family otherwise.
pub fn type_name(adapter: Option<&str>) -> SharedString {
    match adapter {
        None => SHELL_SCRIPT.into(),
        Some("CodeLLDB") => "Native Application".into(),
        Some("Debugpy") => "Python".into(),
        Some("JavaScript") => "Node.js".into(),
        Some("Delve") => "Go Build".into(),
        Some("GDB") => "GDB".into(),
        Some(JAVA) => "Application".into(),
        Some(other) => other.to_owned().into(),
    }
}

pub fn type_icon(adapter: Option<&str>) -> &'static str {
    match adapter {
        None => "fileTypes/shell",
        Some("Debugpy") => "fileTypes/python",
        Some("JavaScript") => "fileTypes/javaScript",
        Some(REMOTE_JVM_DEBUG) => "runConfigurations/remoteDebug",
        Some(JUNIT) => "runConfigurations/junit",
        Some(KOTLIN) => "fileTypes/kotlin",
        Some(SPRING_BOOT) => "runConfigurations/springBoot",
        Some(_) => "runConfigurations/application",
    }
}

/// A configuration type that also runs without its debugger: Java's Application, which
/// `spork_java` runs as the `java` command line it builds.
pub trait Runner {
    /// The task that runs the configuration, once what it needs is ready (the build, the
    /// classpath).
    fn task(
        &self,
        configuration: &Configuration,
        workspace: &workspace::Workspace,
        cx: &mut gpui::App,
    ) -> gpui::Task<Result<task::TaskTemplate>>;

    /// Whether its debugger can debug it now.
    fn can_debug(&self, cx: &gpui::App) -> bool;

    /// The type's Modify options (IntelliJ's settings fragments), by group; none by default.
    fn modify_options(&self, _configuration: &Configuration) -> Vec<ModifyOption> {
        Vec::new()
    }

    /// Whether the form shows `field` for the configuration (a fragment Modify options adds).
    fn shows(&self, _configuration: &Configuration, _field: Field) -> bool {
        true
    }

    /// A view its run shows beside the console, with its tab's title (Spring Boot's Actuator).
    fn content(
        &self,
        _configuration: &Configuration,
        _workspace: &gpui::Entity<workspace::Workspace>,
        _window: &mut gpui::Window,
        _cx: &mut gpui::App,
    ) -> Option<(SharedString, gpui::AnyView)> {
        None
    }

    /// Whether its Services node has Edit Default Path: the path its link opens
    /// ([`URL_PATH`]).
    fn has_default_path(&self) -> bool {
        false
    }

    /// What a running configuration's node shows in Services beyond its status, from its
    /// console's text: still starting, a link (Spring Boot's `:8080/`).
    fn service(&self, _configuration: &Configuration, _console: &str) -> Option<ServiceState> {
        None
    }

    /// The type's own rows of the form, after Build and run's title (Application's JRE); `edit`
    /// changes the configuration the form edits.
    fn controls(
        &self,
        _configuration: &Configuration,
        _edit: Edit,
        _cx: &mut gpui::App,
    ) -> Vec<gpui::AnyElement> {
        Vec::new()
    }
}

/// A running service as its type sees it: starting, and the link its node shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServiceState {
    pub starting: bool,
    /// The link's text and the URL it opens.
    pub link: Option<(SharedString, String)>,
}

/// A change to the configuration a form edits.
pub type Edit = std::rc::Rc<dyn Fn(&dyn Fn(&mut Configuration), &mut gpui::App)>;

/// An item of a configuration type's Modify options: its group ("Java"), its text, whether it is
/// on, and what turning it on or off does to the configuration; greyed without one.
pub struct ModifyOption {
    pub group: &'static str,
    pub label: &'static str,
    pub checked: bool,
    pub toggle: Option<fn(&mut Configuration)>,
}

/// The runners, by the debug adapter of the configurations they run.
#[derive(Default)]
struct Runners(collections::HashMap<String, std::rc::Rc<dyn Runner>>);

impl gpui::Global for Runners {}

/// Lets the configurations of a debug adapter run without debugging.
pub fn register_runner(adapter: &str, runner: std::rc::Rc<dyn Runner>, cx: &mut gpui::App) {
    cx.default_global::<Runners>()
        .0
        .insert(adapter.to_owned(), runner);
}

/// The runner of a configuration's type, when it has one.
pub fn runner(configuration: &Configuration, cx: &gpui::App) -> Option<std::rc::Rc<dyn Runner>> {
    cx.try_global::<Runners>()?
        .0
        .get(configuration.adapter.as_deref()?)
        .cloned()
}

/// A run configuration: its entry in a tasks.json or debug.json, the keys Spork IDE doesn't edit
/// kept as written.
#[derive(Clone, Debug, PartialEq)]
pub struct Configuration {
    /// The debug adapter of a debug scenario; `None` for a task.
    pub adapter: Option<SharedString>,
    /// Stored as a project file (`.zed/`), or in the user's files.
    pub shared: bool,
    pub entry: Map<String, Value>,
}

/// The fields the dialog edits, as IntelliJ's forms show them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    /// A task's command, a debug scenario's program.
    Command,
    Arguments,
    WorkingDirectory,
    Environment,
    /// A task's "Allow multiple instances".
    AllowMultiple,
    /// A debug scenario's task to run first (IntelliJ's Before launch).
    BeforeLaunch,
    /// An Application's options for the JVM.
    VmOptions,
    /// A Remote JVM Debug's host and port.
    Host,
    Port,
    /// A Spring Boot configuration's Active profiles.
    ActiveProfiles,
}

impl Configuration {
    pub fn new_task(name: &str) -> Self {
        let mut entry = Map::new();
        entry.insert("label".into(), name.into());
        entry.insert("command".into(), "".into());
        Self {
            adapter: None,
            shared: true,
            entry,
        }
    }

    pub fn new_debug(adapter: &str, name: &str) -> Self {
        let mut entry = Map::new();
        entry.insert("label".into(), name.into());
        entry.insert("adapter".into(), adapter.into());
        if adapter == REMOTE_JVM_DEBUG {
            // IntelliJ's `RemoteConnection` defaults
            entry.insert("request".into(), "attach".into());
            entry.insert("hostName".into(), "localhost".into());
            entry.insert("port".into(), 5005.into());
            return Self {
                adapter: Some(adapter.to_owned().into()),
                shared: true,
                entry,
            };
        }
        entry.insert("request".into(), "launch".into());
        let mut configuration = Self {
            adapter: Some(adapter.to_owned().into()),
            shared: true,
            entry,
        };
        let program = configuration.key(Field::Command);
        configuration.entry.insert(program.into(), "".into());
        configuration
    }

    pub fn name(&self) -> SharedString {
        self.text(Field::Name).into()
    }

    pub fn type_name(&self) -> SharedString {
        type_name(self.adapter.as_deref())
    }

    pub fn icon(&self) -> &'static str {
        type_icon(self.adapter.as_deref())
    }

    pub fn is_task(&self) -> bool {
        self.adapter.is_none()
    }

    fn key(&self, field: Field) -> &'static str {
        match (field, self.is_task()) {
            (Field::Name, _) => "label",
            (Field::Command, true) => "command",
            // java-debug's and the Kotlin server's launch arguments: the main class and VM options
            (Field::Command, false)
                if is_java(self.adapter.as_deref()) || self.adapter.as_deref() == Some(KOTLIN) =>
            {
                "mainClass"
            }
            (Field::Command, false) if self.adapter.as_deref() == Some(JUNIT) => "testClass",
            (Field::VmOptions, _) => "vmArgs",
            (Field::ActiveProfiles, _) => "activeProfiles",
            (Field::Command, false) => "program",
            (Field::Arguments, _) => "args",
            (Field::WorkingDirectory, _) => "cwd",
            (Field::Environment, _) => "env",
            (Field::AllowMultiple, _) => "allow_concurrent_runs",
            (Field::BeforeLaunch, _) => "build",
            (Field::Host, _) => "hostName",
            (Field::Port, _) => "port",
        }
    }

    /// A field as its text box shows it: arguments separated by spaces, the environment as
    /// IntelliJ writes it (`A=1;B=2`), a flag as "true" or "".
    pub fn text(&self, field: Field) -> String {
        match self.entry.get(self.key(field)) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(quote_argument)
                .collect::<Vec<_>>()
                .join(" "),
            Some(Value::Object(map)) if field == Field::Environment => map
                .iter()
                .map(|(name, value)| format!("{name}={}", value.as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(";"),
            // a build task given in full rather than by its label
            Some(Value::Object(map)) => map
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Some(Value::Bool(true)) => "true".into(),
            Some(Value::Number(number)) => number.to_string(),
            _ => String::new(),
        }
    }

    /// Sets a field from its text; an empty optional field leaves the entry.
    pub fn set_text(&mut self, field: Field, text: &str) {
        let key = self.key(field);
        let value = match field {
            Field::Name => Some(Value::from(text)),
            Field::Command if self.is_task() => Some(Value::from(text)),
            Field::Arguments => {
                (!text.trim().is_empty()).then(|| Value::from(split_arguments(text)))
            }
            Field::Environment => (!text.trim().is_empty()).then(|| {
                Value::Object(
                    text.split(';')
                        .filter_map(|pair| {
                            let (name, value) = pair.split_once('=')?;
                            let name = name.trim();
                            (!name.is_empty()).then(|| (name.to_owned(), Value::from(value)))
                        })
                        .collect(),
                )
            }),
            Field::AllowMultiple => (text == "true").then_some(Value::Bool(true)),
            // a number when it is one, as java-debug reads it; the text otherwise, so the form
            // keeps what was typed
            Field::Port => Some(match text.trim().parse::<u16>() {
                Ok(port) => Value::from(port),
                Err(_) => Value::from(text),
            }),
            Field::Host => Some(Value::from(text)),
            Field::Command
            | Field::WorkingDirectory
            | Field::BeforeLaunch
            | Field::VmOptions
            | Field::ActiveProfiles => (!text.trim().is_empty()).then(|| Value::from(text)),
        };
        match value {
            Some(value) => {
                self.entry.insert(key.into(), value);
            }
            None => {
                self.entry.remove(key);
            }
        }
    }

    /// The task Zed runs for it, when it is a task.
    pub fn task_template(&self) -> Option<task::TaskTemplate> {
        if !self.is_task() {
            return None;
        }
        serde_json::from_value(Value::Object(self.entry.clone())).ok()
    }

    /// The debug scenario Zed starts for it, when it is one; a Remote JVM Debug's attaches
    /// with Java's debugger.
    pub fn debug_scenario(&self) -> Option<task::DebugScenario> {
        if self.is_task() {
            return None;
        }
        let mut entry = self.entry.clone();
        if self.adapter.as_deref() == Some(REMOTE_JVM_DEBUG) {
            entry.insert("adapter".into(), JAVA.into());
            entry.insert("request".into(), "attach".into());
        }
        // JUnit's runner runs the class on the test classpath
        if self.adapter.as_deref() == Some(JUNIT) {
            let class = entry.remove("testClass").unwrap_or_default();
            entry.insert("adapter".into(), JAVA.into());
            entry.insert("mainClass".into(), JUNIT_RUNNER.into());
            entry.insert("args".into(), Value::Array(vec![class]));
            entry.insert("classpathScope".into(), "test".into());
        }
        serde_json::from_value(Value::Object(entry)).ok()
    }
}

/// An argument as typed in a text box: quoted when it holds a space.
fn quote_argument(argument: &str) -> String {
    match argument.contains(char::is_whitespace) || argument.is_empty() {
        true => format!("\"{argument}\""),
        false => argument.to_owned(),
    }
}

/// Splits a line of arguments at spaces, keeping what double quotes enclose together.
pub fn split_arguments(text: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let (mut quoted, mut started) = (false, false);
    for character in text.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    arguments.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            character => {
                current.push(character);
                started = true;
            }
        }
    }
    if started {
        arguments.push(current);
    }
    arguments
}

/// Where a project's configurations are kept: its first folder's `.zed` files and the user's.
#[derive(Clone, Debug, PartialEq)]
pub struct Files {
    pub project_tasks: Option<PathBuf>,
    pub project_debug: Option<PathBuf>,
    pub user_tasks: PathBuf,
    pub user_debug: PathBuf,
}

impl Files {
    pub fn new(root: Option<&Path>) -> Self {
        let local =
            |relative: &util::rel_path::RelPath| root.map(|root| root.join(relative.as_std_path()));
        Self {
            project_tasks: local(paths::local_tasks_file_relative_path()),
            project_debug: local(paths::local_debug_file_relative_path()),
            user_tasks: paths::tasks_file().clone(),
            user_debug: paths::debug_scenarios_file().clone(),
        }
    }

    fn file(&self, task: bool, shared: bool) -> Option<&PathBuf> {
        match (task, shared) {
            (true, true) => self.project_tasks.as_ref(),
            (false, true) => self.project_debug.as_ref(),
            (true, false) => Some(&self.user_tasks),
            (false, false) => Some(&self.user_debug),
        }
    }
}

/// Reads the configurations of the project's and the user's files, the project's first, each
/// file's in its order.
pub async fn load(fs: Arc<dyn Fs>, files: Files) -> Vec<Configuration> {
    let mut configurations = Vec::new();
    for (task, shared) in [(true, true), (false, true), (true, false), (false, false)] {
        let Some(path) = files.file(task, shared) else {
            continue;
        };
        let Ok(text) = fs.load(path).await else {
            continue;
        };
        let entries: Vec<Value> = match settings::parse_json_with_comments(&text) {
            Ok(entries) => entries,
            Err(error) => {
                log::warn!("reading run configurations from {path:?}: {error:#}");
                continue;
            }
        };
        configurations.extend(entries.into_iter().filter_map(|entry| {
            let Value::Object(entry) = entry else {
                return None;
            };
            let adapter = match task {
                true => None,
                false => Some(entry.get("adapter")?.as_str()?.to_owned().into()),
            };
            Some(Configuration {
                adapter,
                shared,
                entry,
            })
        }));
    }
    configurations
}

/// Writes the configurations back to their files, each file rewritten only when what it lists
/// changed.
// shortcut: a rewritten tasks.json or debug.json loses its comments; edit the JSON text in
// place (as `settings::update_value_in_json_text` does) once people keep comments in them
pub async fn save(
    fs: Arc<dyn Fs>,
    files: Files,
    before: Vec<Configuration>,
    after: Vec<Configuration>,
) -> Result<()> {
    for (task, shared) in [(true, true), (false, true), (true, false), (false, false)] {
        let Some(path) = files.file(task, shared) else {
            continue;
        };
        let entries = |configurations: &[Configuration]| -> Vec<Value> {
            configurations
                .iter()
                .filter(|c| c.is_task() == task && c.shared == shared)
                .map(|c| Value::Object(c.entry.clone()))
                .collect()
        };
        let (old, new) = (entries(&before), entries(&after));
        if old == new {
            continue;
        }
        let text = serde_json::to_string_pretty(&new)?;
        if let Some(folder) = path.parent() {
            fs.create_dir(folder)
                .await
                .with_context(|| format!("creating {folder:?}"))?;
        }
        fs.atomic_write(path.clone(), format!("{text}\n"))
            .await
            .with_context(|| format!("writing {path:?}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Configuration, Field, split_arguments};

    #[test]
    fn fields_round_trip_through_their_text() {
        let mut configuration = Configuration::new_task("hello");
        configuration.set_text(Field::Arguments, "a \"b c\" d");
        assert_eq!(
            configuration.entry["args"],
            serde_json::json!(["a", "b c", "d"])
        );
        assert_eq!(configuration.text(Field::Arguments), "a \"b c\" d");
        configuration.set_text(Field::Environment, "A=1; B=two");
        assert_eq!(configuration.text(Field::Environment), "A=1;B=two");
        configuration.set_text(Field::Environment, " ");
        assert!(!configuration.entry.contains_key("env"));
        configuration.set_text(Field::AllowMultiple, "true");
        assert_eq!(configuration.text(Field::AllowMultiple), "true");
        assert_eq!(split_arguments("  "), Vec::<String>::new());
        assert_eq!(split_arguments("\"\""), vec![String::new()]);
    }

    #[test]
    fn a_debug_scenario_names_its_program() {
        let mut configuration = Configuration::new_debug("CodeLLDB", "app");
        configuration.set_text(Field::Command, "target/debug/app");
        assert_eq!(configuration.entry["program"], "target/debug/app");
        assert!(configuration.debug_scenario().is_some());
        assert!(configuration.task_template().is_none());
    }
}
