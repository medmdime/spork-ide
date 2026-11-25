//! IntelliJ's Markdown structure actions beside markdown.rs's styling: Set Header Style and
//! Increase/Decrease Header Level (`SetHeaderLevelAction`, `HeaderUpAction`, `HeaderDownAction`),
//! Create Or Change List (`CreateOrChangeListPopupAction`), Convert to Reference
//! (`MarkdownIntroduceLinkReferenceAction`), Insert Table with its size grid
//! (`InsertEmptyTableAction`), Insert Image with its dialog (`InsertImageAction`), and the
//! floating toolbar over a selection (`Markdown.Toolbar.Floating`).

use std::ops::Range;

use editor::{Editor, EditorEvent};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels,
    Point, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _, WeakEntity,
    Window, anchored, deferred, div, point, px, svg,
};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use spork_code::Row;
use spork_navigate::list_popup::{RowIcon, anchored_popup, caret_origin, show_list_popup};
use spork_workbench::Tokens;
use spork_workbench::tokens::metrics;
use util::ResultExt as _;
use workspace::{ModalView, Workspace};

use crate::markdown::{
    CreateLink, GenerateTableOfContents, ToggleBold, ToggleCodeSpan, ToggleItalic,
    ToggleStrikethrough, apply, enclosing, is_markdown, selections, snapshot,
};

gpui::actions!(
    spork_editor,
    [
        /// Shows the header styles for the caret's line: Normal, Title, Subtitle, Heading 1 to 4.
        SetHeaderStyle,
        /// Takes a `#` off the caret's header (IntelliJ's Increase Header Level); a first-level
        /// header becomes text.
        IncreaseHeaderLevel,
        /// Puts a `#` more on the caret's header (IntelliJ's Decrease Header Level); text becomes
        /// a first-level header.
        DecreaseHeaderLevel,
        /// Shows the list kinds (unordered, ordered, checkmark) for the selected lines.
        CreateOrChangeList,
        /// Turns the inline link at the caret into a reference link, its definition at the end.
        ConvertToReference,
        /// Shows a grid to choose a table's size, then inserts the empty table at the caret.
        InsertTable,
        /// Shows the Insert Image dialog, then inserts the image at the carets.
        InsertImage,
    ]
);

pub(crate) fn init(cx: &mut App) {
    // IntelliJ's `Markdown.InsertGroup` in the Generate popup (⌘N) of a Markdown file
    spork_code::add_generate_rows("Markdown", generate_rows, cx);
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(
            |workspace, _: &markdown_preview::AdjustFontSize, window, cx| {
                let origin = window.mouse_position();
                let fs = workspace.app_state().fs.clone();
                workspace.toggle_modal(window, cx, move |_, cx| PreviewFontSize {
                    origin,
                    fs,
                    focus: cx.focus_handle(),
                });
            },
        );
    })
    .detach();
}

/// IntelliJ's preview font sizes (`AdjustFontSizeAction`'s steps).
const PREVIEW_FONT_SIZES: [f32; 16] = [
    8., 9., 10., 11., 12., 14., 16., 18., 20., 22., 24., 26., 28., 36., 48., 72.,
];

/// The step after (or before) `size` among IntelliJ's preview font sizes.
fn preview_font_step(size: f32, up: bool) -> Option<f32> {
    match up {
        true => PREVIEW_FONT_SIZES
            .iter()
            .copied()
            .find(|step| *step > size + 0.01),
        false => PREVIEW_FONT_SIZES
            .iter()
            .rev()
            .copied()
            .find(|step| *step < size - 0.01),
    }
}

/// IntelliJ's Adjust Font Size… of the Markdown preview: "Font size:", − the size +, writing the
/// preview's font size setting.
struct PreviewFontSize {
    origin: Point<Pixels>,
    fs: std::sync::Arc<dyn fs::Fs>,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for PreviewFontSize {}

impl ModalView for PreviewFontSize {}

impl Focusable for PreviewFontSize {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PreviewFontSize {
    fn size(cx: &App) -> f32 {
        use settings::Settings as _;
        f32::from(theme_settings::ThemeSettings::get_global(cx).markdown_preview_font_size(cx))
    }

    fn step(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(size) = preview_font_step(Self::size(cx), up) else {
            return;
        };
        settings::update_settings_file(self.fs.clone(), cx, move |settings, _| {
            settings.markdown_preview.get_or_insert_default().font_size = Some(size.into());
        });
        cx.notify();
    }
}

impl Render for PreviewFontSize {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = Self::size(cx);
        let (down, up) = (
            preview_font_step(size, false).is_some(),
            preview_font_step(size, true).is_some(),
        );
        let shown: SharedString = format!("{}", size.round()).into();
        let surface = spork_workbench::popup::surface(cx)
            .id("preview-font-size")
            .role(Role::Dialog)
            .aria_label("Adjust Font Size")
            .track_focus(&self.focus)
            .key_context("PreviewFontSize")
            .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(5.))
            .child(
                div()
                    .id("preview-font-size-label")
                    .role(Role::Label)
                    .aria_label("Font size:")
                    .child("Font size:"),
            )
            .child(
                spork_workbench::ActionButton::new("preview-font-smaller", "general/remove")
                    .tooltip("Decrease")
                    .disabled(!down)
                    .on_click(cx.listener(|this, _, _, cx| this.step(false, cx))),
            )
            .child(
                div()
                    .id("preview-font-size-value")
                    .role(Role::Label)
                    .aria_label(shown.clone())
                    .w(px(22.))
                    .text_center()
                    .child(shown),
            )
            .child(
                spork_workbench::ActionButton::new("preview-font-larger", "general/add")
                    .tooltip("Increase")
                    .disabled(!up)
                    .on_click(cx.listener(|this, _, _, cx| this.step(true, cx))),
            );
        anchored_popup(self.origin, surface, cx)
    }
}

