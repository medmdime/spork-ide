//! Editor › File Types, IntelliJ's `FileTypeConfigurable`: its Recognized File Types tab (the
//! languages, each with the file name patterns the user gave it: Zed's `file_types`) and its
//! Ignored Files and Folders tab (Zed's `file_scan_exclusions`). Edits wait for Apply as every
//! page's do. Left out: the patterns a language has of its own (Zed's can't be edited), hashbang
//! patterns and the operating system's file associations.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::Value;
use spork_workbench::{Tokens, Typography as _};

use crate::SettingsDialog;

/// The page's path in the tree.
pub(crate) const PAGE: &str = "Editor/File Types";
/// The settings it writes.
pub(crate) const PATHS: &[&str] = &["/file_types", "/file_scan_exclusions"];
pub(crate) const WORDS: &str = "file types recognized file types file name patterns wildcard \
    ignored files and folders";

const EXCLUSIONS: &str = "/file_scan_exclusions";

#[derive(Default)]
pub(crate) struct FileTypesState {
    /// The Ignored Files and Folders tab shows, not Recognized File Types.
    ignored_tab: bool,
    language: Option<SharedString>,
    pattern: Option<usize>,
    ignored: Option<usize>,
    /// The Ignored tab's field over its list, adding a pattern or editing the one at the index.
    editing: Option<(Entity<InputState>, Option<usize>, Subscription)>,
}

/// A JSON array of strings, as a list.
fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

impl SettingsDialog {
    fn languages(cx: &App) -> Vec<SharedString> {
        let Some(app_state) = workspace::AppState::try_global(cx) else {
            return Vec::new();
        };
        let mut names: Vec<SharedString> = app_state
            .languages
            .language_names()
            .into_iter()
            .map(|name| SharedString::from(name.to_string()))
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.dedup();
        names
    }

    fn patterns_path(language: &str) -> String {
        format!("/file_types/{language}")
    }

    fn set_list(&mut self, path: &str, items: Vec<String>, cx: &mut Context<Self>) {
        let value = Value::Array(items.into_iter().map(Value::String).collect());
        self.draft.set(path, Some(value));
        cx.notify();
    }

