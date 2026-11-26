//! Run configurations: Zed's tasks (`tasks.json`) and debug scenarios (`debug.json`) as IntelliJ's
//! run configurations. A task is a Shell Script configuration, a debug scenario one of its debug
//! adapter's type. "Store as project file" keeps a configuration in the project's `.zed` folder,
//! otherwise in the IDE's own files of the project (IntelliJ's `workspace.xml`, where it keeps
//! the configurations it makes); the dialog writes them back there.

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

/// IntelliJ's JUnit: the tests of a class, a method, a package or a pattern, run by Spork IDE's
/// test runner (`spork_java`'s) with the project's framework on its test classpath, or by Gradle,
/// and debugged by Java's debugger.
pub const JUNIT: &str = "JUnit";

/// IntelliJ's TestNG, run and debugged as JUnit is.
pub const TESTNG: &str = "TestNG";

/// The main class a test configuration runs: Spork IDE's test runner, its arguments the tests
/// ([`test_selectors`]).
pub const TEST_RUNNER: &str = "spork.tests.TestRunner";

/// A configuration that runs tests: JUnit's or TestNG's.
pub fn is_test(adapter: Option<&str>) -> bool {
    matches!(adapter, Some(JUNIT | TESTNG))
}

/// What a test configuration runs (IntelliJ's Test kind), by the key its entry keeps it under,
/// and its text in the form; the kinds of `adapter`'s form.
pub fn test_kinds(adapter: Option<&str>) -> &'static [(&'static str, &'static str)] {
    match adapter {
        // TestNG's `TestType`
        Some(TESTNG) => &[
            ("package", "All in package"),
            ("class", "Class"),
            ("method", "Method"),
            ("group", "Group"),
            ("suite", "Suite"),
            ("pattern", "Pattern"),
        ],
        // `JUnitConfigurationModel.reloadTestKindModel`'s order
        _ => &[
            ("package", "All in package"),
            ("directory", "All in directory"),
            ("pattern", "Pattern"),
            ("class", "Class"),
            ("method", "Method"),
            ("category", "Category"),
            ("uniqueId", "UniqueId"),
            ("tags", "Tags"),
        ],
    }
}

/// The key of a test configuration's kind (`class` by default).
pub const TEST_KIND: &str = "testKind";

/// A JUnit configuration's fork mode (IntelliJ's Modify options ▸ Tests ▸ Fork mode): `none`,
/// `method` or `class`, each test method or class in a JVM of its own.
pub const FORK_MODE: &str = "forkMode";

/// The fork modes, by the key the entry keeps them under, and their texts.
pub const FORK_MODES: [(&str, &str); 3] =
    [("none", "None"), ("method", "Method"), ("class", "Class")];

/// The tests Rerun Failed Tests runs again, each `com.example.GreetingTest#greets`, over the
/// configuration's kind.
pub const RERUN_TESTS: &str = "rerunTests";

/// The fields a test configuration's kind takes, as its form shows them.
pub fn test_kind_fields(kind: &str) -> &'static [Field] {
    match kind {
        "method" => &[Field::Command, Field::TestMethod],
        "package" => &[Field::TestPackage],
        "pattern" => &[Field::TestPattern],
        "directory" => &[Field::TestDirectory],
        "category" => &[Field::TestCategory],
        "uniqueId" => &[Field::TestUniqueId],
        "tags" => &[Field::TestTags],
        "group" => &[Field::TestGroup],
        "suite" => &[Field::TestSuite],
        _ => &[Field::Command],
    }
}

/// A test configuration's kind: `class`, `method`, `package`, `pattern`, `directory`, `category`,
/// `uniqueId`, `tags`, or TestNG's `group` and `suite`.
pub fn test_kind(configuration: &Configuration) -> &str {
    configuration
        .entry
        .get(TEST_KIND)
        .and_then(Value::as_str)
        .unwrap_or("class")
}

/// The tests a test configuration runs, as the test runner takes them: `class:<class>`,
/// `method:<class>#<method>`, `package:<package>`, `pattern:<regex>`, `category:<class>`,
/// `uniqueId:<id>`, `tags:<expression>`, `group:<group>`, `suite:<file>`; a directory's are its
/// package's (with the packages in it), every package's above the source roots.
pub fn test_selectors(configuration: &Configuration) -> Vec<String> {
    let mut selectors = kind_selectors(configuration);
    if let Some(mode) = configuration
        .entry
        .get(FORK_MODE)
        .and_then(Value::as_str)
        .filter(|mode| *mode != "none")
    {
        selectors.push(format!("fork:{mode}"));
    }
    selectors
}

