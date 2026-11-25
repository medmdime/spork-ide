//! Recent Files (⌘E) and the Switcher (⌃⇥), IntelliJ's `Switcher` panel: the tool windows on the
//! left, the files on the right, the selected file's path under them. Recent Files lists the
//! files opened lately, with "Show edited only" (⌘E again) for the files edited lately; the
//! Switcher lists the open files and chooses the selected one when ⌃ is let go. Typing filters
//! both lists, as IntelliJ's speed search.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, Modifiers, ModifiersChangedEvent, ParentElement as _, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, TaskExt as _, WeakEntity, Window,
    div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use project::ProjectPath;
use spork_workbench::{Tokens, TreeRow, TreeState, TreeView, Typography as _, popup, tree_keys};
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::{ToolWindow, navigate};

/// IntelliJ's Switcher lists: as wide as their text; these are its usual sizes.
const TOOL_WINDOWS_WIDTH: f32 = 200.;
const FILES_WIDTH: f32 = 360.;
const MAX_ROWS: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    RecentFiles,
    Switcher,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    ToolWindows,
    Files,
}

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &navigate::RecentFiles, window, cx| {
        open(workspace, Kind::RecentFiles, false, false, window, cx)
    });
    workspace.register_action(
        |workspace, _: &navigate::RecentlyChangedFiles, window, cx| {
            open(workspace, Kind::RecentFiles, true, false, window, cx)
        },
    );
    workspace.register_action(|workspace, action: &navigate::Switcher, window, cx| {
        open(
            workspace,
            Kind::Switcher,
            false,
            action.select_last,
            window,
            cx,
        )
    });
}

/// Opens Recent Files or the Switcher. While Recent Files shows, ⌘E again shows the edited files
/// only (or all again); while the Switcher shows, ⌃⇥ again selects the next file (⌃⇧⇥ the one
/// before), as in IntelliJ.
fn open(
    workspace: &mut Workspace,
    kind: Kind,
    edited_only: bool,
    backward: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(switcher) = workspace.active_modal::<Switcher>(cx) {
        switcher.update(cx, |switcher, cx| match (switcher.kind, kind) {
            (Kind::RecentFiles, Kind::RecentFiles) => {
                switcher.edited_only = edited_only || !switcher.edited_only;
                switcher.refilter(cx);
            }
            (Kind::Switcher, Kind::Switcher) => {
                switcher.column = Column::Files;
                switcher.select_next(if backward { -1 } else { 1 }, cx);
            }
            _ => {}
        });
        return;
    }
    let recent = crate::recent_files(workspace, cx);
    let files = match kind {
        Kind::RecentFiles => recent,
        // the open files, the one used last first
        Kind::Switcher => {
            let open: Vec<ProjectPath> = workspace
                .panes()
                .iter()
                .flat_map(|pane| pane.read(cx).items())
                .filter_map(|item| item.project_path(cx))
                .collect();
            let mut files: Vec<ProjectPath> = recent
                .into_iter()
                .filter(|path| open.contains(path))
                .collect();
            for path in open {
                if !files.contains(&path) {
                    files.push(path);
                }
            }
            files
        }
    };
    let mut edited: Vec<ProjectPath> = Vec::new();
    for place in crate::edit_locations(workspace, cx).into_iter().rev() {
        if !edited.contains(&place.path) {
            edited.push(place.path);
        }
    }
    let active = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx));
    // IntelliJ's problem files (`WolfTheProblemSolver`): the ones with errors
    let problems: collections::HashSet<ProjectPath> = workspace
        .project()
        .read(cx)
        .diagnostic_summaries(false, cx)
        .filter(|(_, _, summary)| summary.error_count > 0)
        .map(|(path, _, _)| path)
        .collect();
    let held = window.modifiers();
    let held = (kind == Kind::Switcher && held.modified()).then_some(held);
    let project = workspace.project().clone();
    crate::after_update(window, cx, move |workspace, window, cx| {
        let tool_windows = crate::tool_windows(&workspace, window, cx);
        let handle = workspace.downgrade();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |_, cx| {
                let mut switcher = Switcher {
                    workspace: handle,
                    project,
                    kind,
                    focus: cx.focus_handle(),
                    tool_windows,
                    files,
                    edited,
                    problems,
                    edited_only,
                    filter: String::new(),
                    shown: Vec::new(),
                    column: Column::Files,
                    list: TreeState::list(),
                    held,
                };
                switcher.refilter(cx);
                // the file before the active one, as the Switcher first selects
                let first_is_active = active.is_some() && switcher.shown.first() == active.as_ref();
                switcher
                    .list
                    .select(Some(match (backward, first_is_active) {
                        (true, _) => switcher.shown.len().saturating_sub(1),
                        (false, true) => 1.min(switcher.shown.len().saturating_sub(1)),
                        (false, false) => 0,
                    }));
                if switcher.shown.is_empty() && !switcher.tool_windows.is_empty() {
                    switcher.column = Column::ToolWindows;
                }
                switcher
            });
        });
    });
}

