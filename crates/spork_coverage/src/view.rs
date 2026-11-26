//! IntelliJ's Coverage tool window (`CoverageView`, Java's `JavaCoverageViewExtension`): on the
//! right, added when coverage first shows and activated as "Activate Coverage View" says, its tab
//! the coverage shown (closing it hides the coverage), its toolbar (Flatten Packages, Navigate
//! with Single Click, Always Select Opened Element, Generate Coverage Report…, Filters), the
//! suite's tree by package and class with the Class, Method, Line and Branch columns; a double
//! click or ⏎ on a class goes to it.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use collections::HashSet;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TaskExt as _, WeakEntity, Window,
    div, px,
};
use gpui_kit::component::Selectable as _;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use serde_json::{Value, json};
use spork_workbench::tokens::metrics;
use spork_workbench::{ActionButton, Tokens};
use util::ResultExt as _;
use workspace::Workspace;
use workspace::dock::{DockPosition, Panel, PanelEvent};

use crate::data::{Class, Counts, Data};
use crate::{Coverage, GenerateCoverageReport, ToggleFocus};

/// A node of the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Root,
    Package,
    Class,
}

#[derive(Clone, Debug)]
struct Node {
    /// What the Element column shows: `all`, `com.example`, `Calculator`.
    name: String,
    /// The node's identity across rebuilds: its package or class.
    key: String,
    kind: Kind,
    counts: Counts,
    file: Option<PathBuf>,
    children: Vec<Node>,
}

/// The columns IntelliJ's Java view shows: Element, then Class, Method, Line and Branch, %.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Element,
    Class,
    Method,
    Line,
    Branch,
}

impl Column {
    fn title(self) -> &'static str {
        match self {
            Column::Element => "Element",
            Column::Class => "Class, %",
            Column::Method => "Method, %",
            Column::Line => "Line, %",
            Column::Branch => "Branch, %",
        }
    }

    fn ratio(self, counts: &Counts) -> Option<crate::data::Ratio> {
        match self {
            Column::Element => None,
            Column::Class => Some(counts.classes),
            Column::Method => Some(counts.methods),
            Column::Line => Some(counts.lines),
            Column::Branch => Some(counts.branches),
        }
    }
}

/// A data column's width.
const COLUMN_WIDTH: f32 = 100.;

/// The tree of the classes: by package (a chain of packages with one package each shown as one,
/// `a.b.c`, as `CoverageClassStructure.collapseLongEdges`), or flattened, each package by its
/// full name; inner classes counted with their top-level class.
fn tree(data: &Data, flatten: bool, hide_full: bool) -> Node {
    let mut by_top: BTreeMap<&str, Vec<&Class>> = BTreeMap::new();
    for class in &data.classes {
        by_top.entry(class.top_level()).or_default().push(class);
    }
    let classes: Vec<Node> = by_top
        .into_iter()
        .map(|(top, classes)| {
            let counts = Data::counts_of(classes.iter().copied());
            Node {
                name: top
                    .rsplit_once('.')
                    .map_or(top, |(_, name)| name)
                    .to_owned(),
                key: top.to_owned(),
                kind: Kind::Class,
                counts,
                file: classes.iter().find_map(|class| class.file.clone()),
                children: Vec::new(),
            }
        })
        .filter(|node| !(hide_full && full(&node.counts)))
        .collect();
    let package_of = |node: &Node| {
        node.key
            .rsplit_once('.')
            .map_or(String::new(), |(package, _)| package.to_owned())
    };
    let mut root = Node {
        name: "all".into(),
        key: String::new(),
        kind: Kind::Root,
        counts: Counts::default(),
        file: None,
        children: Vec::new(),
    };
    if flatten {
        let mut packages: BTreeMap<String, Vec<Node>> = BTreeMap::new();
        for class in classes {
            packages.entry(package_of(&class)).or_default().push(class);
        }
        root.children = packages
            .into_iter()
            .map(|(package, classes)| package_node(&package, &package, classes))
            .collect();
    } else {
        for class in classes {
            let package = package_of(&class);
            let mut node = &mut root;
            let mut path = String::new();
            for segment in package.split('.').filter(|segment| !segment.is_empty()) {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(segment);
                let ix = match node
                    .children
                    .iter()
                    .position(|child| child.kind == Kind::Package && child.key == path)
                {
                    Some(ix) => ix,
                    None => {
                        node.children.push(package_node(segment, &path, Vec::new()));
                        node.children.len() - 1
                    }
                };
                // `ix` is a child's: found, or the one just pushed
                node = &mut node.children[ix];
            }
            node.children.push(class);
        }
        for child in &mut root.children {
            collapse(child);
        }
    }
    total(&mut root);
    root
}

