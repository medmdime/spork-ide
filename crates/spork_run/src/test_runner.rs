//! IntelliJ's test runner in the Run tool window (`SMTestRunnerResultsForm`): a test run's tab
//! shows the tests as a tree (the root "Test Results", the classes as suites, their tests, each
//! with its state icon and duration), the status line ("2 tests passed  2 tests total, 120 ms")
//! and the console of the selected node: the whole run's for the root, a test's output and
//! failure otherwise, with its stack trace's links and a comparison's "Click to see
//! difference". A task marked [`TEST_RUN`] is a test run: the Run tool window gives it a port
//! on the loopback interface ([`TEST_PORT`]), where the test runner (`spork_java`'s, or its
//! Gradle init script) writes IntelliJ's service messages (`##teamcity[testStarted …]`).

use std::path::PathBuf;

use collections::HashMap;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Entity, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Task, TaskExt as _, WeakEntity, Window, div, px, relative,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use serde_json::{Value, json};
use spork_workbench::tree::{Tree, TreeItem};
use spork_workbench::{ActionButton, Tokens};
use terminal_view::TerminalView;
use util::ResultExt as _;
use workspace::{OpenOptions, Workspace};

/// The environment variable that marks a task as a test run.
pub const TEST_RUN: &str = "SPORK_TEST_RUN";
/// The environment variable that gives a test run's runner the IDE's port.
pub const TEST_PORT: &str = "SPORK_TEST_PORT";

/// A node's state, as its icon shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestState {
    Running,
    Passed,
    /// A suite that passed with some of its tests ignored.
    PassedIgnored,
    Failed,
    Error,
    Ignored,
    Terminated,
    NotRun,
}

impl TestState {
    fn icon(self) -> Option<&'static str> {
        match self {
            Self::Running => None,
            Self::Passed => Some("run/testPassed"),
            Self::PassedIgnored => Some("run/testPassedIgnored"),
            Self::Failed => Some("run/testFailed"),
            Self::Error => Some("run/testError"),
            Self::Ignored => Some("run/testIgnored"),
            Self::Terminated => Some("run/testTerminated"),
            Self::NotRun => Some("run/testNotRunYet"),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Passed | Self::PassedIgnored => "passed",
            Self::Failed => "failed",
            Self::Error => "error",
            Self::Ignored => "ignored",
            Self::Terminated => "terminated",
            Self::NotRun => "not run",
        }
    }

    fn is_defect(self) -> bool {
        matches!(self, Self::Failed | Self::Error)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Failure {
    message: String,
    details: String,
    expected: Option<String>,
    actual: Option<String>,
}

#[derive(Clone, Debug)]
struct Node {
    name: String,
    /// IntelliJ's location hint: `java:suite://com.example.GreetingTest`,
    /// `java:test://com.example.GreetingTest/greets`.
    location: Option<String>,
    suite: bool,
    children: Vec<usize>,
    /// A test's own state; a suite's comes from its tests.
    state: TestState,
    duration: Option<u64>,
    finished: bool,
    /// What it wrote, each piece with whether it went to stderr.
    output: Vec<(String, bool)>,
    failure: Option<Failure>,
}

/// A test run's tests, from its service messages.
#[derive(Clone, Debug)]
pub struct TestResults {
    nodes: Vec<Node>,
    ids: HashMap<String, usize>,
    count: Option<usize>,
    ended: bool,
}

impl Default for TestResults {
    fn default() -> Self {
        Self {
            nodes: vec![Node {
                name: "Test Results".into(),
                location: None,
                suite: true,
                children: Vec::new(),
                state: TestState::Running,
                duration: None,
                finished: false,
                output: Vec::new(),
                failure: None,
            }],
            ids: HashMap::default(),
            count: None,
            ended: false,
        }
    }
}

/// The counts the status line shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub total: usize,
    pub finished: usize,
    pub failed: usize,
    pub ignored: usize,
}

impl Counts {
    pub fn passed(&self) -> usize {
        self.finished.saturating_sub(self.failed + self.ignored)
    }
}

impl TestResults {
    /// Takes in a service message; whether it changed the tree.
    pub fn apply(&mut self, line: &str) -> bool {
        let Some((name, attributes)) = parse_message(line) else {
            return false;
        };
        let attribute = |key: &str| {
            attributes
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };
        let node = attribute("nodeId").and_then(|id| self.ids.get(&id).copied());
        match name.as_str() {
            "testCount" => {
                self.count = attribute("count").and_then(|count| count.parse().ok());
            }
            "testSuiteStarted" | "testStarted" => {
                let Some(id) = attribute("nodeId") else {
                    return false;
                };
                let parent = attribute("parentNodeId")
                    .and_then(|parent| self.ids.get(&parent).copied())
                    .unwrap_or(0);
                let ix = self.nodes.len();
                self.nodes.push(Node {
                    name: attribute("name").unwrap_or_default(),
                    location: attribute("locationHint"),
                    suite: name == "testSuiteStarted",
                    children: Vec::new(),
                    state: TestState::Running,
                    duration: None,
                    finished: false,
                    output: Vec::new(),
                    failure: None,
                });
                self.nodes[parent].children.push(ix);
                self.ids.insert(id, ix);
            }
            "testStdOut" | "testStdErr" => {
                let (Some(ix), Some(text)) = (node, attribute("out")) else {
                    return false;
                };
                self.nodes[ix].output.push((text, name == "testStdErr"));
            }
            "testFailed" => {
                let Some(ix) = node else { return false };
                let comparison = attribute("type").as_deref() == Some("comparisonFailure");
                let details = attribute("details").unwrap_or_default();
                // a failure that isn't an assertion's is IntelliJ's error
                let assertion = details.lines().next().is_some_and(|line| {
                    line.contains("AssertionError")
                        || line.contains("AssertionFailedError")
                        || line.contains("ComparisonFailure")
                        || line.contains("junit.framework.")
                });
                self.nodes[ix].state = match assertion || comparison {
                    true => TestState::Failed,
                    false => TestState::Error,
                };
                self.nodes[ix].failure = Some(Failure {
                    message: attribute("message").unwrap_or_default(),
                    details,
                    expected: attribute("expected").filter(|_| comparison),
                    actual: attribute("actual").filter(|_| comparison),
                });
            }
            "testIgnored" => {
                let Some(ix) = node else { return false };
                self.nodes[ix].state = TestState::Ignored;
                if let Some(message) = attribute("message").filter(|m| !m.is_empty()) {
                    self.nodes[ix].output.push((format!("{message}\n"), false));
                }
            }
            "testFinished" => {
                let Some(ix) = node else { return false };
                let node = &mut self.nodes[ix];
                node.finished = true;
                node.duration = attribute("duration").and_then(|d| d.parse().ok());
                if node.state == TestState::Running {
                    node.state = TestState::Passed;
                }
            }
            "testSuiteFinished" => {
                let Some(ix) = node else { return false };
                self.nodes[ix].finished = true;
            }
            _ => return false,
        }
        true
    }