/// The selectors of the configuration's kind, or of the tests Rerun Failed Tests runs.
fn kind_selectors(configuration: &Configuration) -> Vec<String> {
    if let Some(Value::Array(tests)) = configuration.entry.get(RERUN_TESTS) {
        return tests
            .iter()
            .filter_map(Value::as_str)
            .map(|test| match test.contains('#') {
                true => format!("method:{test}"),
                false => format!("class:{test}"),
            })
            .collect();
    }
    let text = |field| configuration.text(field).trim().to_owned();
    match test_kind(configuration) {
        "method" => vec![format!(
            "method:{}#{}",
            text(Field::Command),
            text(Field::TestMethod)
        )],
        "package" => vec![format!("package:{}", text(Field::TestPackage))],
        // IntelliJ's Pattern kind keeps a set of patterns, written `A||B`
        "pattern" => test_patterns(configuration)
            .into_iter()
            .map(|pattern| format!("pattern:{pattern}"))
            .collect(),
        "directory" => vec![format!(
            "package:{}",
            directory_package(Path::new(&text(Field::TestDirectory)))
        )],
        "category" => vec![format!("category:{}", text(Field::TestCategory))],
        // IntelliJ separates the ids with ESC
        "uniqueId" => text(Field::TestUniqueId)
            .split('\u{1b}')
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("uniqueId:{}", id.trim()))
            .collect(),
        "tags" => vec![format!("tags:{}", text(Field::TestTags))],
        "group" => vec![format!("group:{}", text(Field::TestGroup))],
        "suite" => vec![format!("suite:{}", text(Field::TestSuite))],
        _ => vec![format!("class:{}", text(Field::Command))],
    }
}

/// A Pattern configuration's patterns (IntelliJ's `||`-separated set).
pub fn test_patterns(configuration: &Configuration) -> Vec<String> {
    configuration
        .text(Field::TestPattern)
        .split("||")
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The package of a directory's classes: its path under a source root (`src/test/java`), the empty
/// package (every one) above them.
fn directory_package(directory: &Path) -> String {
    let parts: Vec<String> = directory
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let root = parts
        .windows(3)
        .position(|window| window[0] == "src" && (window[2] == "java" || window[2] == "kotlin"));
    match root {
        Some(at) => parts[at + 3..].join("."),
        None => String::new(),
    }
}

/// A test run whose JVM waits for a debugger on this port of the loopback interface, for the
/// debug adapter to attach to it, as IntelliJ debugs tests: set on the run, never kept in a
/// configuration.
pub const DEBUG_JVM: &str = "debugJvm";

/// The debug adapter that attaches to a test run's JVM ([`DEBUG_JVM`]): its tests show in that
/// adapter's session in the Debug tool window.
pub const DEBUG_ADAPTER: &str = "debugAdapter";

/// The port a test run's JVM waits for a debugger on ([`DEBUG_JVM`]).
pub fn debug_jvm_port(configuration: &Configuration) -> Option<u16> {
    configuration
        .entry
        .get(DEBUG_JVM)
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
}

/// The line a JVM waiting for a debugger on `port` writes (the JDWP agent's).
pub fn debug_jvm_listening(port: u16) -> String {
    format!("Listening for transport dt_socket at address: {port}")
}

/// The key of a test's debug scenario holding its JUnit or TestNG configuration as JSON: its
/// debug adapter runs the tests with their JVM waiting, then attaches to it.
pub const TEST_CONFIGURATION: &str = "testConfiguration";

/// A run with coverage (IntelliJ's Run 'X' with Coverage), for a runner that gathers it: set on
/// the run, never kept in a configuration.
pub const WITH_COVERAGE: &str = "withCoverage";

/// Whether a configuration's run gathers coverage.
pub fn with_coverage(configuration: &Configuration) -> bool {
    configuration
        .entry
        .get(WITH_COVERAGE)
        .and_then(Value::as_bool)
        == Some(true)
}

/// The environment variables of a gutter task that runs a main class with its own command (Java's
/// and Kotlin's `main`): the class, and the configuration type a run with coverage makes of it.
pub const GUTTER_MAIN_ENV: &str = "SPORK_GUTTER_MAIN";
pub const GUTTER_TYPE_ENV: &str = "SPORK_GUTTER_TYPE";

/// The environment variable of a run with coverage: the suite it gathers, as JSON
/// (`spork_coverage`'s `Suite`), which the run's end hands to the app.
pub const COVERAGE_SUITE_ENV: &str = "SPORK_COVERAGE_SUITE";

/// A configuration's classes and packages to include in coverage or exclude from it (IntelliJ's
/// Modify options ▸ Code Coverage): `[{"pattern": "com.example.*", "include": true, "enabled":
/// true}]`.
pub const COVERAGE_PATTERNS: &str = "coveragePatterns";

/// A class pattern of a configuration's coverage: `com.example.*` or `com.example.Calculator`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CoveragePattern {
    pub pattern: String,
    pub include: bool,
    pub enabled: bool,
}