fn full(counts: &Counts) -> bool {
    counts.lines.covered == counts.lines.total && counts.branches.covered == counts.branches.total
}

fn package_node(name: &str, key: &str, children: Vec<Node>) -> Node {
    Node {
        name: name.to_owned(),
        key: key.to_owned(),
        kind: Kind::Package,
        counts: Counts::default(),
        file: None,
        children,
    }
}

/// A package with one package in it and nothing else shows with it as one.
fn collapse(node: &mut Node) {
    while node.kind == Kind::Package
        && let [only] = node.children.as_slice()
        && only.kind == Kind::Package
    {
        let Some(child) = node.children.pop() else {
            break;
        };
        node.name = format!("{}.{}", node.name, child.name);
        node.key = child.key;
        node.children = child.children;
    }
    for child in &mut node.children {
        collapse(child);
    }
}

/// A package's counts: its classes', those of its packages included.
fn total(node: &mut Node) {
    if node.kind == Kind::Class {
        return;
    }
    for child in &mut node.children {
        total(child);
    }
    node.counts = node
        .children
        .iter()
        .fold(Counts::default(), |all, child| all.plus(child.counts));
}

/// A row of the tree as shown.
struct Row<'a> {
    node: &'a Node,
    depth: usize,
    expanded: bool,
}

pub struct CoverageView {
    coverage: Entity<Coverage>,
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    flatten: bool,
    single_click: bool,
    select_opened: bool,
    hide_full: bool,
    /// The packages closed, by key; the tree starts open two levels.
    collapsed: HashSet<String>,
    selected: Option<String>,
    sort: (Column, bool),
    _subscriptions: Vec<Subscription>,
}

impl CoverageView {
    fn new(
        coverage: Entity<Coverage>,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let _subscriptions = vec![
            cx.observe(&coverage, |this, _, cx| {
                this.collapsed.clear();
                cx.notify();
            }),
            cx.subscribe_in(&workspace, window, |this, _, event, _, cx| {
                if matches!(event, workspace::Event::ActiveItemChanged) && this.select_opened {
                    this.select_opened_element(cx);
                }
            }),
        ];
        Self {
            coverage,
            workspace: workspace.downgrade(),
            focus: cx.focus_handle(),
            flatten: false,
            single_click: false,
            select_opened: false,
            hide_full: false,
            collapsed: HashSet::default(),
            selected: None,
            sort: (Column::Element, true),
            _subscriptions,
        }
    }

    fn root(&self, cx: &App) -> Option<Node> {
        let data = self.coverage.read(cx).data.clone()?;
        let mut root = tree(&data, self.flatten, self.hide_full);
        self.sort_node(&mut root);
        Some(root)
    }

    fn sort_node(&self, node: &mut Node) {
        let (column, ascending) = self.sort;
        node.children.sort_by(|a, b| {
            let order = match column.ratio(&a.counts).zip(column.ratio(&b.counts)) {
                Some((a, b)) => a
                    .percent()
                    .cmp(&b.percent())
                    .then(a.total.cmp(&b.total))
                    .then(a.covered.cmp(&b.covered)),
                // `AlphaComparator`: packages before classes, then by name
                None => weight(a.kind)
                    .cmp(&weight(b.kind))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            };
            match ascending {
                true => order,
                false => order.reverse(),
            }
        });
        for child in &mut node.children {
            self.sort_node(child);
        }
    }

