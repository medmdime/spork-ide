//! IntelliJ's run and debug surfaces over Zed's engines (ROADMAP, phase 8): the main toolbar's
//! run widget and its popup, the Run/Debug Configurations dialog over Zed's tasks and debug
//! scenarios, the Run tool window (⌘4) running Zed's tasks in its consoles, the Debug tool window
//! (⌘5) drawn over Zed's debugger, IntelliJ's gutter breakpoints, their popup and the Breakpoints
//! dialog over Zed's breakpoint store, and the Run menu's actions.

mod attach;
mod breakpoints;
mod class_chooser;
pub mod configurations;
mod configurations_dialog;
mod debug_window;
mod docker;
mod evaluate_dialog;
mod list_popup;
pub mod run_panel;
pub mod run_widget;
pub mod services;
mod smart_step_into;
pub mod test_runner;
mod threads;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use collections::HashMap;
use fs::Fs;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, SharedString, Task, TaskExt as _,
    Window,
};
use project::Project;
use serde_json::{Value, json};
use util::ResultExt as _;
use workspace::Workspace;

pub use attach::jdwp_address;
pub use breakpoints::{
    ExceptionBreakpoints, ExceptionClassBreakpoint, ExceptionClasses, exception_breakpoints,
    register_exception_classes, register_exception_filters,
};
pub use class_chooser::ClassSearch;
pub use configurations::Configuration;
pub use configurations_dialog::follow_ide_theme;
pub use debug_window::{
    AdapterHooks, active_debug_session, e2e_frames, register_adapter_hooks, running_state,
    write_program_console,
};
pub use docker::Confirm;
pub use evaluate_dialog::{ask_object_label, object_label_color};
pub use run_widget::RunWidget;
pub use services::is_running;
pub use threads::{DumpThread, ThreadInfo, ThreadsView, jvm_frame_label};

pub mod run {
    gpui::actions!(
        run,
        [
            /// Opens the Run/Debug Configurations dialog.
            EditConfigurations,
            /// Shows the configurations to run one.
            ChooseRunConfiguration,
            /// Shows the configurations to debug one.
            ChooseDebugConfiguration,
            /// Stops the process of the Run or Debug tool window's selected tab.
            Stop,
            /// Runs the Run tool window's selected tab again.
            Rerun,
            /// Opens the Breakpoints dialog.
            ViewBreakpoints,
            /// Mutes or unmutes the breakpoints of the debug sessions.
            MuteBreakpoints,
            /// Enables or disables the caret line's breakpoint.
            ToggleBreakpointEnabled,
            /// Sets a breakpoint removed once hit on the caret line, or removes the line's.
            ToggleTemporaryLineBreakpoint,
            /// Sets a breakpoint on the entry of the method at the caret, or removes it.
            ToggleMethodBreakpoint,
            /// Watches the field at the caret, or stops watching it.
            ToggleFieldWatchpoint,
            /// Shows the line where the debugged program stopped.
            ShowExecutionPoint,
            /// Opens the Evaluate dialog on the selected frame.
            EvaluateExpression,
            /// Shows the value of the selection, or of the word at the caret, under it.
            QuickEvaluateExpression,
            /// Steps into the call the chooser picks among the calls of the line.
            SmartStepInto,
            /// Steps over, the breakpoints skipped until the program stops again.
            ForceStepOver,
            /// Steps into, past the stepping filters (IntelliJ's libraries, constructors…).
            ForceStepInto,
            /// Runs to the caret's line, the breakpoints skipped until the program stops again.
            ForceRunToCursor,
            /// Runs the selected frame again from its first line.
            ResetFrame,
            /// Shows the local processes to attach the debugger to one.
            AttachToProcess,
            /// Shows the running processes to stop some of them.
            StopBackgroundProcesses,
            /// Shows the configurations running at the moment.
            ShowRunningList,
            /// Shows the popup of the caret line's breakpoint (IntelliJ's Edit Breakpoint); the
            /// Breakpoints dialog without one.
            EditBreakpoint,
            /// Ends the input of the process of the Run tool window's selected tab (IntelliJ's
            /// Send EOF).
            SendEof,
            /// Updates a running application as its configuration's On 'Update' action says
            /// (IntelliJ's Update Running Application).
            UpdateRunningApplication,
            /// Runs the configuration of the caret's test method, test class or `main`, or of the
            /// Project tool window's selection (IntelliJ's Run Context Configuration).
            RunContextConfiguration,
            /// Debugs the configuration of the caret's test method, test class or `main`, or of
            /// the Project tool window's selection (IntelliJ's Debug Context Configuration).
            DebugContextConfiguration,
            /// Opens the selected value's tree in a window of its own.
            Inspect,
            /// Labels the selected object in the debugger's views, or takes its label off.
            MarkObject,
            /// Opens where the selected variable is declared.
            JumpToSource,
            /// Opens the source of the selected value's type.
            JumpToTypeSource,
            /// Shows or hides the values' `{Type@id}`.
            ShowTypes,
            /// Shows the debugged program's threads with their state and frames in a tab.
            GetThreadDump,
        ]
    );

