//! The project widget's popup, as in IntelliJ / the reference app (docs/reference-spec.md §9 "Project
//! widget popup"): Open…, Clone Repository…, then the open projects and the recent ones, each
//! with its badge, path and branch. Typing filters them by name and path.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_workbench::{Tokens, Typography as _, popup};

use crate::RepoView;
use crate::repos::{display_name, head_branch, tilde};

/// Every project window's view, for "Open Projects".
#[derive(Default)]
struct OpenProjects(Vec<(AnyWindowHandle, WeakEntity<RepoView>)>);

impl Global for OpenProjects {}

pub(crate) fn register(window: AnyWindowHandle, view: WeakEntity<RepoView>, cx: &mut App) {
    let projects = &mut cx.default_global::<OpenProjects>().0;
    projects.retain(|(_, view)| view.upgrade().is_some());
    projects.push((window, view));
}

/// A project the popup lists: where it is, its branch, and its window when it is open.
#[derive(Clone)]
pub(crate) struct Project {
    path: PathBuf,
    branch: Option<String>,
    window: Option<AnyWindowHandle>,
}

impl RepoView {
    /// The projects open in other windows and this one, then the recent ones (at most 100).
    /// This view's own entry is read from `self`: it may be the one being updated.
    pub(crate) fn popup_projects(
        &self,
        me: EntityId,
        window: &Window,
        cx: &App,
    ) -> (Vec<Project>, Vec<Project>) {
        let this = self.tab_root();
        let mut open: Vec<Project> = this
            .iter()
            .map(|path| Project {
                branch: head_branch(path),
                path: path.clone(),
                window: Some(window.window_handle()),
            })
            .collect();
        let others = cx
            .try_global::<OpenProjects>()
            .map(|p| p.0.as_slice())
            .unwrap_or_default();
        for (handle, view) in others {
            let Some(view) = view.upgrade() else { continue };
            if view.entity_id() == me {
                continue;
            }
            if let Some(path) = view.read(cx).tab_root()
                && !open.iter().any(|p| p.path == path)
            {
                open.push(Project {
                    branch: head_branch(&path),
                    path,
                    window: Some(*handle),
                });
            }
        }
        let recent = self
            .recent
            .iter()
            .filter(|path| !open.iter().any(|p| &p.path == *path))
            .take(100)
            .map(|path| Project {
                branch: head_branch(path),
                path: path.clone(),
                window: None,
            })
            .collect();
        (open, recent)
    }
}

