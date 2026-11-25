//! IntelliJ's list popup under the editor's caret (`ListPopupImpl`, `showInBestPositionFor`): a
//! title, rows with their icon and number, separators. Refactor This, Generate, Select In and File
//! Path are built from it.

use std::rc::Rc;

use editor::Editor;
use editor::display_map::ToDisplayPoint as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Point, Render,
    Role, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, anchored,
    deferred, div, point, px,
};
use gpui_kit::component::v_flex;
use spork_workbench::{Tokens, TreeRow, TreeState, TreeView, popup, tree_keys};
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

/// Where the caret is in the window, at the bottom of its line: where IntelliJ shows a popup
/// for the editor (`showInBestPositionFor`).
pub fn caret_origin(
    editor: &Entity<Editor>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Point<Pixels>> {
    let head = editor.read(cx).selections.newest_anchor().head();
    anchor_origin(editor, head, window, cx)
}

/// Where `anchor` is in the window, at the bottom of its line.
pub fn anchor_origin(
    editor: &Entity<Editor>,
    anchor: editor::Anchor,
    window: &mut Window,
    cx: &mut App,
) -> Option<Point<Pixels>> {
    editor.update(cx, |editor, cx| {
        let snapshot = editor.snapshot(window, cx);
        let display = anchor.to_display_point(&snapshot);
        let origin = editor.text_origin_below_row(display.row())?;
        let details = editor.text_layout_details(window, cx);
        let x = snapshot.x_for_display_point(display, &details);
        Some(point(origin.x + x, origin.y))
    })
}

/// A row of a list popup: an item, or a line between groups with or without a title.
pub enum Row {
    Item {
        text: SharedString,
        hint: Option<SharedString>,
        /// Its icon, in a column of its own.
        icon: Option<RowIcon>,
        enabled: bool,
        run: Rc<dyn Fn(&mut Window, &mut App)>,
    },
    Separator(Option<SharedString>),
}

impl Row {
    pub fn item(
        text: impl Into<SharedString>,
        run: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::Item {
            text: text.into(),
            hint: None,
            icon: None,
            enabled: true,
            run: Rc::new(run),
        }
    }

    pub fn icon(mut self, path: Option<RowIcon>) -> Self {
        if let Self::Item { icon, .. } = &mut self {
            *icon = path;
        }
        self
    }

    /// A greyed item keeps its place and number, but can't be chosen.
    pub fn enabled(mut self, can_choose: bool) -> Self {
        if let Self::Item { enabled, .. } = &mut self {
            *enabled = can_choose;
        }
        self
    }

    pub fn hint(mut self, shortcut: Option<SharedString>) -> Self {
        if let Self::Item { hint, .. } = &mut self {
            *hint = shortcut;
        }
        self
    }

    fn is_item(&self) -> bool {
        matches!(self, Self::Item { enabled: true, .. })
    }
}

/// A row's icon, by its path among the workbench's icons: one drawn in the text's colour (a tool
/// window's), or one in its own colours (a file type's, a folder's).
#[derive(Clone, Copy)]
pub enum RowIcon {
    Tinted(&'static str),
    Colored(&'static str),
}

/// An item's mnemonic in a numbered popup, IntelliJ's `ALPHA_NUMBERING`: 1 to 9, 0, then A to Z.
fn mnemonic(ix: usize) -> Option<char> {
    match ix {
        0..=8 => char::from_digit(ix as u32 + 1, 10),
        9 => Some('0'),
        _ => u8::try_from(ix - 10)
            .ok()
            .filter(|letter| *letter < 26)
            .map(|letter| (b'A' + letter) as char),
    }
}

/// IntelliJ's list popup (`ListPopupImpl`) under the caret: its title, the rows (numbered in
/// Refactor This, chosen by their digit), "Nothing here" without any.
pub struct ListPopup {
    title: SharedString,
    rows: Vec<Row>,
    numbered: bool,
    selected: TreeState<usize>,
    origin: Point<Pixels>,
    focus: FocusHandle,
    editor: WeakEntity<Editor>,
}

/// Shows a list popup under the editor's caret, as the workspace's modal: not from inside an
/// update of the workspace (an action of its defers it).
pub fn show_list_popup(
    workspace: &WeakEntity<Workspace>,
    editor: &Entity<Editor>,
    title: impl Into<SharedString>,
    rows: Vec<Row>,
    numbered: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(origin) = caret_origin(editor, window, cx) else {
        return;
    };
    let title = title.into();
    let editor = editor.downgrade();
    workspace
        .update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |_, cx| {
                let mut selected = TreeState::list();
                selected.select(rows.iter().position(Row::is_item));
                ListPopup {
                    title,
                    rows,
                    numbered,
                    selected,
                    origin,
                    focus: cx.focus_handle(),
                    editor,
                }
            })
        })
        .log_err();
}

impl EventEmitter<DismissEvent> for ListPopup {}

impl ModalView for ListPopup {}