    /// Shows the selected value with a renderer (IntelliJ's View as), `""` for the automatic one.
    #[derive(PartialEq, Clone, serde::Deserialize, schemars::JsonSchema, Default, gpui::Action)]
    #[action(namespace = run)]
    #[serde(deny_unknown_fields)]
    pub struct ViewAs {
        pub renderer: String,
    }
}

pub fn init(cx: &mut App) {
    breakpoints::init(cx);
    test_runner::init(cx);
    debug_window::init(cx);
    cx.observe_new(|workspace: &mut Workspace, _, cx| {
        let configurations = cx.new(|cx| ProjectConfigurations::new(workspace, cx));
        let id = cx.entity_id();
        cx.default_global::<Registry>().0.insert(id, configurations);
        cx.on_release(move |_, cx| {
            cx.default_global::<Registry>().0.remove(&id);
        })
        .detach();
        run_widget::register(workspace);
        run_panel::register(workspace);
        breakpoints::register(workspace, cx);
        configurations_dialog::register(workspace);
        debug_window::register(workspace);
        attach::register(workspace);
        services::register(workspace);
    })
    .detach();
}

/// Adds the Run tool window to a workspace whose panels are loaded, its consoles taking the
/// workspace's tasks from the Terminal tool window.
pub fn add_run_panel(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    run_panel::add(workspace, window, cx);
    services::add(workspace, window, cx);
}

/// Runs or debugs a configuration made from the code (a context menu's Run 'Tests in …'), made
/// temporary and selected unless one of its name exists, as IntelliJ's.
pub fn run_temporary(
    configuration: Configuration,
    debug: bool,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    // the configuration of that name runs, as IntelliJ's finds the existing one first
    let configuration = match ProjectConfigurations::of(workspace, cx) {
        Some(configurations) => configurations.update(cx, |configurations, cx| {
            configurations.run_temporary(configuration.clone(), cx);
            configurations
                .list
                .iter()
                .find(|listed| listed.name() == configuration.name())
                .cloned()
                .unwrap_or(configuration)
        }),
        None => configuration,
    };
    run_widget::launch(Some(&configuration), debug, workspace, window, cx);
}

/// Runs a configuration made from the code with coverage (a context menu's or the gutter's Run
/// '…' with Coverage), made temporary and selected as [`run_temporary`] makes it.
pub fn run_temporary_with_coverage(
    configuration: Configuration,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(configurations) = ProjectConfigurations::of(workspace, cx) {
        configurations.update(cx, |configurations, cx| {
            configurations.run_temporary(configuration.clone(), cx)
        });
    }
    run_widget::launch_with_coverage(&configuration, workspace, window, cx);
}

/// Whether a configuration runs with coverage (its type's runner gathers it).
pub fn can_cover(configuration: &Configuration, cx: &App) -> bool {
    run_widget::can_cover(Some(configuration), cx)
}

