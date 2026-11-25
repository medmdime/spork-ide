//! Paste from History… (⇧⌘V): IntelliJ's clipboard history (`CopyPasteManagerWithHistory`, the
//! last 100 copies, newest first) and its chooser, "Choose Content to Paste" (`ContentChooser`):
//! the numbered copies over a preview of the selected one; ⏎, OK, a double click or the copy's
//! number pastes it, ⌦ or Remove drops it from the history.

use std::collections::VecDeque;

use editor::Editor;
use editor::actions::Paste;
use gpui::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, px,
};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use settings::Settings as _;
use spork_workbench::controls::dialog_title;
use spork_workbench::{Tokens, Typography as _, popup};

gpui::actions!(
    spork_editor,
    [
        /// Chooses a recent copy to paste (IntelliJ's Paste from History…, ⇧⌘V).
        PasteFromHistory,
        /// Drops the chosen copy from the clipboard history (the chooser's Remove, ⌦).
        RemoveFromHistory,
    ]
);

/// `clipboard.history.max.items`.
const MAX_ENTRIES: usize = 100;
/// IntelliJ's `ContentChooser` title (`choose.content.to.paste.dialog.title`).
const TITLE: &str = "Choose Content to Paste";
/// A copy's line breaks in the list (`ContentChooser.RETURN_SYMBOL`).
const RETURN_SYMBOL: &str = "\u{23ce}";

#[derive(Default)]
struct ClipboardHistory {
    entries: VecDeque<String>,
}

impl Global for ClipboardHistory {}

pub(crate) fn init(cx: &mut App) {
    // a copy or a cut, from a key or a menu, is the clipboard once its action ran
    cx.observe_keystrokes(|event, _, cx| {
        if event.action.as_ref().is_some_and(|action| {
            let name = action.name();
            name.ends_with("Copy") || name.contains("Cut")
        }) {
            remember_clipboard(cx);
        }
    })
    .detach();
}

/// Puts what the clipboard holds first in the history, unless it already is.
fn remember_clipboard(cx: &mut App) {
    let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
        return;
    };
    if text.is_empty() {
        return;
    }
    let history = cx.default_global::<ClipboardHistory>();
    history.entries.retain(|entry| *entry != text);
    history.entries.push_front(text);
    history.entries.truncate(MAX_ENTRIES);
}

pub(crate) fn register(editor: &mut Editor, handle: WeakEntity<Editor>) {
    editor
        .register_action(move |_: &PasteFromHistory, window, cx| {
            show_chooser(handle.clone(), window, cx)
        })
        .detach();
}

fn show_chooser(editor: WeakEntity<Editor>, window: &mut Window, cx: &mut App) {
    remember_clipboard(cx);
    let entries: Vec<String> = cx
        .default_global::<ClipboardHistory>()
        .entries
        .iter()
        .cloned()
        .collect();
    // IntelliJ shows no chooser over an empty history
    if entries.is_empty() {
        return;
    }
    let chooser = cx.new(|cx| PasteChooser {
        entries,
        selected: 0,
        editor,
        focus_handle: cx.focus_handle(),
    });
    let focus = chooser.read(cx).focus_handle.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let chooser = chooser.clone();
        dialog
            .title(dialog_title(TITLE))
            .w(px(500.))
            .close_button(false)
            .on_ok({
                let chooser = chooser.clone();
                move |_, window, cx| {
                    paste_selected(&chooser, window, cx);
                    false
                }
            })
            .child(chooser)
    });
    window.focus(&focus, cx);
}

/// Pastes the chooser's selected copy into its editor: the copy goes first in the history and
/// onto the clipboard, as IntelliJ's `moveContentToStackTop`.
fn paste_selected(chooser: &Entity<PasteChooser>, window: &mut Window, cx: &mut App) {
    let (text, editor) = {
        let chooser = chooser.read(cx);
        (
            chooser.entries.get(chooser.selected).cloned(),
            chooser.editor.clone(),
        )
    };
    window.close_dialog(cx);
    let Some(text) = text else {
        return;
    };
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    remember_clipboard(cx);
    editor
        .update(cx, |editor, cx| {
            window.focus(&editor.focus_handle(cx), cx);
            editor.paste(&Paste, window, cx);
        })
        .ok();
}

struct PasteChooser {
    entries: Vec<String>,
    selected: usize,
    editor: WeakEntity<Editor>,
    focus_handle: FocusHandle,
}

