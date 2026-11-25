//! IntelliJ's popups (docs/reference-spec.md §10 "Popups and menus"): the surface, its 24 px
//! rows with a selection 8 px in from the sides, separators and labeled separators. The main
//! toolbar's popups (branches, projects) and Search Everywhere are built from these.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::tokens::{Tokens, Typography as _, metrics};

/// How far a list row's selection sits in from the popup's sides.
pub const SELECTION_INSET: f32 = 8.;

/// The popup's surface: its fill, a 1 px border, 8 px corners and a shadow.
pub fn surface(cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    v_flex()
        .bg(tokens.popup)
        .border_1()
        .border_color(tokens.popup_border)
        .rounded(px(8.))
        .shadow_lg()
        .overflow_hidden()
        .text_ui()
        .text_color(tokens.text)
}

/// A list row, 24 px: the selection is a 4 px rounded fill 8 px in from the popup's sides,
/// the content 12 px in.
pub fn row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let tokens = Tokens::get(cx);
    let hover = tokens.selection_hover;
    h_flex()
        .id(id)
        .flex_none()
        .h(px(metrics::ROW_HEIGHT))
        .mx(px(SELECTION_INSET))
        .px(px(12. - SELECTION_INSET))
        .gap(px(6.))
        .rounded(px(metrics::RADIUS))
        .cursor_pointer()
        .map(|row| match selected {
            true => row.bg(tokens.selection),
            false => row.hover(move |row| row.bg(hover)),
        })
}

/// A line between groups of rows: 1 px with 4 px above and below, 8 px in from the sides.
pub fn separator(cx: &App) -> Div {
    let line = Tokens::get(cx).popup_border;
    div()
        .flex_none()
        .py(px(4.))
        .px(px(SELECTION_INSET))
        .child(div().w_full().h(px(1.)).bg(line))
}

/// A group's title ("Open Projects"): a line above unless it comes first, then the title in the
/// secondary color, 3 px above and below, 16 px in.
pub fn labeled_separator(title: impl Into<SharedString>, first: bool, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    v_flex()
        .flex_none()
        .when(!first, |group| group.child(separator(cx)))
        .child(
            div()
                .px(px(16.))
                .py(px(3.))
                .text_ui_small()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(tokens.text_secondary)
                .child(title.into()),
        )
}

/// A shortcut or other hint right-aligned in a row, in the secondary color.
pub fn hint(text: impl Into<SharedString>, selected: bool, cx: &App) -> Div {
    let tokens = Tokens::get(cx);
    div()
        .flex_none()
        .ml_auto()
        .pl(px(10.))
        .text_color(match selected {
            true => tokens.text,
            false => tokens.text_secondary,
        })
        .child(text.into())
}

/// A popup's title bar: the title centered, bold and grey, as IntelliJ's popups with a title.
pub fn title(title: impl Into<SharedString>, cx: &App) -> AnyElement {
    let tokens = Tokens::get(cx);
    let title = title.into();
    h_flex()
        .id("popup-title")
        .role(Role::Label)
        .aria_label(title.clone())
        .flex_none()
        .h(px(33.))
        .justify_center()
        .font_weight(FontWeight::BOLD)
        .text_color(tokens.text_secondary)
        .child(title)
        .into_any_element()
}

/// Keystrokes as IntelliJ writes them on macOS (`⇧⌘O`, `⇧ ⇧`), or on other systems
/// (`Ctrl+Shift+N`): a popup row's shortcut.
pub fn keystrokes_text(keystrokes: &[KeybindingKeystroke]) -> String {
    let mac = cfg!(target_os = "macos");
    keystrokes
        .iter()
        .map(|keystroke| {
            let modifiers = keystroke.modifiers();
            let names: [(bool, &str, &str); 4] = [
                (modifiers.control, "⌃", "Ctrl"),
                (modifiers.alt, "⌥", "Alt"),
                (modifiers.shift, "⇧", "Shift"),
                (modifiers.platform, "⌘", "Meta"),
            ];
            let mut parts: Vec<String> = names
                .into_iter()
                .filter(|(held, _, _)| *held)
                .map(|(_, symbol, name)| if mac { symbol } else { name }.to_owned())
                .collect();
            let key = keystroke.key();
            let key = match (key, mac) {
                ("shift", true) => "⇧".to_owned(),
                ("control", true) => "⌃".to_owned(),
                ("alt", true) => "⌥".to_owned(),
                ("platform", true) => "⌘".to_owned(),
                ("enter", true) => "⏎".to_owned(),
                ("backspace", true) => "⌫".to_owned(),
                ("delete", true) => "⌦".to_owned(),
                ("escape", true) => "⎋".to_owned(),
                ("tab", true) => "⇥".to_owned(),
                ("up", true) => "↑".to_owned(),
                ("down", true) => "↓".to_owned(),
                ("left", true) => "←".to_owned(),
                ("right", true) => "→".to_owned(),
                ("home", true) => "↖".to_owned(),
                ("end", true) => "↘".to_owned(),
                ("pageup", true) => "⇞".to_owned(),
                ("pagedown", true) => "⇟".to_owned(),
                (key, _) => {
                    let mut chars = key.chars();
                    chars
                        .next()
                        .map(|first| first.to_uppercase().chain(chars).collect())
                        .unwrap_or_default()
                }
            };
            // a modifier pressed alone (`shift shift`) isn't written twice
            if parts.last() != Some(&key) {
                parts.push(key);
            }
            parts.join(if mac { "" } else { "+" })
        })
        .collect::<Vec<_>>()
        .join(if mac { " " } else { ", " })
}
