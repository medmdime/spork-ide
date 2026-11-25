//! The Hierarchy tool window (IntelliJ's `CallHierarchyBrowser`, on the right): Call Hierarchy
//! (⌃⌥H) shows the function at the caret and its callers, or its callees, a tab per hierarchy;
//! the toolbar switches between callers and callees and expands or collapses the tree; a double
//! click or ⏎ goes to the function, and a row's menu (`CallHierarchyPopupMenu`) bases the
//! hierarchy on it or jumps to it. Its rows open as the server answers for each.

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Div, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Task, TaskExt as _,
    WeakEntity, Window, div, px,
};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use project::{CallHierarchyItem, Project};
use spork_workbench::{ActionButton, Tokens, TreeRow, TreeState, TreeView, tree_keys};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent, PanelHandle};

use crate::{Able, Target, code, open_location, show_hint};

pub mod hierarchy_panel {
    gpui::actions!(
        hierarchy_panel,
        [
            /// Opens the Hierarchy tool window and focuses it, or gives the focus back to the
            /// editor.
            ToggleFocus
        ]
    );
}

pub(crate) fn register(div: Div, able: &Able, cx: &mut Context<Workspace>) -> Div {
    div.when(able.editor && able.call_hierarchy, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::CallHierarchy, window, cx| {
                call_hierarchy(workspace, window, cx)
            }),
        )
    })
}

pub(crate) fn register_panel(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &hierarchy_panel::ToggleFocus, window, cx| {
        hierarchy_panel(workspace, window, cx);
        workspace.toggle_panel_focus::<HierarchyPanel>(window, cx);
    });
}

/// ⌃⌥H: the function at the caret and its callers, in a new tab of the Hierarchy tool window.
fn call_hierarchy(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let caret = target.caret(cx);
    let prepare = target.project.update(cx, |project, cx| {
        project.prepare_call_hierarchy(&target.buffer, caret, cx)
    });
    let workspace_handle = cx.entity().downgrade();
    window
        .spawn(cx, async move |cx| {
            let items = prepare.await.log_err().flatten().unwrap_or_default();
            cx.update(|window, cx| {
                let Some(item) = items.into_iter().next() else {
                    return show_hint(
                        &target.editor.downgrade(),
                        "Cannot find declaration to go to",
                        true,
                        window,
                        cx,
                    );
                };
                workspace_handle
                    .update(cx, |workspace, cx| {
                        let panel = hierarchy_panel(workspace, window, cx);
                        panel.update(cx, |panel, cx| panel.add_tab(item, window, cx));
                        workspace.focus_panel::<HierarchyPanel>(window, cx);
                    })
                    .log_err();
            })
        })
        .detach_and_log_err(cx);
}

/// The workspace's Hierarchy tool window, added to the right dock the first time it is needed.
fn hierarchy_panel(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<HierarchyPanel> {
    if let Some(panel) = workspace.panel::<HierarchyPanel>(cx) {
        return panel;
    }
    let handle = cx.entity().downgrade();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| HierarchyPanel::new(handle, project, cx));
    workspace.add_panel(panel.clone(), window, cx);
    panel
}

/// Callers or callees: IntelliJ's Caller Method Hierarchy and Callee Method Hierarchy.
#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Callers,
    Callees,
}

/// A row of the tree: a function, its children once the server has answered.
struct Node {
    item: CallHierarchyItem,
    depth: usize,
    children: Option<Vec<usize>>,
    open: bool,
}

struct HierarchyTab {
    title: SharedString,
    direction: Direction,
    root: CallHierarchyItem,
    nodes: Vec<Node>,
    /// The selected nodes, by their index in `nodes`.
    tree: TreeState<usize>,
    _requests: Vec<Task<()>>,
}

impl HierarchyTab {
    fn new(root: CallHierarchyItem, direction: Direction) -> Self {
        let title = match direction {
            Direction::Callers => format!("Callers of {}", root.name),
            Direction::Callees => format!("Callees of {}", root.name),
        };
        Self {
            title: title.into(),
            direction,
            nodes: vec![Node {
                item: root.clone(),
                depth: 0,
                children: None,
                open: false,
            }],
            root,
            tree: {
                let mut tree = TreeState::new();
                tree.select(Some(0));
                tree
            },
            _requests: Vec::new(),
        }
    }