    /// The process ended: the tests still running were terminated.
    pub fn end(&mut self) {
        self.ended = true;
        for node in &mut self.nodes {
            if !node.suite && node.state == TestState::Running {
                node.state = TestState::Terminated;
            }
            node.finished = true;
        }
    }

    fn tests(&self, ix: usize) -> Vec<usize> {
        let mut tests = Vec::new();
        let mut stack = vec![ix];
        while let Some(ix) = stack.pop() {
            let node = &self.nodes[ix];
            if !node.suite {
                tests.push(ix);
            }
            stack.extend(node.children.iter().rev());
        }
        tests
    }

    pub fn state(&self, ix: usize) -> TestState {
        let node = &self.nodes[ix];
        if !node.suite {
            return node.state;
        }
        let states: Vec<TestState> = node.children.iter().map(|c| self.state(*c)).collect();
        if states.contains(&TestState::Running) && !self.ended {
            return TestState::Running;
        }
        if states.contains(&TestState::Error) {
            return TestState::Error;
        }
        if states.contains(&TestState::Failed) {
            return TestState::Failed;
        }
        if states.contains(&TestState::Terminated) || (ix == 0 && self.ended && self.terminated()) {
            return TestState::Terminated;
        }
        if states.is_empty() {
            return match self.ended || node.finished {
                true => TestState::NotRun,
                false => TestState::Running,
            };
        }
        if states.iter().all(|state| *state == TestState::Ignored) {
            return TestState::Ignored;
        }
        if states
            .iter()
            .any(|state| matches!(state, TestState::Ignored | TestState::PassedIgnored))
        {
            return TestState::PassedIgnored;
        }
        if !node.finished && !self.ended {
            return TestState::Running;
        }
        TestState::Passed
    }

    /// Whether the run was stopped before all its tests ran.
    fn terminated(&self) -> bool {
        self.count
            .is_some_and(|count| self.tests(0).len() < count && self.ended)
    }

    /// A node's duration: a test's own, a suite's its tests' together.
    pub fn duration(&self, ix: usize) -> Option<u64> {
        let node = &self.nodes[ix];
        if !node.suite {
            return node.duration;
        }
        let durations: Vec<u64> = node
            .children
            .iter()
            .filter_map(|c| self.duration(*c))
            .collect();
        (!durations.is_empty()).then(|| durations.iter().sum())
    }

    pub fn counts(&self) -> Counts {
        let tests = self.tests(0);
        let finished: Vec<TestState> = tests
            .iter()
            .map(|ix| self.nodes[*ix].state)
            .filter(|state| !matches!(state, TestState::Running | TestState::Terminated))
            .collect();
        Counts {
            total: self.count.unwrap_or(0).max(tests.len()),
            finished: finished.len(),
            failed: finished.iter().filter(|state| state.is_defect()).count(),
            ignored: finished
                .iter()
                .filter(|state| **state == TestState::Ignored)
                .count(),
        }
    }

    /// The failed tests, each `com.example.GreetingTest#greets`, for Rerun Failed Tests.
    pub fn failed_tests(&self) -> Vec<String> {
        self.tests(0)
            .into_iter()
            .filter(|ix| self.nodes[*ix].state.is_defect())
            .filter_map(|ix| {
                let (class, method) = location_parts(self.nodes[ix].location.as_deref()?)?;
                Some(match method {
                    Some(method) => format!("{class}#{method}"),
                    None => class.to_owned(),
                })
            })
            .collect()
    }

    /// The state of each test and suite, by location, for the gutter's icons.
    fn states(&self) -> Vec<(String, TestState)> {
        (1..self.nodes.len())
            .filter_map(|ix| Some((self.nodes[ix].location.clone()?, self.state(ix))))
            .collect()
    }
}

/// A location hint's class and method: `java:test://com.example.A/greets`.
fn location_parts(location: &str) -> Option<(&str, Option<&str>)> {
    let (_, rest) = location.split_once("://")?;
    Some(match rest.split_once('/') {
        Some((class, method)) => (class, Some(method)),
        None => (rest, None),
    })
}

/// A service message: its name and attributes, unescaped.
fn parse_message(line: &str) -> Option<(String, Vec<(String, String)>)> {
    let body = line.trim().strip_prefix("##teamcity[")?.strip_suffix(']')?;
    let (name, rest) = body.split_once(' ').unwrap_or((body, ""));
    let mut attributes = Vec::new();
    let mut chars = rest.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let key: String = std::iter::from_fn(|| chars.next_if(|c| *c != '=')).collect();
        if key.is_empty() || chars.next() != Some('=') || chars.next() != Some('\'') {
            break;
        }
        let mut value = String::new();
        loop {
            match chars.next()? {
                '\'' => break,
                '|' => match chars.next()? {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    'x' => value.push('\u{85}'),
                    'l' => value.push('\u{2028}'),
                    'p' => value.push('\u{2029}'),
                    '0' if chars.peek() == Some(&'x') => {
                        chars.next();
                        let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            value.push(c);
                        }
                    }
                    other => value.push(other),
                },
                other => value.push(other),
            }
        }
        attributes.push((key.trim().to_owned(), value));
    }
    Some((name.to_owned(), attributes))
}

/// A duration as IntelliJ writes it (`NlsMessages.formatDurationApproximateNarrow`): at most two
/// units, the number and its unit apart by a thin space, `120 ms`, `1 sec 200 ms`.
pub fn format_duration(millis: u64) -> String {
    const UNITS: [(&str, u64); 5] = [
        ("ms", 1000),
        ("sec", 60),
        ("min", 60),
        ("hr", 24),
        ("day", u64::MAX),
    ];
    // the value in each unit, the largest first
    let mut values: Vec<(u64, &str)> = Vec::new();
    let mut rest = millis;
    for (unit, size) in UNITS {
        values.push((rest % size, unit));
        rest /= size;
        if rest == 0 {
            break;
        }
    }
    values.reverse();
    // drop the zeros after the first value, keep two, the second rounded
    let first = values.first().copied().unwrap_or((0, "ms"));
    let second = values.get(1).copied().filter(|(value, _)| *value > 0);
    let thin = "\u{2009}";
    match second {
        Some((value, unit)) => format!("{}{thin}{} {value}{thin}{unit}", first.0, first.1),
        None => format!("{}{thin}{}", first.0, first.1),
    }
}