impl Focusable for PasteChooser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl PasteChooser {
    fn remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected >= self.entries.len() {
            return;
        }
        let removed = self.entries.remove(self.selected);
        let history = cx.default_global::<ClipboardHistory>();
        let was_current = history.entries.front() == Some(&removed);
        history.entries.retain(|entry| *entry != removed);
        // the clipboard follows its history's first copy, as IntelliJ's `removeContent`
        if was_current {
            let next = history.entries.front().cloned().unwrap_or_default();
            cx.write_to_clipboard(ClipboardItem::new_string(next));
        }
        // IntelliJ closes the chooser once nothing is left
        if self.entries.is_empty() {
            window.close_dialog(cx);
            return;
        }
        self.selected = self.selected.min(self.entries.len() - 1);
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        match keystroke.key.as_str() {
            "up" => self.selected = self.selected.saturating_sub(1),
            "down" => self.selected = (self.selected + 1).min(self.entries.len() - 1),
            key => {
                // a copy's number pastes it, 0 the tenth
                let Some(digit) = key.parse::<usize>().ok().filter(|digit| *digit < 10) else {
                    return;
                };
                let ix = if digit == 0 { 9 } else { digit - 1 };
                if ix >= self.entries.len() {
                    return;
                }
                self.selected = ix;
                let chooser = cx.entity();
                window.defer(cx, move |window, cx| paste_selected(&chooser, window, cx));
            }
        }
        cx.stop_propagation();
        cx.notify();
    }
}

/// A copy as one line of the list: its line breaks shown as ⏎, its start only.
fn short_text(text: &str) -> SharedString {
    let line: String = text.trim().replace('\n', RETURN_SYMBOL);
    let mut short: String = line.chars().take(80).collect();
    if short.len() < line.len() {
        short.push('…');
    }
    short.into()
}

impl Render for PasteChooser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let dark = !theme::ActiveTheme::theme(&**cx).appearance().is_light();
        let width = self.entries.len().to_string().len();
        let buffer_font = theme_settings::ThemeSettings::get_global(cx)
            .buffer_font
            .clone();
        let focused = self.focus_handle.is_focused(window);
        let rows = self.entries.iter().enumerate().map(|(ix, text)| {
            let selected = ix == self.selected;
            let number = format!("{:<width$}  ", ix + 1);
            popup::row(("paste-entry", ix), selected, cx)
                .mx_0()
                .role(Role::ListItem)
                .aria_label(short_text(text))
                .aria_selected(selected)
                .child(spork_workbench::image_in("fileTypes/text", dark).size(px(16.)))
                .child(
                    div()
                        .flex_none()
                        .text_color(tokens.text_secondary)
                        .child(number),
                )
                .child(div().truncate().child(short_text(text)))
                .on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        this.selected = ix;
                        cx.notify();
                        if event.click_count() == 2 {
                            let chooser = cx.entity();
                            window
                                .defer(cx, move |window, cx| paste_selected(&chooser, window, cx));
                        }
                    }),
                )
        });
        let preview = self.entries.get(self.selected).cloned().unwrap_or_default();
        v_flex()
            .id("paste-from-history")
            .role(Role::Group)
            .aria_label(TITLE)
            .key_context("PasteChooser")
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &RemoveFromHistory, window, cx| {
                this.remove_selected(window, cx)
            }))
            .gap(px(8.))
            .child(
                v_flex()
                    .h(px(150.))
                    .border_1()
                    .border_color(if focused {
                        tokens.focus
                    } else {
                        tokens.control_border
                    })
                    .child(
                        h_flex().p(px(2.)).child(
                            spork_workbench::ActionButton::new("paste-remove", "general/remove")
                                .tooltip("Remove")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.remove_selected(window, cx)
                                })),
                        ),
                    )
                    .child(
                        v_flex()
                            .id("paste-entries")
                            .role(Role::List)
                            .aria_label("Contents")
                            // the editor's font, as IntelliJ's chooser lists the copies
                            .font(buffer_font.clone())
                            .flex_1()
                            .overflow_y_scroll()
                            .children(rows),
                    ),
            )
            .child(
                div()
                    .id("paste-preview")
                    .role(Role::Group)
                    .aria_label("Preview")
                    .h(px(250.))
                    .overflow_scroll()
                    .p(px(4.))
                    .border_1()
                    .border_color(tokens.control_border)
                    .font(buffer_font)
                    .text_ui()
                    .whitespace_nowrap()
                    .children(
                        preview
                            .lines()
                            .map(|line| div().child(SharedString::from(line.to_owned())))
                            .collect::<Vec<_>>(),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        spork_workbench::Button::new("paste-cancel", "Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        spork_workbench::Button::new("paste-ok", "OK")
                            .primary(true)
                            .on_click(cx.listener(|_, _, window, cx| {
                                let chooser = cx.entity();
                                window.defer(cx, move |window, cx| {
                                    paste_selected(&chooser, window, cx)
                                });
                            })),
                    ),
            )
    }
}
