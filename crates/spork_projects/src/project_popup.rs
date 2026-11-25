//! The Recent Projects popup of the main toolbar's project widget (reference-spec.md §9 "Project
//! widget popup", with IntelliJ IDEA's New Project…): New Project…, Open…, Clone Repository…,
//! then the open projects and the recent ones, each with its badge, path and branch; typing
//! filters them by name and path. File ▸ Recent Projects ▸ Manage Projects… shows the same
//! projects in IntelliJ's Recent Projects dialog.

use std::path::PathBuf;

use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::{Tokens, Typography as _, popup};
use workspace::MultiWorkspace;

use crate::RecentProject;

/// A project the popup lists.
#[derive(Clone)]
struct Listed {
    project: RecentProject,
}

#[derive(Clone)]
enum Item {
    NewProject,
    Open,
    Clone,
    Header(&'static str),
    Project(Listed),
}

pub struct ProjectPopup {
    search: Entity<InputState>,
    focus: FocusHandle,
    /// In the Recent Projects dialog: no actions, the search always showing.
    in_dialog: bool,
    open: Vec<Listed>,
    recent: Vec<Listed>,
    selected: usize,
    _subscriptions: Vec<Subscription>,
    _load: Task<()>,
}

impl EventEmitter<DismissEvent> for ProjectPopup {}

impl Focusable for ProjectPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ProjectPopup {
    /// The popup the project widget opens: it lists the projects once read.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(false, window, cx)
    }

    fn build(in_dialog: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search projects"));
        let focus = cx.focus_handle();
        let mut subscriptions =
            vec![
                cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                    InputEvent::Change => {
                        this.selected = 0;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.activate(this.selected, window, cx),
                    _ => {}
                }),
            ];
        if !in_dialog {
            subscriptions
                .push(cx.on_focus_out(&focus, window, |_, _, _, cx| cx.emit(DismissEvent)));
        }
        match in_dialog {
            true => search.update(cx, |search, cx| search.focus(window, cx)),
            false => focus.focus(window, cx),
        }
        let windows = open_project_windows(window, cx);
        let load = crate::load_recent_projects(cx);
        let _load = cx.spawn(async move |this, cx| {
            let recent = load.await;
            // the open projects come from their windows: the recent list may not have them yet
            let open: Vec<Listed> = cx
                .background_spawn(async move {
                    windows
                        .into_iter()
                        .filter_map(|paths| {
                            Some(Listed {
                                project: RecentProject::from_paths(paths, None)?,
                            })
                        })
                        .collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.recent = recent
                    .into_iter()
                    .filter(|project| !open.iter().any(|o| o.project.paths == project.paths))
                    .take(100)
                    .map(|project| Listed { project })
                    .collect();
                this.open = open;
                cx.notify();
            })
            .ok();
        });
        Self {
            search,
            focus,
            in_dialog,
            open: Vec::new(),
            recent: Vec::new(),
            selected: 0,
            _subscriptions: subscriptions,
            _load,
        }
    }

    fn items(&self, cx: &App) -> Vec<Item> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let mut items = Vec::new();
        if query.is_empty() && !self.in_dialog {
            items.extend([Item::NewProject, Item::Open, Item::Clone]);
        }
        for (title, projects) in [
            ("Open Projects", &self.open),
            ("Recent Projects", &self.recent),
        ] {
            let shown: Vec<&Listed> = projects
                .iter()
                .filter(|listed| listed.project.matches(&query))
                .collect();
            if !shown.is_empty() {
                items.push(Item::Header(title));
                items.extend(shown.into_iter().cloned().map(Item::Project));
            }
        }
        items
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let count = self
            .items(cx)
            .iter()
            .filter(|item| !matches!(item, Item::Header(_)))
            .count() as isize;
        if count > 0 {
            self.selected = (self.selected as isize + step).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .items(cx)
            .into_iter()
            .filter(|item| !matches!(item, Item::Header(_)))
            .nth(ix)
        else {
            return;
        };
        cx.emit(DismissEvent);
        if self.in_dialog {
            window.close_dialog(cx);
        }
        match item {
            Item::NewProject => crate::dialogs::new_project(window, cx),
            Item::Open => window.dispatch_action(Box::new(workspace::Open::default()), cx),
            Item::Clone => crate::dialogs::get_from_vcs(window, cx),
            Item::Header(_) => {}
            // an open project's window and workspace come forward
            Item::Project(Listed { project, .. }) => crate::open_project(project.paths, cx),
        }
    }

    fn project_row(
        &self,
        ix: usize,
        project: &RecentProject,
        selected: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        popup::row(("project", ix), selected, cx)
            .role(Role::MenuItem)
            .aria_label(project.name.clone())
            .h_auto()
            .items_start()
            .py(px(6.))
            .gap(px(8.))
            .child(spork_workbench::shell::project_badge(
                &project.name,
                project.slot,
            ))
            .child(
                v_flex()
                    .min_w_0()
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .child(project.name.clone()),
                    )
                    .child(
                        div()
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .child(project.location.clone()),
                    )
                    .children(project.branch.clone().map(|branch| {
                        h_flex()
                            .h(px(18.))
                            .gap(px(4.))
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .child(
                                spork_workbench::icon("general/vcs")
                                    .size(px(14.))
                                    .text_color(tokens.text_secondary),
                            )
                            .child(branch)
                    })),
            )
    }
}

