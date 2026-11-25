//! IntelliJ's bookmarks over Zed's bookmark store (ROADMAP, phase 4): Toggle Bookmark (F3), the
//! mnemonic chooser (⌥F3, IntelliJ's `BookmarkTypeChooser`), mnemonics toggled and jumped to with
//! ⌃⇧digit and ⌃digit, Rename Bookmark… and Remove Mnemonic, the Bookmarks popup (⌘F3), Go to
//! Mnemonic (⌥⌘F3), the Bookmarks tool window (⌘2) and the gutter's bookmark and mnemonic icons.
//!
//! IntelliJ keeps bookmarks in lists; Zed has one, shown as the project's default list: Add
//! Bookmark to Another List and Create Bookmark List… are left out until the store has lists.

use std::ops::Range;
use std::sync::Arc;

use editor::Editor;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, AsyncApp, Context, DismissEvent, Entity,
    EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, Render, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, TaskExt as _, WeakEntity, Window, div, img, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use language::{Buffer, Point};
use picker::{MatchLocation, PreviewBackend, PreviewLayout, PreviewUpdate};
use project::bookmark_store::BookmarkStore;
use project::{Project, ProjectPath};
use spork_workbench::tree_keys::highlighted;
use spork_workbench::{Tokens, TreeRow, TreeState, TreeView, popup, tree_keys};
use theme::ActiveTheme as _;
use ui::{Clickable as _, Disableable as _};
use util::ResultExt as _;
use workspace::dock::{DockPosition, Panel, PanelEvent, PanelHandle};
use workspace::{ModalView, Workspace};

use crate::navigate;

pub mod bookmarks_panel {
    gpui::actions!(
        bookmarks_panel,
        [
            /// Opens the Bookmarks tool window and focuses it, or gives the focus back to the editor.
            ToggleFocus,
            /// Edit (F2): renames the selected bookmark, or the list.
            Rename,
            /// Delete (⌫): deletes the selected bookmarks, or the list's.
            Delete,
            /// Remove Mnemonic: takes the selected bookmarks' mnemonics off.
            DeleteType,
            /// Change Mnemonic… (⌥F3): the mnemonic chooser for the selected bookmark.
            ChooseType,
            /// Move Up (⌥⌘↑): the selected bookmark before the one above it.
            MoveUp,
            /// Move Down (⌥⌘↓): the selected bookmark after the one below it.
            MoveDown,
            /// Sort Bookmarks by Type & Name: the list's bookmarks by file and line.
            Sort,
            /// Open All Bookmarked Files (⌘↓, F4) on the list; a bookmark's file at its line.
            OpenBookmarkGroup,
            /// Mark List as Default, or Unmark List as Default.
            ToggleDefault,
        ]
    );
}

/// The mnemonics in IntelliJ's order (`BookmarkType`): the digits 1 to 9 and 0, then the letters.
const MNEMONICS: &str = "1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// Between a mnemonic and the description in Zed's bookmark label.
// shortcut: a bookmark's mnemonic is kept at the start of Zed's label ("A\u{1f}description"), as
// Zed's store and its saved rows have no field for it; give the store one when bookmark lists come
const SEPARATOR: char = '\u{1f}';

/// Between a bookmark's place in its list, once moved or sorted, and the rest of Zed's label.
// shortcut: the place rides in Zed's label too ("3\u{1e}A\u{1f}description"), for the same
// reason as the mnemonic
const ORDER: char = '\u{1e}';

/// A bookmark's place in its list, once Move Up, Move Down or Sort gave it one.
fn label_order(label: &str) -> Option<u32> {
    let (order, _) = label.split_once(ORDER)?;
    order.parse().ok()
}

/// `label` without its place in the list.
fn strip_order(label: &str) -> &str {
    match label.split_once(ORDER) {
        Some((order, rest)) if order.parse::<u32>().is_ok() => rest,
        _ => label,
    }
}

/// `label` at `order` in its list.
fn with_order(label: &str, order: Option<u32>) -> String {
    let label = strip_order(label);
    match order {
        Some(order) => format!("{order}{ORDER}{label}"),
        None => label.to_owned(),
    }
}

/// A bookmark's mnemonic and description, read from Zed's label.
pub(crate) fn read_label(label: &str) -> (Option<char>, &str) {
    let label = strip_order(label);
    let mut chars = label.chars();
    match (chars.next(), chars.next()) {
        (Some(mnemonic), Some(SEPARATOR)) if MNEMONICS.contains(mnemonic) => {
            (Some(mnemonic), chars.as_str())
        }
        _ => (None, label),
    }
}

/// Zed's label for a bookmark with this mnemonic and description.
pub(crate) fn write_label(mnemonic: Option<char>, description: &str) -> String {
    match mnemonic {
        Some(mnemonic) => format!("{mnemonic}{SEPARATOR}{description}"),
        None => description.to_owned(),
    }
}

/// A bookmark as the popups and the tool window show it.
#[derive(Clone)]
pub(crate) struct Mark {
    path: ProjectPath,
    buffer: Entity<Buffer>,
    anchor: language::Anchor,
    row: u32,
    mnemonic: Option<char>,
    description: SharedString,
    /// Its place in the list once moved or sorted; new ones come first, as IntelliJ adds them.
    order: Option<u32>,
}

impl Mark {
    fn file_name(&self) -> SharedString {
        self.path
            .path
            .file_name()
            .unwrap_or_default()
            .to_owned()
            .into()
    }

    /// Its folder, from the project's root, as IntelliJ writes a bookmark's location.
    fn location(&self) -> SharedString {
        self.path
            .path
            .parent()
            .map(|dir| dir.as_unix_str().to_owned())
            .unwrap_or_default()
            .into()
    }
}

/// Every bookmark of the project, in the store's order (by file, then as added).
async fn load_marks(store: Entity<BookmarkStore>, cx: &mut AsyncApp) -> Vec<Mark> {
    let Some(bookmarks) = BookmarkStore::all_bookmarks(&store, cx).await.log_err() else {
        return Vec::new();
    };
    let mut marks = cx.update(|cx| {
        bookmarks
            .into_iter()
            .filter_map(|bookmark| {
                let label = store.update(cx, |store, cx| {
                    store
                        .find_bookmark(&bookmark.buffer, bookmark.anchor, cx)
                        .map(|found| found.label.clone())
                })?;
                let buffer = bookmark.buffer.read(cx);
                let file = buffer.file()?;
                let path = ProjectPath {
                    worktree_id: file.worktree_id(cx),
                    path: file.path().clone(),
                };
                let row = bookmark.anchor.summary::<Point>(&buffer.snapshot()).row;
                let (mnemonic, description) = read_label(&label);
                Some(Mark {
                    path,
                    buffer: bookmark.buffer.clone(),
                    anchor: bookmark.anchor,
                    row,
                    mnemonic,
                    description: description.to_owned().into(),
                    order: label_order(&label),
                })
            })
            .collect::<Vec<_>>()
    });
    marks.sort_by_key(|mark| mark.order.map_or((0, 0), |order| (1, order)));
    marks
}

/// The line the active editor's caret is on, as the bookmark store names it, or a bookmark's
/// line in the tree.
struct CaretLine {
    /// What gets the focus back once a dialog for the line closes.
    focus: FocusHandle,
    buffer: Entity<Buffer>,
    anchor: language::Anchor,
}

fn caret_line(workspace: &Workspace, cx: &mut App) -> Option<CaretLine> {
    let editor = workspace.active_item_as::<Editor>(cx)?;
    let (buffer, anchor) = editor.update(cx, |editor, cx| {
        let head = editor
            .selections
            .newest::<Point>(&editor.display_snapshot(cx))
            .head();
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let (anchor, _) =
            snapshot.anchor_to_buffer_anchor(snapshot.anchor_before(Point::new(head.row, 0)))?;
        let buffer = editor.buffer().read(cx).buffer(anchor.buffer_id)?;
        Some((buffer, anchor))
    })?;
    Some(CaretLine {
        focus: editor.focus_handle(cx),
        buffer,
        anchor,
    })
}

fn store(workspace: &Workspace, cx: &App) -> Entity<BookmarkStore> {
    workspace.project().read(cx).bookmark_store()
}