/// The status line's text, its failed and ignored parts marked, and its grey description
/// (`TestStatusLine.doFormatTestMessage`).
pub fn status_text(
    counts: Counts,
    duration: Option<u64>,
    ended: bool,
) -> (Vec<(String, Option<TestState>)>, String) {
    let tests = |n: usize| match n {
        1 => format!("{n} test"),
        n => format!("{n} tests"),
    };
    let (failed, passed, ignored) = (counts.failed, counts.passed(), counts.ignored);
    let mut parts: Vec<(String, Option<TestState>)> = Vec::new();
    // no test ran: IntelliJ's status line stays empty
    if counts.total == 0 && counts.finished == 0 {
        return (parts, String::new());
    }
    let ongoing = counts.finished != counts.total;
    if ongoing && ended {
        parts.push(("Stopped. ".into(), None));
    }
    let failed_text = |n: usize| (format!("{} failed", tests(n)), Some(TestState::Failed));
    let ignored_text = |n: usize| (format!("{} ignored", tests(n)), Some(TestState::Ignored));
    match (failed, passed, ignored) {
        (0, _, 0) => parts.push((format!("{} passed", tests(counts.finished)), None)),
        (failed, 0, 0) => parts.push(failed_text(failed)),
        (0, 0, ignored) => parts.push(ignored_text(ignored)),
        (failed, passed, 0) => {
            parts.push((
                format!("{},", failed_text(failed).0),
                Some(TestState::Failed),
            ));
            parts.push((format!(" {passed} passed"), None));
        }
        (failed, 0, ignored) => {
            parts.push((
                format!("{},", failed_text(failed).0),
                Some(TestState::Failed),
            ));
            parts.push((" ".into(), None));
            parts.push((format!("{ignored} ignored"), Some(TestState::Ignored)));
        }
        (0, passed, ignored) => {
            parts.push((format!("{} passed, ", tests(passed)), None));
            parts.push((format!("{ignored} ignored"), Some(TestState::Ignored)));
        }
        (failed, passed, ignored) => {
            parts.push((
                format!("{},", failed_text(failed).0),
                Some(TestState::Failed),
            ));
            parts.push((format!(" {passed} passed, "), None));
            parts.push((format!("{ignored} ignored"), Some(TestState::Ignored)));
        }
    }
    let done = counts.failed + counts.passed() + counts.ignored;
    let description = match (ongoing, ended, duration) {
        (true, true, Some(duration)) => format!(
            "{done} / {} tests, {}",
            counts.total,
            format_duration(duration)
        ),
        (true, _, _) => format!("{done} / {} tests", counts.total),
        (false, _, Some(duration)) => {
            format!(
                "{} total, {}",
                tests(counts.total),
                format_duration(duration)
            )
        }
        (false, _, None) => String::new(),
    };
    (parts, description)
}

/// The latest state of each test and suite that ran, by location, for the gutter's icons
/// (IntelliJ's `TestStateStorage`).
#[derive(Default)]
pub struct TestStates(pub HashMap<String, TestState>);

impl Global for TestStates {}

/// The state a test or suite ended its last run in, by its location
/// (`java:test://com.example.GreetingTest/greets`).
pub fn test_state(location: &str, cx: &App) -> Option<TestState> {
    cx.try_global::<TestStates>()?.0.get(location).copied()
}

/// The test a gutter run icon stands for, by its runnable's captures (Java's, and Kotlin's
/// `kotlin_test_*`): `java:test://com.example.GreetingTest/greets`, or the class's suite.
fn gutter_location(look: &editor::RunIndicatorLook<'_>) -> Option<String> {
    if !look.tags.iter().any(|tag| tag.contains("test")) {
        return None;
    }
    let variable = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| look.variables.get(*name).filter(|value| !value.is_empty()))
    };
    let class = variable(&["java_class_name", "kotlin_test_class"])?;
    let class = match variable(&["java_outer_class_name"]) {
        Some(outer) => format!("{outer}${class}"),
        None => class.clone(),
    };
    let class = match variable(&["java_package_name", "kotlin_package_name"]) {
        Some(package) => format!("{package}.{class}"),
        None => class,
    };
    Some(
        match variable(&["java_method_name", "kotlin_test_method"]) {
            Some(method) => format!("java:test://{class}/{method}"),
            None => format!("java:suite://{class}"),
        },
    )
}

/// The gutter's run icon (IntelliJ's): a test's shows how its last run went.
fn gutter_icon(look: editor::RunIndicatorLook<'_>, cx: &mut App) -> AnyElement {
    let location = gutter_location(&look);
    let state = location
        .as_deref()
        .and_then(|location| test_state(location, cx));
    let icon = match state {
        Some(TestState::Passed | TestState::PassedIgnored) => "gutter/runSuccess",
        Some(TestState::Failed) => "gutter/runFailed",
        Some(TestState::Error | TestState::Terminated) => "gutter/runError",
        _ => "gutter/run",
    };
    // IntelliJ's tooltip names the test's run; the end-to-end tests aim at it
    let name = location
        .as_deref()
        .and_then(location_parts)
        .map(|(class, method)| {
            let simple = class.rsplit(['.', '$']).next().unwrap_or(class);
            match method {
                Some(method) => format!("Run test '{simple}.{method}'"),
                None => format!("Run test '{simple}'"),
            }
        })
        // not "Run": a dialog's Run button has that name
        .unwrap_or_else(|| "Run line marker".into());
    let value = match state {
        Some(state) => state.name(),
        None => "",
    };
    div()
        .id(SharedString::from(format!("{icon}-{name}")))
        .role(Role::Button)
        .aria_label(SharedString::from(name))
        .aria_value(value)
        .size(px(14.))
        .child(spork_workbench::image(icon, cx).size(px(14.)))
        .into_any_element()
}

/// Modify Run Configuration…: the run's configuration made temporary and selected, then shown
/// in the Run/Debug Configurations dialog.
fn modify_run_configuration(task: &task::ResolvedTask, window: &mut Window, cx: &mut App) {
    let Some(entry) = task
        .resolved
        .env
        .get(crate::configurations::CONFIGURATION_ENV)
        .and_then(|entry| serde_json::from_str::<serde_json::Map<String, Value>>(entry).log_err())
    else {
        return;
    };
    let Some(workspace) = Workspace::for_window(window, cx) else {
        return;
    };
    let configuration = crate::Configuration {
        adapter: entry
            .get("adapter")
            .and_then(Value::as_str)
            .map(|adapter| adapter.to_owned().into()),
        shared: false,
        entry,
    };
    let name = configuration.name();
    workspace.update(cx, |workspace, cx| {
        if let Some(configurations) = crate::ProjectConfigurations::of(workspace, cx) {
            configurations.update(cx, |configurations, cx| {
                configurations.run_temporary(configuration, cx)
            });
        }
        crate::configurations_dialog::open(workspace, Some(name), window, cx);
    });
}

/// The gutter's run icons and their popup's Modify Run Configuration….
pub(crate) fn init(cx: &mut App) {
    cx.set_global(editor::RunIndicatorIcon(std::rc::Rc::new(gutter_icon)));
    cx.set_global(editor::RunMenuModify(std::rc::Rc::new(
        modify_run_configuration,
    )));
}

/// The test runner's view options, the same for every run and kept as the user leaves them, as
/// IntelliJ's `TestConsoleProperties` are.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Options {
    show_passed: bool,
    show_ignored: bool,
    sort: Sort,
    suites_on_top: bool,
    inline_statistics: bool,
    track_running: bool,
    select_first_failed: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            show_passed: true,
            show_ignored: true,
            sort: Sort::default(),
            suites_on_top: true,
            inline_statistics: true,
            track_running: true,
            select_first_failed: false,
        }
    }
}

