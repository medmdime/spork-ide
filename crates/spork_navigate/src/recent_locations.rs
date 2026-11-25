//! Recent Locations (⇧⌘E), IntelliJ's `RecentLocationsAction`: the places visited lately, the
//! latest first, each with its file, the symbols around it and its code; "Show edited only"
//! (⇧⌘E again) lists the places edited lately instead. Typing filters them. Show Scratch Files
//! (IntelliJ's `Scratch.ShowFilesPopup`) is the same popup over the scratch files, by name.

use editor::NavigationData;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledText, Task, WeakEntity,
    Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use language::{Buffer, LanguageAwareStyling, Point};
use project::ProjectPath;
use settings::Settings as _;
use spork_workbench::{Tokens, TreeRow, TreeState, TreeView, Typography as _, popup, tree_keys};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::{Place, navigate};

const WIDTH: f32 = 640.;
const MAX_HEIGHT: f32 = 560.;
/// How many lines of code each place shows, the place's line in the middle.
const LINES: u32 = 3;

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &navigate::RecentLocations, window, cx| {
        if let Some(popup) = workspace.active_modal::<RecentLocations>(cx) {
            popup.update(cx, |popup, cx| {
                popup.edited_only = !popup.edited_only;
                popup.refilter(cx);
            });
            return;
        }
        let visited = visited_places(workspace, cx);
        let mut edited = crate::edit_locations(workspace, cx);
        edited.reverse();
        let project = workspace.project().clone();
        let handle = cx.entity().downgrade();
        workspace.toggle_modal(window, cx, move |window, cx| {
            RecentLocations::new(handle, project, visited, edited, false, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &navigate::ShowScratchFiles, window, cx| {
        let scratches = scratch_places(workspace, cx);
        let edited: Vec<Place> = crate::edit_locations(workspace, cx)
            .into_iter()
            .rev()
            .filter(|place| scratches.iter().any(|scratch| scratch.path == place.path))
            .fold(Vec::new(), |mut edited, place| {
                if !edited.iter().any(|seen: &Place| seen.path == place.path) {
                    edited.push(place);
                }
                edited
            });
        let project = workspace.project().clone();
        let handle = cx.entity().downgrade();
        workspace.toggle_modal(window, cx, move |window, cx| {
            RecentLocations::new(handle, project, scratches, edited, true, window, cx)
        });
    });
}

/// IntelliJ's most scratch files the popup lists.
const MAX_SCRATCHES: usize = 1000;

/// The scratch files (the configuration folder's `scratches`, which the project shows in its
/// own worktree), by name.
fn scratch_places(workspace: &Workspace, cx: &App) -> Vec<Place> {
    let mut files = Vec::new();
    let mut pending = vec![paths::config_dir().join("scratches")];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match path.is_dir() {
                true => pending.push(path),
                false => files.push(path),
            }
        }
    }
    files.sort_by_key(|path| path.file_name().map(|name| name.to_ascii_lowercase()));
    let project = workspace.project().read(cx);
    files
        .into_iter()
        .filter_map(|path| {
            let (worktree, relative) = project.find_worktree(&path, cx)?;
            Some(Place {
                path: ProjectPath {
                    worktree_id: worktree.read(cx).id(),
                    path: relative,
                },
                point: Point::zero(),
            })
        })
        .take(MAX_SCRATCHES)
        .collect()
}

/// The places the panes' navigation histories kept, and the caret's, the latest first, one per
/// line.
fn visited_places(workspace: &Workspace, cx: &mut App) -> Vec<Place> {
    let mut places: Vec<(usize, Place)> = Vec::new();
    for pane in workspace.panes() {
        pane.read(cx)
            .nav_history()
            .for_each_entry(cx, &mut |entry, (path, _)| {
                let Some(data) = entry
                    .data
                    .as_ref()
                    .and_then(|data| data.downcast_ref::<NavigationData>())
                else {
                    return;
                };
                let point = data.cursor_position();
                places.push((entry.timestamp, Place { path, point }));
            });
    }
    places.sort_by_key(|(timestamp, _)| std::cmp::Reverse(*timestamp));
    let mut unique: Vec<Place> = crate::current_place(workspace, cx).into_iter().collect();
    for (_, place) in places {
        if !unique
            .iter()
            .any(|seen| seen.path == place.path && seen.point.row == place.point.row)
        {
            unique.push(place);
        }
    }
    unique
}

/// A place as the popup shows it, once its file has loaded.
struct Location {
    place: Place,
    name: SharedString,
    /// The symbols the place is in (`impl Greeter › fn hello`).
    breadcrumb: SharedString,
    /// The lines around the place, highlighted as the editor colors them.
    code: SharedString,
    highlights: Vec<(std::ops::Range<usize>, gpui::HighlightStyle)>,
}