    /// The rows shown: the root only when it has more than one child (IntelliJ's), each
    /// package's children while it is open.
    fn rows<'a>(&self, root: &'a Node) -> Vec<Row<'a>> {
        fn walk<'a>(view: &CoverageView, node: &'a Node, depth: usize, rows: &mut Vec<Row<'a>>) {
            let expanded = node.kind != Kind::Class && !view.collapsed.contains(&node.key);
            rows.push(Row {
                node,
                depth,
                expanded,
            });
            if expanded {
                for child in &node.children {
                    walk(view, child, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        match root.children.len() > 1 {
            true => walk(self, root, 0, &mut rows),
            false => {
                for child in &root.children {
                    walk(self, child, 0, &mut rows);
                }
            }
        }
        rows
    }

    fn columns(&self, cx: &App) -> Vec<Column> {
        let branches = self
            .coverage
            .read(cx)
            .active
            .iter()
            .any(|suite| suite.branch_coverage);
        let mut columns = vec![Column::Element, Column::Class, Column::Method, Column::Line];
        if branches {
            columns.push(Column::Branch);
        }
        columns
    }

    /// The tab's title: the run configuration's name, or the suites' names.
    fn title(&self, cx: &App) -> Option<String> {
        let active = &self.coverage.read(cx).active;
        (!active.is_empty()).then(|| {
            active
                .iter()
                .map(|suite| suite.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
    }

    fn open(&self, node: &Node, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(file) = node.file.clone() else {
            return;
        };
        let simple = node.name.clone();
        let line = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| {
                text.lines().position(|line| {
                    ["class", "interface", "enum", "record", "object"]
                        .iter()
                        .any(|keyword| line.contains(&format!("{keyword} {simple}")))
                })
            })
            .unwrap_or(0) as u32;
        let focus_handle = self.focus.clone();
        self.workspace
            .update(cx, |workspace, cx| {
                open_at(workspace, file, line, window, cx);
                if !focus {
                    window.focus(&focus_handle, cx);
                }
            })
            .log_err();
    }

    /// Always Select Opened Element: the class of the active editor's file selected.
    fn select_opened_element(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let Some(path) = workspace
            .read(cx)
            .active_item(cx)
            .and_then(|item| item.project_path(cx))
            .and_then(|path| {
                workspace
                    .read(cx)
                    .project()
                    .read(cx)
                    .absolute_path(&path, cx)
            })
        else {
            return;
        };
        let Some(data) = self.coverage.read(cx).data.clone() else {
            return;
        };
        if let Some(class) = data
            .classes
            .iter()
            .find(|class| class.file.as_deref() == Some(path.as_path()))
        {
            self.selected = Some(class.top_level().to_owned());
            cx.notify();
        }
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        cx.notify();
    }

    fn header_row(&self, columns: &[Column], cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let (sorted, ascending) = self.sort;
        h_flex()
            .id("coverage-columns")
            .role(Role::Row)
            .flex_none()
            .h(px(metrics::ROW_HEIGHT))
            .border_b_1()
            .border_color(tokens.separator)
            .text_color(tokens.text_secondary)
            .children(columns.iter().map(|column| {
                let column = *column;
                let arrow = (column == sorted).then(|| match ascending {
                    true => "general/arrowUp",
                    false => "general/arrowDown",
                });
                h_flex()
                    .id(SharedString::from(format!(
                        "coverage-column-{}",
                        column.title()
                    )))
                    .role(Role::ColumnHeader)
                    .aria_label(column.title())
                    .px(px(6.))
                    .gap(px(2.))
                    .h_full()
                    .cursor_pointer()
                    .map(|cell| match column {
                        Column::Element => cell.flex_1().min_w_0(),
                        _ => cell
                            .w(px(COLUMN_WIDTH))
                            .flex_none()
                            .border_l_1()
                            .border_color(tokens.separator),
                    })
                    .child(column.title())
                    .children(arrow.map(|arrow| spork_workbench::icon(arrow).size(px(12.))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sort = match this.sort {
                            (sorted, ascending) if sorted == column => (column, !ascending),
                            _ => (column, true),
                        };
                        cx.notify();
                    }))
            }))
    }

    fn row(
        &self,
        row: &Row,
        columns: &[Column],
        focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let selection = match focused {
            true => tokens.selection,
            false => tokens.selection_inactive,
        };
        let node = row.node.clone();
        let selected = self.selected.as_deref() == Some(node.key.as_str());
        let icon = match node.kind {
            Kind::Class => "nodes/class",
            Kind::Package | Kind::Root => "nodes/package",
        };
        let chevron = (node.kind != Kind::Class).then(|| {
            spork_workbench::icon(match row.expanded {
                true => "general/chevronDown",
                false => "general/chevronRight",
            })
            .size(px(16.))
        });
        let key = node.key.clone();
        let jump = (cx.entity().downgrade(), node.clone());
        let row = h_flex()
            .id(SharedString::from(format!("coverage-row-{}", node.key)))
            .role(Role::Row)
            .aria_label(SharedString::from(node.name.clone()))
            .aria_selected(selected)
            .h(px(metrics::ROW_HEIGHT))
            .flex_none()
            .cursor_pointer()
            .when(selected, |row| row.bg(selection))
            .when(!selected, |row| row.hover(|row| row.bg(tokens.hover)))
            .children(columns.iter().map(|column| {
                match column {
                    Column::Element => h_flex()
                        .flex_1()
                        .min_w_0()
                        .pl(px(6. + row.depth as f32 * 18.))
                        .gap(px(4.))
                        .child(div().w(px(16.)).flex_none().children(chevron.clone()))
                        .child(spork_workbench::image(icon, cx).size(px(16.)).flex_none())
                        .child(div().truncate().child(node.name.clone()))
                        .into_any_element(),
                    column => div()
                        .w(px(COLUMN_WIDTH))
                        .flex_none()
                        .px(px(6.))
                        .truncate()
                        .children(column.ratio(&node.counts).map(|ratio| ratio.text()))
                        .into_any_element(),
                }
            }))
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.selected = Some(key.clone());
                    window.focus(&this.focus, cx);
                    if node.kind == Kind::Class {
                        match event.click_count() {
                            2.. => this.open(&node, true, window, cx),
                            _ if this.single_click => this.open(&node, false, window, cx),
                            _ => {}
                        }
                    } else if event.click_count() >= 2 {
                        this.toggle(&node.key, cx);
                    }
                    cx.notify();
                }),
            );
        // IntelliJ's one item: Jump to Source
        match jump.1.kind {
            Kind::Class => row
                .context_menu(move |menu, _, _| {
                    let (this, node) = jump.clone();
                    menu.item(PopupMenuItem::new("Jump to Source").on_click(
                        move |_, window, cx| {
                            this.update(cx, |this, cx| this.open(&node, true, window, cx))
                                .log_err();
                        },
                    ))
                })
                .into_any_element(),
            _ => row.into_any_element(),
        }
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let toggle = |id: &'static str,
                      icon: &'static str,
                      tooltip: &'static str,
                      on: bool,
                      set: fn(&mut Self)| {
            ActionButton::new(id, icon)
                .tooltip(tooltip)
                .selected(on)
                .on_click(cx.listener(move |this, _, _, cx| {
                    set(this);
                    cx.notify();
                }))
        };
        let this = cx.entity().downgrade();
        let hide_full = self.hide_full;
        h_flex()
            .id("coverage-toolbar")
            .role(Role::Toolbar)
            .aria_label("CoverageView")
            .flex_none()
            .h(px(32.))
            .px(px(5.))
            .gap(px(4.))
            .child(toggle(
                "coverage-flatten",
                "objectBrowser/flattenPackages",
                "Flatten Packages",
                self.flatten,
                |this| this.flatten = !this.flatten,
            ))
            .child(toggle(
                "coverage-single-click",
                "general/autoscrollToSource",
                "Navigate with Single Click",
                self.single_click,
                |this| this.single_click = !this.single_click,
            ))
            .child(toggle(
                "coverage-select-opened",
                "general/autoscrollFromSource",
                "Always Select Opened Element",
                self.select_opened,
                |this| this.select_opened = !this.select_opened,
            ))
            .child(
                ActionButton::new("coverage-report", "general/export")
                    .tooltip("Generate Coverage Report…")
                    .on_click(|_, window, cx| {
                        window.dispatch_action(GenerateCoverageReport.boxed_clone(), cx)
                    }),
            )
            .child(
                ActionButton::new("coverage-filters", "general/filter")
                    .tooltip("Filters")
                    .dropdown_menu(move |menu, _, _| {
                        let this = this.clone();
                        menu.item(
                            PopupMenuItem::new("Hide Fully Covered Classes")
                                .checked(hide_full)
                                .on_click(move |_, _, cx| {
                                    this.update(cx, |this, cx| {
                                        this.hide_full = !this.hide_full;
                                        cx.notify();
                                    })
                                    .log_err();
                                }),
                        )
                    }),
            )
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let panel = std::sync::Arc::new(cx.entity());
        let tab = self.title(cx).map(|title| {
            h_flex()
                .id("coverage-tab")
                .role(Role::Tab)
                .aria_label(SharedString::from(title.clone()))
                .aria_selected(true)
                .h(px(28.))
                .px(px(8.))
                .gap(px(4.))
                .rounded(px(6.))
                .border_1()
                .bg(tokens.tab_selected)
                .border_color(tokens.tab_selected_border)
                .child(div().max_w(px(260.)).truncate().child(title))
                .child(
                    ActionButton::new("close-coverage-tab", "general/closeSmall")
                        .tooltip("Close")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.coverage.update(cx, |coverage, cx| coverage.hide(cx));
                        })),
                )
        });
        let title = h_flex()
            .gap(px(12.))
            .child(workspace::frame::tool_window_title_label("Coverage"))
            .children(tab)
            .into_any_element();
        workspace::frame::tool_window_header(title, Vec::new(), panel, "Coverage".into(), cx)
    }
}

