//! The Find tool window (⌘3), IntelliJ's usage view for text occurrences and for the usages a
//! language server finds (Find Usages, a refactoring's preview): a tab per search ("Occurrences
//! of 'hello'", "Usages of hello"), the occurrences under their folder and file, the left toolbar
//! (Rerun, Previous and Next Occurrence, Expand All, Collapse All, Preview) and the selected
//! occurrence's code beside the tree; a refactoring's preview ends with Do Refactor and Cancel.
//! It joins the bottom dock the first time results come to it, as IntelliJ shows it once there is
//! something in it.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use collections::HashSet;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Task, WeakEntity, Window, div, px,
};
use gpui_kit::component::Selectable as _;
use gpui_kit::component::{h_flex, v_flex};
use picker::{PreviewBackend, PreviewLayout};
use project::{Project, ProjectPath};
use spork_workbench::tree_keys::highlighted;
use spork_workbench::{ActionButton, Tokens, TreeRow, TreeState, TreeView, tree_keys};
use theme::ActiveTheme as _;
use util::ResultExt as _;
use workspace::dock::{DockPosition, Panel, PanelEvent, PanelHandle};
use workspace::{Workspace, searchable::Direction};

use crate::find_in_files::{FindModel, MAX_OCCURRENCES, Occurrence, find_occurrences};

/// Usages a language server found, for a tab of the Find tool window: Find Usages' (Rerun finds
/// them again) or a refactoring's preview (Do Refactor runs it).
#[derive(Clone)]
pub struct Usages {
    /// The tab's title: "Usages of hello", "Refactoring Preview".
    pub title: SharedString,
    /// The tab's tooltip, IntelliJ's tool window title: "Usages of hello in Project Files".
    pub description: SharedString,
    /// The tree's first row: "Usages", "References to be renamed".
    pub root: SharedString,
    pub occurrences: Vec<Occurrence>,
    pub rerun: Option<Rc<dyn Fn(&mut Window, &mut App) -> Task<Vec<Occurrence>>>>,
    pub refactor: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

/// What a tab shows.
enum Source {
    Text(FindModel),
    Usages(Usages),
}

/// Shows usages in the Find tool window, in a tab of their own, and gives it the focus.
pub fn show_usages(
    workspace: &mut Workspace,
    usages: Usages,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let panel = find_panel(workspace, window, cx);
    panel.update(cx, |panel, cx| panel.add_usages_tab(usages, window, cx));
    workspace.focus_panel::<FindPanel>(window, cx);
}

fn find_panel(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<FindPanel> {
    if let Some(panel) = workspace.panel::<FindPanel>(cx) {
        return panel;
    }
    let handle = cx.entity().downgrade();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| FindPanel::new(handle, project, window, cx));
    workspace.add_panel(panel.clone(), window, cx);
    panel
}

pub mod find_panel {
    gpui::actions!(
        find_panel,
        [
            /// Opens the Find tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus,
            /// Rerun (⌘R): searches again.
            Rerun,
            /// Jump to Source (⌘↓, F4): the selected occurrence in its editor.
            JumpToSource,
            /// Open in Right Split (⇧⏎): the selected occurrence in the split on the right.
            OpenInRightSplit,
            /// Exclude (⌫): the selected occurrences, struck through, left out of the next and
            /// previous occurrence.
            Exclude,
            /// Include (⇧⌫): the selected occurrences back in.
            Include,
            /// Remove (⌘X, ⇧⌦): the selected occurrences out of the tree.
            Remove,
        ]
    );
}

pub(crate) fn register(workspace: &mut Workspace) {
    // IntelliJ's Next and Previous Occurrence (⌥⌘↓, ⌥⌘↑) from anywhere: the last active tool
    // window's occurrences, here the Find tool window's
    // shortcut: the Find tool window only, not the inspection results' problems; take the last
    // active of the two once both navigate
    workspace.register_action(
        |workspace, _: &crate::navigate::NextOccurrence, window, cx| {
            if let Some(panel) = workspace.panel::<FindPanel>(cx) {
                // it opens the occurrence through the workspace
                window.defer(cx, move |window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.go_to_occurrence(Direction::Next, true, window, cx)
                    })
                });
            }
        },
    );
    workspace.register_action(
        |workspace, _: &crate::navigate::PreviousOccurrence, window, cx| {
            if let Some(panel) = workspace.panel::<FindPanel>(cx) {
                // it opens the occurrence through the workspace
                window.defer(cx, move |window, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.go_to_occurrence(Direction::Prev, true, window, cx)
                    })
                });
            }
        },
    );
    workspace.register_action(|workspace, _: &find_panel::ToggleFocus, window, cx| {
        if workspace.panel::<FindPanel>(cx).is_some() {
            workspace.toggle_panel_focus::<FindPanel>(window, cx);
        }
    });
    // the find bar's Open Results in Find Window: its occurrences in the file
    workspace.register_action(
        |workspace, _: &search::buffer_search::OpenResultsInFindWindow, window, cx| {
            let Some(editor) = workspace.active_item_as::<editor::Editor>(cx) else {
                return;
            };
            let pane = workspace.active_pane().clone();
            let Some(bar) = pane
                .read(cx)
                .toolbar()
                .read(cx)
                .item_of_type::<search::BufferSearchBar>()
            else {
                return;
            };
            let (text, options) = {
                let bar = bar.read(cx);
                (bar.query(cx), bar.search_options())
            };
            let Some(buffer) = editor.read(cx).buffer().read(cx).as_singleton() else {
                return;
            };
            if text.is_empty() {
                return;
            }
            let name = buffer
                .read(cx)
                .file()
                .map(|file| file.file_name(cx).to_owned())
                .unwrap_or_default();
            let model = FindModel {
                text,
                match_case: options.contains(search::SearchOptions::CASE_SENSITIVE),
                words: options.contains(search::SearchOptions::WHOLE_WORD),
                regex: options.contains(search::SearchOptions::REGEX),
                scope: crate::find_in_files::Scope::Buffers {
                    title: format!("File '{name}'").into(),
                    buffers: vec![buffer],
                },
                mask: None,
            };
            show_occurrences(workspace, model, None, false, window, cx);
        },
    );
}