    /// The open rows, depth first.
    fn rows(&self) -> Vec<usize> {
        let mut rows = Vec::new();
        let mut stack = vec![0];
        while let Some(ix) = stack.pop() {
            let Some(node) = self.nodes.get(ix) else {
                continue;
            };
            rows.push(ix);
            if node.open
                && let Some(children) = &node.children
            {
                stack.extend(children.iter().rev());
            }
        }
        rows
    }
}

pub(crate) struct HierarchyPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    tabs: Vec<HierarchyTab>,
    active: usize,
    focus: FocusHandle,
    /// The keys' state while no tab shows.
    no_tab: TreeState<usize>,
}

impl HierarchyPanel {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            workspace,
            project,
            tabs: Vec::new(),
            active: 0,
            focus: cx.focus_handle(),
            no_tab: TreeState::new(),
        }
    }

    fn add_tab(&mut self, root: CallHierarchyItem, window: &mut Window, cx: &mut Context<Self>) {
        self.tabs.push(HierarchyTab::new(root, Direction::Callers));
        self.active = self.tabs.len() - 1;
        self.set_open(0, true, window, cx);
    }

    fn set_direction(&mut self, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if tab.direction == direction {
            return;
        }
        *tab = HierarchyTab::new(tab.root.clone(), direction);
        self.set_open(0, true, window, cx);
    }

    /// Opens or closes a row, asking the server for its children the first time.
    fn set_open(&mut self, ix: usize, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.active;
        let Some(tab) = self.tabs.get_mut(active) else {
            return;
        };
        let direction = tab.direction;
        let Some(node) = tab.nodes.get_mut(ix) else {
            return;
        };
        node.open = open;
        if !open || node.children.is_some() {
            cx.notify();
            return;
        }
        node.children = Some(Vec::new());
        let item = node.item.clone();
        let request = self.project.update(cx, |project, cx| match direction {
            Direction::Callers => {
                let calls = project.incoming_calls(item, cx);
                cx.background_spawn(async move {
                    calls
                        .await
                        .log_err()
                        .flatten()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|call| call.from)
                        .collect::<Vec<_>>()
                })
            }
            Direction::Callees => {
                let calls = project.outgoing_calls(item, cx);
                cx.background_spawn(async move {
                    calls
                        .await
                        .log_err()
                        .flatten()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|call| call.to)
                        .collect::<Vec<_>>()
                })
            }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let items = request.await;
            this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.get_mut(active) else {
                    return;
                };
                let depth = tab.nodes.get(ix).map_or(0, |node| node.depth) + 1;
                let mut children = Vec::new();
                for item in items {
                    children.push(tab.nodes.len());
                    tab.nodes.push(Node {
                        item,
                        depth,
                        children: None,
                        open: false,
                    });
                }
                if let Some(node) = tab.nodes.get_mut(ix) {
                    node.children = Some(children);
                }
                cx.notify();
            })
            .ok();
        });
        tab._requests.push(task);
        cx.notify();
    }

    fn set_all_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active) else {
            return;
        };
        let shown = tab.rows();
        for ix in shown {
            self.set_open(ix, open || ix == 0, window, cx);
        }
    }

    fn go_to(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.tabs.get(self.active).and_then(|tab| tab.nodes.get(ix)) else {
            return;
        };
        let (buffer, range) = (node.item.buffer.clone(), node.item.selection_range.clone());
        self.workspace
            .update(cx, |workspace, cx| {
                open_location(workspace, buffer, range, window, cx)
            })
            .log_err();
    }

    /// Base on This Method: the tab's hierarchy again, rooted at the row's function.
    fn base_on(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let Some(item) = tab.nodes.get(ix).map(|node| node.item.clone()) else {
            return;
        };
        *tab = HierarchyTab::new(item, tab.direction);
        self.set_open(0, true, window, cx);
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
                        workspace.close_panel::<HierarchyPanel>(window, cx)
                    })
                    .log_err();
            });
        }
        cx.notify();
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let on = ix == self.active;
            h_flex()
                .id(("hierarchy-tab", ix))
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
                .child(div().max_w(px(220.)).truncate().child(tab.title.clone()))
                .child(
                    ActionButton::new(("close-hierarchy-tab", ix), "general/closeSmall")
                        .tooltip("Close")
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.close_tab(ix, window, cx)),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.active = ix;
                    cx.notify();
                }))
        });
        let title = h_flex()
            .gap(px(12.))
            .child(workspace::frame::tool_window_title_label("Hierarchy"))
            .child(h_flex().gap(px(2.)).children(tabs))
            .into_any_element();
        workspace::frame::tool_window_header(title, Vec::new(), panel, "Hierarchy".into(), cx)
            .into_any_element()
    }

    /// IntelliJ's hierarchy toolbar: Caller and Callee Method Hierarchy, Expand All, Collapse
    /// All.
    fn toolbar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        use gpui_kit::component::Selectable as _;
        let direction = self.tabs.get(self.active).map(|tab| tab.direction);
        h_flex()
            .id("hierarchy-toolbar")
            .role(Role::Toolbar)
            .aria_label("Hierarchy Toolbar")
            .flex_none()
            .h(px(32.))
            .px(px(8.))
            .gap(px(4.))
            .child(
                ActionButton::new("hierarchy-callers", "hierarchy/supertypes")
                    .tooltip("Caller Method Hierarchy")
                    .selected(direction == Some(Direction::Callers))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_direction(Direction::Callers, window, cx)
                    })),
            )
            .child(
                ActionButton::new("hierarchy-callees", "hierarchy/subtypes")
                    .tooltip("Callee Method Hierarchy")
                    .selected(direction == Some(Direction::Callees))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_direction(Direction::Callees, window, cx)
                    })),
            )
            .child(
                ActionButton::new("hierarchy-expand-all", "general/expandAll")
                    .tooltip("Expand All")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.set_all_open(true, window, cx)),
                    ),
            )
            .child(
                ActionButton::new("hierarchy-collapse-all", "general/collapseAll")
                    .tooltip("Collapse All")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.set_all_open(false, window, cx)),
                    ),
            )
    }

    fn render_row(
        &self,
        tab: &HierarchyTab,
        ix: usize,
        selected: bool,
        focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = Tokens::get(cx);
        let Some(node) = tab.nodes.get(ix) else {
            return div().into_any_element();
        };
        let leaf = node.children.as_ref().is_some_and(Vec::is_empty);
        let file = node
            .item
            .buffer
            .read(cx)
            .file()
            .map(|file| file.file_name(cx).to_owned())
            .unwrap_or_default();
        let label = SharedString::from(node.item.name.clone());
        h_flex()
            .id(("hierarchy-row", ix))
            .role(Role::TreeItem)
            .aria_label(label.clone())
            .aria_selected(selected)
            .when(!leaf, |row| row.aria_expanded(node.open))
            .flex_none()
            .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
            .mx(px(spork_workbench::popup::SELECTION_INSET))
            .pl(px(4. + 18. * node.depth as f32))
            .pr(px(8.))
            .gap(px(4.))
            .rounded(px(spork_workbench::tokens::metrics::RADIUS))
            .map(|row| match (selected, focused) {
                (true, true) => row.bg(tokens.selection),
                (true, false) => row.bg(tokens.selection_inactive),
                (false, _) => row.hover(move |row| row.bg(tokens.selection_hover)),
            })
            .child(div().w(px(16.)).flex_none().when(!leaf, |chevron| {
                chevron.text_color(tokens.icon).child(
                    spork_workbench::icon(match node.open {
                        true => "general/chevronDown",
                        false => "general/chevronRight",
                    })
                    .size(px(16.)),
                )
            }))
            .child(spork_workbench::image(
                match node.item.kind {
                    lsp::SymbolKind::METHOD => "nodes/method",
                    _ => "nodes/function",
                },
                cx,
            ))
            .child(label)
            .child(
                div()
                    .text_color(tokens.text_secondary)
                    .child(format!("({file})")),
            )
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if let Some(tab) = this.tabs.get_mut(this.active) {
                        tab.tree.click(ix, &event.modifiers());
                    }
                    if event.click_count() >= 2 {
                        this.go_to(ix, window, cx);
                    } else if !leaf {
                        let open = this
                            .tabs
                            .get(this.active)
                            .and_then(|tab| tab.nodes.get(ix))
                            .is_some_and(|node| node.open);
                        this.set_open(ix, !open, window, cx);
                    }
                    cx.notify();
                }),
            )
            .context_menu({
                let this = cx.entity().downgrade();
                move |menu, _, _| {
                    let item = |label: &'static str,
                                run: fn(
                        &mut HierarchyPanel,
                        usize,
                        &mut Window,
                        &mut Context<HierarchyPanel>,
                    )| {
                        let this = this.clone();
                        PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            this.update(cx, |this, cx| {
                                if let Some(tab) = this.tabs.get_mut(this.active) {
                                    tab.tree.select(Some(ix));
                                }
                                run(this, ix, window, cx)
                            })
                            .log_err();
                        })
                    };
                    menu.item(item("Base on This Method", |this, ix, window, cx| {
                        this.base_on(ix, window, cx)
                    }))
                    .item(item("Jump to Source", |this, ix, window, cx| {
                        this.go_to(ix, window, cx)
                    }))
                }
            })
            .into_any_element()
    }
}