/// The label of the bookmark on the caret's line, if there is one.
fn label_at(store: &Entity<BookmarkStore>, line: &CaretLine, cx: &mut App) -> Option<String> {
    store.update(cx, |store, cx| {
        store
            .find_bookmark(&line.buffer, line.anchor, cx)
            .map(|bookmark| bookmark.label.clone())
    })
}

/// Bookmarks `line` with `label`, or relabels its bookmark.
fn set_label(store: &Entity<BookmarkStore>, line: &CaretLine, label: String, cx: &mut App) {
    set_label_at(store, line.buffer.clone(), line.anchor, label, cx);
}

/// Bookmarks `anchor`'s line with `label`, or relabels its bookmark keeping its place in the list.
fn set_label_at(
    store: &Entity<BookmarkStore>,
    buffer: Entity<Buffer>,
    anchor: language::Anchor,
    label: String,
    cx: &mut App,
) {
    store.update(cx, |store, cx| {
        match store
            .find_bookmark(&buffer, anchor, cx)
            .map(|bookmark| label_order(&bookmark.label))
        {
            Some(order) => store.edit_bookmark(&buffer, anchor, with_order(&label, order), cx),
            None => store.toggle_bookmark(buffer, anchor, label, cx),
        }
    });
}

/// Gives `mnemonic` to the bookmark on `line` (adding it), taking it from any other bookmark, as
/// IntelliJ's `BookmarksManager.setType` does.
fn assign_mnemonic(
    workspace: &Workspace,
    line: CaretLine,
    mnemonic: char,
    description: String,
    cx: &mut App,
) {
    let store = store(workspace, cx);
    cx.spawn(async move |cx| {
        let marks = load_marks(store.clone(), cx).await;
        cx.update(|cx| {
            let here = line.buffer.read(cx).snapshot();
            let row = line.anchor.summary::<Point>(&here).row;
            for mark in marks.iter().filter(|mark| mark.mnemonic == Some(mnemonic)) {
                if mark.buffer == line.buffer && mark.row == row {
                    continue;
                }
                let label = write_label(None, &mark.description);
                set_label_at(&store, mark.buffer.clone(), mark.anchor, label, cx);
            }
            set_label(&store, &line, write_label(Some(mnemonic), &description), cx);
        });
    })
    .detach();
}

/// Whether the file at `abs_path` has a bookmark: IntelliJ's Select In ▸ Bookmarks offers it.
pub(crate) fn has_bookmarks(workspace: &Workspace, abs_path: &std::path::Path, cx: &App) -> bool {
    store(workspace, cx)
        .read(cx)
        .all_serialized_bookmarks(cx)
        .get(abs_path)
        .is_some_and(|rows| !rows.is_empty())
}

/// IntelliJ's Select In ▸ Bookmarks: shows the Bookmarks tool window with the file's first
/// bookmark selected.
pub(crate) fn select_in_bookmarks(
    workspace: &mut Workspace,
    path: ProjectPath,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(panel) = workspace.panel::<BookmarksPanel>(cx) {
        panel.read(cx).tree.clone().update(cx, |tree, cx| {
            if let Some(ix) = tree.marks.iter().position(|mark| mark.path == path) {
                tree.open = true;
                tree.state.select(Some(Row::Mark(ix)));
                cx.emit(TreeEvent::Selected);
                cx.notify();
            }
        });
    }
    crate::show_tool_window(workspace, BookmarksPanel::persistent_name(), window, cx);
}