/// A configuration's coverage patterns, includes and excludes alike.
pub fn coverage_patterns(configuration: &Configuration) -> Vec<CoveragePattern> {
    configuration
        .entry
        .get(COVERAGE_PATTERNS)
        .and_then(|patterns| serde_json::from_value(patterns.clone()).ok())
        .unwrap_or_default()
}

/// Sets a configuration's coverage patterns.
pub fn set_coverage_patterns(configuration: &mut Configuration, patterns: &[CoveragePattern]) {
    match serde_json::to_value(patterns) {
        Ok(value) => {
            configuration.entry.insert(COVERAGE_PATTERNS.into(), value);
        }
        Err(error) => log::error!("writing coverage patterns: {error}"),
    }
}

/// The JUnit or TestNG configuration of a test class or method, named as IntelliJ names it
/// (`GreetingTest`, `GreetingTest.greets`): the gutter's, the test tree's.
pub fn test_configuration(adapter: &str, class: &str, method: Option<&str>) -> Configuration {
    let simple = class.rsplit('.').next().unwrap_or(class);
    let name = match method {
        Some(method) => format!("{simple}.{method}"),
        None => simple.to_owned(),
    };
    let mut configuration = Configuration::new_debug(adapter, &name);
    configuration.set_text(Field::Command, class);
    let kind = match method {
        Some(method) => {
            configuration.set_text(Field::TestMethod, method);
            "method"
        }
        None => "class",
    };
    configuration.entry.insert(TEST_KIND.into(), kind.into());
    configuration
}

/// A configuration made by running something from the gutter or a context menu, kept for the
/// project (not in its files) until saved, as IntelliJ's temporary configurations are.
pub const TEMPORARY: &str = "temporary";

/// The environment variable of a task that runs a configuration (the gutter's): the
/// configuration's entry as JSON, which the Run tool window makes a temporary configuration of
/// and runs with its type's runner.
pub const CONFIGURATION_ENV: &str = "SPORK_RUN_CONFIGURATION";
/// Kotlin's debug adapter (`spork_kotlin`'s), whose configurations are IntelliJ's Kotlin ones.
pub const KOTLIN: &str = "Kotlin";