/// Shows a search's occurrences in the Find tool window (searching when `occurrences` is
/// `None`), in a new tab or in place of the current one, and gives it the focus.
pub(crate) fn show_occurrences(
    workspace: &mut Workspace,
    model: FindModel,
    occurrences: Option<Vec<Occurrence>>,
    new_tab: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let panel = find_panel(workspace, window, cx);
    panel.update(cx, |panel, cx| {
        panel.add_tab(model, occurrences, new_tab, window, cx)
    });
    workspace.focus_panel::<FindPanel>(window, cx);
}

/// A row of a tab's tree.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Node {
    Root,
    Directory(String),
    File(ProjectPath),
    Occurrence(usize),
}

/// A search shown in the Find tool window.
struct FindTab {
    source: Source,
    title: SharedString,
    /// IntelliJ's tool window title for it, the tab's tooltip: "Occurrences of 'x' in Project".
    description: SharedString,
    occurrences: Vec<Occurrence>,
    searching: bool,
    collapsed: HashSet<Node>,
    tree: TreeState<Node>,
    /// Exclude's occurrences, by index: struck through, skipped by Next/Previous Occurrence.
    excluded: HashSet<usize>,
    /// Remove's occurrences, by index: out of the tree.
    removed: HashSet<usize>,
    _search: Task<()>,
}

impl FindTab {
    /// The tree's first row.
    fn root(&self) -> SharedString {
        match &self.source {
            Source::Text(_) => "Found occurrences".into(),
            Source::Usages(usages) => usages.root.clone(),
        }
    }

    /// A refactoring's Refactor, for its preview.
    fn refactor(&self) -> Option<Rc<dyn Fn(&mut Window, &mut App)>> {
        match &self.source {
            Source::Usages(usages) => usages.refactor.clone(),
            Source::Text(_) => None,
        }
    }
}

pub(crate) struct FindPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    tabs: Vec<FindTab>,
    active: usize,
    show_preview: bool,
    preview: Arc<dyn PreviewBackend>,
    focus: FocusHandle,
    /// The keys' state while no tab shows.
    no_tab: TreeState<Node>,
    context_menu: Option<(
        Entity<ui::ContextMenu>,
        gpui::Point<Pixels>,
        gpui::Subscription,
    )>,
}

