//! The Problems tool window (⌘6), IntelliJ's (`ProblemsView`): its File tab lists the problems of
//! the file in the editor, its Project Errors tab the errors of every file, each under its file
//! node with the severity's icon, the left toolbar (View Options, Show Quick-Fixes, Open Editor
//! Preview, Expand All, Collapse All) and a double click or ⏎ going to the problem. The problems
//! are the language servers' diagnostics. Code ▸ Inspect Code…'s results show in a tab of their
//! own, IntelliJ's `InspectionResultsView` ("Inspections on Project 'fixture'"): the Inspection
//! Results root, the files and their problems, Rerun.

use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

use collections::HashSet;
use editor::Editor;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, TaskExt as _,
    WeakEntity, Window, div, px,
};
use gpui_kit::component::Selectable as _;
use gpui_kit::component::{h_flex, v_flex};
use language::{Buffer, ToOffset as _, ToPoint as _};
use language::{DiagnosticSeverity, Point};
use picker::{MatchLocation, PreviewBackend, PreviewLayout, PreviewUpdate};
use project::{Project, ProjectPath};
use spork_code::inspection::Inspection;
use spork_workbench::{ActionButton, Tokens};
use ui::{ContextMenu, IconPosition, PopoverMenu, Tooltip};
use util::ResultExt as _;
use util::paths::PathExt as _;
use workspace::dock::{DockPosition, GearItem, Panel, PanelEvent, PanelHandle};
use workspace::{Workspace, frame};

pub mod problems_panel {
    gpui::actions!(
        problems_panel,
        [
            /// Opens the Problems tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus,
            /// Copies the selected problem's description (⌘C in the Problems tool window).
            CopyProblemDescription,
            /// Shows the quick fixes of the selected problem (⌥⏎).
            ShowQuickFixes,
            /// Goes to the selected problem in its editor (⌘↓, F4).
            JumpToSource,
            /// Opens a problem's file once it is selected (Open Files with Single Click).
            ToggleOpenFilesWithSingleClick,
            /// Opens the problems' files in the preview tab (Enable Preview Tab).
            TogglePreviewTab,
            /// Shows the selected problem's code beside the tree (Open Editor Preview).
            ToggleEditorPreview,
        ]
    );
}

pub(crate) fn add_panel(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    workspace.register_action(|workspace, _: &problems_panel::ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<ProblemsPanel>(window, cx);
    });
    fn toggle(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
        change: fn(&mut ProblemsPanel, &mut Window, &mut Context<ProblemsPanel>),
    ) {
        if let Some(panel) = workspace.panel::<ProblemsPanel>(cx) {
            panel.update(cx, |panel, cx| {
                change(panel, window, cx);
                cx.notify();
            });
        }
    }
    workspace.register_action(
        |workspace, _: &problems_panel::ToggleOpenFilesWithSingleClick, window, cx| {
            toggle(workspace, window, cx, |panel, _, _| {
                panel.open_on_select = !panel.open_on_select
            })
        },
    );
    workspace.register_action(
        |workspace, _: &problems_panel::TogglePreviewTab, window, cx| {
            toggle(workspace, window, cx, |panel, _, _| {
                panel.preview_tab = !panel.preview_tab
            })
        },
    );
    workspace.register_action(
        |workspace, _: &problems_panel::ToggleEditorPreview, window, cx| {
            toggle(workspace, window, cx, |panel, window, cx| {
                panel.show_preview = !panel.show_preview;
                panel.update_preview(window, cx);
            })
        },
    );
    let handle = cx.entity().downgrade();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| ProblemsPanel::new(handle, project, window, cx));
    workspace.add_panel(panel, window, cx);
}

/// Where Inspect Code's results go (`spork_code::inspection::InspectionResults`): the Problems
/// tool window's inspection tab, activated; none found, IntelliJ's notification and no tab.
pub(crate) fn show_inspection(
    workspace: &mut Workspace,
    inspection: Inspection,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(panel) = workspace.panel::<ProblemsPanel>(cx) else {
        return;
    };
    if inspection.problems.is_empty() {
        let files = inspection.paths.len();
        let noun = if files == 1 { "file" } else { "files" };
        let text = format!(
            "Code inspection did not find anything to report. {files} {noun} processed in '{}'.",
            inspection.scope
        );
        panel.update(cx, |panel, cx| panel.close_inspection(window, cx));
        workspace.show_toast(
            workspace::Toast::new(
                workspace::notifications::NotificationId::unique::<Inspected>(),
                text,
            )
            .autohide(),
            cx,
        );
        return;
    }
    panel.update(cx, |panel, cx| {
        panel.show_inspection(inspection, window, cx)
    });
    workspace.focus_panel::<ProblemsPanel>(window, cx);
}

/// A problem in a file: a primary diagnostic.
#[derive(Clone, PartialEq)]
struct Problem {
    severity: DiagnosticSeverity,
    message: SharedString,
    range: Range<Point>,
    /// What reported it (the language server's diagnostic source, `rustc`, `clippy`): its
    /// inspection, for Group by Inspection.
    source: SharedString,
}

impl Problem {
    fn is_error(&self) -> bool {
        self.severity == DiagnosticSeverity::ERROR
    }

    /// Warnings show with Show Warning, the rest (information, hints) with Show Other Problems.
    fn is_other(&self) -> bool {
        self.severity != DiagnosticSeverity::ERROR && self.severity != DiagnosticSeverity::WARNING
    }