#[derive(Default)]
struct GlobalOptions(Options);

impl Global for GlobalOptions {}

/// How the tree sorts its nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Run,
    Alphabetically,
    Duration,
}

/// What Rerun, Rerun Failed Tests and Stop do for the tab.
pub(crate) struct TabActions {
    pub rerun: Box<dyn Fn(&mut Window, &mut App)>,
    pub rerun_failed: Box<dyn Fn(Vec<String>, &mut Window, &mut App)>,
    pub stop: Box<dyn Fn(&mut Window, &mut App)>,
}

/// A test run's tab: the toolbar, the tree and the selected node's console.
pub struct TestRunView {
    results: TestResults,
    selected: usize,
    options: Options,
    /// Expand All and Collapse All key the tree anew, its rows open or closed.
    tree_key: usize,
    expanded: bool,
    console: Option<Entity<TerminalView>>,
    workspace: WeakEntity<Workspace>,
    name: SharedString,
    running: bool,
    actions: TabActions,
    _listen: Task<()>,
}

impl TestRunView {
    /// A view for a run about to start, reading the events its runner sends to `listener`.
    pub(crate) fn new(
        name: SharedString,
        workspace: WeakEntity<Workspace>,
        actions: TabActions,
        listener: smol::net::TcpListener,
        cx: &mut Context<Self>,
    ) -> Self {
        let listen = cx.spawn(async move |this, cx| {
            // a connection a runner (or a Gradle build's test task) opens, its lines in order
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let this = this.clone();
                cx.spawn(async move |cx| {
                    use futures::{AsyncBufReadExt as _, StreamExt as _};
                    let mut lines = futures::io::BufReader::new(stream).lines();
                    while let Some(Ok(line)) = lines.next().await {
                        if this
                            .update(cx, |this, cx| this.take_line(&line, cx))
                            .is_err()
                        {
                            return;
                        }
                    }
                })
                .detach();
            }
        });
        Self {
            results: TestResults::default(),
            selected: 0,
            options: cx
                .try_global::<GlobalOptions>()
                .map(|o| o.0)
                .unwrap_or_default(),
            tree_key: 0,
            expanded: true,
            console: None,
            workspace,
            name,
            running: true,
            actions,
            _listen: listen,
        }
    }

    /// The run's console, which the root shows.
    pub(crate) fn set_console(&mut self, console: Entity<TerminalView>) {
        self.console = Some(console);
    }

    fn take_line(&mut self, line: &str, cx: &mut Context<Self>) {
        let before = self.results.nodes.len();
        if !self.results.apply(line) {
            return;
        }
        // Track Running Test: the test that started is selected
        if self.options.track_running
            && self.results.nodes.len() > before
            && !self.results.nodes[before].suite
        {
            self.selected = before;
        }
        cx.notify();
    }

    /// The process ended: the tests left running were terminated, the gutter learns the states.
    pub(crate) fn finished(&mut self, cx: &mut Context<Self>) {
        self.running = false;
        self.results.end();
        let states = self.results.states();
        let global = cx.default_global::<TestStates>();
        for (location, state) in states {
            global.0.insert(location, state);
        }
        if self.options.select_first_failed
            && let Some(first) = self
                .results
                .tests(0)
                .into_iter()
                .find(|ix| self.results.nodes[*ix].state.is_defect())
        {
            self.selected = first;
        } else if self.options.track_running {
            self.selected = 0;
        }
        // the editors' run icons show the new states
        cx.refresh_windows();
        cx.notify();
    }

    pub fn results(&self) -> &TestResults {
        &self.results
    }

    fn visible(&self, ix: usize) -> bool {
        let node = &self.results.nodes[ix];
        if node.suite {
            return node.children.iter().any(|child| self.visible(*child))
                || (node.children.is_empty() && ix != 0);
        }
        match self.results.state(ix) {
            TestState::Passed => self.options.show_passed,
            TestState::Ignored => self.options.show_ignored,
            _ => true,
        }
    }

    fn sorted_children(&self, ix: usize) -> Vec<usize> {
        let mut children: Vec<usize> = self.results.nodes[ix]
            .children
            .iter()
            .copied()
            .filter(|child| self.visible(*child))
            .collect();
        match self.options.sort {
            Sort::Run => {}
            Sort::Alphabetically => children.sort_by(|a, b| {
                self.results.nodes[*a]
                    .name
                    .cmp(&self.results.nodes[*b].name)
            }),
            Sort::Duration => children
                .sort_by_key(|child| std::cmp::Reverse(self.results.duration(*child).unwrap_or(0))),
        }
        if self.options.suites_on_top && self.options.sort != Sort::Run {
            children.sort_by_key(|child| !self.results.nodes[*child].suite);
        }
        children
    }

    /// The tests in the tree's order, for Previous and Next Failed Test.
    fn order(&self, ix: usize, order: &mut Vec<usize>) {
        order.push(ix);
        for child in self.sorted_children(ix) {
            self.order(child, order);
        }
    }

    fn select_failed(&mut self, next: bool, cx: &mut Context<Self>) {
        let mut order = Vec::new();
        self.order(0, &mut order);
        let failed: Vec<usize> = order
            .into_iter()
            .filter(|ix| !self.results.nodes[*ix].suite && self.results.state(*ix).is_defect())
            .collect();
        let position = failed.iter().position(|ix| *ix == self.selected);
        let target = match (next, position) {
            (true, Some(at)) => failed.get(at + 1).or(failed.first()),
            (true, None) => failed.first(),
            (false, Some(at)) if at > 0 => failed.get(at - 1),
            (false, _) => failed.last(),
        };
        if let Some(target) = target {
            self.selected = *target;
            cx.notify();
        }
    }

    fn item(&self, ix: usize, cx: &mut Context<Self>) -> TreeItem {
        let node = &self.results.nodes[ix];
        let state = self.results.state(ix);
        let this = cx.entity().downgrade();
        let duration = self
            .results
            .duration(ix)
            .filter(|_| self.options.inline_statistics && state != TestState::Ignored)
            .map(format_duration);
        let muted = Tokens::get(cx).text_secondary;
        let location = node.location.clone();
        let children: Vec<TreeItem> = self
            .sorted_children(ix)
            .into_iter()
            .map(|child| self.item(child, cx))
            .collect();
        TreeItem::new(node.name.clone())
            .map(|item| match state.icon() {
                Some(icon) => item.colored_icon(icon),
                None => item.icon(spork_workbench::icon("empty")),
            })
            .when(state == TestState::Running, |item| {
                item.suffix(|_, _| spork_workbench::Spinner::new())
            })
            .when_some(duration, |item, duration| {
                item.suffix(move |_, _| div().text_color(muted).child(duration.clone()))
            })
            .active(self.selected == ix)
            .default_open(self.expanded || ix == 0)
            .when_some(self.node_menu(ix, cx), |item, menu| item.context_menu(menu))
            .on_click(move |event, window, cx| {
                this.update(cx, |this, cx| {
                    this.selected = ix;
                    if event.click_count() == 2
                        && let Some(location) = &location
                    {
                        this.jump_to_source(location, window, cx);
                    }
                    cx.notify();
                })
                .log_err();
            })
            .children(children)
    }

    /// A test's or suite's menu (IntelliJ's `TestTreePopupMenu`): Run and Debug its own
    /// configuration, Jump to Source.
    fn node_menu(
        &self,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> Option<impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static>
    {
        let location = self.results.nodes[ix].location.clone()?;
        let (class, method) = location_parts(&location)?;
        // the run's own type: TestNG's tests rerun as TestNG
        let adapter = self
            .workspace
            .upgrade()
            .and_then(|workspace| crate::ProjectConfigurations::of(workspace.read(cx), cx))
            .and_then(|configurations| {
                configurations
                    .read(cx)
                    .list
                    .iter()
                    .find(|c| c.name() == self.name)
                    .and_then(|c| c.adapter.clone())
            })
            .filter(|adapter| crate::configurations::is_test(Some(adapter)))
            .unwrap_or_else(|| crate::configurations::JUNIT.into());
        let configuration = crate::configurations::test_configuration(&adapter, class, method);
        let name = configuration.name();
        let view = cx.entity().downgrade();
        Some(
            move |menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
                let run = |debug: bool| {
                    let configuration = configuration.clone();
                    move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                        if let Some(workspace) = Workspace::for_window(window, cx) {
                            workspace.update(cx, |workspace, cx| {
                                crate::run_temporary(
                                    configuration.clone(),
                                    debug,
                                    workspace,
                                    window,
                                    cx,
                                )
                            });
                        }
                    }
                };
                let (view, location) = (view.clone(), location.clone());
                menu.item(PopupMenuItem::new(format!("Run '{name}'")).on_click(run(false)))
                    .item(PopupMenuItem::new(format!("Debug '{name}'")).on_click(run(true)))
                    .separator()
                    .item(
                        PopupMenuItem::new("Jump to Source").on_click(move |_, window, cx| {
                            view.update(cx, |view, cx| view.jump_to_source(&location, window, cx))
                                .log_err();
                        }),
                    )
            },
        )
    }

    /// Jump to Source: the test's method, or the suite's class.
    fn jump_to_source(&self, location: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((class, method)) = location_parts(location) else {
            return;
        };
        let Some(path) = self.source_of(class, None, cx) else {
            return;
        };
        let line = std::fs::read_to_string(&path).ok().and_then(|text| {
            let needle = method.map_or_else(
                || format!("class {}", class.rsplit('.').next().unwrap_or(class)),
                |method| format!("{method}("),
            );
            text.lines().position(|line| line.contains(&needle))
        });
        self.open(path, line.unwrap_or(0) as u32, window, cx);
    }

    /// A class's source in the project: `com.example.GreetingTest` (or a stack frame's file name)
    /// is the file `com/example/GreetingTest.java` (or `.kt`) under one of its folders.
    fn source_of(&self, class: &str, file_name: Option<&str>, cx: &App) -> Option<PathBuf> {
        let workspace = self.workspace.upgrade()?;
        let class = class.split('$').next()?;
        let (package, simple) = class.rsplit_once('.').unwrap_or(("", class));
        let folder: PathBuf = package.split('.').filter(|part| !part.is_empty()).collect();
        let names: Vec<String> = match file_name {
            Some(name) => vec![name.to_owned()],
            None => vec![format!("{simple}.java"), format!("{simple}.kt")],
        };
        let project = workspace.read(cx).project().read(cx);
        project.visible_worktrees(cx).find_map(|worktree| {
            let worktree = worktree.read(cx);
            worktree.files(false, 0).find_map(|entry| {
                let path = entry.path.as_std_path();
                let name = path.file_name()?.to_str()?;
                (names.iter().any(|wanted| wanted == name)
                    && path
                        .parent()
                        .is_some_and(|parent| parent.ends_with(&folder)))
                .then(|| worktree.absolutize(&entry.path))
            })
        })
    }

    fn open(&self, path: PathBuf, row: u32, window: &mut Window, cx: &mut App) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let open = workspace.update(cx, |workspace, cx| {
            workspace.open_abs_path(path, OpenOptions::default(), window, cx)
        });
        window
            .spawn(cx, async move |cx| {
                let item = open.await?;
                cx.update(|window, cx| {
                    if let Some(editor) = item.downcast::<editor::Editor>() {
                        editor.update(cx, |editor, cx| {
                            editor.go_to_singleton_buffer_point(
                                language::Point::new(row, 0),
                                window,
                                cx,
                            )
                        });
                    }
                })
            })
            .detach_and_log_err(cx);
    }

    /// "Click to see difference": the expected and actual values side by side in a diff tab.
    fn show_difference(&self, ix: usize, window: &mut Window, cx: &mut App) {
        let Some(failure) = &self.results.nodes[ix].failure else {
            return;
        };
        let (Some(expected), Some(actual)) = (&failure.expected, &failure.actual) else {
            return;
        };
        let folder = paths::temp_dir()
            .join("comparison-failure")
            .join(ix.to_string());
        let (left, right) = (folder.join("Expected"), folder.join("Actual"));
        let written = std::fs::create_dir_all(&folder)
            .and_then(|()| std::fs::write(&left, expected))
            .and_then(|()| std::fs::write(&right, actual));
        if written.log_err().is_none() {
            return;
        }
        project_panel::file_diff_view::FileDiffView::open(
            left,
            right,
            None,
            self.workspace.clone(),
            window,
            cx,
        )
        .detach_and_log_err(cx);
    }

    /// Export Test Results…: the run in IntelliJ's XML format, in the project's folder.
    fn export(&self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(root) = workspace
            .read(cx)
            .project()
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
        else {
            return;
        };
        let path = root.join(format!("Test Results - {}.xml", self.name));
        let text = export_xml(&self.results, &self.name);
        let shown = path.display().to_string();
        cx.spawn(async move |_, cx| {
            let written = std::fs::write(&path, text);
            workspace.update(cx, |workspace, cx| match written {
                Ok(()) => {
                    struct Exported;
                    workspace.show_toast(
                        workspace::Toast::new(
                            workspace::notifications::NotificationId::unique::<Exported>(),
                            format!("Test results exported successfully to {shown}"),
                        ),
                        cx,
                    )
                }
                Err(error) => {
                    workspace.show_error(anyhow::anyhow!("Test results export failed: {error}"), cx)
                }
            })
        })
        .detach();
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let separator = || div().w(px(1.)).h(px(20.)).mx(px(3.)).bg(tokens.separator);
        let failed = self.results.failed_tests();
        let this = cx.entity().downgrade();
        let toggle = |id: &'static str,
                      icon: &'static str,
                      label: &'static str,
                      on: bool,
                      flip: fn(&mut Self),
                      cx: &mut Context<Self>| {
            ActionButton::new(id, icon)
                .tooltip(label)
                .selected(on)
                .on_click(cx.listener(move |this, _, _, cx| {
                    flip(this);
                    cx.notify();
                }))
        };
        let (sort, on_top) = (self.options.sort, self.options.suites_on_top);
        let sorting = ActionButton::new("tests-sort", "runConfigurations/sortByDuration")
            .tooltip("Sorting Options")
            .dropdown_menu({
                let this = this.clone();
                move |menu, _, _| {
                    let item =
                        |label: &'static str, checked: bool, change: fn(&mut TestRunView)| {
                            let this = this.clone();
                            PopupMenuItem::new(label)
                                .checked(checked)
                                .on_click(move |_, _, cx| {
                                    this.update(cx, |this, cx| {
                                        change(this);
                                        cx.notify();
                                    })
                                    .log_err();
                                })
                        };
                    menu.item(item("Suites Always on Top", on_top, |this| {
                        this.options.suites_on_top = !this.options.suites_on_top
                    }))
                    .item(item(
                        "Sort Alphabetically",
                        sort == Sort::Alphabetically,
                        |this| {
                            this.options.sort = match this.options.sort {
                                Sort::Alphabetically => Sort::Run,
                                _ => Sort::Alphabetically,
                            }
                        },
                    ))
                    .item(item(
                        "Sort By Duration",
                        sort == Sort::Duration,
                        |this| {
                            this.options.sort = match this.options.sort {
                                Sort::Duration => Sort::Run,
                                _ => Sort::Duration,
                            }
                        },
                    ))
                }
            });
        let settings = (
            self.options.track_running,
            self.options.inline_statistics,
            self.options.select_first_failed,
        );
        let any_failed = !failed.is_empty();
        let settings_view = this.clone();
        let more = ActionButton::new("tests-more", "general/moreVertical")
            .tooltip("More")
            .dropdown_menu(move |menu, window, cx| {
                let item = |label: &'static str,
                                enabled: bool,
                                act: fn(
                        &mut TestRunView,
                        &mut Window,
                        &mut Context<TestRunView>,
                    )| {
                        let this = this.clone();
                        PopupMenuItem::new(label).disabled(!enabled).on_click(
                            move |_, window, cx| {
                                this.update(cx, |this, cx| {
                                    act(this, window, cx);
                                    cx.notify();
                                })
                                .log_err();
                            },
                        )
                    };
                let (track, statistics, first_failed) = settings;
                let settings_view = settings_view.clone();
                menu.item(item("Expand All", true, |this, _, _| {
                    this.expanded = true;
                    this.tree_key += 1;
                }))
                .item(item("Collapse All", true, |this, _, _| {
                    this.expanded = false;
                    this.tree_key += 1;
                }))
                .separator()
                .item(item("Previous Failed Test", any_failed, |this, _, cx| {
                    this.select_failed(false, cx)
                }))
                .item(item("Next Failed Test", any_failed, |this, _, cx| {
                    this.select_failed(true, cx)
                }))
                .item(item("Export Test Results…", true, |this, _, cx| {
                    this.export(cx)
                }))
                .separator()
                .submenu(
                    "Test Runner Settings",
                    window,
                    cx,
                    move |menu, _, _| {
                        let check =
                            |label: &'static str, checked: bool, change: fn(&mut TestRunView)| {
                                let this = settings_view.clone();
                                PopupMenuItem::new(label).checked(checked).on_click(
                                    move |_, _, cx| {
                                        this.update(cx, |this, cx| {
                                            change(this);
                                            cx.notify();
                                        })
                                        .log_err();
                                    },
                                )
                            };
                        menu.item(check("Track Running Test", track, |this| {
                            this.options.track_running = !this.options.track_running
                        }))
                        .item(check("Show Inline Statistics", statistics, |this| {
                            this.options.inline_statistics = !this.options.inline_statistics
                        }))
                        .separator()
                        .item(PopupMenuItem::new("Scroll To").disabled(true))
                        .item(PopupMenuItem::new("Open Source at Exception").disabled(true))
                        .separator()
                        .item(check(
                            "Select First Failed Test When Finished",
                            first_failed,
                            |this| {
                                this.options.select_first_failed = !this.options.select_first_failed
                            },
                        ))
                    },
                )
            });
        h_flex()
            .id("tests-toolbar")
            .role(Role::Toolbar)
            .aria_label("TestTreeViewToolbar")
            .flex_none()
            .h(px(32.))
            .px(px(5.))
            .gap(px(4.))
            .child(
                ActionButton::new("tests-rerun", "run/rerun")
                    .tooltip(format!("Rerun '{}' (⌘R)", self.name))
                    .on_click(cx.listener(|this, _, window, cx| (this.actions.rerun)(window, cx))),
            )
            .child(
                ActionButton::new("tests-rerun-failed", "run/restartFailedTests")
                    .tooltip("Rerun Failed Tests")
                    .disabled(!any_failed || self.running)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let failed = this.results.failed_tests();
                        (this.actions.rerun_failed)(failed, window, cx)
                    })),
            )
            .child(
                ActionButton::new("tests-stop", "run/stop")
                    .tooltip(format!("Stop '{}' (⌘F2)", self.name))
                    .disabled(!self.running)
                    .on_click(cx.listener(|this, _, window, cx| (this.actions.stop)(window, cx))),
            )
            .child(separator())
            .child(toggle(
                "tests-show-passed",
                "run/showPassed",
                "Show Passed",
                self.options.show_passed,
                |this| this.options.show_passed = !this.options.show_passed,
                cx,
            ))
            .child(toggle(
                "tests-show-ignored",
                "run/showIgnored",
                "Show Ignored",
                self.options.show_ignored,
                |this| this.options.show_ignored = !this.options.show_ignored,
                cx,
            ))
            .child(separator())
            .child(sorting)
            .child(separator())
            .child(more)
    }

    fn status_line(&self, cx: &App) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let counts = self.results.counts();
        let state = self.results.state(0);
        let (parts, description) = status_text(counts, self.results.duration(0), !self.running);
        let starting = counts.finished == 0 && self.running;
        let label: String = match starting {
            true => "Starting…".into(),
            false => parts.iter().map(|(text, _)| text.as_str()).collect(),
        };
        h_flex()
            .id("tests-status")
            .role(Role::Status)
            .aria_label(SharedString::from(
                format!("{label}  {description}").trim().to_owned(),
            ))
            .flex_none()
            .h(px(28.))
            .px(px(8.))
            .gap(px(6.))
            .map(|row| match (self.running, state.icon()) {
                (false, Some(icon)) => row.child(spork_workbench::image(icon, cx)),
                _ => row,
            })
            .map(|row| match starting {
                true => row.child("Starting…"),
                false => row.child(h_flex().children(parts.into_iter().map(|(text, state)| {
                    let color = match state {
                        Some(TestState::Failed) => tokens.text_error,
                        Some(TestState::Ignored) => tokens.text_warning,
                        _ => tokens.text,
                    };
                    div().whitespace_nowrap().text_color(color).child(text)
                }))),
            })
            .child(div().text_color(tokens.text_secondary).child(description))
    }

    /// The selected node's console: its output and failures, its tests' for a suite.
    fn node_console(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let mut lines: Vec<AnyElement> = Vec::new();
        let tests = match self.results.nodes[ix].suite {
            true => self.results.tests(ix),
            false => vec![ix],
        };
        let mono = |line: String, color| {
            div()
                .whitespace_nowrap()
                .text_color(color)
                .child(match line.is_empty() {
                    true => " ".to_owned(),
                    false => line,
                })
                .into_any_element()
        };
        for test in tests {
            let node = &self.results.nodes[test];
            // a line in the error colour when stderr wrote some of it
            let (mut line, mut error) = (String::new(), false);
            for (piece, piece_error) in &node.output {
                for (at, part) in piece.split('\n').enumerate() {
                    if at > 0 {
                        let color = if error {
                            tokens.console_error
                        } else {
                            tokens.console_text
                        };
                        lines.push(mono(std::mem::take(&mut line), color));
                        error = false;
                    }
                    if !part.is_empty() {
                        line.push_str(part.trim_end_matches('\r'));
                        error |= *piece_error;
                    }
                }
            }
            if !line.is_empty() {
                let color = if error {
                    tokens.console_error
                } else {
                    tokens.console_text
                };
                lines.push(mono(line, color));
            }
            let Some(failure) = &node.failure else {
                continue;
            };
            let mut details = failure.details.lines();
            if let Some(first) = details.next() {
                lines.push(mono(first.to_owned(), tokens.console_error));
            }
            if let (Some(expected), Some(actual)) = (&failure.expected, &failure.actual) {
                lines.push(mono(format!("Expected :{expected}"), tokens.console_error));
                lines.push(mono(format!("Actual   :{actual}"), tokens.console_error));
                lines.push(
                    div()
                        .id(("tests-difference", test))
                        .role(Role::Link)
                        .aria_label("Click to see difference")
                        .whitespace_nowrap()
                        .text_color(tokens.text_link)
                        .underline()
                        .cursor_pointer()
                        .child("<Click to see difference>")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_difference(test, window, cx)
                        }))
                        .into_any_element(),
                );
                lines.push(mono(String::new(), tokens.console_error));
            }
            for (row, line) in details.enumerate() {
                lines.push(self.trace_line(test, row, line, cx));
            }
        }
        div()
            .id(("tests-node-console", ix))
            .role(Role::Log)
            .aria_label("Test Output")
            .size_full()
            .p(px(8.))
            .overflow_y_scroll()
            .font_family(SharedString::from("JetBrains Mono"))
            .text_size(px(13.))
            .children(lines)
            .into_any_element()
    }

    /// A stack trace's line, its frame's `File.java:12` a link to the place when the project has
    /// the file.
    fn trace_line(
        &self,
        test: usize,
        row: usize,
        line: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let frame = parse_frame(line);
        let target = frame.and_then(|(class, file, number)| {
            Some((self.source_of(class, Some(file), cx)?, number))
        });
        let Some(((start, end), (path, number))) =
            frame.map(|(_, _, _)| link_span(line)).zip(target)
        else {
            return div()
                .whitespace_nowrap()
                .text_color(tokens.console_error)
                .child(line.replace('\t', "    "))
                .into_any_element();
        };
        let label = line[start..end].to_owned();
        h_flex()
            .whitespace_nowrap()
            .text_color(tokens.console_error)
            .child(line[..start].replace('\t', "    "))
            .child(
                div()
                    .id(SharedString::from(format!("tests-frame-{test}-{row}")))
                    .role(Role::Link)
                    .aria_label(SharedString::from(label.clone()))
                    .text_color(tokens.text_link)
                    .underline()
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open(path.clone(), number.saturating_sub(1), window, cx)
                    })),
            )
            .child(line[end..].to_owned())
            .into_any_element()
    }

    /// What the end-to-end tests read: the tree's rows (`"name state duration"`, indented by
    /// depth), the status line, the selected node and its console's text.
    pub fn e2e_state(&self) -> Value {
        let mut rows = Vec::new();
        self.e2e_rows(0, 0, &mut rows);
        let (parts, description) = status_text(
            self.results.counts(),
            self.results.duration(0),
            !self.running,
        );
        let status: String = parts.iter().map(|(text, _)| text.as_str()).collect();
        let selected = &self.results.nodes[self.selected];
        json!({
            "rows": rows,
            "status": status,
            "description": description,
            "running": self.running,
            "selected": selected.name,
            "output": self.node_text(self.selected),
            "failed": self.results.failed_tests(),
        })
    }

    fn e2e_rows(&self, ix: usize, depth: usize, rows: &mut Vec<String>) {
        let node = &self.results.nodes[ix];
        rows.push(format!(
            "{}{} {}",
            "  ".repeat(depth),
            node.name,
            self.results.state(ix).name()
        ));
        if ix == 0 || self.expanded {
            for child in self.sorted_children(ix) {
                self.e2e_rows(child, depth + 1, rows);
            }
        }
    }

    /// A node's console as text.
    fn node_text(&self, ix: usize) -> String {
        let tests = match self.results.nodes[ix].suite {
            true => self.results.tests(ix),
            false => vec![ix],
        };
        let mut text = String::new();
        for test in tests {
            let node = &self.results.nodes[test];
            for (piece, _) in &node.output {
                text.push_str(piece);
            }
            if let Some(failure) = &node.failure {
                let mut details = failure.details.lines();
                text.push_str(details.next().unwrap_or_default());
                text.push('\n');
                if let (Some(expected), Some(actual)) = (&failure.expected, &failure.actual) {
                    text.push_str(&format!(
                        "Expected :{expected}\nActual   :{actual}\n<Click to see difference>\n\n"
                    ));
                }
                for line in details {
                    text.push_str(line);
                    text.push('\n');
                }
            }
        }
        text
    }

    /// Selects the node named `name` (the first one), for the end-to-end tests.
    pub fn e2e_select(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        match self.results.nodes.iter().position(|node| node.name == name) {
            Some(ix) => {
                self.selected = ix;
                cx.notify();
                true
            }
            None => false,
        }
    }
}