/// Opens `mark`'s file with the caret at its line.
fn open_mark(
    workspace: &mut Workspace,
    mark: &Mark,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    crate::open_at(
        workspace,
        mark.path.clone(),
        Some(Point::new(mark.row, 0)),
        window,
        cx,
    );
}

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(
        |workspace, _: &navigate::ToggleBookmarkMnemonic, window, cx| {
            let Some(line) = caret_line(workspace, cx) else {
                return;
            };
            let store = store(workspace, cx);
            let label = label_at(&store, &line, cx);
            let (current, description) = match &label {
                Some(label) => {
                    let (mnemonic, description) = read_label(label);
                    (mnemonic, description.to_owned())
                }
                None => (None, String::new()),
            };
            let title = match (&label, current) {
                (None, _) => "Add Mnemonic Bookmark",
                (Some(_), None) => "Assign Mnemonic",
                (Some(_), Some(_)) => "Change Mnemonic",
            };
            let workspace_handle = cx.entity().downgrade();
            cx.spawn_in(window, async move |_, cx| {
                let assigned: Vec<char> = load_marks(store, cx)
                    .await
                    .into_iter()
                    .filter_map(|mark| mark.mnemonic)
                    .collect();
                workspace_handle.update_in(cx, |workspace, window, cx| {
                    let handle = cx.entity().downgrade();
                    workspace.toggle_modal(window, cx, move |window, cx| {
                        MnemonicChooser::new(
                            handle,
                            line,
                            title,
                            current,
                            assigned,
                            description,
                            window,
                            cx,
                        )
                    });
                })
            })
            .detach_and_log_err(cx);
        },
    );
    workspace.register_action(|workspace, action: &navigate::ToggleMnemonic, _, cx| {
        let Some(line) = caret_line(workspace, cx) else {
            return;
        };
        let store = store(workspace, cx);
        let label = label_at(&store, &line, cx);
        match label.as_deref().map(read_label) {
            // its own mnemonic toggles the bookmark off
            Some((Some(mnemonic), _)) if mnemonic == action.mnemonic => {
                store.update(cx, |store, cx| {
                    store.toggle_bookmark(line.buffer.clone(), line.anchor, String::new(), cx)
                });
            }
            found => {
                let description = found
                    .map(|(_, description)| description.to_owned())
                    .unwrap_or_default();
                assign_mnemonic(workspace, line, action.mnemonic, description, cx);
            }
        }
    });
    workspace.register_action(|workspace, action: &navigate::GoToBookmark, window, cx| {
        let store = store(workspace, cx);
        let mnemonic = action.mnemonic;
        cx.spawn_in(window, async move |workspace, cx| {
            let marks = load_marks(store, cx).await;
            let Some(mark) = marks
                .into_iter()
                .find(|mark| mark.mnemonic == Some(mnemonic))
            else {
                return Ok(());
            };
            workspace.update_in(cx, |workspace, window, cx| {
                open_mark(workspace, &mark, window, cx)
            })
        })
        .detach_and_log_err(cx);
    });
    workspace.register_action(|workspace, _: &navigate::RemoveMnemonic, _, cx| {
        let Some(line) = caret_line(workspace, cx) else {
            return;
        };
        let store = store(workspace, cx);
        if let Some(label) = label_at(&store, &line, cx) {
            let description = read_label(&label).1.to_owned();
            set_label(&store, &line, description, cx);
        }
    });
    workspace.register_action(|workspace, _: &navigate::RenameBookmark, window, cx| {
        let Some(line) = caret_line(workspace, cx) else {
            return;
        };
        let store = store(workspace, cx);
        let Some(label) = label_at(&store, &line, cx) else {
            return;
        };
        workspace.toggle_modal(window, cx, move |window, cx| {
            RenameBookmark::new(store, line, &label, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &navigate::ShowBookmarks, window, cx| {
        if workspace.active_modal::<BookmarksPopup>(cx).is_some() {
            workspace.hide_modal(window, cx);
            return;
        }
        let handle = cx.entity().downgrade();
        let project = workspace.project().clone();
        workspace.toggle_modal(window, cx, move |window, cx| {
            BookmarksPopup::new(handle, project, false, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &navigate::GoToMnemonic, window, cx| {
        if workspace.active_modal::<BookmarksPopup>(cx).is_some() {
            workspace.hide_modal(window, cx);
            return;
        }
        let handle = cx.entity().downgrade();
        let project = workspace.project().clone();
        workspace.toggle_modal(window, cx, move |window, cx| {
            BookmarksPopup::new(handle, project, true, window, cx)
        });
    });
    workspace.register_action(|workspace, _: &bookmarks_panel::ToggleFocus, window, cx| {
        workspace.toggle_panel_focus::<BookmarksPanel>(window, cx);
    });
}

/// Adds the Bookmarks tool window to `workspace`'s left dock.
pub(crate) fn add_panel(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let handle = cx.entity().downgrade();
    let project = workspace.project().clone();
    let panel = cx.new(|cx| BookmarksPanel::new(handle, project, window, cx));
    workspace.add_panel(panel, window, cx);
}

/// The gutter's icon for a bookmark (IntelliJ's `gutter/bookmark`, or its mnemonic in a box), set
/// as the editor's [`editor::BookmarkGutterIcon`].
pub(crate) fn gutter_icon(label: &str, cx: &mut App) -> AnyElement {
    let dark = !cx.theme().appearance().is_light();
    let suffix = if dark { "_dark" } else { "" };
    match read_label(label).0 {
        None => img(SharedString::from(format!(
            "wb/icons/gutter/bookmark{suffix}.svg"
        )))
        .size(px(14.))
        .flex_none()
        .into_any_element(),
        Some(mnemonic) => mnemonic_icon(mnemonic, px(14.), dark, cx.theme().colors().text),
    }
}

/// IntelliJ's mnemonic icon: the mnemonic in its box (`bookmarks/mnemonic`).
fn mnemonic_icon(mnemonic: char, size: Pixels, dark: bool, text: gpui::Hsla) -> AnyElement {
    let suffix = if dark { "_dark" } else { "" };
    div()
        .relative()
        .size(size)
        .flex_none()
        .child(
            img(SharedString::from(format!(
                "wb/icons/gutter/mnemonic{suffix}.svg"
            )))
            .absolute()
            .size(size),
        )
        .child(
            div()
                .absolute()
                .size(size)
                .flex()
                .items_center()
                .justify_center()
                .text_size(size * 0.72)
                .line_height(size)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text)
                .child(mnemonic.to_string()),
        )
        .into_any_element()
}

/// The icon a bookmark's row shows: the bookmark, or its mnemonic.
fn mark_icon(mark: &Mark, cx: &App) -> AnyElement {
    match mark.mnemonic {
        Some(mnemonic) => mnemonic_icon(
            mnemonic,
            px(16.),
            !cx.theme().appearance().is_light(),
            Tokens::get(cx).text,
        ),
        None => spork_workbench::image("bookmarks/bookmark", cx).into_any_element(),
    }
}

/// A bookmark's row text, as IntelliJ's bookmark node: its description, else the file's name and
/// line, then where the file is, grey.
fn mark_text(
    mark: &Mark,
    selected: bool,
    found: Option<Vec<Range<usize>>>,
    cx: &App,
) -> AnyElement {
    let tokens = Tokens::get(cx);
    let grey = match selected {
        true => tokens.text,
        false => tokens.text_secondary,
    };
    let name = mark.file_name();
    let line = format!(" :{}", mark.row + 1);
    let location = mark.location();
    let row = h_flex().min_w_0().truncate();
    match mark.description.is_empty() {
        true => row
            .child(div().flex_none().child(highlighted(name, found, cx)))
            .child(div().flex_none().text_color(grey).child(line))
            .when(!location.is_empty(), |row| {
                row.child(div().pl(px(8.)).text_color(grey).truncate().child(location))
            }),
        false => row
            .child(div().flex_none().pr(px(8.)).child(highlighted(
                mark.description.clone(),
                found,
                cx,
            )))
            .child(div().flex_none().text_color(grey).child(name))
            .child(div().flex_none().text_color(grey).child(line)),
    }
    .into_any_element()
}

/// What a bookmark row says to assistive technology.
fn mark_label(mark: &Mark) -> SharedString {
    let mut label = format!("{}:{}", mark.file_name(), mark.row + 1);
    if let Some(mnemonic) = mark.mnemonic {
        label = format!("{mnemonic} {label}");
    }
    if !mark.description.is_empty() {
        label = format!("{} {label}", mark.description);
    }
    label.into()
}

/// IntelliJ's mnemonic chooser (`BookmarkTypeChooser`): the digits and letters, the mnemonics in
/// use in the warning colour and the bookmark's own in the accent; typing or double-clicking a
/// mnemonic sets it, ⏎ sets the one selected with the description.
pub(crate) struct MnemonicChooser {
    workspace: WeakEntity<Workspace>,
    line: Option<CaretLine>,
    title: &'static str,
    selected: Option<char>,
    assigned: Vec<char>,
    description: Entity<InputState>,
    focus: FocusHandle,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for MnemonicChooser {}

impl ModalView for MnemonicChooser {}

impl Focusable for MnemonicChooser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl MnemonicChooser {
    #[allow(clippy::too_many_arguments)]
    fn new(
        workspace: WeakEntity<Workspace>,
        line: CaretLine,
        title: &'static str,
        current: Option<char>,
        assigned: Vec<char>,
        description: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Description (Optional)");
            state.set_value(description, window, cx);
            state
        });
        let subscription = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.save(window, cx);
            }
        });
        Self {
            workspace,
            line: Some(line),
            title,
            selected: current,
            assigned,
            description: field,
            focus: cx.focus_handle(),
            _subscription: subscription,
        }
    }

    fn choose(&mut self, mnemonic: char, window: &mut Window, cx: &mut Context<Self>) {
        // IntelliJ's grid: choosing the one selected saves it
        if self.selected == Some(mnemonic) {
            self.save(window, cx);
            return;
        }
        self.selected = Some(mnemonic);
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mnemonic) = self.selected else {
            return;
        };
        let Some(line) = self.line.take() else {
            return;
        };
        let description = self.description.read(cx).value().to_string();
        let focus = line.focus.clone();
        self.workspace
            .update(cx, |workspace, cx| {
                assign_mnemonic(workspace, line, mnemonic, description, cx)
            })
            .log_err();
        cx.emit(DismissEvent);
        window.focus(&focus, cx);
    }

    fn step(&mut self, columns_step: isize, cx: &mut Context<Self>) {
        let order: Vec<char> = MNEMONICS.chars().collect();
        let ix = self
            .selected
            .and_then(|selected| order.iter().position(|c| *c == selected))
            .map_or(0, |ix| ix as isize + columns_step);
        self.selected = order
            .get(ix.rem_euclid(order.len() as isize) as usize)
            .copied();
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.description.focus_handle(cx).is_focused(window) {
            return;
        }
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        match keystroke.key.as_str() {
            "left" => self.step(-1, cx),
            "right" => self.step(1, cx),
            "enter" => self.save(window, cx),
            "escape" => cx.emit(DismissEvent),
            key => {
                let mut chars = key.chars();
                if let (Some(typed), None) = (chars.next(), chars.next()) {
                    let typed = typed.to_ascii_uppercase();
                    if MNEMONICS.contains(typed) {
                        cx.stop_propagation();
                        self.selected = Some(typed);
                        self.save(window, cx);
                    }
                }
            }
        }
    }

    fn cell(&self, mnemonic: char, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let (fill, border, text) = if self.selected == Some(mnemonic) {
            (tokens.accent, tokens.accent, tokens.icon_over_accent)
        } else if self.assigned.contains(&mnemonic) {
            (
                tokens.mnemonic_assigned,
                tokens.mnemonic_assigned,
                tokens.icon_over_accent,
            )
        } else {
            (tokens.popup, tokens.control_border, tokens.text)
        };
        div()
            .id(SharedString::from(format!("mnemonic-{mnemonic}")))
            .role(Role::Button)
            .aria_label(mnemonic.to_string())
            .aria_selected(self.selected == Some(mnemonic))
            .w(px(28.))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .border_1()
            .border_color(border)
            .bg(fill)
            .text_color(text)
            .cursor_pointer()
            .child(mnemonic.to_string())
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    if event.click_count() >= 2 {
                        this.selected = Some(mnemonic);
                        this.save(window, cx);
                    } else {
                        this.choose(mnemonic, window, cx);
                    }
                }),
            )
            .into_any_element()
    }

    /// A block of cells, `columns` to a row, the last row centered as IntelliJ's `RowGridLayout`.
    fn grid(&self, mnemonics: &str, columns: usize, cx: &mut Context<Self>) -> AnyElement {
        let chars: Vec<char> = mnemonics.chars().collect();
        v_flex()
            .gap(px(4.))
            .children(chars.chunks(columns).map(|row| {
                h_flex()
                    .justify_center()
                    .gap(px(4.))
                    .children(row.iter().map(|mnemonic| self.cell(*mnemonic, cx)))
            }))
            .into_any_element()
    }
}