pub(crate) struct Switcher {
    workspace: WeakEntity<Workspace>,
    project: gpui::Entity<project::Project>,
    kind: Kind,
    focus: FocusHandle,
    tool_windows: Vec<ToolWindow>,
    files: Vec<ProjectPath>,
    edited: Vec<ProjectPath>,
    /// The files with errors, their names on a red wavy line.
    problems: collections::HashSet<ProjectPath>,
    edited_only: bool,
    /// What was typed: IntelliJ's speed search.
    filter: String,
    /// The files the list shows.
    shown: Vec<ProjectPath>,
    column: Column,
    /// The selected rows of the column, by their place in it.
    list: TreeState<usize>,
    /// The Switcher's modifiers, held since it opened: letting them go chooses.
    held: Option<Modifiers>,
}

impl EventEmitter<DismissEvent> for Switcher {}

impl ModalView for Switcher {}

impl Focusable for Switcher {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Switcher {
    fn title(&self) -> &'static str {
        match self.kind {
            Kind::RecentFiles => "Recent Files",
            Kind::Switcher => "Switcher",
        }
    }

    fn matches(&self, text: &str) -> bool {
        text.to_lowercase().contains(&self.filter.to_lowercase())
    }

    fn file_name(path: &ProjectPath) -> String {
        path.path
            .file_name()
            .unwrap_or(path.path.as_unix_str())
            .to_owned()
    }

    fn shown_tool_windows(&self) -> Vec<&ToolWindow> {
        self.tool_windows
            .iter()
            .filter(|tool_window| self.matches(&tool_window.title))
            .collect()
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let files = match self.edited_only {
            true => &self.edited,
            false => &self.files,
        };
        self.shown = files
            .iter()
            .filter(|path| self.matches(&Self::file_name(path)))
            .cloned()
            .collect();
        if self.column == Column::Files && self.shown.is_empty() {
            self.column = Column::ToolWindows;
        } else if self.column == Column::ToolWindows
            && self.shown_tool_windows().is_empty()
            && !self.shown.is_empty()
        {
            self.column = Column::Files;
        }
        if self.selected() >= self.column_len().max(1) {
            self.list.select(Some(0));
        }
        cx.notify();
    }

    fn column_len(&self) -> usize {
        match self.column {
            Column::ToolWindows => self.shown_tool_windows().len(),
            Column::Files => self.shown.len(),
        }
    }

    fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.column_len() as isize;
        if len > 0 {
            self.list.select(Some(
                (self.selected() as isize + step).rem_euclid(len) as usize
            ));
        }
        cx.notify();
    }

    fn switch_column(&mut self, column: Column, cx: &mut Context<Self>) {
        let len = match column {
            Column::ToolWindows => self.shown_tool_windows().len(),
            Column::Files => self.shown.len(),
        };
        if self.column != column && len > 0 {
            self.column = column;
            self.list.select(Some(self.selected().min(len - 1)));
            cx.notify();
        }
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = self.workspace.clone();
        match self.column {
            Column::Files => {
                let Some(path) = self.shown.get(self.selected()).cloned() else {
                    return;
                };
                cx.emit(DismissEvent);
                workspace
                    .update(cx, |workspace, cx| {
                        crate::open_at(workspace, path, None, window, cx)
                    })
                    .log_err();
            }
            Column::ToolWindows => {
                let Some(tool_window) = self.shown_tool_windows().get(self.selected()).copied()
                else {
                    return;
                };
                let (position, panel_id, open) =
                    (tool_window.position, tool_window.panel_id, tool_window.open);
                cx.emit(DismissEvent);
                workspace
                    .update(cx, |workspace, cx| {
                        ToolWindow::activate_panel(position, panel_id, open, workspace, window, cx)
                    })
                    .log_err();
            }
        }
    }

    /// Delete from Recent Files (⌫, ⌦, ⌘⌫), IntelliJ's `closeTabOrToolWindow`: a selected file
    /// open in an editor closes, one that isn't leaves the history; both leave the list. A
    /// selected tool window closes.
    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // what is typed keeps the keys, as IntelliJ's speed search does
        if !self.filter.is_empty() {
            return;
        }
        let rows = self.tree_rows(cx);
        let selected = self.list.selected(&rows);
        let Some(first) = selected.first().copied() else {
            return;
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        match self.column {
            Column::Files => {
                let paths: collections::HashSet<ProjectPath> = selected
                    .iter()
                    .filter_map(|ix| self.shown.get(*ix).cloned())
                    .collect();
                for pane in workspace.read(cx).panes().to_vec() {
                    pane.update(cx, |pane, cx| {
                        let open: Vec<_> = pane
                            .items()
                            .filter(|item| {
                                item.project_path(cx)
                                    .is_some_and(|path| paths.contains(&path))
                            })
                            .map(|item| item.item_id())
                            .collect();
                        for item in open {
                            pane.close_item_by_id(item, workspace::SaveIntent::Close, window, cx)
                                .detach_and_log_err(cx);
                        }
                        pane.remove_navigation_history_paths(&paths, cx);
                    });
                }
                self.files.retain(|path| !paths.contains(path));
                self.edited.retain(|path| !paths.contains(path));
            }
            Column::ToolWindows => {
                let open: Vec<gpui::EntityId> = self
                    .shown_tool_windows()
                    .iter()
                    .enumerate()
                    .filter(|(ix, tool_window)| selected.contains(ix) && tool_window.open)
                    .map(|(_, tool_window)| tool_window.panel_id)
                    .collect();
                for tool_window in &mut self.tool_windows {
                    if open.contains(&tool_window.panel_id) {
                        tool_window.open = false;
                        let (position, panel_id) = (tool_window.position, tool_window.panel_id);
                        workspace.update(cx, |workspace, cx| {
                            workspace.toggle_tool_window(position, panel_id, window, cx)
                        });
                    }
                }
            }
        }
        self.refilter(cx);
        let len = self.column_len();
        if len > 0 {
            self.list.select(Some(first.min(len - 1)));
        } else if self.column == Column::Files {
            self.switch_column(Column::ToolWindows, cx);
        }
        cx.notify();
    }

    fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(held) = self.held else { return };
        if !event.modified() || !held.is_subset_of(event) {
            self.held = None;
            self.choose(window, cx);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        if keystroke.key == "backspace" {
            match self.filter.pop() {
                Some(_) => self.refilter(cx),
                None => self.delete_selected(window, cx),
            }
            cx.stop_propagation();
            return;
        }
        let typed = keystroke
            .key_char
            .clone()
            .or_else(|| (keystroke.key.chars().count() == 1).then(|| keystroke.key.clone()));
        if let Some(typed) = typed.filter(|typed| typed.chars().all(|ch| !ch.is_control())) {
            cx.stop_propagation();
            self.filter.push_str(&typed);
            self.list.select(Some(0));
            self.refilter(cx);
        }
    }

    /// Select Next/Previous Problem File (F2, ⇧F2; IntelliJ's `SwitcherProblemAction`): the next
    /// file with errors after the selected one, round the ends, while the popup stays open
    /// without held keys (Recent Files).
    fn select_problem(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.held.is_some() || self.shown.is_empty() {
            return;
        }
        let len = self.shown.len();
        let lead = match self.column {
            Column::Files => Some(self.selected()),
            Column::ToolWindows => None,
        };
        let start = match (forward, lead) {
            (true, Some(lead)) if lead + 1 < len => lead + 1,
            (true, _) => 0,
            (false, Some(lead)) if lead > 0 && lead < len => lead - 1,
            (false, _) => len - 1,
        };
        let found = (0..len)
            .map(|step| match forward {
                true => (start + step) % len,
                false => (start + len - step) % len,
            })
            .find(|ix| {
                self.shown
                    .get(*ix)
                    .is_some_and(|path| self.problems.contains(path))
            });
        if let Some(ix) = found {
            self.column = Column::Files;
            self.list.select(Some(ix));
            self.list.scroll_handle().scroll_to_item(ix);
            cx.notify();
        }
    }

    /// The row the keys move from.
    fn selected(&self) -> usize {
        self.list.lead().copied().unwrap_or(0)
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        column: Column,
        ix: usize,
        selected: bool,
        name: SharedString,
        icon: gpui::AnyElement,
        hint: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        // a file's name in its version control status's colour, as IntelliJ's Switcher has it
        let color = match column {
            Column::Files => self
                .shown
                .get(ix)
                .and_then(|path| crate::file_status_color(&self.project, path, cx)),
            Column::ToolWindows => None,
        };
        let problem = column == Column::Files
            && self
                .shown
                .get(ix)
                .is_some_and(|path| self.problems.contains(path));
        let wave = Tokens::get(cx).problem_wave;
        let id = match column {
            Column::ToolWindows => ("tool-window", ix),
            Column::Files => ("file", ix),
        };
        popup::row(id, selected, cx)
            .role(Role::ListBoxOption)
            .aria_label(name.clone())
            .aria_selected(selected)
            .child(icon)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when_some(color, |name, color| name.text_color(color))
                    .when(problem, |name| {
                        name.text_decoration_1()
                            .text_decoration_wavy()
                            .text_decoration_color(wave)
                    })
                    .child(name),
            )
            .children(hint.map(|hint| popup::hint(hint, selected, cx)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.column = column;
                this.list.select(Some(ix));
                this.choose(window, cx);
            }))
            .into_any_element()
    }
}