/// A stack trace frame's class, file and line: `at com.example.A.greets(A.java:12)`.
fn parse_frame(line: &str) -> Option<(&str, &str, u32)> {
    let frame = line.trim().strip_prefix("at ")?;
    let (call, place) = frame.strip_suffix(')')?.rsplit_once('(')?;
    let (file, number) = place.rsplit_once(':')?;
    let call = call.rsplit('/').next()?;
    let (class, _method) = call.rsplit_once('.')?;
    Some((class, file, number.parse().ok()?))
}

/// The byte range of a frame's `File.java:12` in its line.
fn link_span(line: &str) -> (usize, usize) {
    let start = line.rfind('(').map_or(0, |at| at + 1);
    let end = line.rfind(')').unwrap_or(line.len());
    (start, end)
}

/// IntelliJ's exported test results (`TestResultsXmlFormatter`).
fn export_xml(results: &TestResults, name: &str) -> String {
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    fn node(results: &TestResults, ix: usize, out: &mut String, depth: usize) {
        let entry = &results.nodes[ix];
        let indent = "  ".repeat(depth);
        let tag = if entry.suite { "suite" } else { "test" };
        let duration = results
            .duration(ix)
            .map(|d| format!(" duration=\"{d}\""))
            .unwrap_or_default();
        let location = entry
            .location
            .as_deref()
            .map(|l| format!(" locationUrl=\"{}\"", escape(l)))
            .unwrap_or_default();
        out.push_str(&format!(
            "{indent}<{tag}{duration}{location} name=\"{}\" status=\"{}\">\n",
            escape(&entry.name),
            results.state(ix).name()
        ));
        for child in &entry.children {
            node(results, *child, out, depth + 1);
        }
        let output: String = entry.output.iter().map(|(text, _)| text.as_str()).collect();
        if !output.is_empty() {
            out.push_str(&format!(
                "{indent}  <output type=\"stdout\">{}</output>\n",
                escape(&output)
            ));
        }
        if let Some(failure) = &entry.failure {
            out.push_str(&format!(
                "{indent}  <output type=\"stderr\">{}</output>\n",
                escape(&failure.details)
            ));
        }
        out.push_str(&format!("{indent}</{tag}>\n"));
    }
    let counts = results.counts();
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testrun duration=\"{}\" footerText=\"Generated by Spork IDE\" name=\"{}\">\n  <count name=\"total\" value=\"{}\"/>\n  <count name=\"passed\" value=\"{}\"/>\n  <count name=\"failed\" value=\"{}\"/>\n  <count name=\"ignored\" value=\"{}\"/>\n",
        results.duration(0).unwrap_or(0),
        escape(name),
        counts.total,
        counts.passed(),
        counts.failed,
        counts.ignored
    );
    for child in &results.nodes[0].children {
        node(results, *child, &mut out, 1);
    }
    out.push_str("</testrun>\n");
    out
}