/// Runs a test configuration for its debug adapter `adapter` to attach to, as IntelliJ debugs
/// tests: the configuration made temporary as a run of it would, then run in the window of
/// `workspace` with its JVM waiting for the debugger on a free port of the loopback interface
/// ([`configurations::DEBUG_JVM`]), its tests shown in the Debug tool window's session; the port,
/// once the JVM listens.
pub async fn run_for_debugger(
    workspace: &Entity<Workspace>,
    configuration: Configuration,
    adapter: &str,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<u16> {
    let port = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port();
    let name = configuration.name();
    let window = cx.update(|cx| window_of(workspace, cx));
    let window = window.context("The project's window is closed")?;
    // the debugged run of this name before this one, which the new one replaces
    let debugged_run = |cx: &App| {
        run_panel::debugged_tests(workspace.read(cx), &name, cx).map(|(_, terminal)| terminal)
    };
    let before = cx.update(|cx| debugged_run(cx).map(|terminal| terminal.entity_id()));
    window.update(cx, |_, window, cx| {
        workspace.update(cx, |workspace, cx| {
            // the configuration of that name, as a run of it would; Rerun Failed Tests' own
            let rerun = configuration
                .entry
                .contains_key(configurations::RERUN_TESTS);
            let mut debugged = match ProjectConfigurations::of(workspace, cx) {
                Some(configurations) => configurations.update(cx, |configurations, cx| {
                    configurations.run_temporary(configuration.clone(), cx);
                    configurations
                        .list
                        .iter()
                        .find(|listed| !rerun && listed.name() == configuration.name())
                        .cloned()
                        .unwrap_or(configuration)
                }),
                None => configuration,
            };
            debugged
                .entry
                .insert(configurations::DEBUG_JVM.into(), port.into());
            debugged
                .entry
                .insert(configurations::DEBUG_ADAPTER.into(), adapter.into());
            run_widget::launch(Some(&debugged), false, workspace, window, cx);
        })
    })?;
    // the build comes first, then the JVM says it listens
    let listening = configurations::debug_jvm_listening(port);
    let step = std::time::Duration::from_millis(250);
    let mut waited = std::time::Duration::ZERO;
    loop {
        let run = cx.update(|cx| {
            let terminal = debugged_run(cx).filter(|t| Some(t.entity_id()) != before)?;
            let terminal = terminal.read(cx);
            let running = terminal
                .task()
                .is_some_and(|task| task.status == terminal::TaskStatus::Running);
            Some((terminal.get_content().contains(&listening), running))
        });
        match run {
            Some((true, _)) => return Ok(port),
            Some((false, false)) => {
                anyhow::bail!("The tests ended before the debugger could attach to them")
            }
            _ => {}
        }
        anyhow::ensure!(
            waited < std::time::Duration::from_secs(600),
            "The JVM of the tests isn't waiting for the debugger"
        );
        cx.background_executor().timer(step).await;
        waited += step;
    }
}

/// The window of a workspace.
pub(crate) fn window_of(
    workspace: &Entity<Workspace>,
    cx: &mut App,
) -> Option<gpui::AnyWindowHandle> {
    cx.windows().into_iter().find(|window| {
        window
            .update(cx, |_, window, cx| {
                Workspace::for_window(window, cx).as_ref() == Some(workspace)
            })
            .unwrap_or(false)
    })
}

/// What a run's end hands to the app, set once: the workspace and the environment of the run's
/// task (a run with coverage names its suite there, `spork_coverage`'s).
#[derive(Clone)]
pub struct RunEnded(
    pub std::rc::Rc<dyn Fn(&Entity<Workspace>, &HashMap<String, String>, &mut App)>,
);

impl Global for RunEnded {}

/// The Run menu's Run, Debug and Run with Coverage items as IntelliJ's executors name them for
/// the run widget's configuration (`Run 'GreetingTest'`, `Run 'GreetingTest.java'` for the
/// current file), greyed when the current file doesn't run, and Debugging Actions' Update item:
/// the one running application's update ("Update 'X' application"), `None` for IntelliJ's
/// "Update Running Application…" when none or several are; the app builds its menu bar again
/// when they change.
#[derive(Clone, Debug, PartialEq)]
pub struct RunMenu {
    pub run: SharedString,
    pub debug: SharedString,
    pub coverage: SharedString,
    pub enabled: bool,
    pub update: Option<SharedString>,
}

impl Default for RunMenu {
    fn default() -> Self {
        Self {
            run: "Run".into(),
            debug: "Debug".into(),
            coverage: "Run with Coverage".into(),
            enabled: true,
            update: None,
        }
    }
}

impl Global for RunMenu {}

/// The Run menu's Run and Debug items.
pub fn run_menu(cx: &App) -> RunMenu {
    cx.try_global::<RunMenu>().cloned().unwrap_or_default()
}

/// The configurations of each workspace, by its entity.
#[derive(Default)]
struct Registry(HashMap<EntityId, Entity<ProjectConfigurations>>);

impl Global for Registry {}

/// A workspace's run configurations, which the run widget, its popup and the dialog show, and the
/// one the run widget runs.
pub struct ProjectConfigurations {
    fs: Arc<dyn Fs>,
    project: Entity<Project>,
    pub list: Vec<Configuration>,
    /// The configuration the run widget runs; `None` is IntelliJ's Current File.
    pub selected: Option<SharedString>,
    /// The configurations started and pinned, kept per project.
    pub history: StartHistory,
    /// The folder `history` was read for.
    history_root: Option<PathBuf>,
    _subscriptions: Vec<gpui::Subscription>,
    _load: Task<()>,
}

impl ProjectConfigurations {
    fn new(workspace: &Workspace, cx: &mut Context<Self>) -> Self {
        let project = workspace.project().clone();
        // the project tells a tasks.json or debug.json was read again only by its toasts
        let reload = cx.subscribe(&project, |this, _, event, cx| match event {
            project::Event::Toast {
                notification_id, ..
            }
            | project::Event::HideToast { notification_id }
                if notification_id.starts_with("local-tasks-")
                    || notification_id.starts_with("local-debug-scenarios-") =>
            {
                this.reload(cx)
            }
            project::Event::WorktreeAdded(_) => this.reload(cx),
            _ => {}
        });
        // the run widget shows Stop while a debug session runs
        let sessions = cx.subscribe(&project.read(cx).dap_store(), |_, _, event, cx| {
            if let project::debugger::dap_store::DapStoreEvent::DebugClientStarted(_)
            | project::debugger::dap_store::DapStoreEvent::DebugClientShutdown(_) = event
            {
                cx.notify();
            }
        });
        let mut this = Self {
            fs: workspace.app_state().fs.clone(),
            project,
            list: Vec::new(),
            selected: None,
            history: StartHistory::default(),
            history_root: None,
            _subscriptions: vec![reload, sessions],
            _load: Task::ready(()),
        };
        this.reload(cx);
        this
    }

    /// The configurations of `workspace`.
    pub fn of(workspace: &Workspace, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Registry>()?
            .0
            .get(&workspace.weak_handle().entity_id())
            .cloned()
    }

    fn root(&self, cx: &App) -> Option<PathBuf> {
        self.project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
    }

    pub fn files(&self, cx: &App) -> configurations::Files {
        configurations::Files::new(self.root(cx).as_deref())
    }

    /// Reads the configurations from their files again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let root = self.root(cx);
        if root != self.history_root {
            self.history = root
                .as_deref()
                .map(|root| StartHistory::read(root, cx))
                .unwrap_or_default();
            self.history_root = root;
        }
        let load = configurations::load(self.fs.clone(), self.files(cx));
        self._load = cx.spawn(async move |this, cx| {
            let mut list = load.await;
            this.update(cx, |this, cx| {
                list.extend(this.history.temporary.iter().filter_map(|entry| {
                    Some(Configuration {
                        adapter: Some(entry.get("adapter")?.as_str()?.to_owned().into()),
                        shared: false,
                        entry: entry.clone(),
                    })
                }));
                // the run widget keeps the first configuration once one appears, as IntelliJ
                // selects one it creates
                if this.selected.is_none() && this.list.is_empty() {
                    this.selected = list.first().map(Configuration::name);
                }
                if let Some(name) = &this.selected
                    && !list.iter().any(|c| &c.name() == name)
                {
                    this.selected = list.first().map(Configuration::name);
                }
                this.list = list;
                cx.notify();
            })
            .ok();
        });
    }

    /// The configuration the run widget runs; `None` for the current file.
    pub fn selected(&self) -> Option<&Configuration> {
        let name = self.selected.as_ref()?;
        self.list.iter().find(|c| &c.name() == name)
    }

    /// Saves the dialog's configurations to their files, the temporary ones for the project,
    /// then reads them again.
    pub fn save(
        &mut self,
        list: Vec<Configuration>,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let kept = |list: &[Configuration]| -> Vec<Configuration> {
            list.iter().filter(|c| !c.is_temporary()).cloned().collect()
        };
        let save = configurations::save(
            self.fs.clone(),
            self.files(cx),
            kept(&self.list),
            kept(&list),
        );
        let temporary: Vec<_> = list
            .iter()
            .filter(|c| c.is_temporary())
            .map(|c| c.entry.clone())
            .collect();
        if temporary != self.history.temporary {
            self.history.temporary = temporary;
            self.write_history(cx);
        }
        self.list = list;
        cx.notify();
        cx.spawn(async move |this, cx| {
            save.await?;
            this.update(cx, |this, cx| this.reload(cx))?;
            Ok(())
        })
    }

    /// A run from the gutter or a context menu: the configuration of that name, made temporary
    /// when there is none (IntelliJ keeps the last five), selected in the run widget.
    pub fn run_temporary(&mut self, configuration: Configuration, cx: &mut Context<Self>) {
        let name = configuration.name();
        self.selected = Some(name.clone());
        if self.list.iter().any(|c| c.name() == name) {
            cx.notify();
            return;
        }
        let mut configuration = configuration;
        configuration
            .entry
            .insert(configurations::TEMPORARY.into(), Value::Bool(true));
        configuration.shared = false;
        let mut list = self.list.clone();
        list.push(configuration);
        let temporary: Vec<usize> = (0..list.len())
            .filter(|ix| list[*ix].is_temporary())
            .collect();
        if let Some(oldest) = temporary.first().filter(|_| temporary.len() > 5) {
            list.remove(*oldest);
        }
        self.save(list, cx).detach_and_log_err(cx);
    }

    /// Save Configuration: a temporary configuration kept with the others, private until "Store
    /// as project file".
    pub fn save_temporary(
        &mut self,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let mut list = self.list.clone();
        for configuration in list.iter_mut().filter(|c| c.name().as_ref() == name) {
            configuration.entry.remove(configurations::TEMPORARY);
        }
        self.save(list, cx)
    }

    /// A configuration was run or debugged: it leads the Recent Configurations.
    pub fn started(&mut self, name: &str, cx: &mut Context<Self>) {
        self.history.history.retain(|other| other != name);
        self.history.history.insert(0, name.to_owned());
        self.write_history(cx);
    }

    /// Pin or Unpin: the configuration in the popup's Pinned group, or out of it.
    pub fn toggle_pin(&mut self, name: &str, cx: &mut Context<Self>) {
        match self.history.pinned.iter().position(|other| other == name) {
            Some(ix) => {
                self.history.pinned.remove(ix);
            }
            None => self.history.pinned.push(name.to_owned()),
        }
        self.write_history(cx);
    }

    /// Toggle All Run/Debug Configurations: the popup lists every configuration under its
    /// Recent Configurations, or hides them.
    pub fn toggle_all_configurations(&mut self, cx: &mut Context<Self>) {
        self.history.all_expanded = !self.history.all_expanded;
        self.write_history(cx);
    }

    /// Delete…: the configuration out of its file.
    pub fn delete(&mut self, name: &str, cx: &mut Context<Self>) -> Task<anyhow::Result<()>> {
        let mut list = self.list.clone();
        list.retain(|configuration| configuration.name().as_ref() != name);
        self.save(list, cx)
    }

    fn write_history(&self, cx: &mut Context<Self>) {
        cx.notify();
        let Some(root) = self.history_root.as_deref() else {
            return;
        };
        let key = StartHistory::key(root);
        let value = serde_json::to_string(&self.history);
        let store = db::kvp::KeyValueStore::global(cx);
        cx.background_spawn(async move { store.write_kvp(key, value?).await })
            .detach_and_log_err(cx);
    }

    pub fn fs(&self) -> Arc<dyn Fs> {
        self.fs.clone()
    }

    pub fn project(&self) -> &Entity<Project> {
        &self.project
    }
}