pub(crate) fn register(editor: &mut Editor, handle: WeakEntity<Editor>) {
    macro_rules! on {
        ($action:ty, $run:expr) => {
            editor
                .register_action({
                    let handle = handle.clone();
                    move |_: &$action, window, cx| {
                        let Some(editor) = handle.upgrade() else {
                            return;
                        };
                        if is_markdown(editor.read(cx), cx) {
                            $run(&editor, window, cx)
                        }
                    }
                })
                .detach();
        };
    }
    on!(SetHeaderStyle, show_header_styles);
    on!(IncreaseHeaderLevel, |editor: &Entity<Editor>,
                              window,
                              cx| {
        step_header(editor, -1, window, cx)
    });
    on!(DecreaseHeaderLevel, |editor: &Entity<Editor>,
                              window,
                              cx| {
        step_header(editor, 1, window, cx)
    });
    on!(CreateOrChangeList, show_list_kinds);
    on!(ConvertToReference, |editor: &Entity<Editor>, window, cx| {
        editor.update(cx, |editor, cx| convert_to_reference(editor, window, cx))
    });
    on!(InsertTable, show_table_grid);
    on!(InsertImage, show_image_dialog);
    // Escape puts the floating toolbar away until the selection changes
    editor
        .register_action(move |_: &editor::actions::Cancel, _, cx| {
            handle
                .update(cx, |editor, cx| {
                    if let Some(state) = editor.addon_mut::<FloatingToolbar>() {
                        state.dismissed = true;
                    }
                    cx.propagate();
                })
                .ok();
        })
        .detach();
}

/// The workspace of the editor's window, to show a popup in.
fn workspace_of(window: &mut Window, cx: &mut App) -> Option<WeakEntity<Workspace>> {
    Some(Workspace::for_window(window, cx)?.downgrade())
}

// --- headers ---

/// A line's header, as IntelliJ's `SetHeaderLevelImpl` reads it.
#[derive(Debug, PartialEq)]
struct LineHeader {
    /// 1 to 6, or 0 for text.
    level: usize,
    /// The line, with a setext header's underline.
    range: Range<usize>,
    /// What stays before the header's marks: indentation, a quote's `>`, a list item's marker.
    prefix: String,
    content: String,
    /// A paragraph's line of its own (no prefix, not a header): blank lines go around it.
    plain: bool,
}

/// The start and end (without its line break) of the line holding `offset`.
fn line_bounds(text: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |at| at + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |at| offset + at);
    start..end
}

/// The length of a list item's marker with the spaces after it (`* `, `12. `), if `line` starts
/// with one.
fn marker_len(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mark = match bytes.first()? {
        b'*' | b'+' | b'-' => 1,
        b'0'..=b'9' => {
            let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
            match bytes.get(digits) {
                Some(b'.' | b')') if digits <= 9 => digits + 1,
                _ => return None,
            }
        }
        _ => return None,
    };
    match bytes.get(mark) {
        Some(b' ' | b'\t') => {
            let spaces = bytes[mark..]
                .iter()
                .take_while(|b| **b == b' ' || **b == b'\t')
                .count();
            Some(mark + spaces)
        }
        None => Some(mark),
        _ => None,
    }
}

/// The indentation, quote marks and list marker before a line's text.
fn line_prefix(line: &str) -> usize {
    let mut at = line.len() - line.trim_start().len();
    while line[at..].starts_with('>') {
        at += 1;
        at += line[at..].len() - line[at..].trim_start().len();
    }
    if let Some(marker) = marker_len(&line[at..]) {
        at += marker;
    }
    at
}

fn line_header(text: &str, offset: usize) -> LineHeader {
    let bounds = line_bounds(text, offset);
    let line = &text[bounds.clone()];
    let prefix_len = line_prefix(line);
    let prefix = line[..prefix_len].to_owned();
    let rest = &line[prefix_len..];
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && (rest.len() == hashes || rest[hashes..].starts_with(' ')) {
        let content = rest[hashes..].trim();
        // a closing sequence of `#` after a space isn't the header's text
        let content = match content.trim_end_matches('#') {
            open if open.len() < content.len() && (open.is_empty() || open.ends_with(' ')) => {
                open.trim_end()
            }
            _ => content,
        };
        return LineHeader {
            level: hashes,
            range: bounds,
            prefix,
            content: content.to_owned(),
            plain: false,
        };
    }
    // a setext header: a line of text over a line of `=` (first level) or `-` (second)
    if prefix.is_empty() && !rest.trim().is_empty() && bounds.end < text.len() {
        let next = line_bounds(text, bounds.end + 1);
        let underline = text[next.clone()].trim();
        let level = match underline.chars().next() {
            Some('=') if underline.chars().all(|c| c == '=') => 1,
            Some('-') if underline.chars().all(|c| c == '-') => 2,
            _ => 0,
        };
        if level > 0 {
            return LineHeader {
                level,
                range: bounds.start..next.end,
                prefix,
                content: rest.trim().to_owned(),
                plain: false,
            };
        }
    }
    LineHeader {
        level: 0,
        range: bounds,
        plain: prefix.is_empty(),
        prefix,
        content: rest.to_owned(),
    }
}

/// The edit giving the line at `offset` the header `level` (0 for text): an ATX header, as
/// IntelliJ writes one; a paragraph's line made a header gets a blank line around it.
fn set_header(text: &str, offset: usize, level: usize) -> Option<(Range<usize>, String)> {
    let header = line_header(text, offset);
    if header.level == level || (header.level == 0 && header.content.trim().is_empty()) {
        return None;
    }
    let mut new = header.prefix.clone();
    match level {
        0 => new.push_str(header.content.trim()),
        level => {
            new.push_str(&"#".repeat(level));
            new.push(' ');
            new.push_str(header.content.trim());
        }
    }
    if header.level == 0 && header.plain {
        let before = header.range.start;
        let after = header.range.end;
        if before > 0 && !text[line_bounds(text, before - 1)].trim().is_empty() {
            new.insert(0, '\n');
        }
        if after < text.len() && !text[line_bounds(text, after + 1)].trim().is_empty() {
            new.push('\n');
        }
    }
    Some((header.range, new))
}

/// IntelliJ's header styles: its names, and the levels they are.
const HEADER_STYLES: [(&str, usize); 7] = [
    ("Normal", 0),
    ("Title", 1),
    ("Subtitle", 2),
    ("Heading 1", 3),
    ("Heading 2", 4),
    ("Heading 3", 5),
    ("Heading 4", 6),
];

/// The header level every caret's line has, if they agree.
fn carets_header_level(editor: &Editor, cx: &mut Context<Editor>) -> Option<usize> {
    let text = snapshot(editor, cx)?.text();
    let mut levels = selections(editor, cx)
        .into_iter()
        .map(|selection| line_header(&text, selection.start).level);
    let first = levels.next()?;
    levels.all(|level| level == first).then_some(first)
}

