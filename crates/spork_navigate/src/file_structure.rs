//! File Structure (⌘F12), IntelliJ's `FileStructurePopup`: the active editor's file outline under
//! its name, each element with its icon, the one around the caret selected. Typing filters it
//! (IntelliJ's speed search), keeping the elements around a match; ⏎ goes to the element.

use editor::{Anchor, Editor, SelectionEffects, ToPoint as _, scroll::Autoscroll};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_kit::component::{h_flex, v_flex};
use language::Point;
use spork_workbench::{Tokens, TreeRow, TreeState, TreeView, popup, tree_keys};
use workspace::{ModalView, Workspace};

use crate::navigate;

/// IntelliJ's size for it (`FileStructurePopup.createCenterPanel`).
const WIDTH: f32 = 540.;
const HEIGHT: f32 = 500.;
/// How far a tree level goes in.
const INDENT: f32 = 16.;

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &navigate::FileStructure, window, cx| {
        if workspace.active_modal::<FileStructure>(cx).is_some() {
            workspace.hide_modal(window, cx);
            return;
        }
        let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
            return;
        };
        let Some((title, elements, at_caret)) = editor.update(cx, |editor, cx| {
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let buffer = snapshot.as_singleton()?;
            let title = buffer
                .file()
                .map(|file| file.file_name(cx).to_owned())
                .unwrap_or_else(|| "untitled".to_owned());
            let outline = snapshot.outline(None)?;
            let caret = editor
                .selections
                .newest::<Point>(&editor.display_snapshot(cx))
                .head();
            let elements: Vec<Element> = outline
                .items
                .into_iter()
                .map(|item| {
                    let (name, kind) = split_name(&item.text, &item.name_ranges);
                    Element {
                        name: name.into(),
                        icon: icon(&kind, item.depth),
                        depth: item.depth,
                        start: item.range.start.to_point(&snapshot),
                        end: item.range.end.to_point(&snapshot),
                        target: item.selection_range.start,
                    }
                })
                .collect();
            // the innermost element around the caret
            let at_caret = elements
                .iter()
                .rposition(|element| element.start <= caret && caret <= element.end);
            Some((SharedString::from(title), elements, at_caret))
        }) else {
            return;
        };
        workspace.toggle_modal(window, cx, move |_, cx| {
            let mut popup = FileStructure {
                editor: editor.downgrade(),
                title,
                elements,
                focus: cx.focus_handle(),
                filter: String::new(),
                shown: Vec::new(),
                matched: Vec::new(),
                list: TreeState::new(),
            };
            popup.refilter(at_caret, cx);
            popup
        });
    });
}

/// An element of the file's outline.
struct Element {
    name: SharedString,
    icon: &'static str,
    depth: usize,
    start: Point,
    end: Point,
    /// Where going to it puts the caret: its name.
    target: Anchor,
}

/// The element's name, and the words before it that say what it is (`pub fn`, `struct`).
pub(crate) fn split_name(text: &str, name_ranges: &[std::ops::Range<usize>]) -> (String, String) {
    let Some(first) = name_ranges.first() else {
        return (text.to_owned(), String::new());
    };
    let start = first.start.min(text.len());
    (
        text.get(start..).unwrap_or(text).to_owned(),
        text.get(..start).unwrap_or_default().to_owned(),
    )
}

/// IntelliJ's node icon for an element, told by the keyword before its name.
// shortcut: the outline (tree-sitter's) has no symbol kind, so the keyword tells it, which suits
// the languages that write one (Rust, Python, JavaScript, Go); use the language server's document
// symbols for their kinds once code intelligence comes (phase 7)
pub(crate) fn icon(kind: &str, depth: usize) -> &'static str {
    let words: Vec<&str> = kind.split_whitespace().collect();
    let has = |word: &str| words.contains(&word);
    if has("fn") || has("def") || has("func") || has("function") {
        return if depth > 0 {
            "nodes/method"
        } else {
            "nodes/function"
        };
    }
    if has("struct") || has("class") || has("impl") || has("object") {
        "nodes/class"
    } else if has("enum") {
        "nodes/enum"
    } else if has("trait") || has("interface") || has("protocol") {
        "nodes/interface"
    } else if has("mod") || has("module") || has("namespace") || has("package") {
        "nodes/module"
    } else if has("const") || has("static") {
        "nodes/constant"
    } else if has("type") {
        "nodes/type"
    } else if has("let") || has("var") {
        "nodes/variable"
    } else if depth > 0 {
        "nodes/field"
    } else {
        "nodes/unknown"
    }
}

pub(crate) struct FileStructure {
    editor: gpui::WeakEntity<Editor>,
    title: SharedString,
    elements: Vec<Element>,
    focus: FocusHandle,
    filter: String,
    /// The elements showing: the matches and the elements around them.
    shown: Vec<usize>,
    /// The elements whose name has the filter.
    matched: Vec<usize>,
    /// The selected element, by its index in `elements`.
    list: TreeState<usize>,
}

impl EventEmitter<DismissEvent> for FileStructure {}

impl ModalView for FileStructure {}