pub(crate) struct RecentLocations {
    workspace: WeakEntity<Workspace>,
    focus: FocusHandle,
    visited: Vec<Location>,
    edited: Vec<Location>,
    edited_only: bool,
    /// Show Scratch Files: the scratch files in place of the places visited.
    scratches: bool,
    filter: String,
    /// Indices into the list showing.
    shown: Vec<usize>,
    /// The selected row, by its place in `shown`.
    rows: TreeState<usize>,
    _load: Task<()>,
}

impl EventEmitter<DismissEvent> for RecentLocations {}

impl ModalView for RecentLocations {}

impl Focusable for RecentLocations {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl RecentLocations {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<project::Project>,
        visited: Vec<Place>,
        edited: Vec<Place>,
        scratches: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let paths: Vec<ProjectPath> = visited
            .iter()
            .chain(&edited)
            .map(|place| place.path.clone())
            .fold(Vec::new(), |mut paths, path| {
                if !paths.contains(&path) {
                    paths.push(path);
                }
                paths
            });
        let buffers: Vec<_> = paths
            .iter()
            .map(|path| project.update(cx, |project, cx| project.open_buffer(path.clone(), cx)))
            .collect();
        let load = cx.spawn_in(window, async move |this, cx| {
            let mut loaded = Vec::new();
            for (path, buffer) in paths.into_iter().zip(buffers) {
                if let Some(buffer) = buffer.await.log_err() {
                    loaded.push((path, buffer));
                }
            }
            this.update(cx, |this, cx| {
                this.visited = locations(&visited, &loaded, cx);
                this.edited = locations(&edited, &loaded, cx);
                this.refilter(cx);
            })
            .ok();
        });
        Self {
            workspace,
            focus: cx.focus_handle(),
            visited: Vec::new(),
            edited: Vec::new(),
            edited_only: false,
            scratches,
            filter: String::new(),
            shown: Vec::new(),
            rows: TreeState::list(),
            _load: load,
        }
    }

    fn list(&self) -> &[Location] {
        match self.edited_only {
            true => &self.edited,
            false => &self.visited,
        }
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let filter = self.filter.to_lowercase();
        self.shown = self
            .list()
            .iter()
            .enumerate()
            .filter(|(_, location)| {
                [&location.name, &location.breadcrumb, &location.code]
                    .iter()
                    .any(|text| text.to_lowercase().contains(&filter))
            })
            .map(|(ix, _)| ix)
            .collect();
        if self.rows.lead().is_none_or(|row| *row >= self.shown.len()) {
            self.rows.select(Some(0));
        }
        cx.notify();
    }

    fn selected(&self) -> usize {
        self.rows.lead().copied().unwrap_or(0)
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(place) = self
            .shown
            .get(self.selected())
            .and_then(|ix| self.list().get(*ix))
            .map(|location| location.place.clone())
        else {
            return;
        };
        cx.emit(DismissEvent);
        self.workspace
            .update(cx, |workspace, cx| {
                crate::open_at(workspace, place.path, Some(place.point), window, cx)
            })
            .log_err();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        if keystroke.key == "backspace" {
            if self.filter.pop().is_some() {
                self.refilter(cx);
            }
            return;
        }
        let typed = keystroke
            .key_char
            .clone()
            .or_else(|| (keystroke.key.chars().count() == 1).then(|| keystroke.key.clone()));
        if let Some(typed) = typed.filter(|typed| typed.chars().all(|ch| !ch.is_control())) {
            cx.stop_propagation();
            self.filter.push_str(&typed);
            self.rows.select(Some(0));
            self.refilter(cx);
        }
    }
}

/// The places whose file loaded, with what the popup shows of them.
fn locations(
    places: &[Place],
    loaded: &[(ProjectPath, Entity<Buffer>)],
    cx: &App,
) -> Vec<Location> {
    let syntax = cx.theme().syntax().clone();
    places
        .iter()
        .filter_map(|place| {
            let (_, buffer) = loaded.iter().find(|(path, _)| *path == place.path)?;
            let snapshot = buffer.read(cx).snapshot();
            let row = place.point.row.min(snapshot.max_point().row);
            let first = row.saturating_sub(LINES / 2);
            let last = (first + LINES - 1).min(snapshot.max_point().row);
            let range = Point::new(first, 0)..Point::new(last, snapshot.line_len(last));
            let mut code = String::new();
            let mut highlights = Vec::new();
            for chunk in snapshot.chunks(
                range,
                LanguageAwareStyling {
                    tree_sitter: true,
                    diagnostics: false,
                },
            ) {
                let start = code.len();
                code.push_str(chunk.text);
                if let Some(style) = chunk
                    .syntax_highlight_id
                    .and_then(|id| syntax.get(id).cloned())
                {
                    highlights.push((start..code.len(), style));
                }
            }
            let offset = snapshot.point_to_offset(Point::new(row, 0));
            let breadcrumb = snapshot
                .outline_items_containing(offset..offset, false, None)
                .into_iter()
                .map(|item| item.text.to_string())
                .collect::<Vec<_>>()
                .join(" › ");
            let name = place
                .path
                .path
                .file_name()
                .unwrap_or(place.path.path.as_unix_str())
                .to_owned();
            Some(Location {
                place: place.clone(),
                name: name.into(),
                breadcrumb: breadcrumb.into(),
                code: code.into(),
                highlights,
            })
        })
        .collect()
}