impl Render for HierarchyPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.contains_focused(window, cx);
        let header = self.header(cx);
        let toolbar = self.toolbar(cx);
        let tree_rows = self.tree_rows(cx);
        let (rows, scroll): (Vec<AnyElement>, _) = match self.tabs.get(self.active) {
            Some(tab) => {
                let selected = tab.tree.selected_range(&tree_rows);
                let rows = tree_rows
                    .iter()
                    .enumerate()
                    .map(|(at, row)| {
                        let on = selected.as_ref().is_some_and(|range| range.contains(&at));
                        self.render_row(tab, row.key, on, focused, cx)
                    })
                    .collect();
                (rows, Some(tab.tree.scroll_handle().clone()))
            }
            None => (Vec::new(), None),
        };
        spork_vcs::spork_look(cx).size_full().child(
            tree_keys(
                v_flex()
                    .size_full()
                    .bg(tokens.island)
                    .track_focus(&self.focus)
                    .key_context("HierarchyPanel menu tree"),
                cx,
            )
            // ⌃⌥H in the tree: Base on This Method
            .on_action(cx.listener(|this, _: &code::CallHierarchy, window, cx| {
                if let Some(selected) = this
                    .tabs
                    .get(this.active)
                    .and_then(|tab| tab.tree.lead().copied())
                {
                    this.base_on(selected, window, cx);
                }
            }))
            .child(header)
            .child(toolbar)
            .child(
                v_flex()
                    .id("hierarchy-tree")
                    .role(Role::Tree)
                    .aria_label("Hierarchy")
                    .flex_1()
                    .min_h_0()
                    .py(px(4.))
                    .when_some(scroll, |tree, scroll| tree.track_scroll(&scroll))
                    .overflow_y_scroll()
                    .when(rows.is_empty(), |tree| {
                        tree.child(
                            div()
                                .p(px(12.))
                                .text_color(tokens.text_secondary)
                                .child("Nothing to show"),
                        )
                    })
                    .children(rows),
            ),
        )
    }
}

