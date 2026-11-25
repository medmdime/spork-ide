//! The Git Branches popup under the toolbar's branch widget, as in IntelliJ / the reference app
//! (docs/reference-spec.md §9; IntelliJ's GitBranchesPopupBase and its tree model): a search field
//! with Fetch and the gear, the everyday actions, then Recent, Local, Remote and Tags as a tree
//! grouped by directory. A branch row opens its actions on its right; its star makes it a
//! favorite.

use std::cell::Cell;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::input::{
    Cut, Escape, InputEvent, InputState, MoveDown, MoveLeft, MoveRight, MoveUp, Paste,
};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{Ref, RefKind};
use spork_workbench::{Tokens, popup};

use crate::branches::Ongoing;
use crate::{RemoteOp, RepoView, branches};

/// IntelliJ's sizes: 375 px wide, at least 312 tall; a 12 px drag strip and the search field
/// above the tree, which starts 5 px under them; 24 px rows, 20 px per level; the tree is as tall
/// as its top level plus one row, at most 20 rows.
const WIDTH: f32 = 375.;
const MIN_HEIGHT: f32 = 312.;
const HEADER: f32 = 12. + 28. + 6.;
const TREE_TOP: f32 = 5.;
const BOTTOM: f32 = 8.;
const ROW: f32 = 24.;
const INDENT: f32 = 20.;
const MAX_ROWS: usize = 20;
/// The search field's text while empty (`git.branches.popup.search.field.empty.text`).
const PLACEHOLDER: &str = "Search for branches and actions";
/// The smallest the popup can be dragged to.
const MIN_RESIZED: (f32, f32) = (200., 150.);

/// The popup's corner being dragged: it resizes the popup, as IntelliJ's popups resize.
#[derive(Clone)]
struct PopupResize;

impl Render for PopupResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Recent,
    Local,
    Remote,
    Tags,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Recent => "Recent",
            Section::Local => "Local",
            Section::Remote => "Remote",
            Section::Tags => "Tags",
        }
    }
}

/// The actions above the tree: the reference app's `Git.Experimental.Branch.Popup.Actions` (Commit…),
/// then `Git.Branches.List`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Top {
    Commit,
    Ongoing(Ongoing),
    NewBranch,
    NewWorktree,
    CheckoutRevision,
}

enum Node {
    Action(Top),
    Separator,
    Section(Section, bool),
    Folder {
        key: String,
        name: String,
        depth: usize,
        open: bool,
    },
    Ref {
        r: Ref,
        label: String,
        depth: usize,
        favorite: bool,
        tracked: Option<String>,
    },
}

impl Node {
    fn depth(&self) -> usize {
        match self {
            Node::Folder { depth, .. } | Node::Ref { depth, .. } => *depth,
            _ => 1,
        }
    }
}