impl Render for MnemonicChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let legend = |color, text: &'static str| {
            h_flex()
                .gap(px(6.))
                .child(div().size(px(8.)).rounded_full().bg(color))
                .child(text)
        };
        let bullet = |lines: &[&'static str]| {
            h_flex()
                .items_start()
                .gap(px(6.))
                .child("•")
                .child(v_flex().children(lines.iter().copied()))
        };
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("mnemonic-chooser")
                .role(Role::Dialog)
                .aria_label(self.title)
                .w(px(370.))
                .track_focus(&self.focus)
                .key_context("MnemonicChooser")
                .on_key_down(cx.listener(Self::key_down))
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(spork_workbench::popup::title(self.title, cx))
                .child(
                    v_flex()
                        .px(px(20.))
                        .pb(px(14.))
                        .gap(px(16.))
                        .child(
                            v_flex()
                                .px(px(4.))
                                .pt(px(2.))
                                .text_color(tokens.text_secondary)
                                .child(bullet(&["Quickly set: type or double-click mnemonic"]))
                                .child(bullet(&[
                                    "Provide note: select mnemonic, add description,",
                                    "and press Enter",
                                ])),
                        )
                        .child(
                            h_flex()
                                .justify_center()
                                .items_start()
                                .gap(px(14.))
                                .child(self.grid(&MNEMONICS[..10], 3, cx))
                                .child(self.grid(&MNEMONICS[10..], 7, cx)),
                        )
                        .child(
                            spork_workbench::TextField::new(&self.description)
                                .aria_label("Description (Optional)"),
                        )
                        .child(
                            h_flex()
                                .gap(px(16.))
                                .child(legend(tokens.mnemonic_assigned, "Already used"))
                                .child(legend(tokens.accent, "Current")),
                        ),
                ),
        )
    }
}

/// Rename Bookmark…: IntelliJ's input dialog for the bookmark's description.
pub(crate) struct RenameBookmark {
    store: Entity<BookmarkStore>,
    line: CaretLine,
    mnemonic: Option<char>,
    field: Entity<InputState>,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for RenameBookmark {}

impl ModalView for RenameBookmark {}

impl Focusable for RenameBookmark {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl RenameBookmark {
    fn new(
        store: Entity<BookmarkStore>,
        line: CaretLine,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (mnemonic, description) = read_label(label);
        let description = description.to_owned();
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(description, window, cx);
            state
        });
        let subscription = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.confirm(window, cx);
            }
        });
        Self {
            store,
            line,
            mnemonic,
            field,
            _subscription: subscription,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let description = self.field.read(cx).value().to_string();
        set_label(
            &self.store,
            &self.line,
            write_label(self.mnemonic, &description),
            cx,
        );
        cx.emit(DismissEvent);
        window.focus(&self.line.focus, cx);
    }
}

impl Render for RenameBookmark {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("rename-bookmark")
                .role(Role::Dialog)
                .aria_label("Bookmark Description")
                .w(px(400.))
                .key_context("RenameBookmark")
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(spork_workbench::popup::title("Bookmark Description", cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(8.))
                        .child("Enter a short bookmark description")
                        .child(
                            spork_workbench::TextField::new(&self.field)
                                .aria_label("Enter a short bookmark description"),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    spork_workbench::Button::new("cancel", "Cancel")
                                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                                )
                                .child(
                                    spork_workbench::Button::new("ok", "OK")
                                        .primary(true)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(window, cx)
                                        })),
                                ),
                        ),
                ),
        )
    }
}

/// Rename Bookmark List…: IntelliJ's `GroupRenameDialog`, the list's name and whether it is the
/// default list.
pub(crate) struct RenameList {
    tree: WeakEntity<BookmarksTree>,
    field: Entity<InputState>,
    default: bool,
    _subscription: Subscription,
}

impl EventEmitter<DismissEvent> for RenameList {}

impl ModalView for RenameList {}

impl Focusable for RenameList {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl RenameList {
    fn new(
        tree: WeakEntity<BookmarksTree>,
        name: &str,
        default: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = name.to_owned();
        let field = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(name, window, cx);
            state
        });
        let subscription = cx.subscribe_in(&field, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.confirm(window, cx);
            }
        });
        Self {
            tree,
            field,
            default,
            _subscription: subscription,
        }
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.field.read(cx).value().trim().to_owned();
        if name.is_empty() {
            return;
        }
        let default = self.default;
        cx.emit(DismissEvent);
        self.tree
            .update(cx, |tree, cx| {
                tree.set_list(name, default, cx);
                window.focus(&tree.focus, cx);
            })
            .log_err();
    }
}

impl Render for RenameList {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("rename-bookmark-list")
                .role(Role::Dialog)
                .aria_label("Rename Bookmark List")
                .w(px(400.))
                .key_context("RenameBookmarkList")
                .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
                .child(spork_workbench::popup::title("Rename Bookmark List", cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(5.))
                        .child("Bookmark list:")
                        .child(
                            spork_workbench::TextField::new(&self.field)
                                .aria_label("Bookmark list:"),
                        )
                        .child(
                            spork_workbench::controls::labeled_check(
                                "default-list",
                                "Use as default list",
                                self.default,
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.default = !this.default;
                                cx.notify();
                            })),
                        )
                        .child(
                            h_flex()
                                .pt(px(8.))
                                .justify_end()
                                .gap(px(8.))
                                .child(
                                    spork_workbench::Button::new("cancel", "Cancel")
                                        .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                                )
                                .child(
                                    spork_workbench::Button::new("rename", "Rename")
                                        .primary(true)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm(window, cx)
                                        })),
                                ),
                        ),
                ),
        )
    }
}

/// IntelliJ's confirmation before deleting several bookmarks or a list of them
/// (`NodeDeleteAction.askBeforeDeleting`), with its Delete and Cancel.
pub(crate) struct ConfirmDelete {
    title: &'static str,
    message: SharedString,
    tree: WeakEntity<BookmarksTree>,
    marks: Vec<Mark>,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for ConfirmDelete {}

impl ModalView for ConfirmDelete {}

impl Focusable for ConfirmDelete {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ConfirmDelete {
    fn answer(&mut self, delete: bool, window: &mut Window, cx: &mut Context<Self>) {
        let marks = std::mem::take(&mut self.marks);
        cx.emit(DismissEvent);
        self.tree
            .update(cx, |tree, cx| {
                if delete {
                    tree.remove(marks, cx);
                }
                window.focus(&tree.focus, cx);
            })
            .log_err();
    }
}

impl Render for ConfirmDelete {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        spork_vcs::spork_look(cx).child(
            popup::surface(cx)
                .id("confirm-delete-bookmarks")
                .role(Role::Dialog)
                .aria_label(self.title)
                .w(px(400.))
                .track_focus(&self.focus)
                .key_context("ConfirmDeleteBookmarks menu")
                .on_action(
                    cx.listener(|this, _: &menu::Confirm, window, cx| {
                        this.answer(true, window, cx)
                    }),
                )
                .on_action(
                    cx.listener(|this, _: &menu::Cancel, window, cx| {
                        this.answer(false, window, cx)
                    }),
                )
                .child(spork_workbench::popup::title(self.title, cx))
                .child(
                    v_flex()
                        .px(px(16.))
                        .pb(px(14.))
                        .gap(px(12.))
                        .child(
                            div()
                                .id("message")
                                .role(Role::Label)
                                .aria_label(self.message.clone())
                                .child(self.message.clone()),
                        )
                        .child(
                            h_flex()
                                .justify_end()
                                .gap(px(8.))
                                .child(spork_workbench::Button::new("cancel", "Cancel").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.answer(false, window, cx)
                                    }),
                                ))
                                .child(
                                    spork_workbench::Button::new("delete", "Delete")
                                        .primary(true)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.answer(true, window, cx)
                                        })),
                                ),
                        ),
                ),
        )
    }
}

/// A row of the bookmarks tree: the default list, or one of its bookmarks.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Row {
    List,
    Mark(usize),
}

/// The bookmarks tree the popups and the tool window show: the project's list and its bookmarks,
/// with IntelliJ's tree keys and speed search.
pub(crate) struct BookmarksTree {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    marks: Vec<Mark>,
    /// Go to Mnemonic's: the bookmarks with a mnemonic, without their list.
    mnemonics_only: bool,
    open: bool,
    state: TreeState<Row>,
    focus: FocusHandle,
    /// The list's name and whether it is the default, as Rename Bookmark List… and Mark List as
    /// Default leave them.
    list: ListInfo,
    context_menu: Option<(Entity<ui::ContextMenu>, gpui::Point<Pixels>, Subscription)>,
    loaded: bool,
    _load: Task<()>,
    _subscription: Subscription,
}