#[derive(Clone)]
enum Item {
    Open,
    Clone,
    Header(&'static str),
    Project(Project),
}

pub(crate) struct ProjectPopup {
    view: WeakEntity<RepoView>,
    search: Entity<InputState>,
    focus: FocusHandle,
    open: Vec<Project>,
    recent: Vec<Project>,
    selected: usize,
    /// Where the project widget is: its own click closes the popup.
    trigger: Rc<Cell<Bounds<Pixels>>>,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for ProjectPopup {}

impl ProjectPopup {
    pub(crate) fn new(
        view: WeakEntity<RepoView>,
        trigger: Rc<Cell<Bounds<Pixels>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx));
        let subscription =
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.selected = 0;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.activate(this.selected, window, cx),
                _ => {}
            });
        Self {
            view,
            search,
            focus: cx.focus_handle(),
            open: Vec::new(),
            recent: Vec::new(),
            selected: 0,
            trigger,
            _subscription: subscription,
        }
    }

    /// Opening: these projects, no filter, Open… selected, the keys to the popup.
    pub(crate) fn reset(
        &mut self,
        (open, recent): (Vec<Project>, Vec<Project>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open = open;
        self.recent = recent;
        self.selected = 0;
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.focus.focus(window, cx);
    }

    fn items(&self, cx: &App) -> Vec<Item> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        // IntelliJ's speed search: by name and by path
        let shown = |p: &&Project| {
            query.is_empty()
                || display_name(&p.path).to_lowercase().contains(&query)
                || tilde(&p.path).to_lowercase().contains(&query)
        };
        let mut items = Vec::new();
        if query.is_empty() {
            items.extend([Item::Open, Item::Clone]);
        }
        for (title, projects) in [
            ("Open Projects", &self.open),
            ("Recent Projects", &self.recent),
        ] {
            let projects: Vec<&Project> = projects.iter().filter(shown).collect();
            if !projects.is_empty() {
                items.push(Item::Header(title));
                items.extend(projects.into_iter().cloned().map(Item::Project));
            }
        }
        items
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let items = self.items(cx);
        let count = items.len() as isize;
        let mut ix = self.selected as isize;
        for _ in 0..count {
            ix = (ix + step).rem_euclid(count);
            if !matches!(items[ix as usize], Item::Header(_)) {
                break;
            }
        }
        self.selected = ix.max(0) as usize;
        cx.notify();
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self
            .items(cx)
            .into_iter()
            .filter(|i| !matches!(i, Item::Header(_)))
            .nth(ix)
        else {
            return;
        };
        cx.emit(DismissEvent);
        let Some(view) = self.view.upgrade() else {
            return;
        };
        match item {
            Item::Open => view.update(cx, |this, cx| this.open_repo_dialog(window, cx)),
            Item::Clone => view.update(cx, |this, cx| this.clone_dialog(window, cx)),
            Item::Header(_) => {}
            Item::Project(Project {
                window: Some(handle),
                ..
            }) => {
                handle
                    .update(cx, |_, window, _| window.activate_window())
                    .ok();
            }
            Item::Project(Project { path, .. }) => {
                crate::open_repo_window(vec![path], None, cx);
            }
        }
    }

    fn project_row(&self, ix: usize, project: &Project, selected: bool, cx: &App) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        let name = display_name(&project.path);
        let slot = spork_workbench::shell::project_slot(&project.path);
        popup::row(("project", ix), selected, cx)
            .h_auto()
            .items_start()
            .py(px(6.))
            .gap(px(8.))
            .child(spork_workbench::shell::project_badge(&name, slot))
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().h(px(20.)).flex().items_center().child(name))
                    .child(
                        div()
                            .h(px(18.))
                            .flex()
                            .items_center()
                            .text_ui_small()
                            .text_color(tokens.text_secondary)
                            .child(tilde(&project.path)),
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

impl Render for ProjectPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.items(cx);
        let searching = !self.search.read(cx).value().is_empty();
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
                    Item::Open | Item::Clone => {
                        let (icon, label) = match item {
                            Item::Open => ("general/open", "Open…"),
                            _ => ("general/vcs", "Clone Repository…"),
                        };
                        popup::row(("project-action", ix), selected, cx)
                            .child(spork_workbench::image(icon, cx).size(px(16.)))
                            .child(label)
                    }
                    Item::Project(project) => self.project_row(ix, project, selected, cx),
                    Item::Header(_) => unreachable!("headers return above"),
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
        popup::surface(cx)
            .id("project-popup")
            .track_focus(&self.focus)
            .min_w(px(220.))
            .max_w(px(480.))
            .max_h(px(600.))
            .py(px(8.))
            .key_context("ProjectPopup")
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                cx.stop_propagation();
                this.select_next(-1, cx)
            }))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                cx.stop_propagation();
                this.select_next(1, cx)
            }))
            .capture_action(cx.listener(|_, _: &Escape, _, cx| {
                cx.stop_propagation();
                cx.emit(DismissEvent)
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
                            .filter(|c| !c.trim().is_empty())
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
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !this.trigger.get().contains(&event.position) {
                    cx.emit(DismissEvent)
                }
            }))
            .when(searching, |popup| {
                popup.child(
                    div().px(px(12.)).pb(px(4.)).child(
                        spork_workbench::TextField::new(&self.search)
                            .in_popup()
                            .prefix(spork_workbench::icon("general/search").size(px(16.))),
                    ),
                )
            })
            .children(rows)
    }
}