/// Gives each caret's line the header `level`.
fn apply_header(editor: &mut Editor, level: usize, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let text = buffer.text();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for selection in selections(editor, cx) {
        if let Some(edit) = set_header(&text, selection.start, level)
            && !edits.iter().any(|(range, _)| *range == edit.0)
        {
            edits.push(edit);
        }
    }
    if edits.is_empty() {
        return;
    }
    // each caret at the end of its new line
    let mut ranges = Vec::new();
    let mut delta = 0isize;
    let mut sorted = edits.clone();
    sorted.sort_by_key(|(range, _)| range.start);
    for (range, new) in &sorted {
        let end = (range.start as isize + delta) as usize + new.trim_end_matches('\n').len();
        ranges.push(end..end);
        delta += new.len() as isize - range.len() as isize;
    }
    apply(editor, edits, ranges, window, cx);
}

fn show_header_styles(editor: &Entity<Editor>, window: &mut Window, cx: &mut App) {
    let Some(workspace) = workspace_of(window, cx) else {
        return;
    };
    let current = editor.update(cx, |editor, cx| carets_header_level(editor, cx));
    let rows = HEADER_STYLES
        .iter()
        .enumerate()
        .flat_map(|(ix, (name, level))| {
            let editor = editor.downgrade();
            let level = *level;
            let row = Row::item(*name, move |window, cx| {
                editor
                    .update(cx, |editor, cx| apply_header(editor, level, window, cx))
                    .ok();
            })
            .icon(Some(match current == Some(level) {
                true => RowIcon::Tinted("actions/checked"),
                false => RowIcon::Tinted(""),
            }))
            .hint((level > 0).then(|| SharedString::from(format!("(H{level})"))));
            // IntelliJ's separators: after Normal, and after Heading 2
            let separator = matches!(ix, 0 | 4).then(|| Row::Separator(None));
            std::iter::once(row).chain(separator)
        })
        .collect();
    show_list_popup(&workspace, editor, "", rows, false, window, cx);
}

/// Increase (`step` -1) or Decrease (+1) Header Level on each caret's line: Increase only on a
/// header, Decrease up to the sixth level.
fn step_header(editor: &Entity<Editor>, step: isize, window: &mut Window, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        let Some(level) = carets_header_level(editor, cx) else {
            return;
        };
        let target = level as isize + step;
        if (step < 0 && level == 0) || !(0..=6).contains(&target) {
            return;
        }
        apply_header(editor, target as usize, window, cx);
    });
}

// --- lists ---

#[derive(Clone, Copy, Debug, PartialEq)]
enum ListKind {
    Unordered,
    Ordered,
    Checkmark,
}

impl ListKind {
    const ALL: [ListKind; 3] = [ListKind::Unordered, ListKind::Ordered, ListKind::Checkmark];

    fn label(self) -> &'static str {
        match self {
            Self::Unordered => "Unordered List",
            Self::Ordered => "Ordered List",
            Self::Checkmark => "Checkmark List",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Unordered => "markdown/editorActions/bulletList",
            Self::Ordered => "markdown/editorActions/numberedList",
            Self::Checkmark => "markdown/editorActions/checkmarkList",
        }
    }

    /// The `n`th item's marker (from 1), a checkmark item keeping its state.
    fn marker(self, n: usize, checked: Option<bool>) -> String {
        match self {
            Self::Unordered => "* ".into(),
            Self::Ordered => format!("{n}. "),
            Self::Checkmark => {
                let check = if checked == Some(true) { 'x' } else { ' ' };
                format!("{n}. [{check}] ")
            }
        }
    }
}

/// A list item's line: where its marker (with a checkbox) starts and ends, and its checkbox.
#[derive(Debug, PartialEq)]
struct ListItem {
    marker: Range<usize>,
    checked: Option<bool>,
}

/// A line's list item: its indentation, and its marker and checkbox as offsets in the line.
fn item_of_line(line: &str) -> Option<(usize, Range<usize>, Option<bool>, bool)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let marker = marker_len(rest)?;
    let ordered = rest.as_bytes().first().is_some_and(u8::is_ascii_digit);
    let after = &rest[marker..];
    let check = ["[ ]", "[x]", "[X]"]
        .iter()
        .find(|check| after.starts_with(**check))
        .map(|check| *check != "[ ]");
    let mut end = indent + marker;
    if check.is_some() {
        end += 3;
        end += line[end..].len() - line[end..].trim_start().len();
    }
    Some((indent, indent..end, check, ordered))
}

/// The list holding the line at `offset`: its kind (its first item's) and its items at the
/// level of the caret's item, as IntelliJ re-marks a list's top-level items.
fn list_at(text: &str, offset: usize) -> Option<(ListKind, Vec<ListItem>)> {
    let mut lines = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        lines.push(start..start + line.len());
        start += line.len() + 1;
    }
    let at = lines
        .iter()
        .position(|line| line.start <= offset && offset <= line.end)?;
    let line = |ix: usize| &text[lines[ix].clone()];
    let indent = |ix: usize| line(ix).len() - line(ix).trim_start().len();
    let blank = |ix: usize| line(ix).trim().is_empty();
    // the item the caret's line is, or belongs to (a continuation line is indented more)
    let mut item = at;
    while item_of_line(line(item)).is_none() || (item != at && indent(item) > indent(at)) {
        if item == 0
            || (blank(item) && item != at)
            || (indent(item) == 0
                && !blank(item)
                && item_of_line(line(item)).is_none()
                && item != at)
        {
            return None;
        }
        item -= 1;
    }
    let base = indent(item);
    // the lines of the list: items at its level, and what is indented under them
    let belongs = |ix: usize| {
        !blank(ix)
            && (indent(ix) > base || (indent(ix) == base && item_of_line(line(ix)).is_some()))
    };
    let next_belongs = |from: usize, step: isize| {
        let mut ix = from as isize + step;
        while ix >= 0 && (ix as usize) < lines.len() && blank(ix as usize) {
            ix += step;
        }
        ix >= 0 && (ix as usize) < lines.len() && belongs(ix as usize)
    };
    let mut first = item;
    while first > 0 && (belongs(first - 1) || (blank(first - 1) && next_belongs(first - 1, -1))) {
        first -= 1;
    }
    let mut last = item;
    while last + 1 < lines.len()
        && (belongs(last + 1) || (blank(last + 1) && next_belongs(last + 1, 1)))
    {
        last += 1;
    }
    let mut items = Vec::new();
    let mut kind = None;
    for ix in first..=last {
        if indent(ix) != base {
            continue;
        }
        let Some((_, marker, checked, ordered)) = item_of_line(line(ix)) else {
            continue;
        };
        kind.get_or_insert(match (checked, ordered) {
            (Some(_), _) => ListKind::Checkmark,
            (None, true) => ListKind::Ordered,
            (None, false) => ListKind::Unordered,
        });
        let start = lines[ix].start;
        items.push(ListItem {
            marker: start + marker.start..start + marker.end,
            checked,
        });
    }
    Some((kind?, items))
}