/// IntelliJ's run configuration start history (`RunConfigurationStartHistory`): the
/// configurations started, the last first, those pinned, and whether the popup shows All
/// Configurations under its recent ones.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct StartHistory {
    pub history: Vec<String>,
    pub pinned: Vec<String>,
    pub all_expanded: bool,
    /// The temporary configurations' entries.
    #[serde(default)]
    pub temporary: Vec<serde_json::Map<String, Value>>,
}

impl StartHistory {
    fn key(root: &std::path::Path) -> String {
        format!("spork_run_history_{}", root.display())
    }

    fn read(root: &std::path::Path, cx: &App) -> Self {
        db::kvp::KeyValueStore::global(cx)
            .read_kvp(&Self::key(root))
            .log_err()
            .flatten()
            .and_then(|value| serde_json::from_str::<Self>(&value).log_err())
            .unwrap_or_default()
    }
}

/// What the end-to-end tests read of the run and debug surfaces of the window's workspace.
pub fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let configurations = ProjectConfigurations::of(workspace, cx).map(|c| {
        let c = c.read(cx);
        json!({
            "names": c.list.iter().map(|c| c.name().to_string()).collect::<Vec<_>>(),
            "selected": c.selected.as_ref().map(|s| s.to_string()),
            "recent": c.history.history,
            "pinned": c.history.pinned,
        })
    });
    json!({
        "configurations": configurations,
        "tabs": run_panel::e2e_tabs(workspace, cx),
        "breakpoints": breakpoints::e2e_breakpoints(workspace, cx),
        "muted": breakpoints::muted(cx),
        "debug": debug_window::e2e_debug(workspace, cx),
        "evaluate": evaluate_dialog::e2e_evaluate(cx),
        "exceptions": breakpoints::e2e_exceptions(workspace, cx),
        "exception_classes": cx
            .try_global::<ExceptionClasses>()
            .and_then(|classes| serde_json::to_value(&classes.0).log_err()),
        "services": services::e2e_services(workspace, cx),
        "smart_step": workspace
            .active_modal::<smart_step_into::SmartStepChooser>(cx)
            .map(|chooser| chooser.read(cx).e2e_state()),
        "updaters": services::e2e_updaters(workspace, cx),
    })
}
