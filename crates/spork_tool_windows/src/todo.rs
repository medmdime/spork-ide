//! The TODO tool window, IntelliJ's (`TodoView`, reference/idea/screenshots/toolwindows/todo.jpg):
//! the comments matching its default patterns, `\btodo\b.*` and `\bfixme\b.*` (any case), found
//! by the project search, in a Project tab ("Found 2 TODO items in 2 files", then the folders,
//! the files and their items, each "line text") and a Current File tab; the left toolbar's
//! Previous and Next TODO Item, Expand All, Collapse All, Group By (Modules, Packages, Flatten
//! View) and Preview, the preview of the selected item beside the tree.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use editor::Editor;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, WeakEntity,
    Window, div, px,
};
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use language::{Buffer, Point, ToOffset as _, ToPoint as _};
use picker::{MatchLocation, PreviewBackend, PreviewLayout, PreviewUpdate};
use project::search::{SearchQuery, SearchResult};
use project::{Project, ProjectPath};
use spork_workbench::{ActionButton, Tokens};
use ui::{ContextMenu, IconPosition, PopoverMenu, Tooltip};
use util::ResultExt as _;
use util::paths::PathMatcher;
use workspace::dock::{DockPosition, Panel, PanelEvent, PanelHandle};
use workspace::{Workspace, frame};

use crate::problems::accessible_text;

pub mod todo_panel {
    gpui::actions!(
        todo_panel,
        [
            /// Opens the TODO tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus
        ]
    );
}

/// IntelliJ's default TODO patterns (`TodoConfiguration`), as one case-insensitive expression.
const PATTERN: &str = r"\b(todo|fixme)\b.*";
/// How long the files rest unchanged before the search runs again.
const RESEARCH_DELAY: Duration = Duration::from_millis(500);

pub(crate) fn add_panel(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    workspace.register_action(|workspace, _: &todo_panel::ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<TodoPanel>(window, cx);
    });
    let handle = cx.entity().downgrade();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| TodoPanel::new(handle, project, window, cx));
    workspace.add_panel(panel, window, cx);
}

/// A TODO item: its line in its file and that line's text.
#[derive(Clone, PartialEq)]
struct Item {
    path: ProjectPath,
    row: u32,
    text: SharedString,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Project,
    CurrentFile,
}

/// A row of the tree.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Node {
    /// "Found N TODO items in M files".
    Summary,
    /// The project's folder (Group By Modules).
    Module,
    /// A folder of the project, by its path in its root ("src", or "src/util" flattened).
    Folder(String),
    File(ProjectPath),
    /// An item, by its index in `items`.
    Item(usize),
}