/// IntelliJ's Create Or Change List for a selection: its list re-marked as `kind`, or made text
/// when it is one already; lines not in a list each become an item.
fn change_list(text: &str, selection: Range<usize>, kind: ListKind) -> Vec<(Range<usize>, String)> {
    if let Some((current, items)) = list_at(text, selection.start) {
        return items
            .iter()
            .enumerate()
            .map(|(ix, item)| {
                let marker = match current == kind {
                    true => String::new(),
                    false => kind.marker(ix + 1, item.checked),
                };
                (item.marker.clone(), marker)
            })
            .collect();
    }
    let first = line_bounds(text, selection.start).start;
    let last = line_bounds(text, selection.end).start;
    let mut edits = Vec::new();
    let mut start = first;
    let mut n = 1;
    loop {
        edits.push((start..start, kind.marker(n, None)));
        n += 1;
        let end = line_bounds(text, start).end;
        if start >= last || end >= text.len() {
            break;
        }
        start = end + 1;
    }
    edits
}

/// The list kind of the caret's line, if it is in a list.
fn caret_list_kind(editor: &Editor, cx: &mut Context<Editor>) -> Option<ListKind> {
    let text = snapshot(editor, cx)?.text();
    let selection = selections(editor, cx).into_iter().last()?;
    list_at(&text, selection.start).map(|(kind, _)| kind)
}

fn apply_list(editor: &mut Editor, kind: ListKind, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let text = buffer.text();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for selection in selections(editor, cx) {
        for edit in change_list(&text, selection, kind) {
            if !edits.iter().any(|(range, _)| range.start == edit.0.start) {
                edits.push(edit);
            }
        }
    }
    let carets: Vec<Range<usize>> = selections(editor, cx)
        .into_iter()
        .map(|selection| {
            let start = crate::markdown::map_offset(selection.start, &edits, false);
            let end = crate::markdown::map_offset(selection.end, &edits, false);
            start..end
        })
        .collect();
    apply(editor, edits, carets, window, cx);
}

fn show_list_kinds(editor: &Entity<Editor>, window: &mut Window, cx: &mut App) {
    let Some(workspace) = workspace_of(window, cx) else {
        return;
    };
    let Some(origin) = caret_origin(editor, window, cx) else {
        return;
    };
    let current = editor.update(cx, |editor, cx| caret_list_kind(editor, cx));
    let handle = editor.downgrade();
    workspace
        .update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |_, cx| ListKindPopup {
                editor: handle,
                current,
                origin,
                focus: cx.focus_handle(),
            })
        })
        .log_err();
}

/// IntelliJ's Create Or Change List popup: a row of the three list kinds, the caret's pressed.
struct ListKindPopup {
    editor: WeakEntity<Editor>,
    current: Option<ListKind>,
    origin: Point<Pixels>,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for ListKindPopup {}

impl ModalView for ListKindPopup {}

impl Focusable for ListKindPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl ListKindPopup {
    fn choose(&mut self, kind: ListKind, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        window.focus(&editor.focus_handle(cx), cx);
        editor.update(cx, |editor, cx| apply_list(editor, kind, window, cx));
    }
}

impl Render for ListKindPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buttons = ListKind::ALL.map(|kind| {
            toolbar_button(kind.label(), kind.icon(), self.current == Some(kind), cx)
                .on_click(cx.listener(move |this, _, window, cx| this.choose(kind, window, cx)))
                .into_any_element()
        });
        let surface = spork_workbench::popup::surface(cx)
            .id("create-or-change-list")
            .role(Role::Toolbar)
            .aria_label("Create Or Change List")
            .track_focus(&self.focus)
            .key_context("MarkdownListPopup")
            .on_action(cx.listener(|_, _: &menu::Cancel, _, cx| cx.emit(DismissEvent)))
            // a click in it isn't one outside the modal, which would dismiss it
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_row()
            .gap(px(2.))
            .p(px(4.))
            .children(buttons);
        anchored_popup(self.origin, surface, cx)
    }
}

// --- reference links ---

/// An inline link's parts: its text, destination and title (as written, quotes included).
fn inline_link_parts(link: &str) -> Option<(String, String, Option<String>)> {
    let inner = link.strip_prefix('[')?;
    // the text's closing bracket: the one before `(`, brackets inside it paired
    let mut depth = 0;
    let mut close = None;
    for (at, c) in inner.char_indices() {
        match c {
            '[' => depth += 1,
            ']' if depth == 0 => {
                close = Some(at);
                break;
            }
            ']' => depth -= 1,
            _ => {}
        }
    }
    let close = close?;
    let text = inner[..close].to_owned();
    let target = inner[close + 1..]
        .strip_prefix('(')?
        .strip_suffix(')')?
        .trim();
    // a destination in angle brackets may hold spaces
    let end = match target.strip_prefix('<') {
        Some(rest) => rest.find('>').map_or(target.len(), |close| close + 2),
        None => target.find([' ', '\t']).unwrap_or(target.len()),
    };
    let destination = target[..end]
        .strip_prefix('<')
        .and_then(|d| d.strip_suffix('>'))
        .unwrap_or(&target[..end])
        .to_owned();
    let title = Some(target[end..].trim().to_owned()).filter(|title| !title.is_empty());
    Some((text, destination, title))
}

/// IntelliJ's Convert to Reference: the link at the caret becomes `[text][]` and its definition
/// `[]: url "title"` goes to the end after a blank line, a caret in each label so that what is
/// typed names both (IntelliJ's template variable over the two).
fn convert_to_reference(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let Some(selection) = selections(editor, cx).into_iter().last() else {
        return;
    };
    let text = buffer.text();
    let (range, label_text, destination, title) =
        if let Some(link) = enclosing(&buffer, &selection, "inline_link") {
            let Some((label, destination, title)) = inline_link_parts(&text[link.clone()]) else {
                return;
            };
            (link, label, destination, title)
        } else if let Some(link) = enclosing(&buffer, &selection, "uri_autolink") {
            let url = text[link.clone()]
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_owned();
            (link, url.clone(), url, None)
        } else {
            return;
        };
    let reference = format!("[{label_text}][]");
    let definition = match &title {
        Some(title) => format!("\n\n[]: {destination} {title}"),
        None => format!("\n\n[]: {destination}"),
    };
    let label_caret = range.start + reference.len() - 1;
    let delta = reference.len() as isize - range.len() as isize;
    let definition_caret = (text.len() as isize + delta) as usize + 3;
    apply(
        editor,
        vec![(range, reference), (text.len()..text.len(), definition)],
        vec![label_caret..label_caret, definition_caret..definition_caret],
        window,
        cx,
    );
}