    /// IntelliJ's level for the count texts: information and hints are weak warnings, as the
    /// inspection widget counts them.
    fn level(&self) -> usize {
        match self.severity {
            DiagnosticSeverity::ERROR => 0,
            DiagnosticSeverity::WARNING => 1,
            _ => 2,
        }
    }

    /// The severity's icon (`HighlightDisplayLevel`'s, expUI).
    fn icon(&self) -> &'static str {
        match self.severity {
            DiagnosticSeverity::ERROR => "status/error",
            DiagnosticSeverity::WARNING => "status/warning",
            _ => "status/info",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    /// "File": the problems of the file in the editor.
    File,
    /// "Project Errors": the errors of every file.
    ProjectErrors,
    /// "Inspections on …": Inspect Code's last results.
    Inspection,
}

/// A row of the tree.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Node {
    /// The inspection tab's "Inspection Results".
    Root,
    File(ProjectPath),
    /// Group by Inspection's node: a file's problems from one source.
    Group(ProjectPath, SharedString),
    Problem(ProjectPath, usize),
}

/// Inspect Code's results as the inspection tab shows them: what the run found then, each
/// file's problems in their order.
struct Inspected {
    inspection: Inspection,
    problems: BTreeMap<ProjectPath, Vec<Problem>>,
    buffers: BTreeMap<ProjectPath, Entity<Buffer>>,
    root_collapsed: bool,
}

impl Inspected {
    fn new(inspection: Inspection, cx: &App) -> Self {
        let mut problems: BTreeMap<ProjectPath, Vec<Problem>> = BTreeMap::new();
        let mut buffers = BTreeMap::new();
        for problem in &inspection.problems {
            let snapshot = problem.buffer.read(cx).snapshot();
            let range =
                problem.range.start.to_point(&snapshot)..problem.range.end.to_point(&snapshot);
            problems
                .entry(problem.path.clone())
                .or_default()
                .push(Problem {
                    severity: problem.severity,
                    message: problem
                        .message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                        .into(),
                    range,
                    source: SharedString::default(),
                });
            buffers.insert(problem.path.clone(), problem.buffer.clone());
        }
        Self {
            inspection,
            problems,
            buffers,
            root_collapsed: false,
        }
    }

    /// The tab's title, IntelliJ's `inspection.results.toolwindow.title`.
    fn title(&self) -> SharedString {
        format!("Inspections on {}", self.inspection.scope).into()
    }
}

/// IntelliJ's tail of an inspection tree node: each level's count ("1 error 2 warnings"), an
/// error's count red.
fn level_counts<'a>(problems: impl Iterator<Item = &'a Problem>) -> Vec<(String, bool)> {
    let mut counts = [0usize; 3];
    for problem in problems {
        if let Some(count) = counts.get_mut(problem.level()) {
            *count += 1;
        }
    }
    [
        ("error", "errors"),
        ("warning", "warnings"),
        ("weak warning", "weak warnings"),
    ]
    .into_iter()
    .zip(counts)
    .enumerate()
    .filter(|(_, (_, count))| *count > 0)
    .map(|(level, ((one, many), count))| {
        let noun = if count == 1 { one } else { many };
        (format!("{count} {noun}"), level == 0)
    })
    .collect()
}