/// What the project's one list keeps beyond its bookmarks.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct ListInfo {
    name: Option<String>,
    default: bool,
}

/// Where the project's list is kept: the key-value store, by the project's first folder.
fn list_key(project: &Entity<Project>, cx: &App) -> Option<String> {
    let root = project.read(cx).visible_worktrees(cx).next()?;
    Some(format!(
        "spork_bookmarks_list_{}",
        root.read(cx).abs_path().display()
    ))
}

fn read_list(project: &Entity<Project>, cx: &App) -> ListInfo {
    list_key(project, cx)
        .and_then(|key| {
            db::kvp::KeyValueStore::global(cx)
                .read_kvp(&key)
                .log_err()
                .flatten()
        })
        .and_then(|value| serde_json::from_str(&value).log_err())
        .unwrap_or_default()
}

fn write_list(project: &Entity<Project>, list: &ListInfo, cx: &mut App) {
    let Some(key) = list_key(project, cx) else {
        return;
    };
    let value = serde_json::to_string(list);
    let store = db::kvp::KeyValueStore::global(cx);
    cx.background_spawn(async move { store.write_kvp(key, value?).await })
        .detach_and_log_err(cx);
}

pub(crate) enum TreeEvent {
    Chose,
    Selected,
}

impl EventEmitter<TreeEvent> for BookmarksTree {}

impl Focusable for BookmarksTree {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl BookmarksTree {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        mnemonics_only: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let store = project.read(cx).bookmark_store();
        let subscription = cx.observe(&store, |this, _, cx| this.reload(cx));
        let list = read_list(&project, cx);
        let mut this = Self {
            workspace,
            project,
            marks: Vec::new(),
            mnemonics_only,
            open: true,
            state: TreeState::new(),
            focus: cx.focus_handle(),
            list,
            context_menu: None,
            loaded: false,
            _load: Task::ready(()),
            _subscription: subscription,
        };
        this.reload(cx);
        this
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let store = self.project.read(cx).bookmark_store();
        self._load = cx.spawn(async move |this, cx| {
            let marks = load_marks(store, cx).await;
            this.update(cx, |this, cx| {
                this.marks = match this.mnemonics_only {
                    true => marks.into_iter().filter(|m| m.mnemonic.is_some()).collect(),
                    false => marks,
                };
                this.loaded = true;
                let valid = match this.state.lead() {
                    Some(Row::Mark(ix)) => *ix < this.marks.len(),
                    Some(Row::List) => !this.mnemonics_only,
                    None => false,
                };
                if !valid {
                    let first = this
                        .rows()
                        .into_iter()
                        .find(|row| matches!(row, Row::Mark(_)));
                    this.state.select(first);
                }
                cx.emit(TreeEvent::Selected);
                cx.notify();
            })
            .ok();
        });
    }

    /// The list's name: the project's, as IntelliJ names the first list, until renamed.
    fn list_name(&self, cx: &App) -> SharedString {
        if let Some(name) = &self.list.name {
            return name.clone().into();
        }
        self.project
            .read(cx)
            .visible_worktrees(cx)
            .next()
            .map(|worktree| worktree.read(cx).root_name_str().to_owned())
            .unwrap_or_else(|| "Bookmarks".to_owned())
            .into()
    }

    fn rows(&self) -> Vec<Row> {
        // no bookmarks, no list: IntelliJ's empty text shows
        if self.marks.is_empty() {
            return Vec::new();
        }
        let marks = (0..self.marks.len()).map(Row::Mark);
        match (self.mnemonics_only, self.open) {
            (true, _) => marks.collect(),
            (false, true) => std::iter::once(Row::List).chain(marks).collect(),
            (false, false) => vec![Row::List],
        }
    }

    fn selected_mark(&self) -> Option<&Mark> {
        match self.state.lead()? {
            Row::Mark(ix) => self.marks.get(*ix),
            Row::List => None,
        }
    }

    fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.open = open;
        if !open && matches!(self.state.lead(), Some(Row::Mark(_))) {
            self.state.select(Some(Row::List));
        }
        cx.notify();
    }

    /// Opens the selected bookmark's file at its line.
    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.lead().copied() {
            Some(Row::List) => self.set_open(!self.open, cx),
            Some(Row::Mark(ix)) => {
                let Some(mark) = self.marks.get(ix).cloned() else {
                    return;
                };
                cx.emit(TreeEvent::Chose);
                self.workspace
                    .update(cx, |workspace, cx| open_mark(workspace, &mark, window, cx))
                    .log_err();
            }
            None => {}
        }
    }

    /// Jumps to the bookmark with the typed mnemonic, as IntelliJ's bookmark trees do.
    fn jump_to_mnemonic(
        &mut self,
        typed: char,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let typed = typed.to_ascii_uppercase();
        let Some(ix) = self
            .marks
            .iter()
            .position(|mark| mark.mnemonic == Some(typed))
        else {
            return false;
        };
        self.state.select(Some(Row::Mark(ix)));
        self.open_selected(window, cx);
        true
    }

    /// The bookmarks selected.
    fn selected_marks(&self, cx: &App) -> Vec<Mark> {
        let rows = self.tree_rows(cx);
        self.state
            .selected(&rows)
            .into_iter()
            .filter_map(|row| match row {
                Row::Mark(ix) => self.marks.get(ix).cloned(),
                Row::List => None,
            })
            .collect()
    }

    fn list_selected(&self) -> bool {
        self.state.lead() == Some(&Row::List)
    }

    fn remove(&mut self, marks: Vec<Mark>, cx: &mut Context<Self>) {
        // the list goes with its last bookmark, and comes back under the project's name
        if marks.len() == self.marks.len() {
            self.list = ListInfo::default();
            write_list(&self.project, &self.list, cx);
        }
        let store = self.project.read(cx).bookmark_store();
        store.update(cx, |store, cx| {
            for mark in marks {
                store.toggle_bookmark(mark.buffer.clone(), mark.anchor, String::new(), cx)
            }
        });
    }

    /// Delete (⌫, ⌦, ⌘⌫), IntelliJ's `NodeDeleteAction`: the selected bookmarks, or the list's
    /// (asking first, as Ask Before Deleting Multiple Bookmarks does, when it holds more than one).
    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (marks, title, message) = match self.list_selected() {
            true => (
                self.marks.clone(),
                "Delete Bookmark List",
                format!(
                    "Are you sure you want to delete '{}' bookmark list? This action can't be undone.",
                    self.list_name(cx)
                ),
            ),
            false => (
                self.selected_marks(cx),
                "Delete Bookmark Nodes",
                "Are you sure you want to delete the selected bookmark nodes? This action can't be undone."
                    .to_owned(),
            ),
        };
        if marks.len() < 2 {
            self.remove(marks, cx);
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let tree = cx.entity().downgrade();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |_, cx| ConfirmDelete {
                title,
                message: message.into(),
                tree,
                marks,
                focus: cx.focus_handle(),
            })
        });
    }

    /// The selected bookmark's line, for its dialogs.
    fn mark_line(&self, mark: &Mark) -> CaretLine {
        CaretLine {
            focus: self.focus.clone(),
            buffer: mark.buffer.clone(),
            anchor: mark.anchor,
        }
    }

    /// Edit (F2): Rename Bookmark… for a bookmark, Rename Bookmark List… for the list.
    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let store = self.project.read(cx).bookmark_store();
        if self.list_selected() {
            let tree = cx.entity().downgrade();
            let (name, default) = (self.list_name(cx), self.list.default);
            workspace.update(cx, |workspace, cx| {
                workspace.toggle_modal(window, cx, move |window, cx| {
                    RenameList::new(tree, &name, default, window, cx)
                })
            });
            return;
        }
        let Some(mark) = self.selected_mark().cloned() else {
            return;
        };
        let line = self.mark_line(&mark);
        let label = write_label(mark.mnemonic, &mark.description);
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |window, cx| {
                RenameBookmark::new(store, line, &label, window, cx)
            });
        });
    }

    /// Remove Mnemonic: the selected bookmarks keep their descriptions.
    fn remove_mnemonics(&mut self, cx: &mut Context<Self>) {
        let store = self.project.read(cx).bookmark_store();
        for mark in self.selected_marks(cx) {
            let label = write_label(None, &mark.description);
            set_label_at(&store, mark.buffer.clone(), mark.anchor, label, cx);
        }
    }

    /// Change Mnemonic… (Assign Mnemonic… for a bookmark without one): the chooser.
    fn choose_type(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mark) = self.selected_mark().cloned() else {
            return;
        };
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let line = self.mark_line(&mark);
        let title = match mark.mnemonic {
            Some(_) => "Change Mnemonic",
            None => "Assign Mnemonic",
        };
        let assigned: Vec<char> = self.marks.iter().filter_map(|mark| mark.mnemonic).collect();
        let handle = self.workspace.clone();
        let description = mark.description.to_string();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |window, cx| {
                MnemonicChooser::new(
                    handle,
                    line,
                    title,
                    mark.mnemonic,
                    assigned,
                    description,
                    window,
                    cx,
                )
            })
        });
    }

    /// Whether the selected bookmark can go up (or down) past its neighbour, IntelliJ's
    /// `NodeMoveAction`: one bookmark selected, and another above (or below) it.
    fn can_move(&self, up: bool) -> bool {
        match self.state.lead() {
            Some(Row::Mark(ix)) if self.state.selected(&self.rows_only()).len() == 1 => match up {
                true => *ix > 0,
                false => ix + 1 < self.marks.len(),
            },
            _ => false,
        }
    }

    /// The rows as keys, without their texts.
    fn rows_only(&self) -> Vec<TreeRow<Row>> {
        self.rows()
            .into_iter()
            .map(|row| TreeRow::item(row, SharedString::default()))
            .collect()
    }

    /// Writes each bookmark's place in the list, in `order`.
    fn reorder(&mut self, order: Vec<Mark>, cx: &mut Context<Self>) {
        let store = self.project.read(cx).bookmark_store();
        store.update(cx, |store, cx| {
            for (place, mark) in order.iter().enumerate() {
                if mark.order == Some(place as u32) {
                    continue;
                }
                let label = write_label(mark.mnemonic, &mark.description);
                store.edit_bookmark(
                    &mark.buffer,
                    mark.anchor,
                    with_order(&label, Some(place as u32)),
                    cx,
                );
            }
        });
    }

    /// Move Up and Move Down: the selected bookmark past its neighbour, the selection with it.
    fn move_selected(&mut self, up: bool, cx: &mut Context<Self>) {
        if !self.can_move(up) {
            return;
        }
        let Some(Row::Mark(ix)) = self.state.lead().copied() else {
            return;
        };
        let other = if up { ix - 1 } else { ix + 1 };
        let mut order = self.marks.clone();
        order.swap(ix, other);
        self.state.select(Some(Row::Mark(other)));
        self.reorder(order, cx);
    }

    /// Sort Bookmarks by Type & Name: by file, then line, as IntelliJ's line bookmarks compare.
    fn sort(&mut self, cx: &mut Context<Self>) {
        let mut order = self.marks.clone();
        order.sort_by(|a, b| {
            (a.path.path.as_unix_str(), a.row).cmp(&(b.path.path.as_unix_str(), b.row))
        });
        self.reorder(order, cx);
    }

    /// Open All Bookmarked Files (⌘↓, F4) on the list; on a bookmark, its file at its line.
    fn open_group(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let marks = match self.list_selected() {
            true => self.marks.clone(),
            false => self.selected_marks(cx),
        };
        if marks.is_empty() {
            return;
        }
        cx.emit(TreeEvent::Chose);
        self.workspace
            .update(cx, |workspace, cx| {
                for mark in &marks {
                    open_mark(workspace, mark, window, cx);
                }
            })
            .log_err();
    }

    /// Mark List as Default, or Unmark it.
    fn toggle_default(&mut self, cx: &mut Context<Self>) {
        self.list.default = !self.list.default;
        write_list(&self.project, &self.list, cx);
        cx.notify();
    }

    fn set_list(&mut self, name: String, default: bool, cx: &mut Context<Self>) {
        let project_name = {
            self.list.name = None;
            self.list_name(cx)
        };
        self.list = ListInfo {
            name: (name != project_name.as_ref()).then_some(name),
            default,
        };
        write_list(&self.project, &self.list, cx);
        cx.notify();
    }

    /// IntelliJ's `Bookmarks.ToolWindow.PopupMenu` for the selection, at `position`.
    fn deploy_menu(
        &mut self,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = cx.entity().downgrade();
        let list = self.list_selected();
        let marks = self.selected_marks(cx);
        let single = marks.len() == 1;
        let has_mnemonic = !marks.is_empty() && marks.iter().all(|mark| mark.mnemonic.is_some());
        let no_mnemonic = single && marks.iter().all(|mark| mark.mnemonic.is_none());
        let (up, down) = (self.can_move(true), self.can_move(false));
        let default = self.list.default;
        let focus = self.focus.clone();
        let menu = ui::ContextMenu::build(window, cx, move |menu, _, _| {
            let run = |f: fn(&mut BookmarksTree, &mut Window, &mut Context<BookmarksTree>)| {
                let tree = tree.clone();
                move |window: &mut Window, cx: &mut App| {
                    tree.update(cx, |tree, cx| f(tree, window, cx)).log_err();
                }
            };
            let menu = menu.context(focus.clone());
            match list {
                true => menu
                    .entry(
                        match default {
                            true => "Unmark List as Default",
                            false => "Mark List as Default",
                        },
                        Some(Box::new(bookmarks_panel::ToggleDefault)),
                        run(|tree, _, cx| tree.toggle_default(cx)),
                    )
                    .entry(
                        "Rename Bookmark List…",
                        Some(Box::new(bookmarks_panel::Rename)),
                        run(|tree, window, cx| tree.rename(window, cx)),
                    )
                    .entry(
                        "Delete Bookmark List",
                        Some(Box::new(bookmarks_panel::Delete)),
                        run(|tree, window, cx| tree.delete_selected(window, cx)),
                    )
                    .entry(
                        "Sort Bookmarks by Type & Name",
                        Some(Box::new(bookmarks_panel::Sort)),
                        run(|tree, _, cx| tree.sort(cx)),
                    )
                    .entry(
                        "Open All Bookmarked Files",
                        Some(Box::new(bookmarks_panel::OpenBookmarkGroup)),
                        run(|tree, window, cx| tree.open_group(window, cx)),
                    ),
                false => menu
                    .when(single, |menu| {
                        menu.entry(
                            "Rename Bookmark…",
                            Some(Box::new(bookmarks_panel::Rename)),
                            run(|tree, window, cx| tree.rename(window, cx)),
                        )
                    })
                    .entry(
                        "Delete Bookmark",
                        Some(Box::new(bookmarks_panel::Delete)),
                        run(|tree, window, cx| tree.delete_selected(window, cx)),
                    )
                    .when(has_mnemonic, |menu| {
                        menu.entry(
                            "Remove Mnemonic",
                            Some(Box::new(bookmarks_panel::DeleteType)),
                            run(|tree, _, cx| tree.remove_mnemonics(cx)),
                        )
                    })
                    .when(single, |menu| {
                        menu.entry(
                            match no_mnemonic {
                                true => "Assign Mnemonic…",
                                false => "Change Mnemonic…",
                            },
                            Some(Box::new(bookmarks_panel::ChooseType)),
                            run(|tree, window, cx| tree.choose_type(window, cx)),
                        )
                    })
                    // IntelliJ shows both while either can move, the other greyed
                    .when(up || down, |menu| {
                        menu.item(
                            ui::ContextMenuEntry::new("Move Up")
                                .action(Box::new(bookmarks_panel::MoveUp))
                                .disabled(!up)
                                .handler(run(|tree, _, cx| tree.move_selected(true, cx))),
                        )
                        .item(
                            ui::ContextMenuEntry::new("Move Down")
                                .action(Box::new(bookmarks_panel::MoveDown))
                                .disabled(!down)
                                .handler(run(|tree, _, cx| tree.move_selected(false, cx))),
                        )
                    })
                    .entry(
                        "Sort Bookmarks by Type & Name",
                        Some(Box::new(bookmarks_panel::Sort)),
                        run(|tree, _, cx| tree.sort(cx)),
                    ),
            }
        });
        window.focus(&menu.focus_handle(cx), cx);
        let subscription =
            cx.subscribe_in(&menu, window, |this, menu, _: &DismissEvent, window, cx| {
                // the focus comes back unless what the menu ran took it (a dialog)
                let had_focus = menu.focus_handle(cx).contains_focused(window, cx);
                if this.context_menu.take().is_some() && had_focus {
                    window.focus(&this.focus, cx);
                }
                cx.notify();
            });
        self.context_menu = Some((menu, position, subscription));
        cx.notify();
    }

    /// A row's text, as speed search reads it: the list's name, a bookmark's description or else
    /// its file's name.
    fn row_text(&self, row: Row, cx: &App) -> SharedString {
        match row {
            Row::List => self.list_name(cx),
            Row::Mark(ix) => match self.marks.get(ix) {
                Some(mark) if mark.description.is_empty() => mark.file_name(),
                Some(mark) => mark.description.clone(),
                None => SharedString::default(),
            },
        }
    }

    fn render_rows(
        &self,
        rows: &[TreeRow<Row>],
        focused: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let tokens = Tokens::get(cx);
        let selected_rows = self.state.selected_range(rows);
        rows.iter()
            .enumerate()
            .map(|(ix, tree_row)| {
                let row = tree_row.key;
                let selected = selected_rows
                    .as_ref()
                    .is_some_and(|range| range.contains(&ix));
                let found = self.state.matches(&tree_row.text);
                let base = tree_row_element(
                    ("bookmark-row", ix),
                    selected,
                    focused,
                    tree_row.depth as f32,
                    cx,
                );
                let base = match row {
                    Row::List => base
                        .aria_label(tree_row.text.clone())
                        .aria_expanded(self.open)
                        .child(
                            div()
                                .id("chevron")
                                .flex_none()
                                .text_color(tokens.icon)
                                .child(
                                    spork_workbench::icon(match self.open {
                                        true => "general/chevronDown",
                                        false => "general/chevronRight",
                                    })
                                    .size(px(16.)),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.set_open(!this.open, cx);
                                })),
                        )
                        .child(spork_workbench::image("bookmarks/bookmarksList", cx))
                        .child(highlighted(tree_row.text.clone(), found, cx)),
                    Row::Mark(mark_ix) => {
                        let Some(mark) = self.marks.get(mark_ix) else {
                            return div().into_any_element();
                        };
                        base.aria_label(mark_label(mark))
                            .when(!self.mnemonics_only, |row| {
                                row.child(div().w(px(16.)).flex_none())
                            })
                            .child(mark_icon(mark, cx))
                            .child(mark_text(mark, selected && focused, found, cx))
                    }
                };
                base.on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                        if this.mnemonics_only {
                            return;
                        }
                        // a right click keeps a selection it falls in, as IntelliJ's trees
                        let rows = this.tree_rows(cx);
                        if !this.state.selected(&rows).contains(&row) {
                            this.state.select(Some(row));
                            cx.emit(TreeEvent::Selected);
                        }
                        this.deploy_menu(event.position, window, cx);
                    }),
                )
                .on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        this.state.click(row, &event.modifiers());
                        cx.emit(TreeEvent::Selected);
                        cx.notify();
                        if event.click_count() >= 2 {
                            this.open_selected(window, cx);
                        }
                    }),
                )
                .into_any_element()
            })
            .collect()
    }
}