/// The labels' column of IntelliJ's Kotlin form, as wide as its longest label ("Use classpath of
/// module:"), where the other forms' is 160 px.
pub const KOTLIN_LABEL_WIDTH: f32 = 176.;

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
        Some(TESTNG) => "runConfigurations/testng",
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

    /// Whether it runs with coverage (IntelliJ's coverage executor applies to it).
    fn can_cover(&self) -> bool {
        false
    }

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

    /// What Update Running Application does to a running configuration of the type, as its menu
    /// item says it ("Update 'DemoApplication' application"); `None` when it does nothing.
    fn update_text(
        &self,
        _configuration: &Configuration,
        _workspace: &workspace::Workspace,
        _cx: &gpui::App,
    ) -> Option<SharedString> {
        None
    }

    /// Updates the running configuration, `session` its debug session when it is debugged.
    fn update(
        &self,
        _configuration: &Configuration,
        _session: Option<gpui::Entity<project::debugger::session::Session>>,
        _workspace: &gpui::Entity<workspace::Workspace>,
        _cx: &mut gpui::App,
    ) {
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

    /// Whether the form shows [`Runner::controls`] after the program's fields rather than before
    /// them (IntelliJ's Kotlin form: module, JRE and Shorten command line last).
    fn controls_last(&self) -> bool {
        false
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
    /// Stored as a project file (`.zed/`), or in the IDE's files of the project.
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
    /// A test configuration's method, package, pattern and the other kinds' fields (its class
    /// is `Command`).
    TestMethod,
    TestPackage,
    TestPattern,
    TestDirectory,
    TestCategory,
    TestUniqueId,
    TestTags,
    TestGroup,
    TestSuite,
}

impl Configuration {
    pub fn new_task(name: &str) -> Self {
        let mut entry = Map::new();
        entry.insert("label".into(), name.into());
        entry.insert("command".into(), "".into());
        // IntelliJ's new configurations are the workspace's until "Store as project file"
        Self {
            adapter: None,
            shared: false,
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
                shared: false,
                entry,
            };
        }
        entry.insert("request".into(), "launch".into());
        let mut configuration = Self {
            adapter: Some(adapter.to_owned().into()),
            shared: false,
            entry,
        };
        let program = configuration.key(Field::Command);
        configuration.entry.insert(program.into(), "".into());
        configuration
    }

    /// A configuration of the IDE's files from its entry (one a run carries, the gutter's).
    pub fn from_entry(entry: Map<String, Value>) -> Self {
        Self {
            adapter: entry
                .get("adapter")
                .and_then(Value::as_str)
                .map(|adapter| adapter.to_owned().into()),
            shared: false,
            entry,
        }
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

    /// Whether it is temporary: made by a run from the gutter or a context menu, not saved yet.
    pub fn is_temporary(&self) -> bool {
        self.entry.get(TEMPORARY).and_then(Value::as_bool) == Some(true)
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
            (Field::Command, false) if is_test(self.adapter.as_deref()) => "testClass",
            (Field::TestMethod, _) => "testMethod",
            (Field::TestPackage, _) => "testPackage",
            (Field::TestPattern, _) => "testPattern",
            (Field::TestDirectory, _) => "testDirectory",
            (Field::TestCategory, _) => "testCategory",
            (Field::TestUniqueId, _) => "testUniqueId",
            (Field::TestTags, _) => "testTags",
            (Field::TestGroup, _) => "testGroup",
            (Field::TestSuite, _) => "testSuite",
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
            | Field::ActiveProfiles
            | Field::TestMethod
            | Field::TestPackage
            | Field::TestPattern
            | Field::TestDirectory
            | Field::TestCategory
            | Field::TestUniqueId
            | Field::TestTags
            | Field::TestGroup
            | Field::TestSuite => (!text.trim().is_empty()).then(|| Value::from(text)),
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
        // the tests run as a run of the configuration does, Java's debugger attached to them
        if is_test(self.adapter.as_deref()) {
            let mut test = self.entry.clone();
            test.remove(TEMPORARY);
            let mut scenario = Map::new();
            scenario.insert("label".into(), self.name().to_string().into());
            scenario.insert("adapter".into(), JAVA.into());
            scenario.insert("request".into(), "attach".into());
            scenario.insert(
                TEST_CONFIGURATION.into(),
                Value::Object(test).to_string().into(),
            );
            entry = scenario;
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

/// Where a project's configurations are kept: its first folder's `.zed` files, shared, and the
/// IDE's files of the project, private (IntelliJ's `workspace.xml`).
#[derive(Clone, Debug, PartialEq)]
pub struct Files {
    pub project_tasks: Option<PathBuf>,
    pub project_debug: Option<PathBuf>,
    pub workspace_tasks: Option<PathBuf>,
    pub workspace_debug: Option<PathBuf>,
    /// The list's order, kept apart from where each configuration is stored, as IntelliJ's
    /// `workspace.xml` keeps its RunManager `<list>`.
    pub order: Option<PathBuf>,
}

impl Files {
    pub fn new(root: Option<&Path>) -> Self {
        let local =
            |relative: &util::rel_path::RelPath| root.map(|root| root.join(relative.as_std_path()));
        let workspace = root.map(workspace_folder);
        Self {
            project_tasks: local(paths::local_tasks_file_relative_path()),
            project_debug: local(paths::local_debug_file_relative_path()),
            workspace_tasks: workspace.as_ref().map(|folder| folder.join("tasks.json")),
            workspace_debug: workspace.as_ref().map(|folder| folder.join("debug.json")),
            order: workspace.map(|folder| folder.join("order.json")),
        }
    }

    fn file(&self, task: bool, shared: bool) -> Option<&PathBuf> {
        match (task, shared) {
            (true, true) => self.project_tasks.as_ref(),
            (false, true) => self.project_debug.as_ref(),
            (true, false) => self.workspace_tasks.as_ref(),
            (false, false) => self.workspace_debug.as_ref(),
        }
    }
}

/// The IDE's folder of the project at `root` in its data folder, named after the folder and a
/// hash of its path.
pub fn workspace_folder(root: &Path) -> PathBuf {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(root.to_string_lossy().as_bytes());
    let hash: String = digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    paths::data_dir()
        .join("workspace")
        .join(format!("{name}-{hash}"))
}

/// A configuration's key in the order file: its type and name, as IntelliJ's `Type.Name`.
fn order_key(configuration: &Configuration) -> String {
    format!("{}.{}", configuration.type_name(), configuration.name())
}

/// Sorts `configurations` by the saved order; those it doesn't list keep their files' order,
/// after the listed ones.
fn ordered(configurations: &mut [Configuration], order: &[String]) {
    configurations.sort_by_key(|configuration| {
        let key = order_key(configuration);
        order
            .iter()
            .position(|listed| *listed == key)
            .unwrap_or(usize::MAX)
    });
}

/// Reads the configurations of the project's and the IDE's files in the saved order, or the
/// project's first, each file's in its order.
pub async fn load(fs: Arc<dyn Fs>, files: Files) -> Vec<Configuration> {
    let mut configurations = load_files(fs.clone(), &files).await;
    if let Some(path) = &files.order
        && let Ok(text) = fs.load(path).await
    {
        match serde_json::from_str::<Vec<String>>(&text) {
            Ok(order) => ordered(&mut configurations, &order),
            Err(error) => {
                log::warn!("reading the run configurations' order from {path:?}: {error:#}")
            }
        }
    }
    configurations
}

async fn load_files(fs: Arc<dyn Fs>, files: &Files) -> Vec<Configuration> {
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
    let keys = |configurations: &[Configuration]| -> Vec<String> {
        configurations.iter().map(order_key).collect()
    };
    if let Some(path) = &files.order
        && keys(&before) != keys(&after)
    {
        if let Some(folder) = path.parent() {
            fs.create_dir(folder)
                .await
                .with_context(|| format!("creating {folder:?}"))?;
        }
        let text = serde_json::to_string_pretty(&keys(&after))?;
        fs.atomic_write(path.clone(), format!("{text}\n"))
            .await
            .with_context(|| format!("writing {path:?}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        Configuration, FORK_MODE, Field, JUNIT, TEST_KIND, order_key, ordered, split_arguments,
        test_selectors,
    };

    #[test]
    fn selects_each_kinds_tests() {
        let mut configuration = Configuration::new_debug(JUNIT, "Kinds");
        configuration
            .entry
            .insert(TEST_KIND.into(), "directory".into());
        configuration.set_text(Field::TestDirectory, "app/src/test/java/com/example/kinds");
        assert_eq!(
            test_selectors(&configuration),
            ["package:com.example.kinds"]
        );
        configuration.set_text(Field::TestDirectory, "/p/app");
        assert_eq!(test_selectors(&configuration), ["package:"]);
        configuration
            .entry
            .insert(TEST_KIND.into(), "uniqueId".into());
        configuration.set_text(Field::TestUniqueId, "[engine:a]\u{1b}[engine:b]");
        assert_eq!(
            test_selectors(&configuration),
            ["uniqueId:[engine:a]", "uniqueId:[engine:b]"]
        );
        configuration
            .entry
            .insert(TEST_KIND.into(), "pattern".into());
        configuration.set_text(Field::TestPattern, "com.a.ATest|| com.b.BTest");
        assert_eq!(
            test_selectors(&configuration),
            ["pattern:com.a.ATest", "pattern:com.b.BTest"]
        );
        configuration.entry.insert(TEST_KIND.into(), "tags".into());
        configuration.set_text(Field::TestTags, "fast & !slow");
        configuration.entry.insert(FORK_MODE.into(), "class".into());
        assert_eq!(
            test_selectors(&configuration),
            ["tags:fast & !slow", "fork:class"]
        );
    }

    #[test]
    fn the_saved_order_wins_over_where_configurations_are_stored() {
        let mut list = vec![
            Configuration::new_task("hello"),
            Configuration::new_task("greet"),
            Configuration::new_task("new"),
        ];
        let order = vec![order_key(&list[1]), order_key(&list[0])];
        ordered(&mut list, &order);
        let names: Vec<_> = list.iter().map(Configuration::name).collect();
        assert_eq!(names, ["greet", "hello", "new"]);
    }

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