pub(crate) struct ProblemsPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    /// Each file's problems, as its buffer has them.
    problems: BTreeMap<ProjectPath, Vec<Problem>>,
    buffers: BTreeMap<ProjectPath, Entity<Buffer>>,
    /// The file of the active editor.
    current: Option<ProjectPath>,
    tab: Tab,
    collapsed: HashSet<ProjectPath>,
    selected: Option<Node>,
    show_warnings: bool,
    show_other: bool,
    sort_by_severity: bool,
    sort_by_name: bool,
    show_preview: bool,
    /// View Options' Folders Always on Top and Group by Inspection.
    folders_first: bool,
    group_by_inspection: bool,
    /// The ⋮'s Open Files with Single Click and Enable Preview Tab.
    open_on_select: bool,
    preview_tab: bool,
    collapsed_groups: HashSet<(ProjectPath, SharedString)>,
    context_menu: Option<(Entity<ContextMenu>, gpui::Point<Pixels>, Subscription)>,
    preview: Arc<dyn PreviewBackend>,
    inspected: Option<Inspected>,
    focus: FocusHandle,
    _reads: BTreeMap<ProjectPath, Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl ProblemsPanel {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = picker_preview::editor_preview(project.clone(), window, cx);
        let mut _subscriptions = vec![cx.subscribe_in(
            &project,
            window,
            |this, project, event, window, cx| match event {
                project::Event::DiagnosticsUpdated { paths, .. } => {
                    for path in paths {
                        this.read(path.clone(), window, cx);
                    }
                }
                project::Event::DiskBasedDiagnosticsFinished { .. } => {
                    let paths: Vec<ProjectPath> = project
                        .read(cx)
                        .diagnostic_summaries(false, cx)
                        .map(|(path, _, _)| path)
                        .chain(this.problems.keys().cloned())
                        .collect();
                    for path in paths {
                        this.read(path, window, cx);
                    }
                }
                _ => {}
            },
        )];
        if let Some(workspace) = workspace.upgrade() {
            _subscriptions.push(cx.subscribe_in(
                &workspace,
                window,
                |this, workspace, event, window, cx| {
                    if let workspace::Event::ActiveItemChanged = event {
                        this.follow_editor(&workspace.clone(), window, cx);
                    }
                },
            ));
        }
        let mut this = Self {
            workspace,
            project: project.clone(),
            problems: BTreeMap::new(),
            buffers: BTreeMap::new(),
            current: None,
            tab: Tab::File,
            collapsed: HashSet::default(),
            selected: None,
            show_warnings: true,
            show_other: true,
            sort_by_severity: true,
            sort_by_name: false,
            show_preview: false,
            folders_first: true,
            group_by_inspection: false,
            open_on_select: false,
            preview_tab: false,
            collapsed_groups: HashSet::default(),
            context_menu: None,
            preview,
            inspected: None,
            focus: cx.focus_handle(),
            _reads: BTreeMap::new(),
            _subscriptions,
        };
        let paths: Vec<ProjectPath> = project
            .read(cx)
            .diagnostic_summaries(false, cx)
            .map(|(path, _, _)| path)
            .collect();
        for path in paths {
            this.read(path, window, cx);
        }
        // the workspace adding the panel is being updated: its editor is read after
        cx.defer_in(window, |this, window, cx| {
            if let Some(workspace) = this.workspace.upgrade() {
                this.follow_editor(&workspace, window, cx);
            }
        });
        this
    }

    fn follow_editor(
        &mut self,
        workspace: &Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = workspace
            .read(cx)
            .active_item(cx)
            .filter(|item| item.downcast::<Editor>().is_some())
            .and_then(|item| item.project_path(cx));
        if current == self.current {
            return;
        }
        self.current = current.clone();
        if let Some(path) = current {
            self.read(path, window, cx);
        }
        cx.notify();
    }

    /// Reads a file's problems again from its buffer.
    fn read(&mut self, path: ProjectPath, window: &mut Window, cx: &mut Context<Self>) {
        let open = self
            .project
            .update(cx, |project, cx| project.open_buffer(path.clone(), cx));
        let task = cx.spawn_in(window, {
            let path = path.clone();
            async move |this, cx| {
                let Some(buffer) = open.await.log_err() else {
                    return;
                };
                this.update(cx, |this, cx| {
                    let snapshot = buffer.read(cx).snapshot();
                    let problems: Vec<Problem> = snapshot
                        .diagnostics_in_range::<_, Point>(
                            Point::zero()..snapshot.max_point(),
                            false,
                        )
                        .filter(|entry| entry.diagnostic.is_primary)
                        .map(|entry| Problem {
                            severity: entry.diagnostic.severity,
                            message: entry
                                .diagnostic
                                .message
                                .as_str()
                                .lines()
                                .next()
                                .unwrap_or_default()
                                .to_owned()
                                .into(),
                            range: entry.range,
                            source: entry.diagnostic.source.clone().unwrap_or_default().into(),
                        })
                        .collect();
                    match problems.is_empty() {
                        true => {
                            this.problems.remove(&path);
                            this.buffers.remove(&path);
                        }
                        false => {
                            this.problems.insert(path.clone(), problems);
                            this.buffers.insert(path.clone(), buffer);
                        }
                    }
                    this._reads.remove(&path);
                    cx.notify();
                })
                .ok();
            }
        });
        self._reads.insert(path, task);
    }

    /// A file's problems as the tab and View Options show them, in their order, with their
    /// index in the file's list.
    fn shown(&self, path: &ProjectPath) -> Vec<(usize, &Problem)> {
        let mut shown: Vec<(usize, &Problem)> = self
            .tab_problems()
            .get(path)
            .map(|problems| problems.iter().enumerate().collect())
            .unwrap_or_default();
        shown.retain(|(_, problem)| match self.tab {
            Tab::ProjectErrors => problem.is_error(),
            Tab::Inspection => true,
            Tab::File => {
                problem.is_error()
                    || (problem.severity == DiagnosticSeverity::WARNING && self.show_warnings)
                    || (problem.is_other() && self.show_other)
            }
        });
        // IntelliJ's order: the most severe first, then by name or by place
        shown.sort_by(|(_, a), (_, b)| {
            let severity = match self.sort_by_severity {
                true => a.severity.cmp(&b.severity),
                false => std::cmp::Ordering::Equal,
            };
            let place = (a.range.start.row, a.range.start.column)
                .cmp(&(b.range.start.row, b.range.start.column));
            let name = a.message.cmp(&b.message);
            severity.then(match self.sort_by_name {
                true => name.then(place),
                false => place.then(name),
            })
        });
        shown
    }

    /// The problems the tab draws from: the inspection's, or the language servers' now.
    fn tab_problems(&self) -> &BTreeMap<ProjectPath, Vec<Problem>> {
        match (&self.inspected, self.tab) {
            (Some(inspected), Tab::Inspection) => &inspected.problems,
            _ => &self.problems,
        }
    }

    fn tab_buffers(&self) -> &BTreeMap<ProjectPath, Entity<Buffer>> {
        match (&self.inspected, self.tab) {
            (Some(inspected), Tab::Inspection) => &inspected.buffers,
            _ => &self.buffers,
        }
    }

    /// The files the tab lists, each with its shown problems.
    fn files(&self) -> Vec<ProjectPath> {
        match self.tab {
            Tab::File => self.current.iter().cloned().collect(),
            Tab::Inspection => self.tab_problems().keys().cloned().collect(),
            Tab::ProjectErrors => self
                .problems
                .keys()
                .filter(|path| !self.shown(path).is_empty())
                .cloned()
                .collect(),
        }
    }

    fn rows(&self) -> Vec<Node> {
        let mut rows = Vec::new();
        if let (Some(inspected), Tab::Inspection) = (&self.inspected, self.tab) {
            rows.push(Node::Root);
            if inspected.root_collapsed {
                return rows;
            }
        }
        for path in self.files() {
            let shown = self.shown(&path);
            if shown.is_empty() {
                continue;
            }
            rows.push(Node::File(path.clone()));
            if self.collapsed.contains(&path) {
                continue;
            }
            if !self.grouped() {
                rows.extend(shown.iter().map(|(ix, _)| Node::Problem(path.clone(), *ix)));
                continue;
            }
            // Group by Inspection: a node per source, by name, its problems in their order
            let mut sources: Vec<SharedString> = shown
                .iter()
                .map(|(_, problem)| problem.source.clone())
                .collect();
            sources.sort();
            sources.dedup();
            for source in sources {
                rows.push(Node::Group(path.clone(), source.clone()));
                if self
                    .collapsed_groups
                    .contains(&(path.clone(), source.clone()))
                {
                    continue;
                }
                rows.extend(
                    shown
                        .iter()
                        .filter(|(_, problem)| problem.source == source)
                        .map(|(ix, _)| Node::Problem(path.clone(), *ix)),
                );
            }
        }
        rows
    }

    /// Whether the tree groups a file's problems by inspection (not the inspection tab's).
    fn grouped(&self) -> bool {
        self.group_by_inspection && self.tab != Tab::Inspection
    }

    fn problem<'a>(&'a self, node: &'a Node) -> Option<(&'a ProjectPath, &'a Problem)> {
        match node {
            Node::Problem(path, ix) => Some((path, self.tab_problems().get(path)?.get(*ix)?)),
            Node::File(_) | Node::Group(..) | Node::Root => None,
        }
    }

    fn count(&self, tab: Tab) -> usize {
        match tab {
            Tab::File => self
                .current
                .as_ref()
                .and_then(|path| self.problems.get(path))
                .map_or(0, Vec::len),
            Tab::ProjectErrors => self
                .problems
                .values()
                .flatten()
                .filter(|problem| problem.is_error())
                .count(),
            Tab::Inspection => self
                .inspected
                .as_ref()
                .map_or(0, |inspected| inspected.problems.values().flatten().count()),
        }
    }

    fn select(&mut self, node: Node, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(node);
        self.update_preview(window, cx);
        // Open Files with Single Click: the problem's file opens, the focus staying here
        if self.open_on_select {
            self.open_problem(false, window, cx);
        }
        cx.notify();
    }

    fn step(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|node| node == selected))
            .map_or(0, |ix| {
                (ix as isize + step).clamp(0, rows.len() as isize - 1) as usize
            });
        if let Some(node) = rows.get(at).cloned() {
            self.select(node, window, cx);
        }
    }

    fn set_all_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.collapsed.clear();
        if let Some(inspected) = self
            .inspected
            .as_mut()
            .filter(|_| self.tab == Tab::Inspection)
        {
            inspected.root_collapsed = false;
        }
        self.collapsed_groups.clear();
        if !open {
            self.collapsed.extend(self.files());
            if let Some(Node::Problem(path, _)) = &self.selected {
                self.selected = Some(Node::File(path.clone()));
            }
        }
        cx.notify();
    }

    /// ⏎ or a double click: goes to the problem in its editor, or opens or closes a file.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(node) = self.selected.clone() else {
            return false;
        };
        if self.problem(&node).is_none() {
            match node {
                Node::File(path) => {
                    if !self.collapsed.remove(&path) {
                        self.collapsed.insert(path);
                    }
                }
                Node::Root => {
                    if let Some(inspected) = &mut self.inspected {
                        inspected.root_collapsed = !inspected.root_collapsed;
                    }
                }
                Node::Group(path, source) => {
                    let group = (path, source);
                    if !self.collapsed_groups.remove(&group) {
                        self.collapsed_groups.insert(group);
                    }
                }
                Node::Problem(..) => {}
            }
            cx.notify();
            return false;
        }
        self.open_problem(true, window, cx)
    }

    /// Opens the selected problem's file at the problem, in the preview tab with Enable Preview
    /// Tab, the editor taking the focus when `focus`.
    fn open_problem(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((path, point)) = self
            .selected
            .as_ref()
            .and_then(|node| self.problem(node))
            .map(|(path, problem)| (path.clone(), problem.range.start))
        else {
            return false;
        };
        let preview_tab = self.preview_tab;
        self.workspace
            .update(cx, |workspace, cx| {
                let open =
                    workspace.open_path_preview(path, None, focus, preview_tab, true, window, cx);
                cx.spawn_in(window, async move |_, cx| {
                    let item = open.await?;
                    if let Some(editor) = item.downcast::<Editor>() {
                        editor.update_in(cx, |editor, window, cx| {
                            editor.change_selections(
                                editor::SelectionEffects::scroll(
                                    editor::scroll::Autoscroll::center(),
                                ),
                                window,
                                cx,
                                |selections| selections.select_ranges([point..point]),
                            );
                        })?;
                    }
                    anyhow::Ok(())
                })
                .detach_and_log_err(cx);
            })
            .log_err()
            .is_some()
    }

    /// Copy Problem Description: the selected problem's text onto the clipboard.
    fn copy_description(&mut self, cx: &mut Context<Self>) {
        if let Some((_, problem)) = self.selected.as_ref().and_then(|node| self.problem(node)) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(problem.message.to_string()));
        }
    }

    /// The tree's context menu, IntelliJ's (`ProblemsView.ToolWindow.TreePopup`): Show
    /// Quick-Fixes, Copy Problem Description, Jump to Source.
    fn deploy_context_menu(
        &mut self,
        node: Node,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select(node.clone(), window, cx);
        window.focus(&self.focus, cx);
        let is_problem = self.problem(&node).is_some();
        let focus = self.focus.clone();
        let menu = ContextMenu::build(window, cx, move |menu, _, _| {
            menu.context(focus)
                .action_disabled_when(
                    !is_problem,
                    "Show Quick-Fixes",
                    Box::new(problems_panel::ShowQuickFixes),
                )
                .action_disabled_when(
                    !is_problem,
                    "Copy Problem Description",
                    Box::new(problems_panel::CopyProblemDescription),
                )
                .action_disabled_when(
                    !is_problem,
                    "Jump to Source",
                    Box::new(problems_panel::JumpToSource),
                )
        });
        let subscription = cx.subscribe(&menu, |this, _, _: &gpui::DismissEvent, cx| {
            this.context_menu = None;
            cx.notify();
        });
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// Show Quick-Fixes: goes to the problem and shows the code actions there (IntelliJ's
    /// intention popup).
    fn show_quick_fixes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open_selected(window, cx) {
            return;
        }
        // the editor opens after this: its code actions once it has the focus
        window.defer(cx, |window, cx| {
            window.dispatch_action(
                Box::new(editor::actions::ToggleCodeActions {
                    deployed_from: None,
                    quick_launch: false,
                }),
                cx,
            )
        });
    }

    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self
            .selected
            .as_ref()
            .and_then(|node| self.problem(node))
            .and_then(|(path, problem)| {
                Some((self.tab_buffers().get(path)?.clone(), problem.range.clone()))
            });
        match selected {
            Some((buffer, range)) if self.show_preview => {
                let snapshot = buffer.read(cx).snapshot();
                let location = MatchLocation {
                    anchor_range: snapshot.anchor_before(range.start)
                        ..snapshot.anchor_after(range.end),
                    range: range.start.to_offset(&snapshot)..range.end.to_offset(&snapshot),
                };
                self.preview
                    .update(PreviewUpdate::from_buffer(buffer, location), window, cx)
            }
            _ => self.preview.clear(cx),
        }
    }

    fn render_row(
        &self,
        node: Node,
        ix: usize,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let selected = self.selected.as_ref() == Some(&node);
        let grey = match selected && focused {
            true => tokens.text,
            false => tokens.text_secondary,
        };
        let row = h_flex()
            .id(("problem-row", ix))
            .role(Role::TreeItem)
            .aria_selected(selected)
            .flex_none()
            .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
            .mx(px(spork_workbench::popup::SELECTION_INSET))
            .pr(px(8.))
            .gap(px(4.))
            .rounded(px(spork_workbench::tokens::metrics::RADIUS))
            .map(|row| match (selected, focused) {
                (true, true) => row.bg(tokens.selection),
                (true, false) => row.bg(tokens.selection_inactive),
                (false, _) => row.hover(move |row| row.bg(tokens.selection_hover)),
            });
        // the inspection tab's files and problems are under its root
        let indent = match self.tab {
            Tab::Inspection => 18.,
            _ => 0.,
        };
        let tail = |counts: Vec<(String, bool)>| {
            h_flex()
                .flex_none()
                .ml(px(4.))
                .gap(px(4.))
                .children(counts.into_iter().map(|(text, error)| {
                    div()
                        .text_color(match error {
                            true => tokens.text_error_count,
                            false => grey,
                        })
                        .child(text)
                }))
        };
        let chevron = |open: bool| {
            div().flex_none().text_color(tokens.icon).child(
                spork_workbench::icon(match open {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                })
                .size(px(16.)),
            )
        };
        let row = match &node {
            Node::Root => {
                let open = !self
                    .inspected
                    .as_ref()
                    .is_some_and(|inspected| inspected.root_collapsed);
                let problems = self.tab_problems().values().flatten();
                row.pl(px(4.))
                    .aria_label("Inspection Results")
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(div().flex_none().child("Inspection Results"))
                    .child(tail(level_counts(problems)))
            }
            Node::File(path) => {
                let open = !self.collapsed.contains(path);
                let name = path.path.file_name().unwrap_or_default().to_owned();
                // the inspection tree names a file alone, the Problems tabs with its folder
                let location = match self.tab {
                    Tab::Inspection => None,
                    _ => self
                        .project
                        .read(cx)
                        .absolute_path(path, cx)
                        .and_then(|path| path.parent().map(|dir| dir.compact()))
                        .map(|dir| dir.to_string_lossy().into_owned()),
                };
                // IntelliJ's inspection tree counts each level, the Problems tabs the problems
                let count = match (self.tab, self.shown(path).len()) {
                    (Tab::Inspection, _) => tail(level_counts(
                        self.shown(path).into_iter().map(|(_, problem)| problem),
                    )),
                    (_, 1) => tail(vec![("1 problem".to_owned(), false)]),
                    (_, n) => tail(vec![(format!("{n} problems"), false)]),
                };
                row.pl(px(4. + indent))
                    .aria_label(SharedString::from(name.clone()))
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(spork_workbench::image(
                        spork_workbench::file_icon(&name),
                        cx,
                    ))
                    .child(div().flex_none().child(name))
                    .children(location.map(|location| {
                        div()
                            .flex_none()
                            .ml(px(4.))
                            .text_color(grey)
                            .child(location)
                    }))
                    .child(count)
            }
            Node::Group(path, source) => {
                let open = !self
                    .collapsed_groups
                    .contains(&(path.clone(), source.clone()));
                let count = self
                    .shown(path)
                    .iter()
                    .filter(|(_, problem)| problem.source == *source)
                    .count();
                let name: SharedString = match source.is_empty() {
                    true => "Other".into(),
                    false => source.clone(),
                };
                row.pl(px(4. + 18.))
                    .aria_label(name.clone())
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(div().flex_none().child(name))
                    .child(tail(vec![(
                        match count {
                            1 => "1 problem".to_owned(),
                            n => format!("{n} problems"),
                        },
                        false,
                    )]))
            }
            Node::Problem(..) => {
                let Some((_, problem)) = self.problem(&node) else {
                    return div().into_any_element();
                };
                let group_indent = if self.grouped() { 18. } else { 0. };
                row.pl(px(4. + indent + group_indent + 18. + 20.))
                    .aria_label(problem.message.clone())
                    .child(spork_workbench::image(problem.icon(), cx))
                    .child(div().min_w_0().truncate().child(problem.message.clone()))
                    .child(
                        div()
                            .flex_none()
                            .text_color(grey)
                            .child(format!(":{}", problem.range.start.row + 1)),
                    )
            }
        };
        let menu_node = node.clone();
        row.on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                this.select(node.clone(), window, cx);
                if event.click_count() >= 2 {
                    this.open_selected(window, cx);
                }
            }),
        )
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.deploy_context_menu(menu_node.clone(), event.position, window, cx)
            }),
        )
        .into_any_element()
    }

    /// Inspect Code's results in their tab, in place of the last run's.
    fn show_inspection(
        &mut self,
        inspection: Inspection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.inspected = Some(Inspected::new(inspection, cx));
        self.tab = Tab::Inspection;
        self.selected = None;
        self.update_preview(window, cx);
        cx.notify();
    }

    fn close_inspection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inspected = None;
        if self.tab == Tab::Inspection {
            self.tab = Tab::File;
            self.selected = None;
            self.update_preview(window, cx);
        }
        cx.notify();
    }

    /// Rerun (⌘R): Inspect Code again over the same scope; its results replace these.
    fn rerun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(inspection) = self
            .inspected
            .as_ref()
            .map(|inspected| inspected.inspection.clone())
        else {
            return;
        };
        self.workspace
            .update(cx, |workspace, cx| {
                spork_code::inspection::rerun(workspace, &inspection, window, cx)
            })
            .log_err();
    }

    /// Next and Previous Problem: the problem after (or before) the selection, opened in its
    /// editor as IntelliJ's occurrence navigator does.
    fn step_problem(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows();
        let problems: Vec<&Node> = rows
            .iter()
            .filter(|node| matches!(node, Node::Problem(..)))
            .collect();
        let at = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|node| node == selected));
        let next = match at {
            Some(at) if step > 0 => rows
                .iter()
                .skip(at + 1)
                .find(|node| matches!(node, Node::Problem(..))),
            Some(at) => rows
                .iter()
                .take(at)
                .rev()
                .find(|node| matches!(node, Node::Problem(..))),
            None => problems.first().copied(),
        };
        let Some(next) = next.cloned() else { return };
        self.select(next, window, cx);
        self.open_selected(window, cx);
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
        let inspection = self
            .inspected
            .as_ref()
            .map(|inspected| (Tab::Inspection, inspected.title()));
        let tabs =
            [
                (Tab::File, SharedString::from("File")),
                (Tab::ProjectErrors, SharedString::from("Project Errors")),
            ]
            .into_iter()
            .chain(inspection)
            .enumerate()
            .map(|(ix, (tab, title))| {
                let on = tab == self.tab;
                // the inspection's tab has no count, and closes
                let count = match tab {
                    Tab::Inspection => 0,
                    _ => self.count(tab),
                };
                h_flex()
                    .id(("problems-tab", ix))
                    .role(Role::Tab)
                    .aria_label(title.clone())
                    .aria_selected(on)
                    .h(px(28.))
                    .px(px(8.))
                    .gap(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .cursor_pointer()
                    .map(|t| match on {
                        true => t
                            .bg(tokens.tab_selected)
                            .border_color(tokens.tab_selected_border),
                        false => t
                            .border_color(gpui::transparent_black())
                            .hover(move |t| t.bg(tokens.hover)),
                    })
                    .child(title)
                    .when(count > 0, |t| {
                        t.child(
                            div()
                                .text_color(tokens.text_secondary)
                                .child(count.to_string()),
                        )
                    })
                    .when(tab == Tab::Inspection, |t| {
                        t.child(
                            ActionButton::new("close-inspection-tab", "general/closeSmall")
                                .tooltip("Close")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_inspection(window, cx)
                                })),
                        )
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.tab = tab;
                        this.selected = None;
                        this.update_preview(window, cx);
                        cx.notify();
                    }))
            });
        let title = h_flex()
            .gap(px(12.))
            .child(frame::tool_window_title_label("Problems"))
            .child(h_flex().gap(px(2.)).children(tabs))
            .into_any_element();
        frame::tool_window_header(title, Vec::new(), panel, "Problems".into(), cx)
            .into_any_element()
    }

    /// View Options: Show (Warning, Show Other Problems) and Sort By (Severity, Name), for the
    /// File tab.
    fn view_options(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let state = (
            self.show_warnings,
            self.show_other,
            self.sort_by_severity,
            self.sort_by_name,
            self.folders_first,
            self.group_by_inspection,
        );
        PopoverMenu::new("problems-view-options")
            .trigger_with_tooltip(
                frame::header_button(
                    "problems-view-options-button",
                    "general/show",
                    "View Options",
                    cx,
                ),
                Tooltip::text("View Options"),
            )
            .anchor(gpui::Anchor::TopLeft)
            .menu(move |window, cx| {
                let this = this.clone();
                let (warnings, other, severity, name, folders_first, grouped) = state;
                Some(ContextMenu::build(window, cx, move |menu, _, _| {
                    let toggle = |menu: ContextMenu,
                                  label: &'static str,
                                  on: bool,
                                  change: fn(&mut ProblemsPanel)| {
                        let this = this.clone();
                        menu.toggleable_entry(label, on, IconPosition::Start, None, move |_, cx| {
                            this.update(cx, |this, cx| {
                                change(this);
                                cx.notify();
                            })
                            .ok();
                        })
                    };
                    let menu = menu.header("Show");
                    let menu = toggle(menu, "Warning", warnings, |this| {
                        this.show_warnings = !this.show_warnings
                    });
                    let menu = toggle(menu, "Show Other Problems", other, |this| {
                        this.show_other = !this.show_other
                    });
                    let menu = menu.header("Sort By");
                    // shortcut: no folder nodes: the tree lists files, so the order is the same
                    let menu = toggle(menu, "Folders Always on Top", folders_first, |this| {
                        this.folders_first = !this.folders_first
                    });
                    let menu = toggle(menu, "Severity", severity, |this| {
                        this.sort_by_severity = !this.sort_by_severity
                    });
                    let menu = toggle(menu, "Name", name, |this| {
                        this.sort_by_name = !this.sort_by_name
                    });
                    toggle(menu.separator(), "Group by Inspection", grouped, |this| {
                        this.group_by_inspection = !this.group_by_inspection
                    })
                }))
            })
            .into_any_element()
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_problem = self
            .selected
            .as_ref()
            .is_some_and(|node| self.problem(node).is_some());
        let button = |id: &'static str, icon: &'static str, tooltip: &'static str| {
            ActionButton::new(id, icon).tooltip(tooltip)
        };
        let empty = self.rows().is_empty();
        if self.tab == Tab::Inspection {
            return self.inspection_toolbar(has_problem, cx).into_any_element();
        }
        v_flex()
            .id("problems-toolbar")
            .role(Role::Toolbar)
            .aria_label("Problems Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(self.view_options(cx))
            .child(
                button(
                    "problems-quick-fixes",
                    "codeInsight/intentionBulb",
                    "Show Quick-Fixes (⌥⏎)",
                )
                .disabled(!has_problem)
                .on_click(cx.listener(|this, _, window, cx| this.show_quick_fixes(window, cx))),
            )
            .child(
                button(
                    "problems-preview",
                    "general/previewVertically",
                    "Open Editor Preview",
                )
                .selected(self.show_preview)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_preview = !this.show_preview;
                    this.update_preview(window, cx);
                    cx.notify();
                })),
            )
            // IntelliJ shows them once the tree has something to open or close
            .when(!empty, |toolbar| {
                toolbar
                    .child(
                        button("problems-expand-all", "general/expandAll", "Expand All")
                            .on_click(cx.listener(|this, _, _, cx| this.set_all_open(true, cx))),
                    )
                    .child(
                        button(
                            "problems-collapse-all",
                            "general/collapseAll",
                            "Collapse All",
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.set_all_open(false, cx))),
                    )
            })
            .into_any_element()
    }

    /// The inspection tab's toolbar, `InspectionResultsView`'s in the New UI: Rerun, Previous and
    /// Next Problem, Show Quick Fixes, Expand All, Collapse All.
    // shortcut: no Edit Inspection Settings, View Options or Export: the language servers have no
    // inspection profile to edit, group by or export from
    fn inspection_toolbar(&self, has_problem: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, icon: &'static str, tooltip: &'static str| {
            ActionButton::new(id, icon).tooltip(tooltip)
        };
        v_flex()
            .id("inspection-toolbar")
            .role(Role::Toolbar)
            .aria_label("Inspection Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(
                button("inspection-rerun", "general/refresh", "Rerun")
                    .on_click(cx.listener(|this, _, window, cx| this.rerun(window, cx))),
            )
            .child(
                button("inspection-previous", "general/up", "Previous Problem")
                    .on_click(cx.listener(|this, _, window, cx| this.step_problem(-1, window, cx))),
            )
            .child(
                button("inspection-next", "general/down", "Next Problem")
                    .on_click(cx.listener(|this, _, window, cx| this.step_problem(1, window, cx))),
            )
            .child(
                button(
                    "inspection-quick-fixes",
                    "codeInsight/intentionBulb",
                    "Show Quick Fixes",
                )
                .disabled(!has_problem)
                .on_click(cx.listener(|this, _, window, cx| this.show_quick_fixes(window, cx))),
            )
            .child(
                button("inspection-expand-all", "general/expandAll", "Expand All")
                    .on_click(cx.listener(|this, _, _, cx| this.set_all_open(true, cx))),
            )
            .child(
                button(
                    "inspection-collapse-all",
                    "general/collapseAll",
                    "Collapse All",
                )
                .on_click(cx.listener(|this, _, _, cx| this.set_all_open(false, cx))),
            )
    }

    /// The tree's empty text, IntelliJ's.
    fn empty_text(&self) -> SharedString {
        match self.tab {
            Tab::ProjectErrors => "No errors found by the IDE".into(),
            Tab::Inspection => "Nothing here".into(),
            Tab::File => match &self.current {
                None => "Open file in editor to see problems".into(),
                Some(path) => {
                    let name = path.path.file_name().unwrap_or_default();
                    format!("No problems in {name}").into()
                }
            },
        }
    }
}