impl Focusable for ListPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ListPopup {
    fn choose(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Row::Item {
            run, enabled: true, ..
        }) = self.rows.get(row)
        else {
            return;
        };
        let run = run.clone();
        cx.emit(DismissEvent);
        if let Some(editor) = self.editor.upgrade() {
            window.focus(&editor.focus_handle(cx), cx);
        }
        window.defer(cx, move |window, cx| run(window, cx));
    }

    /// A digit (or letter) chooses the item it numbers, unless it's greyed.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if !self.numbered || modifiers.platform || modifiers.control || modifiers.alt {
            return;
        }
        let key = event.keystroke.key.to_uppercase();
        let Some(row) = (0..self.rows.len())
            .filter(|ix| matches!(self.rows[*ix], Row::Item { .. }))
            .enumerate()
            .find(|(number, _)| mnemonic(*number).is_some_and(|m| key == m.to_string()))
            .map(|(_, row)| row)
        else {
            return;
        };
        cx.stop_propagation();
        self.choose(row, window, cx);
    }
}

impl Render for ListPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let mut number = 0;
        let mut first_group = true;
        // the icons' column, when any row has one: the others leave it empty
        let icons = self
            .rows
            .iter()
            .any(|row| matches!(row, Row::Item { icon: Some(_), .. }));
        let rows: Vec<AnyElement> = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| match row {
                Row::Separator(Some(title)) => {
                    let first = std::mem::take(&mut first_group) && ix == 0;
                    popup::labeled_separator(title.clone(), first, cx).into_any_element()
                }
                Row::Separator(None) => popup::separator(cx).into_any_element(),
                Row::Item {
                    text,
                    hint,
                    icon,
                    enabled,
                    ..
                } => {
                    first_group = false;
                    let mnemonic = mnemonic(number);
                    number += 1;
                    let selected = self.selected.lead() == Some(&ix);
                    let icon = icon.map(|icon| match icon {
                        RowIcon::Tinted(path) => spork_workbench::icon(path)
                            .size(px(16.))
                            .text_color(match enabled {
                                true => tokens.icon,
                                false => tokens.text_disabled,
                            })
                            .into_any_element(),
                        RowIcon::Colored(path) => spork_workbench::image(path, cx)
                            .size(px(16.))
                            .into_any_element(),
                    });
                    let row = popup::row(("list-popup-row", ix), selected, cx)
                        .role(Role::MenuItem)
                        .aria_label(text.clone())
                        .aria_selected(selected)
                        .when(!enabled, |row| row.text_color(tokens.text_disabled))
                        .when(icons, |row| {
                            row.child(div().size(px(16.)).flex_none().children(icon))
                        })
                        .when(self.numbered, |row| {
                            row.child(
                                div()
                                    .w(px(14.))
                                    .flex_none()
                                    .text_color(tokens.text_secondary)
                                    .children(mnemonic.map(String::from)),
                            )
                        })
                        .child(div().min_w_0().truncate().child(text.clone()))
                        .children(hint.clone().map(|hint| popup::hint(hint, selected, cx)))
                        .on_click(
                            cx.listener(move |this, _, window, cx| this.choose(ix, window, cx)),
                        );
                    spork_workbench::controls::A11yDisabled::new(row, !enabled).into_any_element()
                }
            })
            .collect();
        let empty = !self.rows.iter().any(Row::is_item);
        let surface = tree_keys(
            popup::surface(cx)
                .id("list-popup")
                .role(Role::Menu)
                .aria_label(self.title.clone())
                .min_w(px(220.))
                .max_w(px(640.))
                .max_h(px(520.))
                .pb(px(6.))
                .track_focus(&self.focus)
                .key_context("CodeListPopup menu tree"),
            cx,
        )
        .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
        .on_key_down(cx.listener(Self::key_down))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        // a popup without a title (Markdown's Set Header Style) starts with its rows
        .when(self.title.is_empty(), |surface| surface.pt(px(6.)))
        .when(!self.title.is_empty(), |surface| {
            surface.child(
                div()
                    .id("list-popup-title")
                    .role(Role::Label)
                    .aria_label(self.title.clone())
                    .flex_none()
                    .px(px(12.))
                    .pt(px(8.))
                    .pb(px(6.))
                    .w_full()
                    .text_center()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(self.title.clone()),
            )
        })
        .child(
            v_flex()
                .id("list-popup-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .when(empty, |list| {
                    // IntelliJ's empty list popup: one greyed row
                    list.child(
                        popup::row("list-popup-nothing", true, cx)
                            .role(Role::MenuItem)
                            .aria_label("Nothing here")
                            .child("Nothing here"),
                    )
                })
                .children(rows),
        );
        anchored_popup(self.origin, surface, cx)
    }
}

impl TreeView for ListPopup {
    type Key = usize;

    fn tree(&mut self) -> &mut TreeState<usize> {
        &mut self.selected
    }

    /// The items that can be chosen, by their place among the rows.
    fn tree_rows(&self, _: &App) -> Vec<TreeRow<usize>> {
        self.rows
            .iter()
            .enumerate()
            .filter_map(|(ix, row)| match row {
                Row::Item {
                    text,
                    enabled: true,
                    ..
                } => Some(TreeRow::item(ix, text.clone())),
                _ => None,
            })
            .collect()
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(row) = self.selected.lead().copied() {
            self.choose(row, window, cx);
        }
    }

    // its digits and letters choose the items they number
    fn speed_search(&self) -> bool {
        false
    }
}

/// A popup drawn at `origin` in the window, over everything, in Spork's look.
pub fn anchored_popup(
    origin: Point<Pixels>,
    content: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    deferred(
        anchored()
            .position(origin)
            .snap_to_window_with_margin(px(8.))
            .child(spork_vcs::spork_look(cx).child(content)),
    )
    .with_priority(1)
}