// --- tables ---

/// IntelliJ's empty table (`TableBuildingUtils`' aligned style): `rows` counts the header.
fn table_text(rows: usize, columns: usize) -> String {
    let row = |cell: &str| format!("|{}\n", format!("{cell}|").repeat(columns));
    let mut table = row("   ");
    table.push_str(&row("---"));
    for _ in 1..rows {
        table.push_str(&row("   "));
    }
    table
}

/// The table inserted at `offset`: a line break before it when the line above isn't empty.
fn table_insertion(text: &str, offset: usize, rows: usize, columns: usize) -> (String, usize) {
    let line = line_bounds(text, offset);
    let mut table = table_text(rows, columns);
    if line.start > 0 && !text[line_bounds(text, line.start - 1)].trim().is_empty() {
        table.insert(0, '\n');
    }
    // the caret in the first header cell
    let caret = offset + table.find('|').map_or(0, |bar| bar + 1);
    (table, caret)
}

fn insert_table(
    editor: &mut Editor,
    rows: usize,
    columns: usize,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let Some(caret) = selections(editor, cx).into_iter().last().map(|s| s.end) else {
        return;
    };
    let (table, at) = table_insertion(&buffer.text(), caret, rows, columns);
    apply(
        editor,
        vec![(caret..caret, table)],
        vec![at..at],
        window,
        cx,
    );
}

fn show_table_grid(editor: &Entity<Editor>, window: &mut Window, cx: &mut App) {
    let Some(workspace) = workspace_of(window, cx) else {
        return;
    };
    let Some(origin) = caret_origin(editor, window, cx) else {
        return;
    };
    let handle = editor.downgrade();
    workspace
        .update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, move |_, cx| TableGrid {
                editor: handle,
                rows: 1,
                columns: 1,
                shown: (GRID_START, GRID_START),
                origin,
                focus: cx.focus_handle(),
            })
        })
        .log_err();
}

/// The grid's first size, IntelliJ's largest, its growth and cell (`InsertEmptyTableAction`).
const GRID_START: usize = 4;
const GRID_MAX: usize = 10;
const GRID_GROWTH: usize = 2;
const GRID_CELL: f32 = 15.;
const GRID_GAP: f32 = 3.;

/// IntelliJ's table size chooser: cells to pick the rows and columns from, the size under them.
struct TableGrid {
    editor: WeakEntity<Editor>,
    rows: usize,
    columns: usize,
    /// The rows and columns the grid shows: it grows as the choice reaches its edge.
    shown: (usize, usize),
    origin: Point<Pixels>,
    focus: FocusHandle,
}

impl EventEmitter<DismissEvent> for TableGrid {}

impl ModalView for TableGrid {}

impl Focusable for TableGrid {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TableGrid {
    fn pick(&mut self, rows: usize, columns: usize, cx: &mut Context<Self>) {
        self.rows = rows.clamp(1, GRID_MAX);
        self.columns = columns.clamp(1, GRID_MAX);
        if self.rows >= self.shown.0 {
            self.shown.0 = (self.shown.0 + GRID_GROWTH).min(GRID_MAX);
        }
        if self.columns >= self.shown.1 {
            self.shown.1 = (self.shown.1 + GRID_GROWTH).min(GRID_MAX);
        }
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        window.focus(&editor.focus_handle(cx), cx);
        let (rows, columns) = (self.rows, self.columns);
        editor.update(cx, |editor, cx| {
            insert_table(editor, rows, columns, window, cx)
        });
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (rows, columns) = (self.rows, self.columns);
        match event.keystroke.key.as_str() {
            "up" => self.pick(rows.saturating_sub(1), columns, cx),
            "down" => self.pick(rows + 1, columns, cx),
            "left" => self.pick(rows, columns.saturating_sub(1), cx),
            "right" => self.pick(rows, columns + 1, cx),
            "enter" => self.confirm(window, cx),
            "escape" => cx.emit(DismissEvent),
            _ => return,
        }
        cx.stop_propagation();
    }
}

impl Render for TableGrid {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let editor_background = {
            use theme::ActiveTheme as _;
            cx.theme().colors().editor_background
        };
        let rows = (1..=self.shown.0).map(|row| {
            h_flex()
                .gap(px(GRID_GAP))
                .children((1..=self.shown.1).map(|column| {
                    let picked = row <= self.rows && column <= self.columns;
                    div()
                        .id(("table-cell", row * 16 + column))
                        .size(px(GRID_CELL))
                        .border_1()
                        .border_color(tokens.control_border)
                        .bg(if picked {
                            tokens.selection
                        } else {
                            editor_background
                        })
                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                            if (this.rows, this.columns) != (row, column) {
                                this.pick(row, column, cx)
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.pick(row, column, cx);
                            this.confirm(window, cx)
                        }))
                }))
        });
        let size: SharedString = format!("{}×{}", self.rows, self.columns).into();
        let surface = spork_workbench::popup::surface(cx)
            .id("insert-table")
            .role(Role::Dialog)
            .aria_label("Insert Table")
            .track_focus(&self.focus)
            .key_context("MarkdownTableGrid")
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .p(px(8.))
            .gap(px(GRID_GAP))
            .children(rows)
            .child(
                div()
                    .id("insert-table-size")
                    .role(Role::Label)
                    .aria_label(size.clone())
                    .w_full()
                    .pt(px(4.))
                    .text_center()
                    .child(size),
            );
        anchored_popup(self.origin, surface, cx)
    }
}

// --- images ---

/// IntelliJ's image text (`ImageUtils.createMarkdownImageText`): `![description](path "title")`,
/// the title in single quotes when it has a double one; a Windows path's backslashes as
/// slashes, but a network path's.
fn image_text(path: &str, title: &str, description: &str) -> String {
    let path = path.trim();
    let path = match path.starts_with("\\\\") {
        true => path.to_owned(),
        false => path.replace('\\', "/"),
    };
    let (title, description) = (title.trim(), description.trim());
    match title.is_empty() {
        true => format!("![{description}]({path})"),
        false if title.contains('"') => format!("![{description}]({path} '{title}')"),
        false => format!("![{description}]({path} \"{title}\")"),
    }
}