fn weight(kind: Kind) -> u8 {
    match kind {
        Kind::Root => 0,
        Kind::Package => 30,
        Kind::Class => 40,
    }
}

/// Opens `path` with its caret on `line`.
pub(crate) fn open_at(
    workspace: &mut Workspace,
    path: PathBuf,
    line: u32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let open = workspace.open_abs_path(path, workspace::OpenOptions::default(), window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let item = open.await?;
        if let Some(editor) = item.downcast::<editor::Editor>() {
            editor.update_in(cx, |editor, window, cx| {
                let point = text::Point::new(line, 0);
                editor.change_selections(
                    editor::SelectionEffects::scroll(editor::scroll::Autoscroll::center()),
                    window,
                    cx,
                    |selections| selections.select_ranges([point..point]),
                );
            })?;
        }
        anyhow::Ok(())
    })
    .detach_and_log_err(cx);
}

impl EventEmitter<PanelEvent> for CoverageView {}

impl Focusable for CoverageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for CoverageView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.contains_focused(window, cx);
        let columns = self.columns(cx);
        let root = self.root(cx);
        let body = match &root {
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_muted)
                .child("No coverage results.")
                .into_any_element(),
            Some(root) => {
                let rows: Vec<gpui::AnyElement> = self
                    .rows(root)
                    .iter()
                    .map(|row| self.row(row, &columns, focused, cx).into_any_element())
                    .collect();
                v_flex()
                    .size_full()
                    .child(self.header_row(&columns, cx))
                    .child(
                        v_flex()
                            .id("coverage-tree")
                            .role(Role::Tree)
                            .aria_label("Coverage")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(rows),
                    )
                    .into_any_element()
            }
        };
        let header = self.header(cx).into_any_element();
        let toolbar = self.toolbar(cx).into_any_element();
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("CoverageView")
                .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                    let Some(root) = this.root(cx) else { return };
                    let selected = this.selected.clone();
                    let node = this
                        .rows(&root)
                        .into_iter()
                        .find(|row| Some(&row.node.key) == selected.as_ref())
                        .map(|row| row.node.clone());
                    match node {
                        Some(node) if node.kind == Kind::Class => {
                            this.open(&node, true, window, cx)
                        }
                        Some(node) => this.toggle(&node.key, cx),
                        None => {}
                    }
                }))
                .child(header)
                .child(toolbar)
                .child(div().flex_1().min_h_0().child(body)),
        )
    }
}