/// A level of a section's tree before it is flattened: a branch, or a folder of them.
enum Entry<'a> {
    Leaf(&'a Ref, String),
    Folder(String, Vec<(&'a Ref, String)>),
}

/// What every level of a section's tree is built with.
struct Grouping<'a> {
    favorites: &'a [String],
    by_directory: bool,
    /// IntelliJ's order: current and favorite branches, folders, the rest; Recent keeps the
    /// order of the checkouts.
    sort: bool,
    folders_open: bool,
    searching: bool,
}

struct Submenu {
    menu: Entity<PopupMenu>,
    /// Where the search field's menu was asked for; a branch's actions open beside its row.
    at: Option<Point<Pixels>>,
    _subscription: Subscription,
}

pub(crate) struct BranchesPopup {
    view: WeakEntity<RepoView>,
    search: Entity<InputState>,
    /// Sections and folders opened or closed by the user, by key: they read the other way
    /// round from their default.
    toggled: HashSet<String>,
    rows: Vec<Node>,
    selected: Option<usize>,
    scroll: UniformListScrollHandle,
    height: f32,
    submenu: Option<Submenu>,
    /// Where the branch widget, the open submenu and the selected row are: a click on the first
    /// two doesn't close the popup; the submenu opens beside the last.
    trigger: Rc<Cell<Bounds<Pixels>>>,
    submenu_bounds: Rc<Cell<Bounds<Pixels>>>,
    selected_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The next render picks the selection again (opened, or the search changed), and sizes
    /// the popup when it was just opened. Not done on the spot: that may run while the
    /// repository view is being updated, and the rows read it.
    reselect: bool,
    resize: bool,
    /// The size the user dragged the popup to, kept between openings until Restore Popup Size.
    resized: Option<Size<Pixels>>,
    /// Compare with Branch or Tag…'s popup: the file compared with the branch picked.
    compare: Option<String>,
    /// The search field's text while empty, which in Compare with Branch or Tag…'s follows
    /// Show Tags.
    placeholder: &'static str,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for BranchesPopup {}

impl BranchesPopup {
    pub(crate) fn new(
        view: WeakEntity<RepoView>,
        trigger: Rc<Cell<Bounds<Pixels>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // the field's right-click menu is the reference app's own (`Git.Branches.Popup.SpeedSearch`)
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(PLACEHOLDER)
                .context_menu(false)
        });
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.reselect = true;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate(window, cx),
                _ => {}
            });
        Self {
            view,
            search,
            toggled: HashSet::new(),
            rows: Vec::new(),
            selected: None,
            scroll: UniformListScrollHandle::new(),
            height: MIN_HEIGHT,
            submenu: None,
            trigger,
            submenu_bounds: Rc::default(),
            selected_bounds: Rc::default(),
            reselect: true,
            resize: true,
            resized: None,
            compare: None,
            placeholder: PLACEHOLDER,
            _subscription: subscription,
        }
    }

    /// Compare with Branch or Tag…'s popup: the tree without the actions, whose branch or tag
    /// `path` is compared with.
    pub(crate) fn comparing(
        view: WeakEntity<RepoView>,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut popup = Self::new(view, Rc::default(), window, cx);
        popup.compare = Some(path);
        popup
            .search
            .update(cx, |search, cx| search.focus(window, cx));
        popup
    }

    /// Opening again: an empty search, the current branch selected, the size for this tree.
    pub(crate) fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.submenu = None;
        self.search.update(cx, |search, cx| {
            search.set_value("", window, cx);
            search.focus(window, cx);
        });
        self.reselect = true;
        self.resize = true;
    }

    /// Whether its search field has the focus, which the popup keeps while it shows.
    pub(crate) fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.search.read(cx).focus_handle(cx).is_focused(window)
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.submenu = None;
        cx.emit(DismissEvent);
    }

    /// The tree's top level and the first section's children, as IntelliJ sizes the popup.
    fn top_level_rows(&self) -> usize {
        let top = self.rows.iter().filter(|n| n.depth() == 1).count();
        let first = self
            .rows
            .iter()
            .position(|n| matches!(n, Node::Section(..)))
            .map_or(0, |start| {
                self.rows[start + 1..]
                    .iter()
                    .take_while(|n| n.depth() > 1)
                    .filter(|n| n.depth() == 2)
                    .count()
            });
        top + first
    }

    /// The rows as they show: the actions, then each section's tree with its folders open or
    /// closed; while searching only what matches, all open.
    fn rebuild(&mut self, cx: &App) {
        self.rows.clear();
        let Some(view) = self.view.upgrade() else {
            return;
        };
        let this = view.read(cx);
        let Some(Ok(repo)) = &this.repo else { return };
        let prefs = &this.prefs;
        let query = self.search.read(cx).value().trim().to_lowercase();
        let searching = !query.is_empty();
        // shortcut: a plain substring match, where IntelliJ also matches camel humps and
        // initials; switch once the workspace has a fuzzy matcher
        let matches = |text: &str| !searching || text.to_lowercase().contains(&query);

        let mut actions = vec![Top::Commit];
        actions.extend(Ongoing::for_state(repo.op).into_iter().map(Top::Ongoing));
        actions.extend([Top::NewBranch, Top::NewWorktree, Top::CheckoutRevision]);
        if self.compare.is_some() {
            actions.clear();
        }
        let shown: Vec<Top> = actions
            .into_iter()
            .filter(|a| !searching || (prefs.branches_search_actions && matches(action_label(*a))))
            .collect();
        for (ix, action) in shown.iter().enumerate() {
            // Commit… stands apart from the rest
            if ix == 1 && shown[0] == Top::Commit {
                self.rows.push(Node::Separator);
            }
            self.rows.push(Node::Action(*action));
        }
        if !shown.is_empty() {
            self.rows.push(Node::Separator);
        }

        let recent: Vec<&Ref> = match prefs.branches_recent && self.compare.is_none() {
            true => repo
                .recent_branches
                .iter()
                .filter_map(|name| {
                    repo.refs
                        .iter()
                        .find(|r| r.kind == RefKind::Local && &r.name == name)
                })
                .collect(),
            false => Vec::new(),
        };
        let recent_names: HashSet<&str> = recent.iter().map(|r| r.name.as_str()).collect();
        let by_directory = prefs.branches_by_directory;
        let sorted = |kind: RefKind| {
            let mut refs: Vec<&Ref> = repo
                .refs
                .iter()
                .filter(|r| r.kind == kind)
                .filter(|r| !(kind == RefKind::Local && recent_names.contains(r.name.as_str())))
                .collect();
            // IntelliJ's ref order: current, favorites, those in folders, then by name
            refs.sort_by_cached_key(|r| {
                (
                    !r.is_head,
                    !repo.favorites.contains(&r.name),
                    !(by_directory && r.name.contains('/')),
                    r.name.clone(),
                )
            });
            refs
        };
        let mut sections = vec![
            (Section::Recent, recent.clone()),
            (Section::Local, sorted(RefKind::Local)),
            (Section::Remote, sorted(RefKind::Remote)),
        ];
        if prefs.branches_tags {
            sections.push((Section::Tags, sorted(RefKind::Tag)));
        }
        for (section, refs) in sections {
            let refs: Vec<(&Ref, String)> = refs
                .into_iter()
                .filter(|r| matches(&r.name))
                .map(|r| (r, r.name.clone()))
                .collect();
            if refs.is_empty() {
                continue;
            }
            let default_open = match section {
                Section::Recent => true,
                Section::Local => recent.is_empty(),
                Section::Remote | Section::Tags => false,
            };
            let key = section.title().to_owned();
            let open = searching || default_open != self.toggled.contains(&key);
            self.rows.push(Node::Section(section, open));
            if open {
                let grouping = Grouping {
                    favorites: &repo.favorites,
                    by_directory,
                    sort: section != Section::Recent,
                    folders_open: section == Section::Recent,
                    searching,
                };
                self.push_tree(&key, refs, 2, &grouping);
            }
        }
    }

    fn push_tree(&mut self, key: &str, refs: Vec<(&Ref, String)>, depth: usize, g: &Grouping) {
        let mut entries: Vec<Entry> = Vec::new();
        for (r, rest) in refs {
            let Some((folder, inner)) = rest.split_once('/').filter(|_| g.by_directory) else {
                entries.push(Entry::Leaf(r, rest));
                continue;
            };
            let existing = entries.iter_mut().find_map(|e| match e {
                Entry::Folder(name, items) if name == folder => Some(items),
                _ => None,
            });
            match existing {
                Some(items) => items.push((r, inner.to_owned())),
                None => entries.push(Entry::Folder(
                    folder.to_owned(),
                    vec![(r, inner.to_owned())],
                )),
            }
        }
        if g.sort {
            // stable, so each group keeps its order
            entries.sort_by_key(|e| match e {
                Entry::Leaf(r, _) if r.is_head || g.favorites.contains(&r.name) => 0,
                Entry::Folder(..) => 1,
                Entry::Leaf(..) => 2,
            });
        }
        for entry in entries {
            match entry {
                Entry::Leaf(r, label) => self.rows.push(Node::Ref {
                    favorite: g.favorites.contains(&r.name),
                    tracked: (r.kind == RefKind::Local)
                        .then(|| r.upstream.clone())
                        .flatten(),
                    r: r.clone(),
                    label,
                    depth,
                }),
                Entry::Folder(name, items) => {
                    let key = format!("{key}/{name}");
                    let open = g.searching || g.folders_open != self.toggled.contains(&key);
                    self.rows.push(Node::Folder {
                        key: key.clone(),
                        name,
                        depth,
                        open,
                    });
                    if open {
                        self.push_tree(&key, items, depth + 1, g);
                    }
                }
            }
        }
    }

    /// The row selected when the popup opens or the search changes: the current branch, else
    /// the first branch, else the first row that can be selected.
    fn preferred(&self) -> Option<usize> {
        let is_ref = |n: &Node| matches!(n, Node::Ref { .. });
        let current = self
            .rows
            .iter()
            .position(|n| matches!(n, Node::Ref { r, .. } if r.is_head));
        let searching = !self
            .rows
            .iter()
            .any(|n| matches!(n, Node::Section(_, false)));
        current
            .filter(|_| !searching)
            .or_else(|| self.rows.iter().position(is_ref))
            .or_else(|| self.rows.iter().position(|n| !matches!(n, Node::Separator)))
    }

    fn scroll_to_selected(&self) {
        if let Some(ix) = self.selected {
            self.scroll.scroll_to_item(ix, ScrollStrategy::Nearest);
        }
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self.rows.len() as isize;
        let mut ix = self.selected.map_or(-1, |ix| ix as isize);
        for _ in 0..count {
            ix = (ix + step).rem_euclid(count);
            if !matches!(self.rows[ix as usize], Node::Separator) {
                break;
            }
        }
        if count > 0 {
            self.submenu = None;
            self.selected = Some(ix as usize);
            self.scroll_to_selected();
            cx.notify();
        }
    }

    fn toggle(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.toggled.remove(&key) {
            self.toggled.insert(key);
        }
        self.rebuild(cx);
        cx.notify();
    }

    fn key_of(&self, ix: usize) -> Option<String> {
        match self.rows.get(ix)? {
            Node::Section(section, _) => Some(section.title().to_owned()),
            Node::Folder { key, .. } => Some(key.clone()),
            _ => None,
        }
    }

    /// → on a closed folder opens it; on a branch, its actions.
    fn expand(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected else { return };
        match self.rows.get(ix) {
            Some(Node::Section(_, false) | Node::Folder { open: false, .. }) => {
                if let Some(key) = self.key_of(ix) {
                    self.toggle(key, cx);
                }
            }
            Some(Node::Ref { .. }) => self.open_submenu(ix, window, cx),
            _ => {}
        }
    }

    /// ← closes the submenu, an open folder, or goes up to the folder around.
    fn collapse(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.selected else { return };
        if self.submenu.take().is_some() {
            return cx.notify();
        }
        match self.rows.get(ix) {
            Some(Node::Section(_, true) | Node::Folder { open: true, .. }) => {
                if let Some(key) = self.key_of(ix) {
                    self.toggle(key, cx);
                }
            }
            Some(node) => {
                let depth = node.depth();
                if let Some(parent) = self.rows[..ix].iter().rposition(|n| n.depth() < depth) {
                    self.selected = Some(parent);
                    self.scroll_to_selected();
                    cx.notify();
                }
            }
            None => {}
        }
    }

    /// Enter or a click: runs an action, opens or closes a folder, shows a branch's actions.
    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.selected else { return };
        match self.rows.get(ix) {
            Some(Node::Action(action)) => self.run(*action, window, cx),
            Some(Node::Section(..) | Node::Folder { .. }) => {
                if let Some(key) = self.key_of(ix) {
                    self.toggle(key, cx);
                }
            }
            Some(Node::Ref { r, .. }) => match self.compare.clone() {
                Some(path) => {
                    let rev = r.name.clone();
                    self.dismiss(cx);
                    if let Some(view) = self.view.upgrade() {
                        view.update(cx, |this, cx| {
                            this.show_diff_with_ref(path, rev, window, cx)
                        });
                    }
                }
                None => self.open_submenu(ix, window, cx),
            },
            Some(Node::Separator) | None => {}
        }
    }

    fn run(&mut self, action: Top, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss(cx);
        let Some(view) = self.view.upgrade() else {
            return;
        };
        view.update(cx, |this, cx| match action {
            Top::Commit => this.show_commit_window(window, cx),
            Top::Ongoing(action) => this.ongoing_action(action, window, cx),
            Top::NewBranch => this.new_branch_dialog("HEAD".into(), window, cx),
            Top::NewWorktree => this.new_branch_worktree_dialog(window, cx),
            Top::CheckoutRevision => this.checkout_revision_dialog(window, cx),
        });
    }

    fn open_submenu(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Node::Ref { r, .. }), Some(view)) = (self.rows.get(ix), self.view.upgrade())
        else {
            return;
        };
        self.selected = Some(ix);
        let build = branches::ref_menu(r, view);
        let menu = PopupMenu::build(window, cx, |menu, window, cx| build(menu, window, cx));
        // a chosen item (the pointer is on the submenu) closes the popup too; a click elsewhere
        // in the popup or Escape, the submenu alone
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                let chosen = this.submenu_bounds.get().contains(&window.mouse_position());
                this.submenu = None;
                match chosen {
                    true => this.dismiss(cx),
                    false => {
                        this.search
                            .update(cx, |search, cx| search.focus(window, cx));
                        cx.notify();
                    }
                }
            });
        menu.focus_handle(cx).focus(window, cx);
        self.submenu = Some(Submenu {
            menu,
            at: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// The search field's right-click menu, the reference app's `Git.Branches.Popup.SpeedSearch`: Cut
    /// (the whole text when none is selected, as IntelliJ cuts the line) and Paste.
    fn open_search_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.search.read(cx).focus_handle(cx);
        let search = self.search.downgrade();
        let cut = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            let Some(search) = search.upgrade() else {
                return;
            };
            let focus = search.update(cx, |search, cx| {
                if search.selected_range().is_empty() {
                    search.select_all(window, cx);
                }
                search.focus(window, cx);
                search.focus_handle(cx)
            });
            focus.dispatch_action(&Cut, window, cx);
        };
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            menu.action_context(focus)
                .item(
                    PopupMenuItem::new("Cut")
                        .action(Box::new(Cut))
                        .on_click(cut),
                )
                .item(PopupMenuItem::new("Paste").action(Box::new(Paste)))
        });
        let subscription =
            cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                this.submenu = None;
                this.search
                    .update(cx, |search, cx| search.focus(window, cx));
                cx.notify();
            });
        menu.focus_handle(cx).focus(window, cx);
        self.submenu = Some(Submenu {
            menu,
            at: Some(at),
            _subscription: subscription,
        });
        cx.notify();
    }

    fn toggle_favorite(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.view.upgrade() {
            view.update(cx, |this, cx| {
                this.ref_action(branches::Action::Star, name, window, cx)
            });
        }
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let comparing = self.compare.is_some();
        range
            .filter_map(|ix| {
                let node = self.rows.get(ix)?;
                if let Node::Separator = node {
                    // a 24 px row with the line in its middle, as IntelliJ's tree separators
                    return Some(
                        div()
                            .w_full()
                            .h(px(ROW))
                            .flex()
                            .items_center()
                            .child(popup::separator(cx).w_full())
                            .into_any_element(),
                    );
                }
                let selected = self.selected == Some(ix);
                let depth = node.depth();
                let pad = 12. + (depth - 1) as f32 * INDENT;
                let icon_slot = |name: Option<&str>, color: Option<Hsla>| {
                    div().size(px(16.)).flex_none().children(name.map(|name| {
                        spork_workbench::icon(name)
                            .size(px(16.))
                            .text_color(color.unwrap_or(tokens.icon))
                    }))
                };
                let chevron = |open: bool| match open {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                };
                let text = |label: SharedString| div().flex_1().min_w_0().truncate().child(label);
                // actions are the menu's items, the rest the tree's; a branch by its full name
                let (role, name, open) = match node {
                    Node::Action(action) => (Role::MenuItem, action_label(*action).into(), None),
                    Node::Section(section, open) => {
                        (Role::TreeItem, section.title().into(), Some(*open))
                    }
                    Node::Folder { name, open, .. } => (
                        Role::TreeItem,
                        SharedString::from(name.clone()),
                        Some(*open),
                    ),
                    Node::Ref { r, .. } => (Role::TreeItem, r.name.clone().into(), None),
                    Node::Separator => (Role::Splitter, SharedString::default(), None),
                };
                let mut row = popup::row(("branch-row", ix), selected, cx)
                    .role(role)
                    .aria_label(name)
                    .aria_selected(selected)
                    .when_some(open, |row, open| row.aria_expanded(open))
                    .gap_0()
                    .child(div().w(px(pad - 4.)).flex_none());
                row = match node {
                    Node::Separator => row,
                    Node::Action(action) => row
                        .child(
                            div().size(px(16.)).flex_none().children(
                                action_icon(*action)
                                    .map(|name| spork_workbench::image(name, cx).size(px(16.))),
                            ),
                        )
                        .child(div().w(px(7.)).flex_none())
                        .child(text(action_label(*action).into()))
                        .children(action_key(*action).map(|key| popup::hint(key, selected, cx))),
                    Node::Section(section, open) => row
                        .child(icon_slot(Some(chevron(*open)), None))
                        .child(div().w(px(7.)).flex_none())
                        .child(text(section.title().into())),
                    Node::Folder { name, open, .. } => row
                        .child(icon_slot(Some(chevron(*open)), None))
                        .child(div().w(px(2.)).flex_none())
                        .child(spork_workbench::image("nodes/folder", cx).size(px(16.)))
                        .child(div().w(px(7.)).flex_none())
                        .child(text(name.clone().into())),
                    Node::Ref {
                        r,
                        label,
                        favorite,
                        tracked,
                        ..
                    } => {
                        let icon = match (selected && !favorite, r.is_head, favorite) {
                            (true, ..) => "nodes/starEmpty",
                            (false, true, true) => "dvcs/currentBranchFavoriteLabel",
                            (false, true, false) => "dvcs/currentBranchLabel",
                            (false, false, true) => "nodes/star",
                            _ if r.kind == RefKind::Tag => "dvcs/branchLabel",
                            _ => "general/vcs",
                        };
                        let name = r.name.clone();
                        let mono = !icon.starts_with("dvcs/") && icon != "nodes/star";
                        let icon = match mono {
                            true => spork_workbench::icon(icon)
                                .size(px(16.))
                                .text_color(tokens.icon)
                                .into_any_element(),
                            false => spork_workbench::image(icon, cx)
                                .size(px(16.))
                                .into_any_element(),
                        };
                        let (incoming, outgoing) = (r.behind, r.ahead);
                        row.child(div().w(px(18.)).flex_none())
                            .child(
                                div()
                                    .id(("branch-star", ix))
                                    .flex_none()
                                    .child(icon)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.toggle_favorite(name.clone(), window, cx)
                                    })),
                            )
                            .child(div().w(px(7.)).flex_none())
                            .child(div().min_w_0().truncate().child(label.clone()))
                            .when(incoming > 0, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .pl(px(10.))
                                        .text_color(tokens.text_link)
                                        .child(format!("↙ {}", incoming.min(99))),
                                )
                            })
                            .when(outgoing > 0, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .pl(px(4.))
                                        .text_color(tokens.text_success)
                                        .child(format!("↗ {}", outgoing.min(99))),
                                )
                            })
                            .child(div().flex_1())
                            .children(
                                tracked
                                    .clone()
                                    .map(|tracked| popup::hint(tracked, selected, cx)),
                            )
                            .when(!comparing, |row| {
                                row.child(
                                    div().pl(px(4.)).flex_none().child(
                                        spork_workbench::icon("general/chevronRight")
                                            .size(px(16.))
                                            .text_color(match selected {
                                                true => tokens.text,
                                                false => tokens.icon,
                                            }),
                                    ),
                                )
                            })
                    }
                };
                let bounds = self.selected_bounds.clone();
                Some(
                    div()
                        .w_full()
                        .child(
                            row.when(selected, |row| {
                                row.relative().child(
                                    canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                                        .absolute()
                                        .top_0()
                                        .left_0()
                                        .size_full(),
                                )
                            })
                            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                // the selection follows the pointer, but stays on a row whose actions show
                                if this.selected != Some(ix) && this.submenu.is_none() {
                                    this.selected = Some(ix);
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.selected = Some(ix);
                                    this.activate(window, cx);
                                },
                            )),
                        )
                        .into_any_element(),
                )
            })
            .collect()
    }

    /// The gear (`Git.Branches.Popup.Settings`): IntelliJ's view options, kept in the
    /// preferences, and Restore Popup Size, greyed until the popup is resized.
    fn gear_menu(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
        let (view, search, popup) = (
            self.view.clone(),
            self.search.downgrade(),
            cx.entity().downgrade(),
        );
        let comparing = self.compare.is_some();
        move |menu, _, cx| {
            let Some(strong) = view.upgrade() else {
                return menu;
            };
            let prefs = strong.read(cx).prefs.clone();
            let resized = popup
                .upgrade()
                .is_some_and(|popup| popup.read(cx).resized.is_some());
            // the popup keeps its search field focused, as IntelliJ's; after the menu has
            // closed and given the focus back
            let refocus = {
                let search = search.clone();
                move |window: &mut Window, cx: &mut App| {
                    let search = search.clone();
                    window.defer(cx, move |window, cx| {
                        if let Some(search) = search.upgrade() {
                            search.update(cx, |search, cx| search.focus(window, cx));
                        }
                    });
                }
            };
            let item = |label: &str, on: bool, flip: fn(&mut crate::prefs::Prefs)| {
                let (view, refocus) = (view.clone(), refocus.clone());
                PopupMenuItem::new(label.to_owned())
                    .checked(on)
                    .on_click(move |_, window, cx| {
                        if let Some(view) = view.upgrade() {
                            view.update(cx, |this, cx| {
                                flip(&mut this.prefs);
                                this.prefs.save();
                                cx.notify();
                            })
                        }
                        refocus(window, cx);
                    })
            };
            let restore = {
                let (popup, refocus) = (popup.clone(), refocus.clone());
                PopupMenuItem::new("Restore Popup Size")
                    .disabled(!resized)
                    .on_click(move |_, window, cx| {
                        if let Some(popup) = popup.upgrade() {
                            popup.update(cx, |this, cx| {
                                this.resized = None;
                                this.resize = true;
                                cx.notify();
                            })
                        }
                        refocus(window, cx);
                    })
            };
            let group_by_directory = item("Group by Directory", prefs.branches_by_directory, |p| {
                p.branches_by_directory ^= true
            });
            let show_tags = item("Show Tags", prefs.branches_tags, |p| {
                p.branches_tags ^= true
            });
            // Compare with Branch or Tag…'s, `Git.Compare.With.Branch.Popup.Settings`, as
            // the reference app shows it: no recent branches, Restore Popup Size once resized
            if comparing {
                return menu
                    .item(group_by_directory)
                    .item(show_tags)
                    .when(resized, |menu| menu.item(restore));
            }
            menu.item(item(
                "Show Actions in Search Results",
                prefs.branches_search_actions,
                |p| p.branches_search_actions ^= true,
            ))
            .separator()
            .item(group_by_directory)
            .item(item("Show Recent Branches", prefs.branches_recent, |p| {
                p.branches_recent ^= true
            }))
            .item(show_tags)
            .item(restore)
        }
    }
}