/// The open projects, this window's first: every workspace of every project window, by its
/// folders. This window is being drawn, so its root is read from `window`.
fn open_project_windows(window: &Window, cx: &App) -> Vec<Vec<PathBuf>> {
    let this = window.window_handle();
    let folders = |workspace: &Entity<workspace::Workspace>| -> Vec<PathBuf> {
        workspace
            .read(cx)
            .visible_worktrees(cx)
            .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
            .collect()
    };
    let mut open = Vec::new();
    if let Some(Some(multi_workspace)) = window.root::<MultiWorkspace>() {
        for workspace in multi_workspace.read(cx).workspaces() {
            open.push(folders(workspace));
        }
    }
    for handle in cx.windows() {
        if handle.window_id() == this.window_id() {
            continue;
        }
        let Some(multi_workspace) = handle
            .downcast::<MultiWorkspace>()
            .and_then(|handle| handle.read(cx).ok())
        else {
            continue;
        };
        for workspace in multi_workspace.workspaces() {
            open.push(folders(workspace));
        }
    }
    open.retain(|paths| !paths.is_empty());
    open
}

/// Manage Projects…: IntelliJ's Recent Projects dialog, the projects with a search above.
pub(crate) fn manage(window: &mut Window, cx: &mut App) {
    // the list reads the workspaces, one of which runs this action: build it once it has
    window.defer(cx, open_manage);
}

fn open_manage(window: &mut Window, cx: &mut App) {
    let list = cx.new(|cx| ProjectPopup::build(true, window, cx));
    let search = list.read(cx).search.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(spork_workbench::controls::dialog_title("Recent Projects"))
            .on_ok(|_, _, _| false)
            .w(px(300.))
            .close_button(false)
            .child(list.clone())
    });
    search.update(cx, |search, cx| search.focus(window, cx));
}

impl Render for ProjectPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let items = self.items(cx);
        let searching = self.in_dialog || !self.search.read(cx).value().is_empty();
        let mut index = 0;
        let mut first_header = true;
        let rows: Vec<AnyElement> = items
            .iter()
            .map(|item| {
                if let Item::Header(title) = item {
                    let header = popup::labeled_separator(*title, first_header && index == 0, cx);
                    first_header = false;
                    return header.into_any_element();
                }
                let ix = index;
                index += 1;
                let selected = self.selected == ix;
                let row = match item {
                    Item::Project(listed) => self.project_row(ix, &listed.project, selected, cx),
                    action => {
                        let (icon, label) = match action {
                            Item::NewProject => ("general/add", "New Project…"),
                            Item::Open => ("general/open", "Open…"),
                            _ => ("general/vcs", "Clone Repository…"),
                        };
                        popup::row(("project-action", ix), selected, cx)
                            .role(Role::MenuItem)
                            .aria_label(label)
                            .child(spork_workbench::image(icon, cx).size(px(16.)))
                            .child(label)
                    }
                };
                row.on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.selected != ix {
                        this.selected = ix;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
                .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let list = v_flex()
            .id("project-popup")
            .role(Role::Menu)
            .aria_label("Recent Projects")
            .track_focus(&self.focus)
            // "menu": Zed's keys for lists (↑ ↓ ↩ ⎋) reach it before the workspace's
            .key_context("ProjectPopup menu")
            .on_action(
                cx.listener(|this, _: &menu::SelectPrevious, _, cx| this.select_next(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &menu::SelectNext, _, cx| this.select_next(1, cx)))
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                this.activate(this.selected, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &menu::Cancel, window, cx| match this.in_dialog {
                    true => window.close_dialog(cx),
                    false => cx.emit(DismissEvent),
                }),
            )
            .min_w(px(220.))
            .max_w(px(480.))
            .max_h(px(600.))
            .py(px(8.))
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.select_next(-1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.select_next(1, cx)
            }))
            .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                cx.stop_propagation();
                match this.in_dialog {
                    true => window.close_dialog(cx),
                    false => cx.emit(DismissEvent),
                }
            }))
            // before anything is typed the popup has the keys: arrows, Enter, Escape, and the
            // first letter, which opens the search; then the search field has them
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.search.read(cx).focus_handle(cx).is_focused(window) {
                    return;
                }
                let keystroke = &event.keystroke;
                match keystroke.key.as_str() {
                    "up" => this.select_next(-1, cx),
                    "down" => this.select_next(1, cx),
                    "enter" => this.activate(this.selected, window, cx),
                    "escape" => cx.emit(DismissEvent),
                    _ => {
                        let typed = keystroke
                            .key_char
                            .clone()
                            .filter(|typed| !typed.trim().is_empty())
                            .filter(|_| {
                                !keystroke.modifiers.platform && !keystroke.modifiers.control
                            });
                        let Some(typed) = typed else { return };
                        this.search.update(cx, |search, cx| {
                            search.set_value(typed, window, cx);
                            search.focus(window, cx);
                        });
                    }
                }
                cx.stop_propagation();
            }))
            .when(searching, |list| {
                list.child(
                    div().px(px(12.)).pb(px(4.)).child(
                        spork_workbench::TextField::new(&self.search)
                            .in_popup()
                            .aria_label("Search projects")
                            .prefix(spork_workbench::icon("general/search").size(px(16.))),
                    ),
                )
            })
            .when(empty && self.in_dialog, |list| {
                list.child(
                    div()
                        .h(px(400.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(tokens.text_secondary)
                        .child("Nothing to show"),
                )
            })
            .children(rows);
        let list = match self.in_dialog {
            true => list.into_any_element(),
            false => popup::surface(cx).child(list).into_any_element(),
        };
        spork_vcs::spork_look(cx).child(list)
    }
}