impl Panel for CoverageView {
    fn persistent_name() -> &'static str {
        "Coverage"
    }

    fn panel_key() -> &'static str {
        "CoverageToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Right
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        position != DockPosition::Left
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(560.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::ListTree)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Coverage")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    // the coverage wave's tool windows take 300–309
    fn activation_priority(&self) -> u32 {
        300
    }
}

/// Follows the workspace's coverage: the tool window added the first time coverage shows, and
/// activated each time a new coverage shows when "Activate Coverage View" is on.
pub(crate) fn add(
    _: &mut Workspace,
    coverage: Entity<Coverage>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let shown = Rc::new(Cell::new(0usize));
    cx.observe_in(&coverage, window, move |workspace, coverage, window, cx| {
        let (generation, data) = {
            let coverage = coverage.read(cx);
            (coverage.generation, coverage.data.is_some())
        };
        if generation == shown.get() || !data {
            return;
        }
        shown.set(generation);
        if workspace.panel::<CoverageView>(cx).is_none() {
            let handle = cx.entity();
            let view = cx.new(|cx| CoverageView::new(coverage.clone(), handle, window, cx));
            workspace.add_panel(view, window, cx);
        }
        if spork_settings::coverage::settings(cx).activate_view {
            workspace.open_panel::<CoverageView>(window, cx);
        }
    })
    .detach();
}