fn show_image_dialog(editor: &Entity<Editor>, window: &mut Window, cx: &mut App) {
    let dialog = cx.new(|cx| ImageDialog::new(editor.downgrade(), window, cx));
    let path = dialog.read(cx).path.clone();
    window.open_dialog(cx, move |modal, _, _| {
        let dialog = dialog.clone();
        modal
            .title(spork_workbench::controls::dialog_title("Insert Image"))
            .on_ok(|_, _, _| false)
            .w(px(460.))
            .child(dialog)
    });
    path.update(cx, |path, cx| path.focus(window, cx));
}

/// IntelliJ's Insert Image dialog (`ConfigureImageDialog`): the path, the size, and the screen
/// reader's title and description in a group closed at first.
struct ImageDialog {
    editor: WeakEntity<Editor>,
    path: Entity<InputState>,
    width: Entity<InputState>,
    height: Entity<InputState>,
    title: Entity<InputState>,
    description: Entity<TextareaState>,
    screen_reader: bool,
    _subscriptions: Vec<gpui::Subscription>,
}

impl ImageDialog {
    fn new(editor: WeakEntity<Editor>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            |window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx));
        let (path, width, height, title) = (
            input(window, cx),
            input(window, cx),
            input(window, cx),
            input(window, cx),
        );
        let description = cx.new(|cx| TextareaState::new(window, cx));
        let _subscriptions =
            [&path, &width, &height, &title]
                .into_iter()
                .map(|input| {
                    cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.save(window, cx),
                            InputEvent::Change => cx.notify(),
                            _ => {}
                        }
                    })
                })
                .collect();
        Self {
            editor,
            path,
            width,
            height,
            title,
            description,
            screen_reader: false,
            _subscriptions,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = image_text(
            &self.path.read(cx).value(),
            &self.title.read(cx).value(),
            &self.description.read(cx).value(),
        );
        window.close_dialog(cx);
        let Some(editor) = self.editor.upgrade() else {
            return;
        };
        window.focus(&editor.focus_handle(cx), cx);
        editor.update(cx, |editor, cx| {
            // at each caret, each caret after it
            let edits: Vec<(Range<usize>, String)> = selections(editor, cx)
                .into_iter()
                .map(|selection| (selection, text.clone()))
                .collect();
            let mut sorted: Vec<Range<usize>> =
                edits.iter().map(|(range, _)| range.clone()).collect();
            sorted.sort_by_key(|range| range.start);
            let mut delta = 0isize;
            let carets = sorted
                .iter()
                .map(|range| {
                    let end = (range.start as isize + delta) as usize + text.len();
                    delta += text.len() as isize - range.len() as isize;
                    end..end
                })
                .collect();
            apply(editor, edits, carets, window, cx);
        });
    }
}