impl Render for ProblemsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.contains_focused(window, cx);
        let header = self.header(cx);
        let rows: Vec<AnyElement> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(ix, node)| self.render_row(node, ix, focused, cx))
            .collect();
        let tree = match rows.is_empty() {
            true => v_flex()
                .id("problems-tree")
                .flex_1()
                .h_full()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child(accessible_text("problems-empty", self.empty_text())),
            false => v_flex()
                .id("problems-tree")
                .role(Role::Tree)
                .aria_label("Problems")
                .flex_1()
                .min_w_0()
                .h_full()
                .py(px(4.))
                .overflow_y_scroll()
                .children(rows),
        };
        let preview = self.show_preview.then(|| {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .border_l_1()
                .border_color(tokens.separator)
                .child(self.preview.render(PreviewLayout::Below, cx))
        });
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("ProblemsPanel menu")
                .on_action(
                    cx.listener(|this, _: &menu::SelectNext, window, cx| this.step(1, window, cx)),
                )
                .on_action(cx.listener(|this, _: &menu::SelectPrevious, window, cx| {
                    this.step(-1, window, cx)
                }))
                .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                    this.open_selected(window, cx);
                }))
                .on_action(cx.listener(
                    |this, _: &problems_panel::CopyProblemDescription, _, cx| {
                        this.copy_description(cx)
                    },
                ))
                .on_action(
                    cx.listener(|this, _: &problems_panel::ShowQuickFixes, window, cx| {
                        this.show_quick_fixes(window, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &problems_panel::JumpToSource, window, cx| {
                        this.open_problem(true, window, cx);
                    }),
                )
                .child(header)
                .child(
                    h_flex()
                        .flex_1()
                        .min_h_0()
                        .items_start()
                        .child(self.toolbar(cx))
                        .child(tree)
                        .children(preview),
                )
                .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                    gpui::deferred(
                        gpui::anchored()
                            .position(*position)
                            .anchor(gpui::Anchor::TopLeft)
                            .child(menu.clone()),
                    )
                    .with_priority(1)
                })),
        )
    }
}