impl FindPanel {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = picker_preview::editor_preview(project.clone(), window, cx);
        Self {
            workspace,
            project,
            tabs: Vec::new(),
            active: 0,
            show_preview: true,
            preview,
            focus: cx.focus_handle(),
            no_tab: TreeState::new(),
            context_menu: None,
        }
    }

    fn add_tab(
        &mut self,
        model: FindModel,
        occurrences: Option<Vec<Occurrence>>,
        new_tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let scope = model.scope_title(self.project.read(cx), cx);
        let tab = FindTab {
            title: format!("Occurrences of '{}'", model.text).into(),
            description: format!("Occurrences of '{}' in {scope}", model.text).into(),
            source: Source::Text(model),
            occurrences: Vec::new(),
            searching: false,
            collapsed: HashSet::default(),
            tree: TreeState::new(),
            excluded: HashSet::default(),
            removed: HashSet::default(),
            _search: Task::ready(()),
        };
        self.put_tab(tab, new_tab);
        match occurrences {
            Some(occurrences) => self.set_occurrences(self.active, occurrences, window, cx),
            None => self.rerun(window, cx),
        }
    }

    /// IntelliJ reuses the current tab unless asked for a new one.
    fn put_tab(&mut self, tab: FindTab, new_tab: bool) {
        if new_tab || self.tabs.is_empty() {
            self.tabs.push(tab);
            self.active = self.tabs.len() - 1;
        } else {
            self.active = self.active.min(self.tabs.len() - 1);
            if let Some(current) = self.tabs.get_mut(self.active) {
                *current = tab;
            }
        }
    }

    /// A tab of usages: a new one, as IntelliJ opens each Find Usages in a tab of its own.
    fn add_usages_tab(&mut self, usages: Usages, window: &mut Window, cx: &mut Context<Self>) {
        let occurrences = usages.occurrences.clone();
        let tab = FindTab {
            title: usages.title.clone(),
            description: usages.description.clone(),
            source: Source::Usages(usages),
            occurrences: Vec::new(),
            searching: false,
            collapsed: HashSet::default(),
            tree: TreeState::new(),
            excluded: HashSet::default(),
            removed: HashSet::default(),
            _search: Task::ready(()),
        };
        self.put_tab(tab, true);
        self.set_occurrences(self.active, occurrences, window, cx);
    }

    fn set_occurrences(
        &mut self,
        tab: usize,
        occurrences: Vec<Occurrence>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(tab) {
            tab.occurrences = occurrences;
            tab.excluded.clear();
            tab.removed.clear();
            tab.searching = false;
            tab.tree
                .select((!tab.occurrences.is_empty()).then_some(Node::Occurrence(0)));
        }
        self.update_preview(window, cx);
        cx.notify();
    }

    /// Rerun: searches again with the tab's search.
    fn rerun(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.active;
        let Some(tab) = self.tabs.get(ix) else {
            return;
        };
        let search: Task<Vec<Occurrence>> = match &tab.source {
            Source::Text(model) => {
                let Some(query) = model.query(self.project.read(cx), cx).log_err() else {
                    return;
                };
                let search = find_occurrences(self.project.clone(), query, MAX_OCCURRENCES, cx);
                cx.background_spawn(async move { search.await.0 })
            }
            Source::Usages(Usages {
                rerun: Some(rerun), ..
            }) => rerun(window, cx),
            Source::Usages(_) => return,
        };
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        tab.searching = true;
        tab._search = cx.spawn_in(window, async move |this, cx| {
            let occurrences = search.await;
            this.update_in(cx, |this, window, cx| {
                this.set_occurrences(ix, occurrences, window, cx)
            })
            .ok();
        });
        cx.notify();
    }

    fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);
        self.active = self.active.min(self.tabs.len().saturating_sub(1));
        if self.tabs.is_empty() {
            let workspace = self.workspace.clone();
            window.defer(cx, move |window, cx| {
                workspace
                    .update(cx, |workspace, cx| {
                        workspace.close_panel::<FindPanel>(window, cx)
                    })
                    .log_err();
            });
        }
        self.update_preview(window, cx);
        cx.notify();
    }

    /// The tab's rows: the root, the folders, the files and their occurrences, as far as open.
    fn rows(&self, tab: &FindTab) -> Vec<(Node, usize)> {
        let mut by_dir: BTreeMap<String, BTreeMap<String, Vec<usize>>> = BTreeMap::new();
        let mut files: BTreeMap<String, ProjectPath> = BTreeMap::new();
        for (ix, occurrence) in tab.occurrences.iter().enumerate() {
            if tab.removed.contains(&ix) {
                continue;
            }
            let dir = occurrence
                .path
                .path
                .parent()
                .map(|dir| dir.as_unix_str().to_owned())
                .unwrap_or_default();
            let file = occurrence.path.path.as_unix_str().to_owned();
            files.insert(file.clone(), occurrence.path.clone());
            by_dir
                .entry(dir)
                .or_default()
                .entry(file)
                .or_default()
                .push(ix);
        }
        let mut rows = vec![(Node::Root, 0)];
        if tab.collapsed.contains(&Node::Root) {
            return rows;
        }
        for (dir, dir_files) in by_dir {
            // a file at the root has no folder row
            let depth = match dir.is_empty() {
                true => 1,
                false => {
                    let node = Node::Directory(dir.clone());
                    let open = !tab.collapsed.contains(&node);
                    rows.push((node, 1));
                    if !open {
                        continue;
                    }
                    2
                }
            };
            for (file, occurrences) in dir_files {
                let Some(path) = files.get(&file).cloned() else {
                    continue;
                };
                let node = Node::File(path);
                let open = !tab.collapsed.contains(&node);
                rows.push((node, depth));
                if open {
                    rows.extend(
                        occurrences
                            .into_iter()
                            .map(|ix| (Node::Occurrence(ix), depth + 1)),
                    );
                }
            }
        }
        rows
    }

    fn active_tab(&self) -> Option<&FindTab> {
        self.tabs.get(self.active)
    }

    /// A click on a row: ⇧ extends the selection to it.
    fn click(
        &mut self,
        node: Node,
        modifiers: &gpui::Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.tree.click(node, modifiers);
        }
        self.update_preview(window, cx);
        cx.notify();
    }

    /// Next / Previous Occurrence: the occurrence after (or before) the selection, opened in its
    /// editor (focused with `focus`, as ⌥⌘↓ and ⌥⌘↑ do), its file and folder opened in the tree.
    fn go_to_occurrence(
        &mut self,
        direction: Direction,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.collapsed.clear();
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        // the occurrences in the tree's order, without the excluded ones
        let rows = self.rows(tab);
        let order: Vec<usize> = rows
            .iter()
            .filter_map(|(node, _)| match node {
                Node::Occurrence(ix) if !tab.excluded.contains(ix) => Some(*ix),
                _ => None,
            })
            .collect();
        if order.is_empty() {
            return;
        }
        let lead = tab
            .tree
            .lead()
            .and_then(|lead| rows.iter().position(|(node, _)| node == lead));
        let next = match (lead, direction) {
            (Some(at), Direction::Next) => rows[at + 1..]
                .iter()
                .find_map(|(node, _)| match node {
                    Node::Occurrence(ix) if order.contains(ix) => Some(*ix),
                    _ => None,
                })
                .or(order.last().copied()),
            (Some(at), Direction::Prev) => rows[..at]
                .iter()
                .rev()
                .find_map(|(node, _)| match node {
                    Node::Occurrence(ix) if order.contains(ix) => Some(*ix),
                    _ => None,
                })
                .or(order.first().copied()),
            (None, _) => order.first().copied(),
        };
        let Some(next) = next else {
            return;
        };
        let number = order.iter().position(|ix| *ix == next).unwrap_or(0) + 1;
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.tree.select(Some(Node::Occurrence(next)));
        }
        // IntelliJ's status bar message (`OccurenceNavigatorActionBase`)
        crate::set_status_info(format!("Occurrence {number} of {}", order.len()), cx);
        self.update_preview(window, cx);
        self.open_selected(focus, window, cx);
        cx.notify();
    }

    /// The occurrences under `node`, by index.
    fn occurrences_under(tab: &FindTab, node: &Node) -> Vec<usize> {
        let under = |ix: &usize| {
            let Some(occurrence) = tab.occurrences.get(*ix) else {
                return false;
            };
            match node {
                Node::Root => true,
                Node::Directory(dir) => {
                    occurrence.path.path.parent().map(|p| p.as_unix_str()) == Some(dir.as_str())
                }
                Node::File(path) => &occurrence.path == path,
                Node::Occurrence(at) => at == ix,
            }
        };
        (0..tab.occurrences.len())
            .filter(|ix| !tab.removed.contains(ix) && under(ix))
            .collect()
    }

    /// The selected rows' occurrences, by index.
    fn selected_occurrences(&self, cx: &App) -> Vec<usize> {
        let Some(tab) = self.active_tab() else {
            return Vec::new();
        };
        let rows = self.tree_rows(cx);
        let mut found: Vec<usize> = tab
            .tree
            .selected(&rows)
            .iter()
            .flat_map(|node| Self::occurrences_under(tab, node))
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    /// Exclude (⌫) and Include (⇧⌫): the selected occurrences struck through, or back.
    fn set_excluded(&mut self, exclude: bool, cx: &mut Context<Self>) {
        let found = self.selected_occurrences(cx);
        if let Some(tab) = self.tabs.get_mut(self.active) {
            for ix in found {
                match exclude {
                    true => tab.excluded.insert(ix),
                    false => tab.excluded.remove(&ix),
                };
            }
        }
        cx.notify();
    }

    /// Remove (⌘X): the selected occurrences out of the tree, the row after them selected.
    fn remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let found = self.selected_occurrences(cx);
        let rows = self.tree_rows(cx);
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let after = tab
            .tree
            .selected_range(&rows)
            .and_then(|range| rows.get(range.end() + 1))
            .map(|row| row.key.clone());
        tab.removed.extend(found);
        tab.tree.select(after);
        let rows = self.tree_rows(cx);
        if let Some(tab) = self.tabs.get_mut(self.active)
            && tab.tree.selected_range(&rows).is_none()
        {
            tab.tree.select(rows.last().map(|row| row.key.clone()));
        }
        self.update_preview(window, cx);
        cx.notify();
    }

    /// Open in Right Split (⇧⏎): the occurrence in the split on the right.
    fn open_in_right_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let Some(Node::Occurrence(ix)) = tab.tree.lead().cloned() else {
            return;
        };
        let Some(occurrence) = tab.occurrences.get(ix).cloned() else {
            return;
        };
        self.workspace
            .update(cx, |workspace, cx| {
                crate::open_at(
                    workspace,
                    occurrence.path,
                    Some(occurrence.point),
                    window,
                    cx,
                );
                window.dispatch_action(Box::new(workspace::SplitAndMoveRight), cx);
            })
            .log_err();
    }

    /// IntelliJ's `UsageView.Popup` for the selection, at `position`.
    fn deploy_menu(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        let found = self.selected_occurrences(cx);
        let any_excluded = found.iter().any(|ix| tab.excluded.contains(ix));
        let any_included = found.iter().any(|ix| !tab.excluded.contains(ix));
        let on_occurrence = matches!(tab.tree.lead(), Some(Node::Occurrence(_)));
        let focus = self.focus.clone();
        let menu = ui::ContextMenu::build(window, cx, move |menu, _, _| {
            menu.context(focus.clone())
                .action("Rerun", Box::new(find_panel::Rerun))
                .separator()
                .when(on_occurrence, |menu| {
                    menu.action("Jump to Source", Box::new(find_panel::JumpToSource))
                        .action(
                            "Open in Right Split",
                            Box::new(find_panel::OpenInRightSplit),
                        )
                })
                .when(any_excluded, |menu| {
                    menu.action("Include", Box::new(find_panel::Include))
                })
                .when(any_included, |menu| {
                    menu.action("Exclude", Box::new(find_panel::Exclude))
                })
                .action("Remove", Box::new(find_panel::Remove))
        });
        window.focus(&menu.focus_handle(cx), cx);
        let subscription = cx.subscribe_in(
            &menu,
            window,
            |this, menu, _: &gpui::DismissEvent, window, cx| {
                let had_focus = menu.focus_handle(cx).contains_focused(window, cx);
                if this.context_menu.take().is_some() && had_focus {
                    window.focus(&this.focus, cx);
                }
                cx.notify();
            },
        );
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    fn set_all_open(&mut self, open: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.collapsed.clear();
        if !open {
            for occurrence in &tab.occurrences {
                tab.collapsed.insert(Node::File(occurrence.path.clone()));
                if let Some(dir) = occurrence.path.path.parent() {
                    tab.collapsed
                        .insert(Node::Directory(dir.as_unix_str().to_owned()));
                }
            }
            tab.collapsed.insert(Node::Root);
            tab.tree.select(Some(Node::Root));
        }
        cx.notify();
    }

    fn toggle(&mut self, node: Node, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active)
            && !tab.collapsed.remove(&node)
        {
            tab.collapsed.insert(node);
        }
        cx.notify();
    }

    /// ⏎ or a double click: opens the occurrence in its editor (focusing it with `focus`), or
    /// opens or closes a folder or file.
    fn open_selected(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        match tab.tree.lead().cloned() {
            Some(Node::Occurrence(ix)) => {
                let Some(occurrence) = tab.occurrences.get(ix).cloned() else {
                    return;
                };
                let panel_focus = self.focus.clone();
                self.workspace
                    .update(cx, |workspace, cx| {
                        crate::open_at(
                            workspace,
                            occurrence.path,
                            Some(occurrence.point),
                            window,
                            cx,
                        );
                    })
                    .log_err();
                if !focus {
                    window.focus(&panel_focus, cx);
                }
            }
            Some(node) => self.toggle(node, cx),
            None => {}
        }
    }

    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let occurrence = self.active_tab().and_then(|tab| match tab.tree.lead() {
            Some(Node::Occurrence(ix)) => tab.occurrences.get(*ix).cloned(),
            _ => None,
        });
        match occurrence {
            Some(occurrence) if self.show_preview => {
                let update = occurrence.preview(cx);
                self.preview.update(update, window, cx)
            }
            _ => self.preview.clear(cx),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        tab: &FindTab,
        node: Node,
        depth: usize,
        ix: usize,
        selected: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let found = |text: &str| tab.tree.matches(text);
        // a row whose occurrences are all excluded is struck through, IntelliJ's STYLE_STRIKEOUT
        let under = Self::occurrences_under(tab, &node);
        let excluded = !under.is_empty() && under.iter().all(|ix| tab.excluded.contains(ix));
        let shown = |ix: &usize| !tab.removed.contains(ix);
        let grey = match selected && focused {
            true => tokens.text,
            false => tokens.text_secondary,
        };
        let count = |n: usize| match n {
            1 => "1 result".to_owned(),
            n => format!("{n} results"),
        };
        let chevron = |open: bool| {
            div()
                .flex_none()
                .text_color(tokens.icon)
                .child(
                    spork_workbench::icon(match open {
                        true => "general/chevronDown",
                        false => "general/chevronRight",
                    })
                    .size(px(16.)),
                )
                .into_any_element()
        };
        let row = h_flex()
            .id(("find-row", ix))
            .role(Role::TreeItem)
            .aria_selected(selected)
            .flex_none()
            .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
            .mx(px(spork_workbench::popup::SELECTION_INSET))
            .pl(px(4. + 18. * depth as f32))
            .pr(px(8.))
            .gap(px(4.))
            .rounded(px(spork_workbench::tokens::metrics::RADIUS))
            .map(|row| match (selected, focused) {
                (true, true) => row.bg(tokens.selection),
                (true, false) => row.bg(tokens.selection_inactive),
                (false, _) => row.hover(move |row| row.bg(tokens.selection_hover)),
            });
        let row = match &node {
            Node::Root => row
                .aria_label(tab.root())
                .aria_expanded(!tab.collapsed.contains(&node))
                .child(chevron(!tab.collapsed.contains(&node)))
                .child(div().font_weight(gpui::FontWeight::BOLD).child(highlighted(
                    tab.root(),
                    found(&tab.root()),
                    cx,
                )))
                .child(
                    div()
                        .text_color(grey)
                        .child(count((0..tab.occurrences.len()).filter(shown).count())),
                ),
            Node::Directory(dir) => {
                let n = tab
                    .occurrences
                    .iter()
                    .enumerate()
                    .filter(|(ix, o)| {
                        shown(ix)
                            && o.path.path.parent().map(|p| p.as_unix_str()) == Some(dir.as_str())
                    })
                    .count();
                row.aria_label(SharedString::from(dir.clone()))
                    .aria_expanded(!tab.collapsed.contains(&node))
                    .child(chevron(!tab.collapsed.contains(&node)))
                    .child(spork_workbench::image("nodes/folder", cx))
                    .child(highlighted(SharedString::from(dir.clone()), found(dir), cx))
                    .child(div().text_color(grey).child(count(n)))
            }
            Node::File(path) => {
                let name = path.path.file_name().unwrap_or_default().to_owned();
                let n = tab
                    .occurrences
                    .iter()
                    .enumerate()
                    .filter(|(ix, o)| shown(ix) && &o.path == path)
                    .count();
                row.aria_label(SharedString::from(name.clone()))
                    .aria_expanded(!tab.collapsed.contains(&node))
                    .child(chevron(!tab.collapsed.contains(&node)))
                    .child(spork_workbench::image(
                        spork_workbench::file_icon(&name),
                        cx,
                    ))
                    .child(highlighted(
                        SharedString::from(name.clone()),
                        found(&name),
                        cx,
                    ))
                    .child(div().text_color(grey).child(count(n)))
            }
            Node::Occurrence(ix) => {
                let Some(occurrence) = tab.occurrences.get(*ix) else {
                    return div().into_any_element();
                };
                let line = &occurrence.line;
                let found = occurrence.found.clone();
                let (before, at, after) = match (
                    line.get(..found.start),
                    line.get(found.clone()),
                    line.get(found.end..),
                ) {
                    (Some(before), Some(at), Some(after)) => (before, at, after),
                    _ => (line.as_ref(), "", ""),
                };
                row.aria_label(line.clone())
                    .child(div().w(px(16.)).flex_none())
                    .child(
                        div()
                            .flex_none()
                            .text_color(grey)
                            .child(format!("{}", occurrence.point.row + 1)),
                    )
                    .child(
                        h_flex()
                            .min_w_0()
                            .truncate()
                            .child(before.to_owned())
                            .child(
                                div()
                                    .bg(cx.theme().colors().search_match_background)
                                    .child(at.to_owned()),
                            )
                            .child(after.to_owned()),
                    )
            }
        };
        let menu_node = node.clone();
        row.when(excluded, |row| row.line_through())
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    // a right click keeps a selection it falls in, as IntelliJ's trees
                    let rows = this.tree_rows(cx);
                    if let Some(tab) = this.tabs.get_mut(this.active)
                        && !tab.tree.selected(&rows).contains(&menu_node)
                    {
                        tab.tree.select(Some(menu_node.clone()));
                        this.update_preview(window, cx);
                    }
                    this.deploy_menu(event.position, window, cx);
                }),
            )
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.click(node.clone(), &event.modifiers(), window, cx);
                    if event.click_count() >= 2 {
                        this.open_selected(true, window, cx);
                    }
                }),
            )
            .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let on = ix == self.active;
            let description = tab.description.clone();
            h_flex()
                .id(("find-tab", ix))
                .role(Role::Tab)
                .aria_label(tab.title.clone())
                .aria_selected(on)
                .h(px(28.))
                .px(px(8.))
                .gap(px(4.))
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
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(description.clone())
                        .build(window, cx)
                })
                .child(div().max_w(px(260.)).truncate().child(tab.title.clone()))
                .child(
                    ActionButton::new(("close-find-tab", ix), "general/closeSmall")
                        .tooltip("Close")
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.close_tab(ix, window, cx)),
                        ),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.active = ix;
                    this.update_preview(window, cx);
                    cx.notify();
                }))
        });
        let title = h_flex()
            .gap(px(12.))
            .child(workspace::frame::tool_window_title_label("Find"))
            .child(h_flex().gap(px(2.)).children(tabs))
            .into_any_element();
        workspace::frame::tool_window_header(title, Vec::new(), panel, "Find".into(), cx)
            .into_any_element()
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let button = |id: &'static str, icon: &'static str, tooltip: &'static str| {
            ActionButton::new(id, icon).tooltip(tooltip)
        };
        v_flex()
            .id("find-toolbar")
            .role(Role::Toolbar)
            .aria_label("Find Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(
                button("find-rerun", "run/rerun", "Rerun")
                    .on_click(cx.listener(|this, _, window, cx| this.rerun(window, cx))),
            )
            .child(
                button(
                    "find-previous-occurrence",
                    "general/up",
                    "Previous Occurrence",
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.go_to_occurrence(Direction::Prev, false, window, cx)
                })),
            )
            .child(
                button("find-next-occurrence", "general/down", "Next Occurrence").on_click(
                    cx.listener(|this, _, window, cx| {
                        this.go_to_occurrence(Direction::Next, false, window, cx)
                    }),
                ),
            )
            .child(
                button("find-expand-all", "general/expandAll", "Expand All")
                    .on_click(cx.listener(|this, _, _, cx| this.set_all_open(true, cx))),
            )
            .child(
                button("find-collapse-all", "general/collapseAll", "Collapse All")
                    .on_click(cx.listener(|this, _, _, cx| this.set_all_open(false, cx))),
            )
            .child(
                button("find-preview", "actions/preview", "Preview Source")
                    .selected(self.show_preview)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_preview = !this.show_preview;
                        this.update_preview(window, cx);
                        cx.notify();
                    })),
            )
    }
}

