//! IntelliJ's code coverage (the platform's `coverage-common`): the suites a run with coverage
//! gathers, the ones shown (IntelliJ's active suites, merged into one bundle), and what shows
//! them: the editor's gutter marks and their popup, the Coverage tool window, the Project tool
//! window's percentages, Run ▸ Manage Coverage Reports…, Generate Coverage Report… and Hide
//! Coverage, and the Code Coverage question when a run's coverage comes in. An engine
//! (`spork_java`'s, IntelliJ's own agent on the JVM) runs, reads and reports the suites.

pub mod data;
mod dialogs;
mod gutter;
mod view;

use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::Result;
use collections::HashMap;
use gpui::{
    App, AppContext as _, Context, Entity, EntityId, Global, InteractiveElement as _,
    ParentElement as _, Task, WeakEntity, Window,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spork_settings::coverage::WhenGathered;
use util::ResultExt as _;
use workspace::Workspace;

pub use data::Data;
pub use view::CoverageView;

gpui::actions!(
    coverage,
    [
        /// Shows the coverage suites to choose those shown (IntelliJ's Manage Coverage
        /// Reports…, ⌥⌘F6).
        ManageCoverageReports,
        /// Writes the HTML report of the coverage shown into a folder.
        GenerateCoverageReport,
        /// Hides the coverage shown.
        HideCoverage,
        /// Opens the Coverage tool window and focuses it, or gives the focus back to the editor.
        ToggleFocus,
    ]
);

pub use spork_run::configurations::COVERAGE_SUITE_ENV as SUITE_ENV;

/// A run's coverage (IntelliJ's `CoverageSuite`): its data file, kept in the project's list of
/// suites.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Suite {
    /// The run configuration's name.
    pub name: String,
    pub file: PathBuf,
    /// When the run ended, in milliseconds since the epoch.
    pub timestamp: i64,
    /// The project's folder.
    pub root: PathBuf,
    pub branch_coverage: bool,
    pub track_per_test: bool,
}

impl Suite {
    /// IntelliJ's presentable name (`coverage.results.suite.name`).
    pub fn title(&self) -> String {
        format!("{} Coverage Results", self.name)
    }
}

/// What runs, reads and reports a language's coverage.
pub trait Engine {
    /// Reads a suite's data.
    fn load(&self, suite: &Suite, workspace: &Workspace, cx: &mut App) -> Task<Result<Data>>;

    /// Writes the HTML report of `suites` into `folder`; answers its index page.
    fn report(
        &self,
        suites: &[Suite],
        folder: PathBuf,
        workspace: &Workspace,
        cx: &mut App,
    ) -> Task<Result<PathBuf>>;
}

struct GlobalEngine(Rc<dyn Engine>);

impl Global for GlobalEngine {}

/// The engine of the suites (Spork IDE has one, the JVM's).
pub fn register_engine(engine: Rc<dyn Engine>, cx: &mut App) {
    cx.set_global(GlobalEngine(engine));
}

fn engine(cx: &App) -> Option<Rc<dyn Engine>> {
    cx.try_global::<GlobalEngine>()
        .map(|engine| engine.0.clone())
}

/// A workspace's coverage: the suites it knows, kept in the IDE's files of the project, and the
/// ones shown with their data merged.
pub struct Coverage {
    workspace: WeakEntity<Workspace>,
    project: Entity<project::Project>,
    /// Whether the list of suites was read from the project's files (once it has a folder).
    listed: bool,
    pub(crate) suites: Vec<Suite>,
    pub(crate) active: Vec<Suite>,
    pub(crate) data: Option<Rc<Data>>,
    /// Bumped each time other coverage shows, for the views that follow it.
    pub(crate) generation: usize,
    loading: Option<Task<()>>,
    /// The editors marked with the coverage shown, by their entity.
    pub(crate) marked: HashMap<EntityId, gutter::Marked>,
}

#[derive(Default)]
struct Registry(HashMap<EntityId, Entity<Coverage>>);

impl Global for Registry {}

impl Coverage {
    pub fn of(workspace: &Workspace, cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<Registry>()?
            .0
            .get(&workspace.weak_handle().entity_id())
            .cloned()
    }

    fn new(workspace: &Workspace, _: &mut Context<Self>) -> Self {
        Self {
            workspace: workspace.weak_handle(),
            project: workspace.project().clone(),
            listed: false,
            suites: Vec::new(),
            active: Vec::new(),
            data: None,
            generation: 0,
            loading: None,
            marked: HashMap::default(),
        }
    }

    /// The project's first folder, whose files keep the suites.
    fn root(&self, cx: &App) -> Option<PathBuf> {
        let project = self.project.read(cx);
        let worktree = project.visible_worktrees(cx).next()?;
        Some(worktree.read(cx).abs_path().to_path_buf())
    }

    /// Reads the list of suites the first time it is needed.
    pub(crate) fn list(&mut self, cx: &App) {
        if !self.listed && self.root(cx).is_some() {
            self.listed = true;
            let mut suites = self.load_list(cx).unwrap_or_default();
            suites.retain(|suite| !self.suites.iter().any(|known| known.file == suite.file));
            self.suites.extend(suites);
        }
    }