/// The Coverage tool window as the end-to-end tests read it: its tab, columns and rows, each
/// `"  com.example | 100% (1/1) | …"`, indented by depth.
pub(crate) fn e2e_state(workspace: &Workspace, cx: &App) -> Value {
    let Some(view) = workspace.panel::<CoverageView>(cx) else {
        return Value::Null;
    };
    let view = view.read(cx);
    let columns = view.columns(cx);
    let rows: Vec<String> = view
        .root(cx)
        .map(|root| {
            view.rows(&root)
                .iter()
                .map(|row| {
                    let mut cells = vec![format!("{}{}", "  ".repeat(row.depth), row.node.name)];
                    cells.extend(
                        columns
                            .iter()
                            .filter_map(|column| column.ratio(&row.node.counts))
                            .map(|ratio| ratio.text()),
                    );
                    cells.join(" | ")
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "title": view.title(cx),
        "columns": columns.iter().map(|column| column.title()).collect::<Vec<_>>(),
        "rows": rows,
        "selected": view.selected,
        "flatten": view.flatten,
    })
}

#[cfg(test)]
mod tests {
    use super::tree;
    use crate::data::{Class, Data, Line};

    fn class(name: &str, hits: u32) -> Class {
        Class {
            name: name.into(),
            file: None,
            lines: vec![Line {
                line: 1,
                hits,
                method: "m()V".into(),
                ..Line::default()
            }],
        }
    }

    #[test]
    fn groups_classes_by_package_as_intellij_does() {
        let data = Data {
            classes: vec![
                class("com.example.A", 1),
                class("com.example.A$Inner", 0),
                class("com.example.util.B", 0),
            ],
            ..Data::default()
        };
        let root = tree(&data, false, false);
        assert_eq!(root.children.len(), 1);
        let package = &root.children[0];
        assert_eq!(package.name, "com.example");
        assert_eq!(package.counts.classes.text(), "50% (1/2)");
        assert_eq!(package.counts.lines.text(), "33% (1/3)");
        let flat = tree(&data, true, false);
        let names: Vec<&str> = flat
            .children
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        assert_eq!(names, ["com.example", "com.example.util"]);
    }
}