impl Focusable for FileStructure {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl FileStructure {
    /// Shows the elements whose name has the filter and those around them; selects `select`
    /// (an element), else the first match.
    fn refilter(&mut self, select: Option<usize>, cx: &mut Context<Self>) {
        let filter = self.filter.to_lowercase();
        self.matched = (0..self.elements.len())
            .filter(|ix| self.elements[*ix].name.to_lowercase().contains(&filter))
            .collect();
        let mut keep = vec![false; self.elements.len()];
        for &ix in &self.matched {
            keep[ix] = true;
            // the elements around it: the ones before it that are less deep
            let mut depth = self.elements[ix].depth;
            for parent in (0..ix).rev() {
                if depth == 0 {
                    break;
                }
                if self.elements[parent].depth < depth {
                    keep[parent] = true;
                    depth = self.elements[parent].depth;
                }
            }
        }
        self.shown = (0..self.elements.len()).filter(|ix| keep[*ix]).collect();
        let select = select
            .or_else(|| self.matched.first().copied())
            .filter(|element| self.shown.contains(element))
            .or_else(|| self.shown.first().copied());
        self.list.select(select);
        cx.notify();
    }

    fn choose(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self
            .list
            .lead()
            .and_then(|ix| self.elements.get(*ix))
            .map(|element| element.target)
        else {
            return;
        };
        cx.emit(DismissEvent);
        self.editor
            .update(cx, |editor, cx| {
                editor.change_selections(
                    SelectionEffects::scroll(Autoscroll::center()).unfold(),
                    window,
                    cx,
                    |selections| selections.select_ranges([target..target]),
                );
                window.focus(&editor.focus_handle(cx), cx);
            })
            .ok();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return;
        }
        if keystroke.key == "backspace" {
            if self.filter.pop().is_some() {
                self.refilter(None, cx);
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
            self.refilter(None, cx);
        }
    }
}

impl Render for FileStructure {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let filter = self.filter.to_lowercase();
        let selected_rows = self.list.selected_range(&self.tree_rows(cx));
        let rows: Vec<gpui::AnyElement> = self
            .shown
            .iter()
            .enumerate()
            .filter_map(|(row, ix)| Some((row, *ix, self.elements.get(*ix)?)))
            .map(|(row, ix, element)| {
                let selected = selected_rows
                    .as_ref()
                    .is_some_and(|rows| rows.contains(&row));
                // a match's name in the link color where it has the filter, as IntelliJ marks it
                let matched = !filter.is_empty() && self.matched.contains(&ix);
                let name = element.name.clone();
                let found = matched
                    .then(|| name.to_lowercase().find(&filter))
                    .flatten()
                    .filter(|start| {
                        name.is_char_boundary(*start) && name.is_char_boundary(start + filter.len())
                    });
                let label = match found {
                    Some(start) => h_flex()
                        .child(name[..start].to_owned())
                        .child(
                            div()
                                .text_color(tokens.text_link)
                                .child(name[start..start + filter.len()].to_owned()),
                        )
                        .child(name[start + filter.len()..].to_owned()),
                    None => h_flex().child(name.clone()),
                };
                popup::row(("element", row), selected, cx)
                    .role(Role::TreeItem)
                    .aria_label(name)
                    .aria_selected(selected)
                    .pl(px(
                        12. - popup::SELECTION_INSET + INDENT * element.depth as f32
                    ))
                    .child(spork_workbench::image(element.icon, cx))
                    .child(label.min_w_0().truncate())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.list.select(Some(ix));
                        this.choose(window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        let surface = tree_keys(
            popup::surface(cx)
                .id("file-structure")
                .role(Role::Dialog)
                .aria_label("File Structure")
                .w(px(WIDTH))
                .h(px(HEIGHT))
                .track_focus(&self.focus)
                .key_context("FileStructure menu tree"),
            cx,
        );
        spork_vcs::spork_look(cx).child(
            surface
                .on_action(cx.listener(|this, _: &menu::Cancel, _, cx| {
                    if this.filter.is_empty() {
                        cx.emit(DismissEvent);
                    } else {
                        this.filter.clear();
                        this.refilter(None, cx);
                    }
                }))
                .on_key_down(cx.listener(Self::key_down))
                .child(
                    h_flex()
                        .flex_none()
                        .h(px(32.))
                        .px(px(12.))
                        .border_b_1()
                        .border_color(tokens.popup_border)
                        .child(
                            div()
                                .id("title")
                                .role(Role::Label)
                                .aria_label(self.title.clone())
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(self.title.clone()),
                        ),
                )
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
                        .id("structure")
                        .role(Role::Tree)
                        .aria_label("Structure")
                        .flex_1()
                        .min_h_0()
                        .py(px(4.))
                        .track_scroll(self.list.scroll_handle())
                        .overflow_y_scroll()
                        .when(empty, |tree| {
                            tree.child(
                                div()
                                    .p(px(12.))
                                    .text_color(tokens.text_secondary)
                                    .child("Nothing to show"),
                            )
                        })
                        .children(rows),
                ),
        )
    }
}

impl TreeView for FileStructure {
    type Key = usize;

    fn tree(&mut self) -> &mut TreeState<usize> {
        &mut self.list
    }

    fn tree_rows(&self, _: &App) -> Vec<TreeRow<usize>> {
        self.shown
            .iter()
            .filter_map(|ix| {
                let element = self.elements.get(*ix)?;
                Some(TreeRow::new(*ix, element.depth, None, element.name.clone()))
            })
            .collect()
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.choose(window, cx);
    }

    // its own speed search, filtering the outline
    fn speed_search(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{icon, split_name};

    #[test]
    fn tells_an_element_by_its_keyword() {
        let (name, kind) = split_name("pub fn hello", &[7..12]);
        assert_eq!((name.as_str(), kind.as_str()), ("hello", "pub fn "));
        assert_eq!(icon(&kind, 0), "nodes/function");
        assert_eq!(icon("fn ", 1), "nodes/method");
        assert_eq!(icon("struct ", 0), "nodes/class");
        assert_eq!(icon("", 1), "nodes/field");
    }
}