impl Render for FindPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.contains_focused(window, cx);
        let header = self.header(cx);
        let tree_rows = self.tree_rows(cx);
        let (rows, search): (Vec<AnyElement>, _) = match self.active_tab() {
            Some(tab) => {
                let selected = tab.tree.selected_range(&tree_rows);
                let rows = tree_rows
                    .iter()
                    .enumerate()
                    .map(|(ix, row)| {
                        let on = selected.as_ref().is_some_and(|range| range.contains(&ix));
                        self.render_row(tab, row.key.clone(), row.depth, ix, on, focused, cx)
                    })
                    .collect();
                (rows, tab.tree.render_search(&tree_rows, cx))
            }
            None => (Vec::new(), None),
        };
        let searching = self.active_tab().is_some_and(|tab| tab.searching);
        let scroll = match self.active_tab() {
            Some(tab) => tab.tree.scroll_handle().clone(),
            None => self.no_tab.scroll_handle().clone(),
        };
        let tree = v_flex()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .when(searching, |tree| {
                tree.child(
                    div()
                        .px(px(12.))
                        .pt(px(4.))
                        .text_color(tokens.text_secondary)
                        .child("Searching…"),
                )
            })
            .child(
                v_flex()
                    .id("find-tree")
                    .role(Role::Tree)
                    .aria_label("Find Results")
                    .flex_1()
                    .min_h_0()
                    .py(px(4.))
                    .track_scroll(&scroll)
                    .overflow_y_scroll()
                    .children(rows),
            )
            .children(search);
        // a refactoring's preview: Refactor and Cancel under it (IntelliJ's `usageView.doAction`)
        let refactor =
            self.active_tab()
                .and_then(FindTab::refactor)
                .map(|refactor| {
                    let active = self.active;
                    h_flex()
                        .flex_none()
                        .justify_end()
                        .gap(px(8.))
                        .p(px(8.))
                        .border_t_1()
                        .border_color(tokens.separator)
                        .child(
                            spork_workbench::Button::new("refactor-preview-cancel", "Cancel")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_tab(active, window, cx)
                                })),
                        )
                        .child(
                            spork_workbench::Button::new("refactor-preview-do", "Refactor")
                                .primary(true)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.close_tab(active, window, cx);
                                    refactor(window, cx);
                                })),
                        )
                });
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
            tree_keys(
                v_flex()
                    .size_full()
                    .bg(tokens.island)
                    .track_focus(&self.focus)
                    .key_context("FindPanel menu tree"),
                cx,
            )
            .on_action(
                cx.listener(|this, _: &find_panel::Rerun, window, cx| this.rerun(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &find_panel::JumpToSource, window, cx| {
                    this.open_selected(true, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &find_panel::OpenInRightSplit, window, cx| {
                    this.open_in_right_split(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &find_panel::Exclude, _, cx| {
                // ⌫ edits speed search's text while it shows
                if this
                    .active_tab()
                    .is_some_and(|tab| tab.tree.search().is_some())
                {
                    cx.propagate();
                    return;
                }
                this.set_excluded(true, cx)
            }))
            .on_action(
                cx.listener(|this, _: &find_panel::Include, _, cx| this.set_excluded(false, cx)),
            )
            .on_action(cx.listener(|this, _: &find_panel::Remove, window, cx| {
                this.remove_selected(window, cx)
            }))
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
            .children(refactor)
            .children(self.context_menu.as_ref().map(|(menu, position, _)| {
                gpui::deferred(gpui::anchored().position(*position).child(menu.clone()))
                    .with_priority(1)
            })),
        )
    }
}

impl TreeView for FindPanel {
    type Key = Node;

    fn tree(&mut self) -> &mut TreeState<Node> {
        match self.tabs.get_mut(self.active) {
            Some(tab) => &mut tab.tree,
            None => &mut self.no_tab,
        }
    }

    fn tree_rows(&self, _: &App) -> Vec<TreeRow<Node>> {
        let Some(tab) = self.active_tab() else {
            return Vec::new();
        };
        self.rows(tab)
            .into_iter()
            .map(|(node, depth)| {
                let open = Some(!tab.collapsed.contains(&node));
                let (expanded, text): (_, SharedString) = match &node {
                    Node::Root => (open, tab.root()),
                    Node::Directory(dir) => (open, dir.clone().into()),
                    Node::File(path) => (
                        open,
                        path.path.file_name().unwrap_or_default().to_owned().into(),
                    ),
                    Node::Occurrence(ix) => (
                        None,
                        tab.occurrences
                            .get(*ix)
                            .map(|occurrence| occurrence.line.clone())
                            .unwrap_or_default(),
                    ),
                };
                TreeRow::new(node, depth, expanded, text)
            })
            .collect()
    }

    fn set_expanded(
        &mut self,
        node: &Node,
        expanded: bool,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            match expanded {
                true => tab.collapsed.remove(node),
                false => tab.collapsed.insert(node.clone()),
            };
        }
        cx.notify();
    }

    fn set_all_expanded(&mut self, expanded: bool, _: &mut Window, cx: &mut Context<Self>) {
        self.set_all_open(expanded, cx);
    }

    fn selection_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_preview(window, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selected(true, window, cx);
    }
}

impl Focusable for FindPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for FindPanel {}

impl Panel for FindPanel {
    fn persistent_name() -> &'static str {
        "Find"
    }

    fn panel_key() -> &'static str {
        "FindToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it, as Spork's do, until the stripes
    // let tool windows move
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(300.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::MagnifyingGlass)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Find")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(find_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        6
    }
}