impl TreeView for HierarchyPanel {
    type Key = usize;

    fn tree(&mut self) -> &mut TreeState<usize> {
        match self.tabs.get_mut(self.active) {
            Some(tab) => &mut tab.tree,
            None => &mut self.no_tab,
        }
    }

    fn tree_rows(&self, _: &App) -> Vec<TreeRow<usize>> {
        let Some(tab) = self.tabs.get(self.active) else {
            return Vec::new();
        };
        tab.rows()
            .into_iter()
            .filter_map(|ix| {
                let node = tab.nodes.get(ix)?;
                let leaf = node.children.as_ref().is_some_and(Vec::is_empty);
                let expanded = (!leaf).then_some(node.open);
                Some(TreeRow::new(
                    ix,
                    node.depth,
                    expanded,
                    node.item.name.clone(),
                ))
            })
            .collect()
    }

    fn set_expanded(
        &mut self,
        ix: &usize,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_open(*ix, open, window, cx);
    }

    fn set_all_expanded(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.set_all_open(open, window, cx);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self
            .tabs
            .get(self.active)
            .and_then(|tab| tab.tree.lead().copied())
        {
            self.go_to(ix, window, cx);
        }
    }

    // shortcut: no speed search over the calls yet; IntelliJ's hierarchy tree has it, worth it
    // with highlighted rows
    fn speed_search(&self) -> bool {
        false
    }
}

impl Focusable for HierarchyPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for HierarchyPanel {}

impl Panel for HierarchyPanel {
    fn persistent_name() -> &'static str {
        "Hierarchy"
    }

    fn panel_key() -> &'static str {
        "HierarchyToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Right
    }

    // shortcut: the tool window stays where IntelliJ puts it, as Spork's do, until the stripes
    // let tool windows move
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Right
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(400.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::ListTree)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Hierarchy")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(hierarchy_panel::ToggleFocus)
    }

    // the docks' priorities are unique: the other tool windows take 2 to 12
    fn activation_priority(&self) -> u32 {
        15
    }
}
