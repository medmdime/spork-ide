//! IntelliJ's run and debug surfaces over Zed's engines (ROADMAP, phase 8): the main toolbar's
//! run widget and its popup, the Run/Debug Configurations dialog over Zed's tasks and debug
//! scenarios, the Run tool window (⌘4) running Zed's tasks in its consoles, the Debug tool window
//! (⌘5) drawn over Zed's debugger, IntelliJ's gutter breakpoints, their popup and the Breakpoints
//! dialog over Zed's breakpoint store, and the Run menu's actions.

mod attach;
mod breakpoints;
pub mod configurations;
mod configurations_dialog;
mod debug_window;
mod docker;
mod evaluate_dialog;
mod list_popup;
pub mod run_panel;
pub mod run_widget;
pub mod services;

use std::path::PathBuf;
use std::sync::Arc;

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
pub use breakpoints::register_exception_filters;
pub use configurations::Configuration;
pub use configurations_dialog::follow_ide_theme;
pub use debug_window::{
    AdapterHooks, active_debug_session, e2e_frames, register_adapter_hooks, write_program_console,
};
pub use docker::Confirm;
pub use run_widget::RunWidget;

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
        ]
    );
}

pub fn init(cx: &mut App) {
    breakpoints::init(cx);
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
        breakpoints::register(workspace);
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
            let list = load.await;
            this.update(cx, |this, cx| {
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

    /// Saves the dialog's configurations to their files, then reads them again.
    pub fn save(
        &mut self,
        list: Vec<Configuration>,
        cx: &mut Context<Self>,
    ) -> Task<anyhow::Result<()>> {
        let save = configurations::save(
            self.fs.clone(),
            self.files(cx),
            self.list.clone(),
            list.clone(),
        );
        self.list = list;
        cx.notify();
        cx.spawn(async move |this, cx| {
            save.await?;
            this.update(cx, |this, cx| this.reload(cx))?;
            Ok(())
        })
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
        "services": services::e2e_services(workspace, cx),
    })
}