    /// IntelliJ's Add Wildcard dialog for the language selected.
    fn add_wildcard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(language) = self.file_types.language.clone() else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        let this = cx.entity().downgrade();
        let ok = {
            let input = input.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let wildcard = input.read(cx).value().trim().to_owned();
                window.close_dialog(cx);
                if wildcard.is_empty() {
                    return;
                }
                let path = Self::patterns_path(&language);
                this.update(cx, |this, cx| {
                    let mut patterns = strings(this.draft.get(&path));
                    if !patterns.contains(&wildcard) {
                        patterns.push(wildcard);
                    }
                    this.set_list(&path, patterns, cx);
                })
                .ok();
            })
        };
        window
            .subscribe(&input, cx, {
                let ok = ok.clone();
                move |_, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        ok(window, cx)
                    }
                }
            })
            .detach();
        let field = input.clone();
        let label = "Enter new wildcard ('*' and '?' allowed):";
        window.open_dialog(cx, move |dialog, _, _| {
            let ok = ok.clone();
            dialog
                .title(spork_workbench::controls::dialog_title("Add Wildcard"))
                .on_ok(|_, _, _| false)
                .close_button(false)
                .w(px(360.))
                .child(
                    v_flex()
                        .id("add-wildcard")
                        .role(Role::Group)
                        .aria_label("Add Wildcard")
                        .gap(px(8.))
                        .child(spork_workbench::controls::form_label(label))
                        .child(
                            div()
                                .w(px(150.))
                                .child(spork_workbench::TextField::new(&field).aria_label(label)),
                        )
                        .child(
                            h_flex()
                                .pt(px(8.))
                                .gap(px(8.))
                                .justify_end()
                                .child(
                                    spork_workbench::Button::new("add-wildcard-cancel", "Cancel")
                                        .on_click(|_, window, cx| window.close_dialog(cx)),
                                )
                                .child(
                                    spork_workbench::Button::new("add-wildcard-ok", "OK")
                                        .primary(true)
                                        .on_click(move |_, window, cx| ok(window, cx)),
                                ),
                        ),
                )
        });
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn remove_wildcard(&mut self, cx: &mut Context<Self>) {
        let (Some(language), Some(ix)) =
            (self.file_types.language.clone(), self.file_types.pattern)
        else {
            return;
        };
        let path = Self::patterns_path(&language);
        let mut patterns = strings(self.draft.get(&path));
        if ix < patterns.len() {
            patterns.remove(ix);
            self.file_types.pattern = None;
            self.set_list(&path, patterns, cx);
        }
    }

    /// The Ignored tab's field over its list, empty to add or holding the pattern to edit.
    fn edit_ignored(&mut self, at: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let current = at
            .and_then(|ix| strings(self.draft.get(EXCLUSIONS)).get(ix).cloned())
            .unwrap_or_default();
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(current, window, cx);
            input
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let pattern = input.read(cx).value().trim().to_owned();
                    let at = this.file_types.editing.take().and_then(|(_, at, _)| at);
                    if pattern.is_empty() {
                        cx.notify();
                        return;
                    }
                    let mut patterns = strings(this.draft.get(EXCLUSIONS));
                    match at {
                        Some(ix) if ix < patterns.len() => patterns[ix] = pattern,
                        // IntelliJ's "The value already exists" keeps the list as it is
                        _ if patterns.contains(&pattern) => {}
                        _ => patterns.push(pattern),
                    }
                    patterns.sort();
                    this.set_list(EXCLUSIONS, patterns, cx);
                }
            });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.file_types.editing = Some((input, at, subscription));
        cx.notify();
    }

    fn remove_ignored(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self.file_types.ignored else {
            return;
        };
        let mut patterns = strings(self.draft.get(EXCLUSIONS));
        if ix < patterns.len() {
            patterns.remove(ix);
            self.file_types.ignored = None;
            self.set_list(EXCLUSIONS, patterns, cx);
        }
    }

    fn list_row(
        id: (&'static str, usize),
        text: SharedString,
        selected: bool,
        cx: &App,
    ) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        div()
            .id(id)
            .role(Role::ListBoxOption)
            .aria_label(text.clone())
            .aria_selected(selected)
            .h(px(spork_workbench::tokens::metrics::ROW_HEIGHT))
            .px(px(8.))
            .flex()
            .items_center()
            .cursor_pointer()
            .when(selected, |row| row.bg(tokens.selection))
            .child(text)
    }

    fn list_box(
        id: &'static str,
        label: &'static str,
        rows: Vec<AnyElement>,
        cx: &App,
    ) -> Stateful<Div> {
        let tokens = Tokens::get(cx);
        v_flex()
            .id(id)
            .role(Role::List)
            .aria_label(label)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .border_1()
            .border_color(tokens.control_border)
            .children(rows)
    }

    fn toolbar(buttons: Vec<spork_workbench::ActionButton>) -> Div {
        h_flex().gap(px(2.)).py(px(2.)).children(buttons)
    }

    pub(crate) fn file_types_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let tokens = Tokens::get(cx);
        let ignored_tab = self.file_types.ignored_tab;
        let tab = |id: &'static str, label: &'static str, ignored: bool| {
            let selected = ignored == ignored_tab;
            v_flex()
                .id(id)
                .role(Role::Tab)
                .aria_label(label)
                .aria_selected(selected)
                .h(px(32.))
                .px(px(12.))
                .justify_center()
                .cursor_pointer()
                .child(div().flex_1().flex().items_center().child(label))
                .child(
                    div()
                        .h(px(3.))
                        .rounded_t(px(2.))
                        .when(selected, |line| line.bg(tokens.accent)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.file_types.ignored_tab = ignored;
                    cx.notify();
                }))
        };
        let tabs = h_flex()
            .id("file-types-tabs")
            .role(Role::TabList)
            .aria_label("File Types")
            .border_b_1()
            .border_color(tokens.separator)
            .child(tab("recognized-file-types", "Recognized File Types", false))
            .child(tab("ignored-files", "Ignored Files and Folders", true));
        let content = match ignored_tab {
            false => self.recognized_tab(cx),
            true => self.ignored_tab(cx),
        };
        v_flex()
            .size_full()
            .px(px(16.))
            .pb(px(16.))
            .gap(px(8.))
            .child(tabs)
            .child(content)
            .into_any_element()
    }

    fn recognized_tab(&mut self, cx: &mut Context<Self>) -> Div {
        let languages = Self::languages(cx);
        let selected = self.file_types.language.clone();
        let language_rows = languages
            .iter()
            .enumerate()
            .map(|(ix, name)| {
                let name = name.clone();
                Self::list_row(
                    ("file-type", ix),
                    name.clone(),
                    selected.as_ref() == Some(&name),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.file_types.language = Some(name.clone());
                    this.file_types.pattern = None;
                    cx.notify();
                }))
                .into_any_element()
            })
            .collect();
        let patterns = selected
            .as_ref()
            .map(|language| strings(self.draft.get(&Self::patterns_path(language))))
            .unwrap_or_default();
        let pattern_rows: Vec<AnyElement> = patterns
            .into_iter()
            .enumerate()
            .map(|(ix, pattern)| {
                Self::list_row(
                    ("file-pattern", ix),
                    pattern.into(),
                    self.file_types.pattern == Some(ix),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.file_types.pattern = Some(ix);
                    cx.notify();
                }))
                .into_any_element()
            })
            .collect();
        let empty = pattern_rows.is_empty();
        let tokens = Tokens::get(cx);
        let toolbar = Self::toolbar(vec![
            spork_workbench::ActionButton::new("add-pattern", "general/add")
                .tooltip("Add")
                .disabled(selected.is_none())
                .on_click(cx.listener(|this, _, window, cx| this.add_wildcard(window, cx))),
            spork_workbench::ActionButton::new("remove-pattern", "general/remove")
                .tooltip("Remove")
                .disabled(self.file_types.pattern.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.remove_wildcard(cx))),
        ]);
        h_flex()
            .flex_1()
            .min_h_0()
            .items_start()
            .gap(px(12.))
            .child(v_flex().w(relative(0.3)).h_full().child(Self::list_box(
                "file-types",
                "Recognized File Types",
                language_rows,
                cx,
            )))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .gap(px(4.))
                    .child(crate::text_label(
                        "file-name-patterns",
                        "File name patterns:",
                    ))
                    .child(toolbar)
                    .child(
                        Self::list_box("file-patterns", "File name patterns", pattern_rows, cx)
                            .when(empty, |list| {
                                list.child(
                                    crate::text_label(
                                        "no-file-patterns",
                                        "No registered file patterns",
                                    )
                                    .p(px(8.))
                                    .text_color(tokens.text_secondary),
                                )
                            }),
                    ),
            )
    }

    fn ignored_tab(&mut self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let rows: Vec<AnyElement> = strings(self.draft.get(EXCLUSIONS))
            .into_iter()
            .enumerate()
            .map(|(ix, pattern)| {
                Self::list_row(
                    ("ignored-pattern", ix),
                    pattern.into(),
                    self.file_types.ignored == Some(ix),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.file_types.ignored = Some(ix);
                    cx.notify();
                }))
                .into_any_element()
            })
            .collect();
        let selected = self.file_types.ignored;
        let toolbar = Self::toolbar(vec![
            spork_workbench::ActionButton::new("add-ignored", "general/add")
                .tooltip("Add")
                .on_click(cx.listener(|this, _, window, cx| this.edit_ignored(None, window, cx))),
            spork_workbench::ActionButton::new("remove-ignored", "general/remove")
                .tooltip("Remove")
                .disabled(selected.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.remove_ignored(cx))),
            spork_workbench::ActionButton::new("edit-ignored", "general/edit")
                .tooltip("Edit")
                .disabled(selected.is_none())
                .on_click(
                    cx.listener(move |this, _, window, cx| this.edit_ignored(selected, window, cx)),
                ),
        ]);
        let field = self.file_types.editing.as_ref().map(|(input, _, _)| {
            spork_workbench::TextField::new(input).aria_label("Ignored pattern")
        });
        v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(4.))
            .child(toolbar)
            .children(field)
            .child(Self::list_box(
                "ignored-files",
                "Ignored Files and Folders",
                rows,
                cx,
            ))
            .child(
                crate::text_label(
                    "ignored-files-hint",
                    "Ignored files and folders are not visible in the IDE and not indexed",
                )
                .text_ui_small()
                .text_color(tokens.text_secondary),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::strings;
    use serde_json::json;

    #[test]
    fn reads_a_list_of_patterns() {
        assert_eq!(strings(Some(&json!(["*.a", 1, "*.b"]))), ["*.a", "*.b"]);
        assert!(strings(None).is_empty());
    }
}