impl Render for BranchesPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the preferences may have changed from the gear, the repository from a fetch
        self.rebuild(cx);
        if std::mem::take(&mut self.resize) {
            let rows = (self.top_level_rows() + 1).min(MAX_ROWS) as f32;
            self.height = (HEADER + TREE_TOP + rows * ROW + BOTTOM).max(MIN_HEIGHT);
        }
        if std::mem::take(&mut self.reselect)
            || self.selected.is_none_or(|ix| ix >= self.rows.len())
        {
            self.selected = self.preferred();
            self.scroll_to_selected();
        }
        let view = self.view.clone();
        let comparing = self.compare.is_some();
        if comparing {
            let tags = self
                .view
                .upgrade()
                .is_some_and(|view| view.read(cx).prefs.branches_tags);
            let wanted = match tags {
                true => "Search for branches and tags",
                false => "Search for branches",
            };
            if self.placeholder != wanted {
                self.placeholder = wanted;
                self.search
                    .update(cx, |search, cx| search.set_placeholder(wanted, window, cx));
            }
        }
        let header = v_flex().flex_none().child(div().h(px(12.))).child(
            h_flex()
                .pl(px(10.))
                .pr(px(8.))
                .pb(px(6.))
                .child(
                    div()
                        .id("branches-search")
                        .flex_1()
                        // in the capture phase: the field takes the right click for itself
                        .capture_any_mouse_down(cx.listener(
                            |this, event: &MouseDownEvent, window, cx| {
                                if event.button == MouseButton::Right {
                                    cx.stop_propagation();
                                    this.open_search_menu(event.position, window, cx);
                                }
                            },
                        ))
                        .child(
                            spork_workbench::TextField::new(&self.search)
                                .in_popup()
                                .prefix(spork_workbench::icon("general/search").size(px(16.))),
                        ),
                )
                .child(
                    h_flex()
                        .pl(px(6.))
                        .gap(px(2.))
                        .when(!comparing, |buttons| {
                            buttons.child(
                                spork_workbench::ActionButton::new("branches-fetch", "vcs/fetch")
                                    .tooltip("Fetch")
                                    .on_click(move |_, window, cx| {
                                        if let Some(view) = view.upgrade() {
                                            view.update(cx, |this, cx| {
                                                this.remote_op(RemoteOp::Fetch, window, cx)
                                            })
                                        }
                                    }),
                            )
                        })
                        .child(
                            spork_workbench::ActionButton::new("branches-gear", "general/settings")
                                .tooltip("Settings")
                                .dropdown_menu_with_anchor(Anchor::TopRight, self.gear_menu(cx)),
                        ),
                ),
        );
        let shown = self
            .resized
            .unwrap_or_else(|| size(px(WIDTH), px(self.height)));
        let count = self.rows.len();
        let list = uniform_list(
            "branches-tree",
            count,
            cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .size_full();
        // a menu for assistive technology: its actions are the items, the branches tree rows
        let list = div()
            .id("branches-list")
            .role(Role::Menu)
            .aria_label("Branches Tree")
            .flex_1()
            .min_h_0()
            .child(list);
        let submenu = self.submenu.as_ref().map(|submenu| {
            let row = self.selected_bounds.get();
            let bounds = self.submenu_bounds.clone();
            let at = submenu
                .at
                .unwrap_or_else(|| point(row.right() + px(2.), row.top() - px(6.)));
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div().relative().child(submenu.menu.clone()).child(
                            // pinned to the corner: without insets an absolute child sits
                            // where it would flow, under the menu
                            canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                        ),
                    ),
            )
            .with_priority(2)
        });
        popup::surface(cx)
            .id("branches-popup")
            // a click on the popup stays in it: the log under it doesn't see it
            .occlude()
            .w(shown.width)
            .h(shown.height)
            .key_context("BranchesPopup")
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.select_next(-1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.select_next(1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveRight, window, cx| {
                // the field's own cursor moves while there is text
                if this.search.read(cx).value().is_empty() {
                    cx.stop_propagation();
                    this.expand(window, cx)
                }
            }))
            .capture_action(cx.listener(|this, _: &MoveLeft, _, cx| {
                if this.search.read(cx).value().is_empty() {
                    cx.stop_propagation();
                    this.collapse(cx)
                }
            }))
            .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                cx.stop_propagation();
                this.dismiss(cx)
            }))
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                let at = event.position;
                let on_submenu = this.submenu.is_some() && this.submenu_bounds.get().contains(&at);
                // the widget's own click closes it
                if !on_submenu && !this.trigger.get().contains(&at) {
                    this.dismiss(cx)
                }
            }))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<PopupResize>, _, cx| {
                    let dragged = event.event.position - event.bounds.origin;
                    this.resized = Some(size(
                        dragged.x.max(px(MIN_RESIZED.0)),
                        dragged.y.max(px(MIN_RESIZED.1)),
                    ));
                    cx.notify();
                }),
            )
            .child(header)
            .child(div().h(px(TREE_TOP)).flex_none())
            .child(list)
            .child(div().h(px(BOTTOM)).flex_none())
            .child(
                // the corner IntelliJ's popups are resized by
                div()
                    .id("branches-resize")
                    .role(Role::Splitter)
                    .aria_label("Resize")
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .size(px(BOTTOM))
                    .cursor(CursorStyle::ResizeUpRightDownLeft)
                    .on_drag(PopupResize, |drag, _, _, cx| cx.new(|_| drag.clone())),
            )
            .children(submenu)
    }
}