    fn list_file(&self, cx: &App) -> Option<PathBuf> {
        Some(spork_run::configurations::workspace_folder(&self.root(cx)?).join("coverage.json"))
    }

    fn load_list(&self, cx: &App) -> Option<Vec<Suite>> {
        let text = std::fs::read_to_string(self.list_file(cx)?).ok()?;
        serde_json::from_str::<Vec<Suite>>(&text)
            .log_err()
            .map(|suites| {
                suites
                    .into_iter()
                    .filter(|suite| suite.file.is_file())
                    .collect()
            })
    }

    fn save_list(&self, cx: &App) {
        let Some(file) = self.list_file(cx) else {
            return;
        };
        let text = match serde_json::to_string_pretty(&self.suites) {
            Ok(text) => text,
            Err(error) => return log::error!("writing the coverage suites: {error}"),
        };
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).log_err();
        }
        std::fs::write(&file, text).log_err();
    }

    /// Keeps a suite in the list, in place of an earlier one of the same data file.
    pub(crate) fn remember(&mut self, suite: &Suite, cx: &mut Context<Self>) {
        self.list(cx);
        self.suites.retain(|known| known.file != suite.file);
        self.suites.insert(0, suite.clone());
        self.save_list(cx);
        cx.notify();
    }

    /// Takes suites out of the list; `delete` removes their data files too.
    pub(crate) fn forget(&mut self, files: &[PathBuf], delete: bool, cx: &mut Context<Self>) {
        self.suites.retain(|suite| !files.contains(&suite.file));
        if delete {
            for file in files {
                std::fs::remove_file(file).log_err();
            }
        }
        self.save_list(cx);
        cx.notify();
    }

    pub fn is_shown(&self) -> bool {
        !self.active.is_empty()
    }

    /// Shows `suites`, their data read and merged; nothing for none.
    pub(crate) fn show(&mut self, suites: Vec<Suite>, cx: &mut Context<Self>) {
        if suites.is_empty() {
            return self.hide(cx);
        }
        let (Some(engine), Some(workspace)) = (engine(cx), self.workspace.upgrade()) else {
            return;
        };
        let loads: Vec<Task<Result<Data>>> = suites
            .iter()
            .map(|suite| workspace.update(cx, |workspace, cx| engine.load(suite, workspace, cx)))
            .collect();
        self.loading = Some(cx.spawn(async move |this, cx| {
            let mut merged = Data::default();
            for load in loads {
                match load.await {
                    Ok(data) => merged.merge(&data),
                    Err(error) => {
                        let error = error.context("Error loading or collecting coverage data");
                        this.update(cx, |this, cx| this.show_error(error, cx)).ok();
                        return;
                    }
                }
            }
            this.update(cx, |this, cx| {
                this.active = suites;
                this.data = Some(Rc::new(merged));
                this.generation += 1;
                this.loading = None;
                this.mark_editors(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Hides the coverage shown (IntelliJ's Hide Coverage, No Coverage).
    pub(crate) fn hide(&mut self, cx: &mut Context<Self>) {
        self.active.clear();
        self.data = None;
        self.generation += 1;
        self.loading = None;
        gutter::close(cx);
        self.mark_editors(cx);
        cx.notify();
    }

    fn show_error(&self, error: anyhow::Error, cx: &mut App) {
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, move |workspace, cx| workspace.show_error(error, cx));
        }
    }
}

/// A run with coverage ended: its suite kept, then shown as the settings say, or as the Code
/// Coverage question answers while other coverage shows.
fn gathered(workspace: &Entity<Workspace>, mut suite: Suite, cx: &mut App) {
    let Some(coverage) = Coverage::of(workspace.read(cx), cx) else {
        return;
    };
    if !suite.file.is_file() {
        let error = anyhow::anyhow!("Coverage data not found");
        workspace.update(cx, move |workspace, cx| workspace.show_error(error, cx));
        return;
    }
    suite.timestamp = chrono::Utc::now().timestamp_millis();
    coverage.update(cx, |coverage, cx| coverage.remember(&suite, cx));
    let active = coverage.read(cx).active.clone();
    match spork_settings::coverage::settings(cx).when_gathered {
        WhenGathered::DoNotApply => {}
        _ if active.is_empty() => apply(&coverage, Vec::new(), suite, WhenGathered::Replace, cx),
        WhenGathered::Ask => dialogs::ask(workspace, coverage, active, suite, cx),
        answer => apply(&coverage, active, suite, answer, cx),
    }
}

/// Shows a gathered suite with the active ones, or in their place.
pub(crate) fn apply(
    coverage: &Entity<Coverage>,
    active: Vec<Suite>,
    suite: Suite,
    answer: WhenGathered,
    cx: &mut App,
) {
    let suites = match answer {
        WhenGathered::DoNotApply | WhenGathered::Ask => return,
        WhenGathered::Replace => vec![suite],
        WhenGathered::Add => {
            let file = suite.file.clone();
            active
                .into_iter()
                .filter(|active| active.file != file)
                .chain([suite])
                .collect()
        }
    };
    coverage.update(cx, |coverage, cx| coverage.show(suites, cx));
}

/// The window of `workspace`, to run something in it.
pub(crate) fn in_window(
    workspace: &Entity<Workspace>,
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    let workspace = workspace.clone();
    let window = cx.windows().into_iter().find(|window| {
        window
            .update(cx, |_, window, cx| {
                Workspace::for_window(window, cx).as_ref() == Some(&workspace)
            })
            .unwrap_or(false)
    });
    if let Some(window) = window {
        window
            .update(cx, |_, window, cx| {
                workspace.update(cx, |workspace, cx| f(workspace, window, cx))
            })
            .log_err();
    }
}

pub fn init(cx: &mut App) {
    gutter::init(cx);
    cx.set_global(project_panel::RowDecoration(Rc::new(
        |workspace, path, cx| project_view_text(workspace, path, cx).map(Into::into),
    )));
    cx.set_global(spork_run::RunEnded(Rc::new(|workspace, env, cx| {
        let Some(suite) = env
            .get(SUITE_ENV)
            .and_then(|suite| serde_json::from_str::<Suite>(suite).log_err())
        else {
            return;
        };
        gathered(workspace, suite, cx);
    })));
    cx.observe_new(|workspace: &mut Workspace, window, cx| {
        let coverage = cx.new(|cx| Coverage::new(workspace, cx));
        let id = cx.entity_id();
        cx.default_global::<Registry>()
            .0
            .insert(id, coverage.clone());
        cx.on_release(move |_, cx| {
            cx.default_global::<Registry>().0.remove(&id);
        })
        .detach();
        workspace.register_action(|workspace, _: &ManageCoverageReports, window, cx| {
            dialogs::manage(workspace, window, cx)
        });
        // Hide Coverage and Generate Coverage Report… are greyed while nothing shows
        workspace.register_action_renderer(|div, workspace, _, cx| {
            match Coverage::of(workspace, cx).is_some_and(|c| c.read(cx).is_shown()) {
                // after the workspace's update: hiding goes through its editors
                true => div
                    .on_action(cx.listener(|workspace, _: &HideCoverage, _, cx| {
                        if let Some(coverage) = Coverage::of(workspace, cx) {
                            cx.defer(move |cx| {
                                coverage.update(cx, |coverage, cx| coverage.hide(cx))
                            });
                        }
                    }))
                    .on_action(
                        cx.listener(|workspace, _: &GenerateCoverageReport, window, cx| {
                            dialogs::generate_report(workspace, window, cx)
                        }),
                    ),
                false => div,
            }
        });
        workspace.register_action(|workspace, _: &ToggleFocus, window, cx| {
            workspace.toggle_panel_focus::<CoverageView>(window, cx);
        });
        workspace.register_action_renderer(|div, _, window, cx| {
            div.children(gutter::render(window, cx))
        });
        // an editor opened while coverage shows gets its marks
        cx.subscribe(&cx.entity(), {
            let coverage = coverage.clone();
            move |_, _, event, cx| {
                if let workspace::Event::ItemAdded { item } = event
                    && let Some(editor) = item.downcast::<editor::Editor>()
                {
                    coverage.update(cx, |coverage, cx| coverage.mark_editor(&editor, cx));
                }
            }
        })
        .detach();
        if let Some(window) = window {
            view::add(workspace, coverage, window, cx);
        }
    })
    .detach();
}

/// The Project tool window's text after a file or folder of the coverage shown: IntelliJ's
/// "x% classes, y% lines covered" (a folder), "x% methods, y% lines covered" (a file).
pub fn project_view_text(workspace: &Workspace, path: &Path, cx: &App) -> Option<String> {
    if !spork_settings::coverage::settings(cx).show_in_project_view {
        return None;
    }
    let coverage = Coverage::of(workspace, cx)?;
    let data = coverage.read(cx).data.clone()?;
    let counts = data.counts_under(path)?;
    if counts.lines.total == 0 {
        return None;
    }
    let is_file = data
        .classes
        .iter()
        .any(|class| class.file.as_deref() == Some(path));
    Some(match is_file {
        true => format!(
            "{}% methods, {}% lines covered",
            counts.methods.percent(),
            counts.lines.percent()
        ),
        false => format!(
            "{}% classes, {}% lines covered",
            counts.classes.percent(),
            counts.lines.percent()
        ),
    })
}

/// What the end-to-end tests read of the coverage: the suites known and shown, the Coverage tool
/// window's rows and the gutter's marks in the active editor.
pub fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(coverage) = Coverage::of(workspace, cx) else {
        return Value::Null;
    };
    let coverage = coverage.read(cx);
    let names = |suites: &[Suite]| suites.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    json!({
        "suites": names(&coverage.suites),
        "active": names(&coverage.active),
        "loading": coverage.loading.is_some(),
        "view": view::e2e_state(workspace, cx),
        "marks": gutter::e2e_marks(workspace, cx),
        "popup": gutter::e2e_popup(cx),
        "dialog": dialogs::e2e_state(workspace, cx),
    })
}