impl TreeView for BookmarksTree {
    type Key = Row;

    fn tree(&mut self) -> &mut TreeState<Row> {
        &mut self.state
    }

    fn tree_rows(&self, cx: &App) -> Vec<TreeRow<Row>> {
        self.rows()
            .into_iter()
            .map(|row| {
                let (depth, expanded) = match (row, self.mnemonics_only) {
                    (Row::List, _) => (0, Some(self.open)),
                    (Row::Mark(_), false) => (1, None),
                    (Row::Mark(_), true) => (0, None),
                };
                TreeRow::new(row, depth, expanded, self.row_text(row, cx))
            })
            .collect()
    }

    fn set_expanded(&mut self, key: &Row, expanded: bool, _: &mut Window, cx: &mut Context<Self>) {
        if *key == Row::List {
            self.set_open(expanded, cx);
        }
    }

    fn set_all_expanded(&mut self, expanded: bool, _: &mut Window, cx: &mut Context<Self>) {
        if !self.mnemonics_only {
            self.set_open(expanded, cx);
        }
    }

    fn selection_changed(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TreeEvent::Selected);
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_selected(window, cx);
    }
}

impl Render for BookmarksTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.contains_focused(window, cx);
        let rows = self.tree_rows(cx);
        let elements = self.render_rows(&rows, focused, cx);
        let search = self.state.render_search(&rows, cx);
        tree_keys(
            div()
                .relative()
                .flex()
                .flex_col()
                .size_full()
                .min_h_0()
                .track_focus(&self.focus)
                .key_context("BookmarksTree menu tree"),
            cx,
        )
        .child(
            v_flex()
                .id("bookmarks-tree")
                .role(Role::Tree)
                .aria_label("Bookmarks")
                .flex_1()
                .min_h_0()
                .py(px(4.))
                .track_scroll(self.state.scroll_handle())
                .overflow_y_scroll()
                .children(elements),
        )
        .children(search)
        .when(!self.mnemonics_only, |tree| {
            tree.on_action(
                cx.listener(|this, _: &bookmarks_panel::Rename, window, cx| {
                    this.rename(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &bookmarks_panel::Delete, window, cx| {
                    // ⌫ edits speed search's text while it shows
                    if this.state.search().is_some() {
                        cx.propagate();
                        return;
                    }
                    this.delete_selected(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &bookmarks_panel::DeleteType, _, cx| {
                    this.remove_mnemonics(cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &bookmarks_panel::ChooseType, window, cx| {
                    this.choose_type(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &bookmarks_panel::MoveUp, _, cx| {
                    this.move_selected(true, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &bookmarks_panel::MoveDown, _, cx| {
                this.move_selected(false, cx)
            }))
            .on_action(cx.listener(|this, _: &bookmarks_panel::Sort, _, cx| this.sort(cx)))
            .on_action(
                cx.listener(|this, _: &bookmarks_panel::OpenBookmarkGroup, window, cx| {
                    this.open_group(window, cx)
                }),
            )
            .on_action(cx.listener(
                |this, _: &bookmarks_panel::ToggleDefault, _, cx| this.toggle_default(cx),
            ))
        })
        .children(self.context_menu.as_ref().map(|(menu, position, _)| {
            gpui::deferred(gpui::anchored().position(*position).child(menu.clone()))
                .with_priority(1)
        }))
    }
}

/// A row of IntelliJ's tree in a tool window or popup: 24 px, the selection's fill (the inactive
/// one while the tree hasn't the focus), each level 18 px further in.
fn tree_row_element(
    id: impl Into<gpui::ElementId>,
    selected: bool,
    focused: bool,
    depth: f32,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let tokens = Tokens::get(cx);
    let hover = tokens.selection_hover;
    h_flex()
        .id(id)
        .role(Role::TreeItem)
        .aria_selected(selected)
        .flex_none()
        .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
        .mx(px(popup::SELECTION_INSET))
        .pl(px(4. + 18. * depth))
        .pr(px(8.))
        .gap(px(4.))
        .rounded(px(spork_workbench::tokens::metrics::RADIUS))
        .map(|row| match (selected, focused) {
            (true, true) => row.bg(tokens.selection),
            (true, false) => row.bg(tokens.selection_inactive),
            (false, _) => row.hover(move |row| row.bg(hover)),
        })
}

/// The Bookmarks popup (⌘F3, IntelliJ's `ShowLineBookmarksAction`) and Go to Mnemonic (⌥⌘F3): the
/// bookmarks tree, the selected bookmark's code beside it in the Bookmarks popup.
pub(crate) struct BookmarksPopup {
    tree: Entity<BookmarksTree>,
    mnemonics_only: bool,
    preview: Arc<dyn PreviewBackend>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissEvent> for BookmarksPopup {}

impl ModalView for BookmarksPopup {}

impl Focusable for BookmarksPopup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tree.focus_handle(cx)
    }
}

/// `JBUI.DialogSizes.large()`, the Bookmarks popup's first size.
const POPUP_WIDTH: f32 = 750.;
const POPUP_HEIGHT: f32 = 525.;

impl BookmarksPopup {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        mnemonics_only: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = picker_preview::editor_preview(project.clone(), window, cx);
        let tree = cx.new(|cx| BookmarksTree::new(workspace, project, mnemonics_only, cx));
        let subscriptions = vec![
            cx.subscribe_in(&tree, window, |this, _, event, window, cx| match event {
                TreeEvent::Chose => cx.emit(DismissEvent),
                TreeEvent::Selected => this.update_preview(window, cx),
            }),
            cx.observe(&tree, |_, _, cx| cx.notify()),
        ];
        Self {
            tree,
            mnemonics_only,
            preview,
            _subscriptions: subscriptions,
        }
    }

    fn update_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mnemonics_only {
            return;
        }
        match self.tree.read(cx).selected_mark().cloned() {
            Some(mark) => {
                let snapshot = mark.buffer.read(cx).snapshot();
                let start = snapshot.point_to_offset(Point::new(mark.row, 0));
                let end =
                    snapshot.point_to_offset(Point::new(mark.row, snapshot.line_len(mark.row)));
                let range: Range<language::Anchor> =
                    snapshot.anchor_before(start)..snapshot.anchor_after(end);
                self.preview.update(
                    PreviewUpdate::from_buffer(
                        mark.buffer,
                        MatchLocation {
                            anchor_range: range,
                            range: start..end,
                        },
                    ),
                    window,
                    cx,
                );
            }
            None => self.preview.clear(cx),
        }
    }

    /// A typed mnemonic goes to its bookmark before speed search sees it.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        if self.tree.read(cx).state.search().is_some() {
            return;
        }
        let mut chars = keystroke.key.chars();
        if let (Some(typed), None) = (chars.next(), chars.next())
            && self
                .tree
                .update(cx, |tree, cx| tree.jump_to_mnemonic(typed, window, cx))
        {
            cx.stop_propagation();
        }
    }
}

impl Render for BookmarksPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let tree = self.tree.read(cx);
        let empty = tree.loaded && tree.marks.is_empty();
        let title = match self.mnemonics_only {
            true => "Go to Mnemonic",
            false => "Bookmarks",
        };
        let list = div().flex_1().min_h_0().child(self.tree.clone());
        let body = match (self.mnemonics_only, empty) {
            (false, true) => empty_state(window, cx).into_any_element(),
            (true, _) => list.into_any_element(),
            (false, false) => h_flex()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(
                    div()
                        .flex()
                        .w(px(POPUP_WIDTH * 0.4))
                        .flex_none()
                        .child(list),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .border_l_1()
                        .border_color(tokens.popup_border)
                        .child(self.preview.render(PreviewLayout::Below, cx)),
                )
                .into_any_element(),
        };
        let surface = popup::surface(cx)
            .id("bookmarks-popup")
            .role(Role::Dialog)
            .aria_label(title)
            .key_context("BookmarksPopup")
            .capture_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
            .child(spork_workbench::popup::title(title, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(tokens.popup_border)
                    .child(body),
            );
        let surface = match self.mnemonics_only {
            true => surface.w(px(400.)).h(px(300.)),
            false => surface.w(px(POPUP_WIDTH)).h(px(POPUP_HEIGHT)),
        };
        spork_vcs::spork_look(cx).child(surface)
    }
}

/// IntelliJ's text for an empty Bookmarks popup or tool window.
fn empty_state(window: &Window, cx: &App) -> impl IntoElement {
    let tokens = Tokens::get(cx);
    let shortcut = crate::shortcut_text(&editor::actions::ToggleBookmark, window)
        .unwrap_or_else(|| "F3".into());
    v_flex()
        .id("bookmarks-empty")
        .flex_1()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .text_color(tokens.text_secondary)
        .role(Role::Label)
        .aria_label("No bookmarks added.")
        .child("No bookmarks added.")
        .child(format!("Add Bookmark ({shortcut}) to code lines,"))
        .child("symbols, files, and directories.")
}

/// The Bookmarks tool window (⌘2): the bookmarks tree under IntelliJ's header.
pub(crate) struct BookmarksPanel {
    tree: Entity<BookmarksTree>,
    _subscription: Subscription,
}

impl BookmarksPanel {
    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree = cx.new(|cx| BookmarksTree::new(workspace, project, false, cx));
        let subscription = cx.observe(&tree, |_, _, cx| cx.notify());
        Self {
            tree,
            _subscription: subscription,
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let panel: Arc<dyn PanelHandle> = Arc::new(cx.entity());
        let has_mark = self.tree.read(cx).selected_mark().is_some();
        let tree = self.tree.clone();
        let actions = vec![
            workspace::frame::header_button("bookmarks-edit", "general/edit", "Edit", cx)
                .disabled(!has_mark)
                .on_click({
                    let tree = tree.clone();
                    move |_, window, cx| tree.update(cx, |tree, cx| tree.rename(window, cx))
                })
                .into_any_element(),
            workspace::frame::header_button(
                "bookmarks-expand",
                "general/expandAll",
                "Expand All",
                cx,
            )
            .on_click({
                let tree = tree.clone();
                move |_, _, cx| tree.update(cx, |tree, cx| tree.set_open(true, cx))
            })
            .into_any_element(),
            workspace::frame::header_button(
                "bookmarks-collapse",
                "general/collapseAll",
                "Collapse All",
                cx,
            )
            .on_click(move |_, _, cx| tree.update(cx, |tree, cx| tree.set_open(false, cx)))
            .into_any_element(),
        ];
        workspace::frame::tool_window_header(
            workspace::frame::tool_window_title_label("Bookmarks"),
            actions,
            panel,
            "Bookmarks".into(),
            cx,
        )
        .into_any_element()
    }
}

impl Render for BookmarksPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.header(cx);
        let tree = self.tree.read(cx);
        let empty = tree.loaded && tree.marks.is_empty();
        let tokens = Tokens::get(cx);
        spork_vcs::spork_look(cx).size_full().child(
            v_flex()
                .size_full()
                .bg(tokens.island)
                .key_context("BookmarksPanel")
                .child(header)
                .child(div().flex().flex_1().min_h_0().child(self.tree.clone()))
                .when(empty, |panel| panel.child(empty_state(window, cx))),
        )
    }
}