fn action_label(action: Top) -> &'static str {
    match action {
        Top::Commit => "Commit…",
        Top::Ongoing(action) => action.label(),
        Top::NewBranch => "New Branch…",
        Top::NewWorktree => "New Worktree…",
        Top::CheckoutRevision => "Checkout Tag or Revision…",
    }
}

fn action_icon(action: Top) -> Option<&'static str> {
    match action {
        Top::Commit => Some("vcs/commit"),
        Top::NewBranch => Some("general/add"),
        Top::NewWorktree => Some("vcs/worktree"),
        Top::Ongoing(_) | Top::CheckoutRevision => None,
    }
}

fn action_key(action: Top) -> Option<&'static str> {
    match action {
        Top::Commit => Some("⌘K"),
        Top::NewBranch => Some("⌥⌘N"),
        _ => None,
    }
}

impl RepoView {
    /// Compare with Branch or Tag…: the branches popup in the window's middle, whose branch or
    /// tag `path` is compared with.
    pub(crate) fn compare_with_branch_popup(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity().downgrade();
        let popup = cx.new(|cx| BranchesPopup::comparing(view, path, window, cx));
        let subscription = cx.subscribe_in(
            &popup,
            window,
            |this, popup, _: &DismissEvent, window, cx| {
                if popup.read(cx).has_focus(window, cx) {
                    window.focus(&this.focus, cx);
                }
                this.compare_popup = None;
                cx.notify();
            },
        );
        self.compare_popup = Some((popup, subscription));
        cx.notify();
    }

    pub(crate) fn compare_popup_overlay(
        &self,
        window: &Window,
    ) -> Option<impl IntoElement + use<>> {
        let (popup, _) = self.compare_popup.as_ref()?;
        let viewport = window.viewport_size();
        let at = point(
            ((viewport.width - px(WIDTH)) / 2.).max(px(8.)),
            ((viewport.height - px(MIN_HEIGHT)) / 3.).max(px(8.)),
        );
        Some(
            deferred(
                anchored()
                    .position(at)
                    .snap_to_window_with_margin(px(8.))
                    .child(popup.clone()),
            )
            .with_priority(1),
        )
    }
}