pub(crate) struct TodoPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    items: Vec<Item>,
    buffers: BTreeMap<ProjectPath, Entity<Buffer>>,
    current: Option<ProjectPath>,
    tab: Tab,
    show_modules: bool,
    show_packages: bool,
    flatten: bool,
    show_preview: bool,
    collapsed: HashSet<Node>,
    selected: Option<Node>,
    preview: Arc<dyn PreviewBackend>,
    searched: bool,
    /// Whether it has shown: IntelliJ's TODO is off the stripe until opened (More tool windows).
    opened: bool,
    search: Task<()>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl TodoPanel {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = picker_preview::editor_preview(project.clone(), window, cx);
        let mut _subscriptions =
            vec![
                cx.subscribe_in(&project, window, |this, _, event, window, cx| {
                    if let project::Event::WorktreeUpdatedEntries(..) = event
                        && this.searched
                    {
                        this.search_later(window, cx);
                    }
                }),
            ];
        if let Some(workspace) = workspace.upgrade() {
            _subscriptions.push(cx.subscribe_in(
                &workspace,
                window,
                |this, workspace, event, _, cx| {
                    if let workspace::Event::ActiveItemChanged = event {
                        this.current = workspace
                            .read(cx)
                            .active_item(cx)
                            .filter(|item| item.downcast::<Editor>().is_some())
                            .and_then(|item| item.project_path(cx));
                        cx.notify();
                    }
                },
            ));
        }
        Self {
            workspace,
            project,
            items: Vec::new(),
            buffers: BTreeMap::new(),
            current: None,
            tab: Tab::Project,
            show_modules: false,
            show_packages: true,
            flatten: false,
            show_preview: true,
            collapsed: HashSet::new(),
            selected: None,
            preview,
            searched: false,
            opened: false,
            search: Task::ready(()),
            focus: cx.focus_handle(),
            _subscriptions,
        }
    }

    fn search_later(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(RESEARCH_DELAY).await;
            this.update_in(cx, |this, window, cx| this.search(window, cx))
                .ok();
        });
    }

    /// Searches the project for the patterns, keeping the matches in comments (in plain text,
    /// everywhere), as IntelliJ's TODO index does.
    fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.searched = true;
        let Some(query) = SearchQuery::regex(
            PATTERN,
            false,
            false,
            false,
            true,
            PathMatcher::default(),
            PathMatcher::default(),
            false,
            None,
        )
        .log_err() else {
            return;
        };
        let results = self
            .project
            .update(cx, |project, cx| project.search(query, cx));
        self.search = cx.spawn_in(window, async move |this, cx| {
            let _search = results.task_handle;
            let mut items = Vec::new();
            let mut buffers = BTreeMap::new();
            while let Ok(result) = results.rx.recv().await {
                let SearchResult::Buffer { buffer, ranges } = result else {
                    continue;
                };
                let found = cx.update(|_, cx| {
                    let buffer_ref = buffer.read(cx);
                    let path = buffer_ref.file().map(|file| ProjectPath {
                        worktree_id: file.worktree_id(cx),
                        path: file.path().clone(),
                    })?;
                    let snapshot = buffer_ref.snapshot();
                    let found: Vec<Item> = ranges
                        .iter()
                        .filter(|range| {
                            snapshot.language().is_none()
                                || snapshot
                                    .language_scope_at(range.start)
                                    .and_then(|scope| scope.override_name().map(str::to_owned))
                                    .is_some_and(|name| name.starts_with("comment"))
                        })
                        .map(|range| {
                            let row = range.start.to_point(&snapshot).row;
                            let line: String = snapshot
                                .text_for_range(
                                    Point::new(row, 0)..Point::new(row, snapshot.line_len(row)),
                                )
                                .collect();
                            Item {
                                path: path.clone(),
                                row,
                                text: line.trim().to_owned().into(),
                            }
                        })
                        .collect();
                    Some((path, found))
                });
                if let Ok(Some((path, found))) = found
                    && !found.is_empty()
                {
                    buffers.insert(path, buffer);
                    items.extend(found);
                }
            }
            items.sort_by(|a, b| (&a.path.path, a.row).cmp(&(&b.path.path, b.row)));
            items.dedup_by(|a, b| a.path == b.path && a.row == b.row);
            this.update(cx, |this, cx| {
                this.items = items;
                this.buffers = buffers;
                cx.notify();
            })
            .ok();
        });
    }

    /// The items the tab shows, by their index.
    fn shown(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|ix| match self.tab {
                Tab::Project => true,
                Tab::CurrentFile => self.current.as_ref() == Some(&self.items[*ix].path),
            })
            .collect()
    }

    /// The files with shown items, in order.
    fn files(&self) -> Vec<ProjectPath> {
        let mut files: Vec<ProjectPath> = Vec::new();
        for ix in self.shown() {
            let path = &self.items[ix].path;
            if files.last() != Some(path) {
                files.push(path.clone());
            }
        }
        files
    }

    fn folder_of(path: &ProjectPath) -> String {
        path.path
            .parent()
            .map(|parent| parent.as_unix_str().to_owned())
            .unwrap_or_default()
    }

    /// The tree's rows, each with its depth.
    fn rows(&self) -> Vec<(Node, usize)> {
        let mut rows = Vec::new();
        let files = self.files();
        let items = |path: &ProjectPath| {
            self.shown()
                .into_iter()
                .filter(|ix| self.items[*ix].path == *path)
                .collect::<Vec<_>>()
        };
        let push_file = |rows: &mut Vec<(Node, usize)>, path: &ProjectPath, depth: usize| {
            let node = Node::File(path.clone());
            let open = !self.collapsed.contains(&node);
            rows.push((node, depth));
            if open {
                rows.extend(
                    items(path)
                        .into_iter()
                        .map(|ix| (Node::Item(ix), depth + 1)),
                );
            }
        };
        if self.tab == Tab::CurrentFile {
            for path in &files {
                push_file(&mut rows, path, 0);
            }
            return rows;
        }
        rows.push((Node::Summary, 0));
        if self.collapsed.contains(&Node::Summary) {
            return rows;
        }
        let mut depth = 1;
        if self.show_modules {
            rows.push((Node::Module, depth));
            if self.collapsed.contains(&Node::Module) {
                return rows;
            }
            depth += 1;
        }
        if !self.show_packages {
            for path in &files {
                push_file(&mut rows, path, depth);
            }
            return rows;
        }
        // the folders holding items: flattened, each one row; else nested as in the project
        let mut folders: Vec<String> = files.iter().map(Self::folder_of).collect();
        folders.dedup();
        let mut shown_folders: Vec<String> = Vec::new();
        for path in &files {
            let folder = Self::folder_of(path);
            if folder.is_empty() {
                continue;
            }
            let chain: Vec<String> = match self.flatten {
                true => vec![folder.clone()],
                false => {
                    let parts: Vec<&str> = folder.split('/').collect();
                    (1..=parts.len()).map(|n| parts[..n].join("/")).collect()
                }
            };
            for (level, folder) in chain.iter().enumerate() {
                if shown_folders.contains(folder) {
                    continue;
                }
                // under a closed folder nothing shows
                if chain[..level]
                    .iter()
                    .any(|parent| self.collapsed.contains(&Node::Folder(parent.clone())))
                {
                    break;
                }
                shown_folders.push(folder.clone());
                rows.push((Node::Folder(folder.clone()), depth + level));
            }
        }
        // files under their folder, the root's files last
        let mut ordered = Vec::new();
        for folder in &shown_folders {
            let level = rows
                .iter()
                .position(|(node, _)| *node == Node::Folder(folder.clone()))
                .map(|ix| rows[ix].1);
            let Some(level) = level else { continue };
            let closed = self.collapsed.contains(&Node::Folder(folder.clone()))
                || shown_folders.iter().any(|parent| {
                    folder.starts_with(&format!("{parent}/"))
                        && self.collapsed.contains(&Node::Folder(parent.clone()))
                });
            ordered.push((Node::Folder(folder.clone()), level));
            if closed {
                continue;
            }
            for path in files.iter().filter(|path| Self::folder_of(path) == *folder) {
                let mut file_rows = Vec::new();
                push_file(&mut file_rows, path, level + 1);
                ordered.extend(file_rows);
            }
        }
        rows.retain(|(node, _)| !matches!(node, Node::Folder(_)));
        rows.extend(ordered);
        for path in files.iter().filter(|path| Self::folder_of(path).is_empty()) {
            push_file(&mut rows, path, depth);
        }
        rows
    }

    fn select(&mut self, node: Node, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(node);
        self.update_preview(window, cx);
        cx.notify();
    }

    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let item = match &self.selected {
            Some(Node::Item(ix)) => self.items.get(*ix).cloned(),
            _ => None,
        };
        let Some((item, buffer)) =
            item.and_then(|item| Some((item.clone(), self.buffers.get(&item.path)?.clone())))
        else {
            return self.preview.clear(cx);
        };
        let snapshot = buffer.read(cx).snapshot();
        let row = item.row.min(snapshot.max_point().row);
        let range = Point::new(row, 0)..Point::new(row, snapshot.line_len(row));
        let location = MatchLocation {
            anchor_range: snapshot.anchor_before(range.start)..snapshot.anchor_after(range.end),
            range: range.start.to_offset(&snapshot)..range.end.to_offset(&snapshot),
        };
        self.preview
            .update(PreviewUpdate::from_buffer(buffer, location), window, cx)
    }

    /// ⏎ or a double click: the item in its editor, or a node opened or closed.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.selected.clone() else {
            return;
        };
        let Node::Item(ix) = node else {
            if !self.collapsed.remove(&node) {
                self.collapsed.insert(node);
            }
            cx.notify();
            return;
        };
        let Some(item) = self.items.get(ix).cloned() else {
            return;
        };
        self.workspace
            .update(cx, |workspace, cx| {
                spork_navigate::open_at(
                    workspace,
                    item.path,
                    Some(Point::new(item.row, 0)),
                    window,
                    cx,
                )
            })
            .log_err();
    }

    fn step(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|(node, _)| node == selected))
            .map_or(0, |ix| {
                (ix as isize + step).clamp(0, rows.len() as isize - 1) as usize
            });
        if let Some((node, _)) = rows.get(at).cloned() {
            self.select(node, window, cx);
        }
    }

    /// Previous and Next TODO Item: the item before (or after) the selection, opened.
    fn step_item(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.rows();
        let at = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|(node, _)| node == selected));
        let is_item = |(node, _): &&(Node, usize)| matches!(node, Node::Item(_));
        let next = match (at, step > 0) {
            (Some(at), true) => rows.iter().skip(at + 1).find(is_item),
            (Some(at), false) => rows.iter().take(at).rev().find(is_item),
            (None, _) => rows.iter().find(is_item),
        };
        let Some((node, _)) = next.cloned() else {
            return;
        };
        self.select(node, window, cx);
        self.open_selected(window, cx);
    }

    fn set_all_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.collapsed.clear();
        if !open {
            let rows = self.rows();
            self.collapsed.extend(
                rows.into_iter()
                    .map(|(node, _)| node)
                    .filter(|node| !matches!(node, Node::Item(_) | Node::Summary)),
            );
        }
        cx.notify();
    }

    fn summary(&self) -> String {
        let (items, files) = (self.shown().len(), self.files().len());
        format!(
            "Found {items} TODO {} in {files} {}",
            if items == 1 { "item" } else { "items" },
            if files == 1 { "file" } else { "files" }
        )
    }

    fn count_text(count: usize) -> String {
        match count {
            1 => "1 item".to_owned(),
            n => format!("{n} items"),
        }
    }

    fn render_row(
        &self,
        node: Node,
        depth: usize,
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
        let chevron = |open: bool| {
            div().flex_none().text_color(tokens.icon).child(
                spork_workbench::icon(match open {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                })
                .size(px(16.)),
            )
        };
        let open = !self.collapsed.contains(&node);
        let row = h_flex()
            .id(("todo-row", ix))
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
        let count = |path_prefix: Option<&str>, path: Option<&ProjectPath>| {
            self.shown()
                .into_iter()
                .filter(|ix| {
                    let item = &self.items[*ix];
                    path.is_none_or(|path| item.path == *path)
                        && path_prefix.is_none_or(|prefix| {
                            let folder = Self::folder_of(&item.path);
                            folder == prefix || folder.starts_with(&format!("{prefix}/"))
                        })
                })
                .count()
        };
        let location = |text: String| div().flex_none().ml(px(4.)).text_color(grey).child(text);
        let row = match &node {
            Node::Summary => {
                let text: SharedString = self.summary().into();
                row.aria_label(text.clone())
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(div().flex_none().child(text))
            }
            Node::Module => {
                let name = self
                    .project
                    .read(cx)
                    .visible_worktrees(cx)
                    .next()
                    .map(|worktree| worktree.read(cx).root_name_str().to_owned())
                    .unwrap_or_default();
                row.aria_label(SharedString::from(name.clone()))
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(spork_workbench::image("nodes/module", cx))
                    .child(div().flex_none().child(name))
                    .child(location(Self::count_text(count(None, None))))
            }
            Node::Folder(folder) => {
                let name = match self.flatten {
                    true => folder.clone(),
                    false => folder.rsplit('/').next().unwrap_or_default().to_owned(),
                };
                row.aria_label(SharedString::from(name.clone()))
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(spork_workbench::image("nodes/folder", cx))
                    .child(div().flex_none().child(name))
                    .child(location(Self::count_text(count(Some(folder), None))))
            }
            Node::File(path) => {
                let name = path.path.file_name().unwrap_or_default().to_owned();
                row.aria_label(SharedString::from(name.clone()))
                    .aria_expanded(open)
                    .child(chevron(open))
                    .child(spork_workbench::image(
                        spork_workbench::file_icon(&name),
                        cx,
                    ))
                    .child(div().flex_none().child(name))
                    .child(location(Self::count_text(count(None, Some(path)))))
            }
            Node::Item(ix) => {
                let Some(item) = self.items.get(*ix) else {
                    return div().into_any_element();
                };
                let text: SharedString = format!("{} {}", item.row + 1, item.text).into();
                row.aria_label(text)
                    .pl(px(4. + 18. * depth as f32 + 18.))
                    .child(
                        div()
                            .flex_none()
                            .text_color(grey)
                            .child((item.row + 1).to_string()),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(tokens.todo)
                            .italic()
                            .child(item.text.clone()),
                    )
            }
        };
        row.on_click(
            cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                this.select(node.clone(), window, cx);
                if event.click_count() >= 2 {
                    this.open_selected(window, cx);
                }
            }),
        )
        .into_any_element()
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
        let tabs = [
            (Tab::Project, "Project"),
            (Tab::CurrentFile, "Current File"),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (tab, title))| {
            let on = tab == self.tab;
            h_flex()
                .id(("todo-tab", ix))
                .role(Role::Tab)
                .aria_label(title)
                .aria_selected(on)
                .h(px(28.))
                .px(px(8.))
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.tab = tab;
                    this.selected = None;
                    this.update_preview(window, cx);
                    cx.notify();
                }))
        });
        let title = h_flex()
            .gap(px(12.))
            .child(frame::tool_window_title_label("TODO"))
            .child(h_flex().gap(px(2.)).children(tabs))
            .into_any_element();
        frame::tool_window_header(title, Vec::new(), panel, "TODO".into(), cx).into_any_element()
    }

    /// Group By: Modules, Packages and Flatten View (`TodoViewGroupByGroup`), for the Project tab.
    fn group_by(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        let state = (self.show_modules, self.show_packages, self.flatten);
        PopoverMenu::new("todo-group-by")
            .trigger_with_tooltip(
                frame::header_button("todo-group-by-button", "actions/groupBy", "Group By", cx),
                Tooltip::text("Group By"),
            )
            .anchor(gpui::Anchor::TopLeft)
            .menu(move |window, cx| {
                let this = this.clone();
                let (modules, packages, flatten) = state;
                Some(ContextMenu::build(window, cx, move |menu, _, _| {
                    let toggle = |menu: ContextMenu,
                                  label: &'static str,
                                  on: bool,
                                  change: fn(&mut TodoPanel)| {
                        let this = this.clone();
                        menu.toggleable_entry(label, on, IconPosition::Start, None, move |_, cx| {
                            this.update(cx, |this, cx| {
                                change(this);
                                cx.notify();
                            })
                            .ok();
                        })
                    };
                    let menu = menu.header("Group By");
                    let menu = toggle(menu, "Modules", modules, |this| {
                        this.show_modules = !this.show_modules
                    });
                    let menu = toggle(menu, "Packages", packages, |this| {
                        this.show_packages = !this.show_packages
                    });
                    toggle(menu, "Flatten View", flatten, |this| {
                        this.flatten = !this.flatten
                    })
                }))
            })
            .into_any_element()
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
        let button = |id: &'static str, icon: &'static str, tooltip: &'static str| {
            ActionButton::new(id, icon).tooltip(tooltip)
        };
        v_flex()
            .id("todo-toolbar")
            .role(Role::Toolbar)
            .aria_label("TODO Toolbar")
            .flex_none()
            .w(px(32.))
            .pt(px(4.))
            .items_center()
            .gap(px(4.))
            .child(
                button("todo-previous", "general/up", "Previous TODO Item")
                    .on_click(cx.listener(|this, _, window, cx| this.step_item(-1, window, cx))),
            )
            .child(
                button("todo-next", "general/down", "Next TODO Item")
                    .on_click(cx.listener(|this, _, window, cx| this.step_item(1, window, cx))),
            )
            .child(
                button("todo-expand-all", "general/expandAll", "Expand All")
                    .on_click(cx.listener(|this, _, _, cx| this.set_all_open(true, cx))),
            )
            .child(
                button("todo-collapse-all", "general/collapseAll", "Collapse All")
                    .on_click(cx.listener(|this, _, _, cx| this.set_all_open(false, cx))),
            )
            .when(self.tab == Tab::Project, |toolbar| {
                toolbar.child(self.group_by(cx))
            })
            .child(
                button("todo-preview", "actions/preview", "Preview")
                    .selected(self.show_preview)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.show_preview = !this.show_preview;
                        this.update_preview(window, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}

impl Render for TodoPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let focused = self.focus.contains_focused(window, cx);
        let header = self.header(cx);
        let rows: Vec<AnyElement> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(ix, (node, depth))| self.render_row(node, depth, ix, focused, cx))
            .collect();
        let tree = v_flex()
            .id("todo-tree")
            .role(Role::Tree)
            .aria_label("TODO")
            .flex_1()
            .min_w_0()
            .h_full()
            .py(px(4.))
            .overflow_y_scroll()
            .children(rows);
        let has_item = matches!(self.selected, Some(Node::Item(_)));
        let preview = self.show_preview.then(|| {
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .border_l_1()
                .border_color(tokens.separator)
                .map(|preview| match has_item {
                    true => preview.child(self.preview.render(PreviewLayout::Below, cx)),
                    false => preview
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(tokens.text_secondary)
                        .child(accessible_text(
                            "todo-no-preview",
                            "There is no preview for the selected item".into(),
                        )),
                })
        });
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .track_focus(&self.focus)
                .key_context("TodoPanel menu")
                .on_action(
                    cx.listener(|this, _: &menu::SelectNext, window, cx| this.step(1, window, cx)),
                )
                .on_action(cx.listener(|this, _: &menu::SelectPrevious, window, cx| {
                    this.step(-1, window, cx)
                }))
                .on_action(
                    cx.listener(|this, _: &menu::Confirm, window, cx| {
                        this.open_selected(window, cx)
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
                ),
        )
    }
}

impl Focusable for TodoPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EventEmitter<PanelEvent> for TodoPanel {}

impl Panel for TodoPanel {
    fn persistent_name() -> &'static str {
        "TODO"
    }

    fn panel_key() -> &'static str {
        "TodoToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Bottom
    }

    // shortcut: the tool window stays where IntelliJ puts it, as Problems' does
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Bottom
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(300.)
    }

    // off the stripe until it has shown, as IntelliJ's New UI has it (View ▸ Tool Windows opens it)
    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        self.opened.then_some(ui::IconName::ListTodo)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("TODO")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(todo_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        61
    }

    // the project is searched once the tool window shows, and again as its files change
    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.opened |= active;
        if active && !self.searched {
            cx.defer_in(window, |this, window, cx| this.search(window, cx));
        }
    }
}