impl Render for Switcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let mut header = h_flex()
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .gap(px(8.))
            .border_b_1()
            .border_color(tokens.popup_border)
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(self.title()),
            )
            .child(div().flex_1());
        if self.kind == Kind::RecentFiles {
            let shortcut = crate::shortcut_text(&navigate::RecentFiles, window);
            header = header
                .child(
                    spork_workbench::controls::labeled_check(
                        "edited-only",
                        "Show edited only",
                        self.edited_only,
                        cx,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.edited_only = !this.edited_only;
                        this.refilter(cx);
                    })),
                )
                .children(
                    shortcut
                        .map(|shortcut| div().text_color(tokens.text_secondary).child(shortcut)),
                );
        }
        let list_rows = self.tree_rows(cx);
        let selected_rows = self.list.selected_range(&list_rows);
        let is_selected = |column: Column, ix: usize| {
            self.column == column
                && selected_rows
                    .as_ref()
                    .is_some_and(|rows| rows.contains(&ix))
        };
        let tool_windows: Vec<gpui::AnyElement> = self
            .shown_tool_windows()
            .into_iter()
            .enumerate()
            .map(|(ix, tool_window)| {
                (
                    ix,
                    tool_window.title.clone(),
                    tool_window.icon_element(cx),
                    tool_window.shortcut.clone(),
                )
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(ix, title, icon, shortcut)| {
                let selected = is_selected(Column::ToolWindows, ix);
                self.row(Column::ToolWindows, ix, selected, title, icon, shortcut, cx)
            })
            .collect();
        let files: Vec<gpui::AnyElement> = self
            .shown
            .clone()
            .into_iter()
            .enumerate()
            .map(|(ix, path)| {
                let name = Self::file_name(&path);
                let icon = spork_workbench::image(spork_workbench::file_icon(&name), cx)
                    .into_any_element();
                let selected = is_selected(Column::Files, ix);
                self.row(Column::Files, ix, selected, name.into(), icon, None, cx)
            })
            .collect();
        let scroll = self.list.scroll_handle().clone();
        let column = self.column;
        let rows = |id: &'static str,
                    label: &'static str,
                    width: f32,
                    of: Column,
                    rows: Vec<gpui::AnyElement>| {
            v_flex()
                .id(id)
                .role(Role::List)
                .aria_label(label)
                .w(px(width))
                .max_h(px(24. * MAX_ROWS as f32))
                .py(px(5.))
                .when(of == column, |list| list.track_scroll(&scroll))
                .overflow_y_scroll()
                .children(rows)
        };
        let path = match self.column {
            Column::Files => self.shown.get(self.selected()).and_then(|path| {
                let path = self.project.read(cx).absolute_path(path, cx)?;
                Some(
                    util::paths::PathExt::compact(&path)
                        .to_string_lossy()
                        .into_owned(),
                )
            }),
            Column::ToolWindows => None,
        };
        let surface = popup::surface(cx)
            .id("switcher")
            .role(Role::Dialog)
            .aria_label(self.title())
            .track_focus(&self.focus)
            // "menu" and "tree": IntelliJ's keys for lists, ←→ between the two
            .key_context("Switcher menu tree")
            // before the list's keys: ←→ go between the lists, IntelliJ's next column
            .on_action(cx.listener(|this, _: &menu::SelectChild, _, cx| {
                this.switch_column(Column::Files, cx)
            }))
            .on_action(cx.listener(|this, _: &menu::SelectParent, _, cx| {
                this.switch_column(Column::ToolWindows, cx)
            }))
            .on_action(
                cx.listener(|this, _: &navigate::DeleteRecentFiles, window, cx| {
                    this.delete_selected(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &navigate::SwitcherNextProblem, _, cx| {
                    this.select_problem(true, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &navigate::SwitcherPreviousProblem, _, cx| {
                    this.select_problem(false, cx)
                }),
            );
        let surface = tree_keys(surface, cx)
            .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| {
                if this.filter.is_empty() {
                    cx.emit(DismissEvent);
                } else {
                    this.filter.clear();
                    this.refilter(cx);
                }
            }))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .on_key_down(cx.listener(Self::key_down))
            .child(header)
            .when(!self.filter.is_empty(), |popup| {
                popup.child(
                    h_flex()
                        .flex_none()
                        .px(px(12.))
                        .py(px(4.))
                        .gap(px(6.))
                        .border_b_1()
                        .border_color(tokens.popup_border)
                        .child(spork_workbench::icon("general/search").size(px(16.)))
                        .child(
                            div()
                                .id("speed-search")
                                .role(Role::TextInput)
                                .aria_label("Search")
                                .child(self.filter.clone()),
                        ),
                )
            })
            .child(
                h_flex()
                    .items_start()
                    .child(rows(
                        "tool-windows",
                        "Tool windows",
                        TOOL_WINDOWS_WIDTH,
                        Column::ToolWindows,
                        tool_windows,
                    ))
                    .child(div().w(px(1.)).self_stretch().bg(tokens.popup_border))
                    .child(rows("files", "Files", FILES_WIDTH, Column::Files, files)),
            )
            .children(path.map(|path| {
                h_flex()
                    .flex_none()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(tokens.popup_border)
                    .text_ui_small()
                    .text_color(tokens.text_secondary)
                    .child(div().truncate().child(path))
            }));
        spork_vcs::spork_look(cx).child(surface)
    }
}

impl TreeView for Switcher {
    type Key = usize;

    fn tree(&mut self) -> &mut TreeState<usize> {
        &mut self.list
    }

    fn tree_rows(&self, _: &App) -> Vec<TreeRow<usize>> {
        let names: Vec<SharedString> = match self.column {
            Column::ToolWindows => self
                .shown_tool_windows()
                .into_iter()
                .map(|tool_window| tool_window.title.clone())
                .collect(),
            Column::Files => self
                .shown
                .iter()
                .map(|path| Self::file_name(path).into())
                .collect(),
        };
        names
            .into_iter()
            .enumerate()
            .map(|(ix, name)| TreeRow::item(ix, name))
            .collect()
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(window, cx);
    }

    // its own speed search, filtering the lists
    fn speed_search(&self) -> bool {
        false
    }
}