impl Render for TestRunView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        self.selected = self
            .selected
            .min(self.results.nodes.len().saturating_sub(1));
        // the toolbar's options, as the user left them, for the next runs
        if cx.try_global::<GlobalOptions>().map(|o| o.0) != Some(self.options) {
            cx.set_global(GlobalOptions(self.options));
        }
        let root = self.item(0, cx);
        let console = match self.selected {
            0 => div()
                .size_full()
                .children(self.console.clone())
                .into_any_element(),
            ix => self.node_console(ix, cx),
        };
        v_flex().size_full().child(self.toolbar(cx)).child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(
                    div()
                        .id("tests-tree")
                        .role(Role::Tree)
                        .aria_label("Test Results")
                        .w(relative(0.35))
                        .flex_none()
                        .h_full()
                        .py(px(4.))
                        .overflow_y_scroll()
                        .child(
                            Tree::new(SharedString::from(format!("tests-tree-{}", self.tree_key)))
                                .children([root]),
                        ),
                )
                .child(div().w(px(1.)).h_full().bg(tokens.separator))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.status_line(cx))
                        .child(div().h(px(1.)).bg(tokens.separator))
                        .child(div().flex_1().min_h_0().child(console)),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Counts, TestResults, TestState, format_duration, parse_frame, parse_message, status_text,
    };

    #[test]
    fn reads_service_messages() {
        let (name, attributes) = parse_message(
            "##teamcity[testFailed nodeId='2' message='expected:<|[a|]> |'x|' |n' type='comparisonFailure' expected='a|0x00e9']",
        )
        .expect("a message");
        assert_eq!(name, "testFailed");
        assert_eq!(
            attributes[1],
            ("message".into(), "expected:<[a]> 'x' \n".into())
        );
        assert_eq!(attributes[3], ("expected".into(), "aé".into()));
        assert!(parse_message("Hello").is_none());
    }

    #[test]
    fn builds_the_tree_and_counts() {
        let mut results = TestResults::default();
        for line in [
            "##teamcity[testCount count='3']",
            "##teamcity[testSuiteStarted nodeId='1' parentNodeId='0' name='GreetingTest' locationHint='java:suite://com.example.GreetingTest']",
            "##teamcity[testStarted nodeId='2' parentNodeId='1' name='greets' locationHint='java:test://com.example.GreetingTest/greets']",
            "##teamcity[testStdOut nodeId='2' out='hi|n']",
            "##teamcity[testFinished nodeId='2' duration='12']",
            "##teamcity[testStarted nodeId='3' parentNodeId='1' name='fails' locationHint='java:test://com.example.GreetingTest/fails']",
            "##teamcity[testFailed nodeId='3' message='m' details='java.lang.AssertionError: m|n\tat x' type='comparisonFailure' expected='a' actual='b']",
            "##teamcity[testFinished nodeId='3' duration='3']",
            "##teamcity[testStarted nodeId='4' parentNodeId='1' name='later' locationHint='java:test://com.example.GreetingTest/later']",
            "##teamcity[testIgnored nodeId='4' message='']",
            "##teamcity[testFinished nodeId='4' duration='0']",
            "##teamcity[testSuiteFinished nodeId='1']",
        ] {
            assert!(results.apply(line), "{line}");
        }
        results.end();
        assert_eq!(results.state(1), TestState::Failed);
        assert_eq!(results.state(2), TestState::Passed);
        assert_eq!(results.state(4), TestState::Ignored);
        assert_eq!(results.duration(0), Some(15));
        assert_eq!(
            results.counts(),
            Counts {
                total: 3,
                finished: 3,
                failed: 1,
                ignored: 1
            }
        );
        assert_eq!(
            results.failed_tests(),
            vec!["com.example.GreetingTest#fails"]
        );
        let (parts, description) = status_text(results.counts(), Some(15), true);
        let text: String = parts.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "1 test failed, 1 passed, 1 ignored");
        assert_eq!(description, "3 tests total, 15\u{2009}ms");
    }

    #[test]
    fn writes_durations_and_frames_as_intellij() {
        assert_eq!(format_duration(120), "120\u{2009}ms");
        assert_eq!(format_duration(1200), "1\u{2009}sec 200\u{2009}ms");
        assert_eq!(format_duration(3000), "3\u{2009}sec");
        assert_eq!(format_duration(0), "0\u{2009}ms");
        assert_eq!(
            parse_frame("\tat com.example.GreetingTest.fails(GreetingTest.java:17)"),
            Some(("com.example.GreetingTest", "GreetingTest.java", 17))
        );
        assert_eq!(
            parse_frame("\tat java.base/java.lang.reflect.Method.invoke(Method.java:569)"),
            Some(("java.lang.reflect.Method", "Method.java", 569))
        );
        let (parts, _) = status_text(
            Counts {
                total: 2,
                finished: 2,
                failed: 0,
                ignored: 0,
            },
            None,
            true,
        );
        assert_eq!(parts[0].0, "2 tests passed");
    }
}