/// A line of text accessibility reads (an empty tool window's text).
pub(crate) fn accessible_text(id: &'static str, text: SharedString) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Label)
        .aria_label(text.clone())
        .child(text)
}

impl Focusable for ProblemsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for ProblemsPanel {}

impl Panel for ProblemsPanel {
    fn persistent_name() -> &'static str {
        "Problems"
    }

    fn panel_key() -> &'static str {
        "ProblemsToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it, as Find's does
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::Warning)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Problems")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(problems_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        // unique in the bottom dock: the Run tool window has 10
        12
    }

    // IntelliJ's `ProblemsView.ToolWindow.SecondaryActions`
    fn gear_items(&self, _: &Window, _: &App) -> Vec<GearItem> {
        vec![
            GearItem {
                label: "Open Files with Single Click".into(),
                checked: Some(self.open_on_select),
                action: Box::new(problems_panel::ToggleOpenFilesWithSingleClick),
            },
            GearItem {
                label: "Enable Preview Tab".into(),
                checked: Some(self.preview_tab),
                action: Box::new(problems_panel::TogglePreviewTab),
            },
            GearItem {
                label: "Open Editor Preview".into(),
                checked: Some(self.show_preview),
                action: Box::new(problems_panel::ToggleEditorPreview),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{Problem, level_counts};
    use language::{DiagnosticSeverity, Point};

    fn problem(severity: DiagnosticSeverity) -> Problem {
        Problem {
            severity,
            message: "".into(),
            range: Point::zero()..Point::zero(),
            source: "".into(),
        }
    }

    #[test]
    fn counts_each_level_as_intellij_does() {
        let problems = [
            problem(DiagnosticSeverity::WARNING),
            problem(DiagnosticSeverity::ERROR),
            problem(DiagnosticSeverity::WARNING),
            problem(DiagnosticSeverity::HINT),
        ];
        assert_eq!(
            level_counts(problems.iter()),
            vec![
                ("1 error".to_owned(), true),
                ("2 warnings".to_owned(), false),
                ("1 weak warning".to_owned(), false),
            ]
        );
        assert!(level_counts([].iter()).is_empty());
    }
}