impl Focusable for BookmarksPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tree.focus_handle(cx)
    }
}

impl EventEmitter<PanelEvent> for BookmarksPanel {}

impl Panel for BookmarksPanel {
    fn persistent_name() -> &'static str {
        "Bookmarks"
    }

    fn panel_key() -> &'static str {
        "BookmarksToolWindow"
    }

    fn position(&self, _: &Window, _: &App) -> DockPosition {
        DockPosition::Left
    }

    // shortcut: the tool window stays where IntelliJ puts it, as Spork's do, until the stripes
    // let tool windows move
    fn position_is_valid(&self, position: DockPosition) -> bool {
        position == DockPosition::Left
    }

    fn set_position(&mut self, _: DockPosition, _: &mut Window, _: &mut Context<Self>) {}

    fn default_size(&self, _: &Window, _: &App) -> Pixels {
        px(285.)
    }

    fn icon(&self, _: &Window, _: &App) -> Option<ui::IconName> {
        Some(ui::IconName::Bookmark)
    }

    fn icon_tooltip(&self, _: &Window, _: &App) -> Option<&'static str> {
        Some("Bookmarks")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(bookmarks_panel::ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        7
    }
}

#[cfg(test)]
mod tests {
    use super::{read_label, write_label};

    #[test]
    fn mnemonics_ride_in_the_label() {
        assert_eq!(
            read_label(&write_label(Some('A'), "start")),
            (Some('A'), "start")
        );
        assert_eq!(read_label(&write_label(Some('1'), "")), (Some('1'), ""));
        assert_eq!(read_label(&write_label(None, "plain")), (None, "plain"));
        // a label Zed or a user wrote stays a description
        assert_eq!(read_label("a\u{1f}b"), (None, "a\u{1f}b"));
        assert_eq!(read_label(""), (None, ""));
    }
}