impl Render for ImageDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let label = |text: &'static str| spork_workbench::controls::form_label(text).w(px(88.));
        let field = |state: &Entity<InputState>, text: &'static str| {
            div()
                .flex_1()
                .child(spork_workbench::TextField::new(state).aria_label(text))
        };
        let sized = !self.width.read(cx).value().trim().is_empty()
            || !self.height.read(cx).value().trim().is_empty();
        let screen_reader = self.screen_reader;
        v_flex()
            .id("insert-image-dialog")
            .role(Role::Group)
            .aria_label("Insert Image")
            .gap(px(10.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(label("Path:"))
                    .child(field(&self.path, "Path:")),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(label("Width:"))
                    .child(div().w(px(80.)).child(field(&self.width, "Width:")))
                    .child(spork_workbench::controls::form_label("Height:"))
                    .child(div().w(px(80.)).child(field(&self.height, "Height:"))),
            )
            .when(sized, |dialog| {
                dialog.child(
                    div()
                        .pl(px(96.))
                        .text_color(tokens.text_secondary)
                        .child("Convert to HTML"),
                )
            })
            .child(
                h_flex()
                    .id("screen-reader-text")
                    .role(Role::Button)
                    .aria_label("Screen Reader Text")
                    .aria_expanded(screen_reader)
                    .gap(px(4.))
                    .cursor_pointer()
                    .child(
                        spork_workbench::icon(match screen_reader {
                            true => "general/chevronDown",
                            false => "general/chevronRight",
                        })
                        .size(px(16.)),
                    )
                    .child("Screen Reader Text")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.screen_reader = !this.screen_reader;
                        cx.notify();
                    })),
            )
            .when(screen_reader, |dialog| {
                dialog
                    .child(
                        h_flex()
                            .pl(px(20.))
                            .gap(px(8.))
                            .child(label("Title:"))
                            .child(field(&self.title, "Title:")),
                    )
                    .child(
                        h_flex()
                            .pl(px(20.))
                            .gap(px(8.))
                            .items_start()
                            .child(label("Description:"))
                            .child(
                                // IntelliJ's text area of four rows
                                Textarea::new(&self.description)
                                    .aria_label("Description:")
                                    .h(px(80.))
                                    .flex_1(),
                            ),
                    )
            })
            .child(
                h_flex()
                    .pt(px(6.))
                    .gap(px(8.))
                    .justify_end()
                    .child(
                        spork_workbench::Button::new("insert-image-cancel", "Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        spork_workbench::Button::new("insert-image-save", "Save")
                            .primary(true)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

// --- the Generate popup ---

fn generate_rows(
    target: &spork_code::Target,
    _: &mut Window,
    cx: &mut App,
) -> Option<gpui::Task<Vec<Row>>> {
    if target.buffer.read(cx).language()?.name().as_ref() != "Markdown" {
        return None;
    }
    let row = |text: &'static str, action: Box<dyn gpui::Action>| {
        let editor = target.editor.downgrade();
        Row::item(text, move |window, cx| {
            if let Some(editor) = editor.upgrade() {
                editor
                    .focus_handle(cx)
                    .dispatch_action(action.as_ref(), window, cx);
            }
        })
    };
    Some(gpui::Task::ready(vec![
        row("Create Link", Box::new(CreateLink)),
        row(
            "Generate Table Of Contents",
            Box::new(GenerateTableOfContents),
        ),
        row("Insert Table", Box::new(InsertTable)),
        row("Insert Image", Box::new(InsertImage)),
    ]))
}

// --- the floating toolbar ---

/// Whether the floating toolbar was put away (Escape) for the selection it showed over.
#[derive(Default)]
pub(crate) struct FloatingToolbar {
    dismissed: bool,
}

impl editor::Addon for FloatingToolbar {
    fn to_any(&self) -> &dyn std::any::Any {
        self
    }

    fn to_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Watches a Markdown editor's selections: a new one brings the floating toolbar back.
pub(crate) fn watch_selections(editor: &mut Editor, cx: &mut Context<Editor>) {
    editor.register_addon(FloatingToolbar::default());
    cx.subscribe_self(|editor: &mut Editor, event: &EditorEvent, _| {
        if let EditorEvent::SelectionsChanged { .. } = event
            && let Some(state) = editor.addon_mut::<FloatingToolbar>()
        {
            state.dismissed = false;
        }
    })
    .detach();
}

/// The block kinds a selection's ends mustn't be in for the toolbar to show (IntelliJ's code,
/// HTML and link destinations).
const NO_TOOLBAR_IN: [&str; 6] = [
    "fenced_code_block",
    "indented_code_block",
    "code_span",
    "html_block",
    "link_destination",
    "html_tag",
];

/// One of the toolbar's 22 px buttons, pressed when its formatting is on.
fn toolbar_button(
    label: &'static str,
    icon: &'static str,
    pressed: bool,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let tokens = Tokens::get(cx);
    let dark = {
        use theme::ActiveTheme as _;
        !cx.theme().appearance().is_light()
    };
    div()
        .id(label)
        .role(Role::Button)
        .aria_label(label)
        .aria_toggled(gpui::Toggled::from(pressed))
        .size(px(metrics::TOOLBAR_BUTTON))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(metrics::RADIUS))
        .cursor_pointer()
        .when(pressed, |button| button.bg(tokens.toolbar_selected))
        .when(!pressed, |button| {
            button.hover(move |button| button.bg(tokens.hover))
        })
        .child(spork_workbench::image_in(icon, dark).size(px(metrics::TOOLBAR_ICON)))
}

/// The toolbar over a Markdown editor's selection, as IntelliJ shows it: Set Header Style, the
/// styling actions, Create Or Change List; above the caret's line, two buttons left of the caret.
// shortcut: it shows for any selection, the keyboard's too (IntelliJ waits for the mouse to make
// or hover it), and hides once the selection is empty or Escape is pressed; follow the mouse if
// keyboard selections find it in the way
pub(crate) fn floating_toolbar(
    editor: &mut Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    if !is_markdown(editor, cx)
        || editor.read_only(cx)
        || !editor.focus_handle(cx).is_focused(window)
        || editor.has_pending_selection()
        || editor
            .addon::<FloatingToolbar>()
            .is_none_or(|state| state.dismissed)
    {
        return None;
    }
    let buffer = snapshot(editor, cx)?;
    let selection = selections(editor, cx).into_iter().last()?;
    if selection.is_empty() {
        return None;
    }
    let inside = |offset: usize| {
        NO_TOOLBAR_IN
            .iter()
            .any(|kind| enclosing(&buffer, &(offset..offset), kind).is_some())
            || block_kind_at(&buffer, offset, &NO_TOOLBAR_IN)
    };
    if inside(selection.start) || inside(selection.end) {
        return None;
    }
    // above the caret's line, as IntelliJ's `FloatingToolbar` places it
    let head = editor.selections.newest_anchor().head();
    let snapshot = editor.snapshot(window, cx);
    let display = {
        use editor::display_map::ToDisplayPoint as _;
        head.to_display_point(&snapshot)
    };
    let line_bottom = editor.text_origin_below_row(display.row())?;
    let details = editor.text_layout_details(window, cx);
    let x = snapshot.x_for_display_point(display, &details);
    let line_height = editor
        .style(cx)
        .text
        .line_height_in_pixels(window.rem_size());
    let height = px(metrics::TOOLBAR_BUTTON + 8.);
    let origin = point(
        line_bottom.x + x - px(2. * metrics::TOOLBAR_BUTTON),
        line_bottom.y - line_height - height - px(2.),
    );

    let tokens = Tokens::get(cx);
    let pressed = |kind: &str| enclosing(&buffer, &selection, kind).is_some();
    let level = carets_header_level(editor, cx);
    let style = HEADER_STYLES
        .iter()
        .find(|(_, header)| Some(*header) == level)
        .map_or("Normal", |(name, _)| *name);
    let list = caret_list_kind(editor, cx);
    let handle = cx.entity().downgrade();
    let run = move |action: Box<dyn gpui::Action>| {
        let handle = handle.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            if let Some(editor) = handle.upgrade() {
                editor
                    .focus_handle(cx)
                    .dispatch_action(action.as_ref(), window, cx);
            }
        }
    };
    let separator = || div().w(px(1.)).h(px(16.)).mx(px(3.)).bg(tokens.separator);
    let header_style = div()
        .id("set-header-style")
        .role(Role::Button)
        .aria_label("Set Header Style")
        .aria_value(SharedString::from(style))
        .h(px(metrics::TOOLBAR_BUTTON))
        .min_w(px(metrics::TOOLBAR_BUTTON))
        .px(px(4.))
        .flex()
        .items_center()
        .gap(px(2.))
        .rounded(px(metrics::RADIUS))
        .cursor_pointer()
        .hover(move |button| button.bg(tokens.hover))
        .child(style)
        .child(
            svg()
                .path("wb/icons/general/chevronDown.svg")
                .size(px(12.))
                .text_color(tokens.icon),
        )
        .on_click(run(Box::new(SetHeaderStyle)));
    let toolbar = h_flex()
        .id("markdown-floating-toolbar")
        .role(Role::Toolbar)
        .aria_label("Markdown Editor Floating Toolbar")
        .occlude()
        .h(height)
        .px(px(4.))
        .gap(px(2.))
        .bg(tokens.popup)
        .border_1()
        .border_color(tokens.popup_border)
        .rounded(px(metrics::RADIUS_LARGE))
        .shadow_md()
        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
            // the editor keeps the focus and its selection
            window.prevent_default();
            cx.stop_propagation();
        })
        .child(header_style)
        .child(separator())
        .child(
            toolbar_button(
                "Bold",
                "markdown/editorActions/bold",
                pressed("strong_emphasis"),
                cx,
            )
            .on_click(run(Box::new(ToggleBold))),
        )
        .child(
            toolbar_button(
                "Italic",
                "markdown/editorActions/italic",
                pressed("emphasis"),
                cx,
            )
            .on_click(run(Box::new(ToggleItalic))),
        )
        .child(
            toolbar_button(
                "Strikethrough",
                "markdown/editorActions/strikeThrough",
                pressed("strikethrough"),
                cx,
            )
            .on_click(run(Box::new(ToggleStrikethrough))),
        )
        .child(
            toolbar_button(
                "Code",
                "markdown/editorActions/codeSpan",
                pressed("code_span"),
                cx,
            )
            .on_click(run(Box::new(ToggleCodeSpan))),
        )
        .child(
            toolbar_button("Create Link", "markdown/editorActions/link", false, cx)
                .on_click(run(Box::new(CreateLink))),
        )
        .child(separator())
        .child(
            toolbar_button(
                "Create Or Change List",
                list.unwrap_or(ListKind::Unordered).icon(),
                list.is_some(),
                cx,
            )
            .on_click(run(Box::new(CreateOrChangeList))),
        );
    Some(
        deferred(
            anchored()
                .position(origin)
                .snap_to_window_with_margin(px(8.))
                .child(toolbar),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// Whether the Markdown block grammar has a node of one of `kinds` around `offset`.
fn block_kind_at(buffer: &language::BufferSnapshot, offset: usize, kinds: &[&str]) -> bool {
    let Some(layer) = buffer
        .syntax_layers()
        .find(|layer| layer.language.name().as_ref() == "Markdown")
    else {
        return false;
    };
    let mut node = layer.node().descendant_for_byte_range(offset, offset);
    while let Some(current) = node {
        if kinds.contains(&current.kind()) {
            return true;
        }
        node = current.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        ListKind, change_list, image_text, inline_link_parts, line_header, list_at,
        preview_font_step, set_header, table_insertion, table_text,
    };

    fn edit(text: &str, edits: Vec<(std::ops::Range<usize>, String)>) -> String {
        let mut text = text.to_owned();
        let mut edits = edits;
        edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
        for (range, new) in edits {
            text.replace_range(range, &new);
        }
        text
    }

    #[test]
    fn reads_headers() {
        assert_eq!(line_header("## Title ##\n", 3).level, 2);
        assert_eq!(line_header("## Title ##\n", 3).content, "Title");
        assert_eq!(line_header("Title\n=====\n", 0).level, 1);
        assert_eq!(line_header("Title\n---\n", 0).range, 0..9);
        assert_eq!(line_header("* ### item\n", 4).level, 3);
        assert_eq!(line_header("* ### item\n", 4).prefix, "* ");
        assert_eq!(line_header("#hashtag\n", 0).level, 0);
    }

    #[test]
    fn sets_header_styles_as_intellij() {
        let text = "Lorem\nipsum\ndolor\n";
        let (range, new) = set_header(text, 7, 2).unwrap();
        assert_eq!(
            edit(text, vec![(range, new)]),
            "Lorem\n\n## ipsum\n\ndolor\n"
        );
        let text = "Title\n=====\n\ntext\n";
        let (range, new) = set_header(text, 0, 3).unwrap();
        assert_eq!(edit(text, vec![(range, new)]), "### Title\n\ntext\n");
        let text = "## Header\n";
        let (range, new) = set_header(text, 0, 0).unwrap();
        assert_eq!(edit(text, vec![(range, new)]), "Header\n");
        assert_eq!(set_header("Title\n=====\n", 0, 1), None);
        let text = "* First\n* Second item\n";
        let (range, new) = set_header(text, 10, 3).unwrap();
        assert_eq!(
            edit(text, vec![(range, new)]),
            "* First\n* ### Second item\n"
        );
    }

    #[test]
    fn finds_a_list_and_its_kind() {
        let text = "intro\n\n1. one\n   more\n2. two\n   * nested\n3. three\n\nafter\n";
        let (kind, items) = list_at(text, text.find("more").unwrap()).unwrap();
        assert_eq!(kind, ListKind::Ordered);
        assert_eq!(items.len(), 3);
        let (kind, items) = list_at(text, text.find("nested").unwrap()).unwrap();
        assert_eq!(kind, ListKind::Unordered);
        assert_eq!(items.len(), 1);
        assert!(list_at(text, 0).is_none());
        let (kind, _) = list_at("- [x] done\n- [ ] todo\n", 3).unwrap();
        assert_eq!(kind, ListKind::Checkmark);
    }

    #[test]
    fn creates_and_changes_lists() {
        let text = "a\nb\nc\n";
        assert_eq!(
            edit(text, change_list(text, 0..3, ListKind::Ordered)),
            "1. a\n2. b\nc\n"
        );
        let text = "* a\n* b\n";
        assert_eq!(
            edit(text, change_list(text, 2..2, ListKind::Checkmark)),
            "1. [ ] a\n2. [ ] b\n"
        );
        let text = "1. [x] a\n2. [ ] b\n";
        assert_eq!(
            edit(text, change_list(text, 0..0, ListKind::Checkmark)),
            "a\nb\n"
        );
        assert_eq!(
            edit(text, change_list(text, 0..0, ListKind::Unordered)),
            "* a\n* b\n"
        );
    }

    #[test]
    fn writes_tables_and_images_as_intellij() {
        assert_eq!(table_text(1, 1), "|   |\n|---|\n");
        assert_eq!(
            table_text(3, 2),
            "|   |   |\n|---|---|\n|   |   |\n|   |   |\n"
        );
        assert_eq!(
            table_insertion("text\n", 5, 1, 1),
            ("\n|   |\n|---|\n".into(), 7)
        );
        assert_eq!(
            table_insertion("text\n\n", 6, 1, 1),
            ("|   |\n|---|\n".into(), 7)
        );
        assert_eq!(image_text(" a\\b.png ", "", "alt"), "![alt](a/b.png)");
        assert_eq!(
            image_text("a.png", "say \"hi\"", ""),
            "![](a.png 'say \"hi\"')"
        );
        assert_eq!(image_text("a.png", "T", "D"), "![D](a.png \"T\")");
    }

    #[test]
    fn steps_through_intellijs_preview_font_sizes() {
        assert_eq!(preview_font_step(14.86, true), Some(16.));
        assert_eq!(preview_font_step(14.86, false), Some(14.));
        assert_eq!(preview_font_step(16., false), Some(14.));
        assert_eq!(preview_font_step(72., true), None);
        assert_eq!(preview_font_step(8., false), None);
    }

    #[test]
    fn reads_inline_links() {
        assert_eq!(
            inline_link_parts("[link]( /url \"title\")"),
            Some(("link".into(), "/url".into(), Some("\"title\"".into())))
        );
        assert_eq!(
            inline_link_parts("[a [b]](<x y>)"),
            Some(("a [b]".into(), "x y".into(), None))
        );
    }
}