impl Render for RecentLocations {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let title = match (self.scratches, self.edited_only) {
            (false, true) => "Recently Edited Locations",
            (false, false) => "Recent Locations",
            (true, true) => "Recently Edited Scratch Files",
            (true, false) => "Scratch Files",
        };
        let (label, empty_text) = match self.scratches {
            true => ("Scratch Files", "No scratch files found"),
            false => ("Recent Locations", "No recent locations found"),
        };
        let shortcut = crate::shortcut_text(&navigate::RecentLocations, window);
        let header = h_flex()
            .flex_none()
            .h(px(32.))
            .px(px(12.))
            .gap(px(8.))
            .border_b_1()
            .border_color(tokens.popup_border)
            .child(div().font_weight(gpui::FontWeight::BOLD).child(title))
            .child(
                div()
                    .text_color(tokens.text_secondary)
                    .child(format!("({})", self.shown.len())),
            )
            .child(div().flex_1())
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
                shortcut.map(|shortcut| div().text_color(tokens.text_secondary).child(shortcut)),
            );
        let font = ThemeSettings::get_global(cx).buffer_font.clone();
        let selected_rows = self.rows.selected_range(&self.tree_rows(cx));
        let rows: Vec<gpui::AnyElement> = self
            .shown
            .iter()
            .enumerate()
            .filter_map(|(row, ix)| Some((row, self.list().get(*ix)?)))
            .map(|(row, location)| {
                let selected = selected_rows
                    .as_ref()
                    .is_some_and(|rows| rows.contains(&row));
                v_flex()
                    .id(("location", row))
                    .role(Role::ListBoxOption)
                    .aria_label(format!(
                        "{} {}",
                        location.name,
                        location.place.point.row + 1
                    ))
                    .aria_selected(selected)
                    .mx(px(popup::SELECTION_INSET))
                    .px(px(12. - popup::SELECTION_INSET))
                    .py(px(4.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .map(|item| match selected {
                        true => item.bg(tokens.selection),
                        false => item.hover(move |item| item.bg(tokens.selection_hover)),
                    })
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(spork_workbench::image(
                                spork_workbench::file_icon(&location.name),
                                cx,
                            ))
                            .child(location.name.clone())
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(tokens.text_secondary)
                                    .child(location.breadcrumb.clone()),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(22.))
                            .pt(px(2.))
                            .font(font.clone())
                            .text_ui_small()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .child(
                                StyledText::new(location.code.clone())
                                    .with_highlights(location.highlights.iter().cloned()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.rows.select(Some(row));
                        this.choose(window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let surface = tree_keys(
            popup::surface(cx)
                .id("recent-locations")
                .role(Role::Dialog)
                .aria_label(label)
                .w(px(WIDTH))
                .max_h(px(MAX_HEIGHT))
                .track_focus(&self.focus)
                .key_context("RecentLocations menu tree"),
            cx,
        );
        spork_vcs::spork_look(cx).child(
            surface
                .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| {
                    if this.filter.is_empty() {
                        cx.emit(DismissEvent);
                    } else {
                        this.filter.clear();
                        this.refilter(cx);
                    }
                }))
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
                    v_flex()
                        .id("locations")
                        .role(Role::List)
                        .aria_label(label)
                        .py(px(4.))
                        .gap(px(2.))
                        .track_scroll(self.rows.scroll_handle())
                        .overflow_y_scroll()
                        .when(empty, |list| {
                            list.child(
                                div()
                                    .id("empty")
                                    .role(Role::Label)
                                    .aria_label(empty_text)
                                    .p(px(12.))
                                    .text_color(tokens.text_secondary)
                                    .child(empty_text),
                            )
                        })
                        .children(rows),
                ),
        )
    }
}

impl TreeView for RecentLocations {
    type Key = usize;

    fn tree(&mut self) -> &mut TreeState<usize> {
        &mut self.rows
    }

    fn tree_rows(&self, _: &App) -> Vec<TreeRow<usize>> {
        (0..self.shown.len())
            .map(|row| TreeRow::item(row, SharedString::default()))
            .collect()
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(window, cx);
    }

    // its own speed search, filtering the places
    fn speed_search(&self) -> bool {
        false
    }
}
