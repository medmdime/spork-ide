//! IntelliJ's Markdown editing actions (the Markdown plugin's styling, table of contents and table
//! actions) on Zed's Markdown grammar: Bold (⌘B), Italic (⌘I), Strikethrough (⇧⌘S) and Code
//! (⇧⌘C) put their markers around the selection or take them away (`BaseToggleStateAction`),
//! Create Link (⇧⌘U) wraps it in `[…]()` or unwraps a link, Generate Table Of Contents writes the
//! headers' list between `<!-- TOC -->` markers, and the table row actions move, select and pick
//! a column's cells.

use std::collections::HashMap;
use std::ops::Range;

use std::rc::Rc;

use editor::{Editor, SelectionEffects};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Entity, EntityId, Focusable as _, InteractiveElement as _,
    IntoElement as _, ParentElement as _, Role, StatefulInteractiveElement as _, Styled as _,
    TaskExt as _, WeakEntity, Window, div, px, svg,
};
use language::BufferSnapshot;
use markdown_preview::markdown_preview_view::{MarkdownPreviewCorner, MarkdownPreviewView};
use markdown_preview::{CloseAndReturnToEditor, OpenPreview, OpenPreviewToTheSide};
use spork_workbench::Tokens;
use spork_workbench::tokens::metrics;
use text::Point;
use workspace::{Pane, Workspace};

gpui::actions!(
    spork_editor,
    [
        /// Formats the selection as bold, or takes its bold away (IntelliJ's Bold, ⌘B in Markdown).
        ToggleBold,
        /// Formats the selection as italic, or takes its italic away (⌘I in Markdown).
        ToggleItalic,
        /// Strikes the selection through, or takes the strikethrough away (⇧⌘S in Markdown).
        ToggleStrikethrough,
        /// Formats the selection as an inline code span, or takes the span away (⇧⌘C in Markdown).
        ToggleCodeSpan,
        /// Wraps the selection in a link, or unwraps the link at the caret (⇧⌘U in Markdown).
        CreateLink,
        /// Writes the headers' list between `<!-- TOC -->` markers, or updates it.
        GenerateTableOfContents,
        /// Swaps the table row at the caret with the one above it.
        MoveTableRowUp,
        /// Swaps the table row at the caret with the one below it.
        MoveTableRowDown,
        /// Selects the table row at the caret.
        SelectTableRow,
        /// Selects the content of each cell of the caret's table column.
        SelectTableColumnCells,
        /// Shows the Markdown file's editor alone (IntelliJ's Editor layout).
        MarkdownEditorOnly,
        /// Shows the Markdown file's editor with its preview beside it (Editor and Preview).
        MarkdownEditorAndPreview,
        /// Shows the Markdown file's preview alone (Preview).
        MarkdownPreviewOnly,
    ]
);

/// IntelliJ's `TextEditorWithPreview.Layout`, over Zed's preview: beside the editor in the next
/// pane, or in the editor's own pane.
#[derive(Clone, Copy, PartialEq)]
enum Layout {
    Editor,
    EditorAndPreview,
    Preview,
}

impl Layout {
    fn label(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::EditorAndPreview => "Editor and Preview",
            Self::Preview => "Preview",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Editor => "general/editorOnly",
            Self::EditorAndPreview => "general/editorPreview",
            Self::Preview => "general/previewOnly",
        }
    }
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(MarkdownPreviewCorner(Rc::new(preview_corner)));
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &MarkdownEditorOnly, window, cx| {
            choose_from_workspace(workspace, Layout::Editor, window, cx)
        });
        workspace.register_action(|workspace, _: &MarkdownEditorAndPreview, window, cx| {
            choose_from_workspace(workspace, Layout::EditorAndPreview, window, cx)
        });
        workspace.register_action(|workspace, _: &MarkdownPreviewOnly, window, cx| {
            choose_from_workspace(workspace, Layout::Preview, window, cx)
        });
    })
    .detach();
}

/// IntelliJ's `TableOfContents` section marker.
const TOC_MARKER: &str = "<!-- TOC -->";
/// IntelliJ's Markdown indent (its code style's), a nested header's indent in the table of contents.
const TOC_INDENT: usize = 4;

pub(crate) fn register(editor: &mut Editor, handle: WeakEntity<Editor>) {
    macro_rules! on {
        ($action:ty, $run:expr) => {
            editor
                .register_action({
                    let handle = handle.clone();
                    move |_: &$action, window, cx| {
                        handle
                            .update(cx, |editor, cx| {
                                if is_markdown(editor, cx) {
                                    $run(editor, window, cx)
                                }
                            })
                            .ok();
                    }
                })
                .detach();
        };
    }
    on!(ToggleBold, |editor, window, cx| toggle(
        editor,
        Emphasis::Bold,
        window,
        cx
    ));
    on!(ToggleItalic, |editor, window, cx| toggle(
        editor,
        Emphasis::Italic,
        window,
        cx
    ));
    on!(ToggleStrikethrough, |editor, window, cx| toggle(
        editor,
        Emphasis::Strikethrough,
        window,
        cx
    ));
    on!(ToggleCodeSpan, |editor, window, cx| toggle(
        editor,
        Emphasis::Code,
        window,
        cx
    ));
    on!(CreateLink, create_link);
    on!(GenerateTableOfContents, generate_toc);
    on!(MoveTableRowUp, |editor, window, cx| move_row(
        editor, true, window, cx
    ));
    on!(MoveTableRowDown, |editor, window, cx| move_row(
        editor, false, window, cx
    ));
    on!(SelectTableRow, select_row);
    on!(SelectTableColumnCells, select_column_cells);
}

pub(crate) fn is_markdown(editor: &Editor, cx: &gpui::App) -> bool {
    editor
        .buffer()
        .read(cx)
        .as_singleton()
        .and_then(|buffer| buffer.read(cx).language().map(|language| language.name()))
        .is_some_and(|name| name.as_ref() == "Markdown")
}

pub(crate) fn snapshot(editor: &Editor, cx: &gpui::App) -> Option<BufferSnapshot> {
    Some(editor.buffer().read(cx).as_singleton()?.read(cx).snapshot())
}

/// The selections as offsets of the (single) buffer.
pub(crate) fn selections(editor: &Editor, cx: &mut Context<Editor>) -> Vec<Range<usize>> {
    let display = editor.display_snapshot(cx);
    editor
        .selections
        .all::<multi_buffer::MultiBufferOffset>(&display)
        .into_iter()
        .map(|selection| selection.start.0..selection.end.0)
        .collect()
}

/// Applies `edits` (in the original text's offsets) and selects `ranges` (in the edited text's).
pub(crate) fn apply(
    editor: &mut Editor,
    mut edits: Vec<(Range<usize>, String)>,
    ranges: Vec<Range<usize>>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    edits.sort_by_key(|(range, _)| range.start);
    editor.transact(window, cx, |editor, window, cx| {
        let edits = edits.into_iter().map(|(range, text)| {
            (
                multi_buffer::MultiBufferOffset(range.start)
                    ..multi_buffer::MultiBufferOffset(range.end),
                text,
            )
        });
        editor.edit(edits, cx);
        editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
            selections.select_ranges(ranges.into_iter().map(|range| {
                multi_buffer::MultiBufferOffset(range.start)
                    ..multi_buffer::MultiBufferOffset(range.end)
            }))
        });
    });
}

/// Where an offset of the original text lands once `edits` are made: text inserted right at it
/// counts before it when `after`, as a selection's start sees a marker put there, and after it
/// otherwise, as its end does.
pub(crate) fn map_offset(offset: usize, edits: &[(Range<usize>, String)], after: bool) -> usize {
    let mut delta = 0isize;
    for (range, text) in edits {
        let change = text.len() as isize - range.len() as isize;
        let before_it = range.end < offset
            || (range.end == offset && !range.is_empty())
            || (range.is_empty() && range.start == offset && after);
        if before_it {
            delta += change;
        } else if range.start < offset && offset < range.end {
            // inside a replaced range: to the replacement's start
            return (range.start as isize + delta).max(0) as usize;
        }
    }
    (offset as isize + delta).max(0) as usize
}

#[derive(Clone, Copy)]
enum Emphasis {
    Bold,
    Italic,
    Strikethrough,
    Code,
}

impl Emphasis {
    /// The Markdown inline grammar's node.
    fn kind(self) -> &'static str {
        match self {
            Self::Bold => "strong_emphasis",
            Self::Italic => "emphasis",
            Self::Strikethrough => "strikethrough",
            Self::Code => "code_span",
        }
    }

    /// Whether the selection's outer whitespace is left out (`shouldMoveToWordBounds`).
    fn word_bounds(self) -> bool {
        !matches!(self, Self::Code)
    }

    /// The markers put around `text` (IntelliJ's asterisks style; a code span's backticks outnumber
    /// the longest run inside).
    fn marker(self, text: &str) -> String {
        match self {
            Self::Bold => "**".into(),
            Self::Italic => "*".into(),
            Self::Strikethrough => "~~".into(),
            Self::Code => {
                let (mut longest, mut run) = (0, 0);
                for c in text.chars() {
                    run = if c == '`' { run + 1 } else { 0 };
                    longest = longest.max(run);
                }
                "`".repeat(longest + 1)
            }
        }
    }

    /// The markers an existing node starts with.
    fn existing_marker(self, text: &str) -> String {
        match self {
            Self::Bold => text.chars().take(2).collect(),
            Self::Strikethrough => text.chars().take_while(|c| *c == '~').collect(),
            Self::Italic => text.chars().take(1).collect(),
            Self::Code => text.chars().take_while(|c| *c == '`').collect(),
        }
    }
}

/// The node of `kind` holding `range`, from the inline grammar's tree.
pub(crate) fn enclosing(
    buffer: &BufferSnapshot,
    range: &Range<usize>,
    kind: &str,
) -> Option<Range<usize>> {
    let mut node = buffer.syntax_ancestor(range.start..range.start)?;
    loop {
        let node_range = node.byte_range();
        if node.kind() == kind && node_range.start <= range.start && range.end <= node_range.end {
            // `~~x~~` is a strikethrough around a `~x~` one: the outer is the formatting
            while let Some(parent) = node.parent().filter(|parent| parent.kind() == kind) {
                node = parent;
            }
            return Some(node.byte_range());
        }
        node = node.parent()?;
    }
}

/// The selection less its outer whitespace.
fn trimmed(text: &str, range: Range<usize>) -> Range<usize> {
    let selected = &text[range.clone()];
    let start = range.start + (selected.len() - selected.trim_start().len());
    let end = range.end - (selected.len() - selected.trim_end().len());
    start..end.max(start)
}

/// IntelliJ's `BaseToggleStateAction` for each caret: inside the formatting, its markers go (around
/// the selection when it is the formatting's whole content, else split there); outside, they are
/// put around the selection, an empty one getting the caret between them.
fn toggle(editor: &mut Editor, emphasis: Emphasis, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let text = buffer.text();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    // each selection to restore, and the marker put at an empty one
    let mut wanted = Vec::new();
    for selection in selections(editor, cx) {
        let range = if emphasis.word_bounds() {
            trimmed(&text, selection.clone())
        } else {
            selection.clone()
        };
        let lookup = if range.is_empty() {
            selection.clone()
        } else {
            range.start..range.end
        };
        match enclosing(&buffer, &lookup, emphasis.kind()) {
            Some(node) => {
                let bound = emphasis.existing_marker(&text[node.clone()]);
                let length = bound.len();
                let content = node.start + length..node.end - length;
                let covers = selection.start <= content.start && content.end <= selection.end;
                if lookup == content || covers {
                    edits.push((node.end - length..node.end, String::new()));
                    edits.push((node.start..node.start + length, String::new()));
                } else {
                    let (mut from, mut to) = (selection.start, selection.end);
                    if emphasis.word_bounds() {
                        while from > node.start + length {
                            let Some(c) = text[..from].chars().next_back() else {
                                break;
                            };
                            if !c.is_whitespace() {
                                break;
                            }
                            from -= c.len_utf8();
                        }
                        while to + length < node.end {
                            let Some(c) = text[to..].chars().next() else {
                                break;
                            };
                            if !c.is_whitespace() {
                                break;
                            }
                            to += c.len_utf8();
                        }
                    }
                    if to + length == node.end {
                        edits.push((node.end - length..node.end, String::new()));
                    } else {
                        edits.push((to..to, bound.clone()));
                    }
                    if from == node.start + length {
                        edits.push((node.start..node.start + length, String::new()));
                    } else {
                        edits.push((from..from, bound));
                    }
                }
                wanted.push((selection, 0));
            }
            None => {
                let bound = emphasis.marker(&text[range.clone()]);
                let length = bound.len();
                edits.push((range.end..range.end, bound.clone()));
                edits.push((range.start..range.start, bound));
                wanted.push((range, length));
            }
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    let ranges = wanted
        .into_iter()
        .map(|(range, inserted)| match range.is_empty() {
            // an empty selection's caret goes between the markers put there
            true => {
                let at = map_offset(range.start, &edits, false) + inserted;
                at..at
            }
            false => map_offset(range.start, &edits, true)..map_offset(range.end, &edits, false),
        })
        .collect();
    apply(editor, edits, ranges, window, cx);
}

/// IntelliJ's Create Link (`MarkdownCreateLinkAction`): inside a link, it unwraps to its text;
/// otherwise the selection becomes `[selection]()` with the caret between the parentheses, or, a
/// selected address, `[](address)` with the caret between the brackets.
fn create_link(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let text = buffer.text();
    let mut edits = Vec::new();
    let mut carets = Vec::new();
    for selection in selections(editor, cx) {
        if let Some(link) = enclosing(&buffer, &selection, "inline_link") {
            let link_text = text[link.clone()]
                .strip_prefix('[')
                .and_then(|rest| rest.split_once(']'))
                .map(|(label, _)| label.to_owned())
                .unwrap_or_default();
            let caret = selection
                .start
                .saturating_sub(1)
                .clamp(link.start, link.start + link_text.len());
            carets.push((caret, link.clone(), link_text.len()));
            edits.push((link, link_text));
            continue;
        }
        let selected = &text[selection.clone()];
        let address = selected.contains("://") && !selected.contains(char::is_whitespace);
        let (replacement, caret) = if address {
            (format!("[]({selected})"), selection.start + 1)
        } else {
            (
                format!("[{selected}]()"),
                selection.start + selected.len() + 3,
            )
        };
        carets.push((caret, selection.clone(), replacement.len()));
        edits.push((selection, replacement));
    }
    // each caret moves by what the edits before its own changed
    let ranges = carets
        .iter()
        .map(|(caret, own, _)| {
            let before: isize = edits
                .iter()
                .filter(|(range, _)| range.end <= own.start && range != own)
                .map(|(range, text)| text.len() as isize - range.len() as isize)
                .sum();
            let at = (*caret as isize + before).max(0) as usize;
            at..at
        })
        .collect();
    apply(editor, edits, ranges, window, cx);
}

/// GitHub's anchor of a header: lower case, spaces as dashes, punctuation but `-` and `_` gone;
/// the same text again gets `-1`, `-2`.
fn anchor(text: &str, seen: &mut HashMap<String, usize>) -> String {
    let base: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect();
    let count = seen.entry(base.clone()).or_insert(0);
    *count += 1;
    match *count - 1 {
        0 => base,
        n => format!("{base}-{n}"),
    }
}

/// The table of contents of `text`'s ATX headers, outside code fences.
fn table_of_contents(text: &str) -> String {
    let mut seen = HashMap::new();
    let mut toc = format!("{TOC_MARKER}\n");
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        let Some(title) = line.get(level..).and_then(|rest| rest.strip_prefix(' ')) else {
            continue;
        };
        if !(1..=6).contains(&level) || title.trim().is_empty() {
            continue;
        }
        let title = title.trim().trim_end_matches('#').trim();
        toc.push_str(&" ".repeat((level - 1) * TOC_INDENT));
        toc.push_str(&format!("* [{title}](#{})\n", anchor(title, &mut seen)));
    }
    toc.push_str(TOC_MARKER);
    toc
}

/// IntelliJ's Generate Table Of Contents: at the caret, or in place of each existing one.
fn generate_toc(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let text = buffer.text();
    let toc = table_of_contents(&text);
    let markers: Vec<usize> = text
        .match_indices(TOC_MARKER)
        .map(|(offset, _)| offset)
        .collect();
    let mut edits: Vec<(Range<usize>, String)> = markers
        .chunks_exact(2)
        .map(|pair| (pair[0]..pair[1] + TOC_MARKER.len(), toc.clone()))
        .collect();
    let caret = selections(editor, cx)
        .last()
        .map_or(0, |selection| selection.end);
    if edits.is_empty() {
        edits.push((caret..caret, toc));
    }
    // the caret stays before a table of contents written at it, as IntelliJ's
    let at = map_offset(caret, &edits, false);
    apply(editor, edits, vec![at..at], window, cx);
}

/// The table rows around `row`: the lines starting with `|`, as row numbers.
fn table_rows(buffer: &BufferSnapshot, row: u32) -> Option<Range<u32>> {
    let is_row = |row: u32| {
        buffer
            .text_for_range(Point::new(row, 0)..Point::new(row, buffer.line_len(row)))
            .collect::<String>()
            .trim_start()
            .starts_with('|')
    };
    if !is_row(row) {
        return None;
    }
    let mut start = row;
    while start > 0 && is_row(start - 1) {
        start -= 1;
    }
    let mut end = row + 1;
    while end <= buffer.max_point().row && is_row(end) {
        end += 1;
    }
    Some(start..end)
}

fn caret_point(editor: &Editor, cx: &mut Context<Editor>) -> Point {
    let display = editor.display_snapshot(cx);
    editor.selections.newest::<Point>(&display).head()
}

/// Move Row Up/Down: the caret's row and its neighbour swap, the header and its delimiter row
/// staying first.
fn move_row(editor: &mut Editor, up: bool, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let caret = caret_point(editor, cx);
    let Some(rows) = table_rows(&buffer, caret.row) else {
        return;
    };
    // the header (rows.start) and the delimiter row under it don't move
    let first_body = rows.start + 2;
    let other = if up {
        caret.row.wrapping_sub(1)
    } else {
        caret.row + 1
    };
    if caret.row < first_body || other < first_body || other >= rows.end {
        return;
    }
    let line = |row: u32| -> String {
        buffer
            .text_for_range(Point::new(row, 0)..Point::new(row, buffer.line_len(row)))
            .collect()
    };
    let (top, bottom) = if up {
        (other, caret.row)
    } else {
        (caret.row, other)
    };
    let start = buffer.point_to_offset(Point::new(top, 0));
    let end = buffer.point_to_offset(Point::new(bottom, buffer.line_len(bottom)));
    let swapped = format!("{}\n{}", line(bottom), line(top));
    let target = buffer.point_to_offset(Point::new(top, 0))
        + if up { 0 } else { line(bottom).len() + 1 }
        + caret.column as usize;
    apply(
        editor,
        vec![(start..end, swapped)],
        vec![target..target],
        window,
        cx,
    );
}

/// Select Row: the caret's table row, without its line break.
fn select_row(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let caret = caret_point(editor, cx);
    if table_rows(&buffer, caret.row).is_none() {
        return;
    }
    let start = buffer.point_to_offset(Point::new(caret.row, 0));
    let end = buffer.point_to_offset(Point::new(caret.row, buffer.line_len(caret.row)));
    apply(editor, Vec::new(), vec![start..end], window, cx);
}

/// The cells of a table row: their content ranges (trimmed), as offsets in the line.
fn cells(line: &str) -> Vec<Range<usize>> {
    let bars: Vec<usize> = line.match_indices('|').map(|(offset, _)| offset).collect();
    bars.windows(2)
        .map(|pair| {
            let (start, end) = (pair[0] + 1, pair[1]);
            let cell = &line[start..end];
            let from = start + (cell.len() - cell.trim_start().len());
            let to = end - (cell.len() - cell.trim_end().len());
            from..to.max(from)
        })
        .collect()
}

/// Select Column Cells: a selection over each content cell of the caret's column (the delimiter
/// row left out).
fn select_column_cells(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let Some(buffer) = snapshot(editor, cx) else {
        return;
    };
    let caret = caret_point(editor, cx);
    let Some(rows) = table_rows(&buffer, caret.row) else {
        return;
    };
    let line = |row: u32| -> String {
        buffer
            .text_for_range(Point::new(row, 0)..Point::new(row, buffer.line_len(row)))
            .collect()
    };
    let caret_line = line(caret.row);
    let column = caret_line[..caret.column as usize].matches('|').count();
    if column == 0 {
        return;
    }
    let ranges = rows
        .filter(|row| *row != rows_delimiter(&buffer, caret.row))
        .filter_map(|row| {
            let cell = cells(&line(row)).get(column - 1)?.clone();
            let start = buffer.point_to_offset(Point::new(row, 0));
            Some(start + cell.start..start + cell.end)
        })
        .collect();
    apply(editor, Vec::new(), ranges, window, cx);
}

/// The delimiter row (`|---|`) of the table holding `row`.
fn rows_delimiter(buffer: &BufferSnapshot, row: u32) -> u32 {
    table_rows(buffer, row).map_or(u32::MAX, |rows| rows.start + 1)
}

/// The previews of `editor` open in the workspace, with their panes; `drawn`, a preview drawing
/// itself now, is known to show it and isn't read.
fn previews_of(
    workspace: &Workspace,
    editor: EntityId,
    drawn: Option<EntityId>,
    cx: &App,
) -> Vec<(Entity<Pane>, Entity<MarkdownPreviewView>)> {
    workspace
        .panes()
        .iter()
        .flat_map(|pane| {
            pane.read(cx)
                .items_of_type::<MarkdownPreviewView>()
                .filter(|preview| {
                    Some(preview.entity_id()) == drawn
                        || preview
                            .read(cx)
                            .source_editor()
                            .is_some_and(|source| source.entity_id() == editor)
                })
                .map(|preview| (pane.clone(), preview))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The layout an editor shows in: with its preview beside it when one is in another pane.
fn editor_layout(
    workspace: &Workspace,
    editor: EntityId,
    drawn: Option<EntityId>,
    cx: &App,
) -> Layout {
    let own_pane = workspace
        .panes()
        .iter()
        .find(|pane| pane.read(cx).items().any(|item| item.item_id() == editor))
        .cloned();
    match previews_of(workspace, editor, drawn, cx)
        .iter()
        .any(|(pane, _)| Some(pane) != own_pane.as_ref())
    {
        true => Layout::EditorAndPreview,
        false => Layout::Editor,
    }
}

/// Chooses a layout for the Markdown editor or preview in front.
fn choose_from_workspace(
    workspace: &mut Workspace,
    layout: Layout,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(item) = workspace.active_item(cx) else {
        return;
    };
    if let Some(preview) = item.downcast::<MarkdownPreviewView>() {
        let workspace = cx.entity().downgrade();
        window.defer(cx, move |window, cx| {
            choose_from_preview(&workspace, &preview, layout, window, cx)
        });
    } else if let Some(editor) = item.act_as::<Editor>(cx) {
        choose_from_editor(workspace, &editor, layout, window, cx);
    }
}

fn choose_from_editor(
    workspace: &mut Workspace,
    editor: &Entity<Editor>,
    layout: Layout,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    match layout {
        Layout::Editor => {
            for (pane, preview) in previews_of(workspace, editor.entity_id(), None, cx) {
                pane.update(cx, |pane, cx| {
                    pane.close_item_by_id(
                        preview.entity_id(),
                        workspace::SaveIntent::Skip,
                        window,
                        cx,
                    )
                    .detach_and_log_err(cx);
                });
            }
        }
        Layout::EditorAndPreview => window.dispatch_action(Box::new(OpenPreviewToTheSide), cx),
        Layout::Preview => window.dispatch_action(Box::new(OpenPreview), cx),
    }
}

fn choose_from_preview(
    workspace: &WeakEntity<Workspace>,
    preview: &Entity<MarkdownPreviewView>,
    layout: Layout,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(editor) = preview.read(cx).source_editor() else {
        return;
    };
    // the preview shows beside its editor, or alone in the editor's place
    let current = match workspace
        .upgrade()
        .map(|workspace| editor_layout(workspace.read(cx), editor.entity_id(), None, cx))
    {
        Some(Layout::EditorAndPreview) => Layout::EditorAndPreview,
        _ => Layout::Preview,
    };
    if layout == current {
        return;
    }
    match (current, layout) {
        // beside the editor: the preview goes, or takes the editor's place
        (Layout::EditorAndPreview, Layout::Editor) => {
            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    choose_from_editor(workspace, &editor, Layout::Editor, window, cx)
                });
            }
        }
        (Layout::EditorAndPreview, Layout::Preview) => {
            let Some(workspace) = workspace.upgrade() else {
                return;
            };
            workspace.update(cx, |workspace, cx| {
                choose_from_editor(workspace, &editor, Layout::Editor, window, cx);
                workspace.activate_item(&editor, true, true, window, cx);
            });
            window.defer(cx, |window, cx| {
                window.dispatch_action(Box::new(OpenPreview), cx)
            });
        }
        // the preview alone: back to the editor, then the preview beside it if asked
        (_, Layout::Editor) | (_, Layout::EditorAndPreview) => {
            preview
                .focus_handle(cx)
                .dispatch_action(&CloseAndReturnToEditor, window, cx);
            if layout == Layout::EditorAndPreview {
                window.defer(cx, |window, cx| {
                    window.defer(cx, |window, cx| {
                        window.dispatch_action(Box::new(OpenPreviewToTheSide), cx)
                    })
                });
            }
        }
        _ => {}
    }
}

/// IntelliJ's layout actions (`TextEditorWithPreview.LayoutGroup`), the shown layout selected.
/// IntelliJ's `Markdown.Toolbar.Right` before the layout actions, while a preview shows: Sync
/// Scroll, pressed while on, and what toggles it.
type SyncScroll = (bool, Rc<dyn Fn(&mut Window, &mut App)>);

fn layout_buttons(
    current: Layout,
    choose: Rc<dyn Fn(Layout, &mut Window, &mut App)>,
    sync: Option<SyncScroll>,
    cx: &App,
) -> AnyElement {
    let tokens = Tokens::get(cx);
    let sync = sync.map(|(on, toggle)| {
        div()
            .id("sync-scroll")
            .role(Role::Button)
            .aria_label("Sync Scroll")
            .aria_toggled(gpui::Toggled::from(on))
            .size(px(metrics::TOOLBAR_BUTTON))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(metrics::RADIUS))
            .cursor_pointer()
            .when(on, |button| button.bg(tokens.toolbar_selected))
            .when(!on, |button| {
                button.hover(move |button| button.bg(tokens.hover))
            })
            .child(
                svg()
                    .path("wb/icons/actions/synchronizeScrolling.svg")
                    .size(px(metrics::TOOLBAR_ICON))
                    .text_color(tokens.icon),
            )
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                toggle(window, cx)
            })
    });
    let buttons = [Layout::Editor, Layout::EditorAndPreview, Layout::Preview].map(|layout| {
        let choose = choose.clone();
        let selected = layout == current;
        div()
            .id(layout.label())
            .role(Role::Button)
            .aria_label(layout.label())
            .aria_selected(selected)
            .size(px(metrics::TOOLBAR_BUTTON))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(metrics::RADIUS))
            .cursor_pointer()
            .when(selected, |button| button.bg(tokens.toolbar_selected))
            .when(!selected, |button| {
                button.hover(move |button| button.bg(tokens.hover))
            })
            .child(
                svg()
                    .path(format!("wb/icons/{}.svg", layout.icon()))
                    .size(px(metrics::TOOLBAR_ICON))
                    .text_color(tokens.icon),
            )
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                choose(layout, window, cx)
            })
            .into_any_element()
    });
    div()
        .id("markdown-layout")
        .role(Role::Toolbar)
        .aria_label("Markdown Editor Layout Actions")
        .occlude()
        .flex()
        .gap(px(2.))
        .children(sync)
        .children(buttons)
        .into_any_element()
}

/// The editor's layout buttons, beside its inspection widget: shown while the preview isn't
/// beside it (then the preview shows them, as IntelliJ's over its preview side).
pub(crate) fn editor_corner(
    editor: &Editor,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    if !is_markdown(editor, cx) {
        return None;
    }
    let workspace = Workspace::for_window(window, cx)?;
    let editor_id = cx.entity_id();
    if editor_layout(workspace.read(cx), editor_id, None, cx) != Layout::Editor {
        return None;
    }
    let handle = cx.entity();
    let workspace = workspace.downgrade();
    let choose: Rc<dyn Fn(Layout, &mut Window, &mut App)> = Rc::new(move |layout, window, cx| {
        let Some(workspace) = workspace.upgrade() else {
            return;
        };
        window.focus(&handle.focus_handle(cx), cx);
        workspace.update(cx, |workspace, cx| {
            choose_from_editor(workspace, &handle, layout, window, cx)
        });
    });
    Some(layout_buttons(Layout::Editor, choose, None, cx))
}

fn preview_corner(
    preview: &Entity<MarkdownPreviewView>,
    editor: &Entity<Editor>,
    sync_on: bool,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let workspace = Workspace::for_window(window, cx)?;
    let current = match editor_layout(
        workspace.read(cx),
        editor.entity_id(),
        Some(preview.entity_id()),
        cx,
    ) {
        Layout::EditorAndPreview => Layout::EditorAndPreview,
        _ => Layout::Preview,
    };
    let preview = preview.downgrade();
    let toggle: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new({
        let preview = preview.clone();
        move |_, cx| {
            preview
                .update(cx, |preview, cx| {
                    let on = !preview.sync_scroll();
                    preview.set_sync_scroll(on, cx)
                })
                .ok();
        }
    });
    let workspace = workspace.downgrade();
    let choose: Rc<dyn Fn(Layout, &mut Window, &mut App)> = Rc::new(move |layout, window, cx| {
        if let Some(preview) = preview.upgrade() {
            choose_from_preview(&workspace, &preview, layout, window, cx)
        }
    });
    Some(layout_buttons(current, choose, Some((sync_on, toggle)), cx))
}

#[cfg(test)]
mod tests {
    use super::{anchor, cells, table_of_contents};
    use std::collections::HashMap;

    #[test]
    fn writes_intellijs_table_of_contents() {
        let text = "# Primary Header 1\n\n## Secondary Header 1\n\n```\n# not a header\n```\n# Primary Header 1\n";
        assert_eq!(
            table_of_contents(text),
            "<!-- TOC -->\n* [Primary Header 1](#primary-header-1)\n    * [Secondary Header 1](#secondary-header-1)\n* [Primary Header 1](#primary-header-1-1)\n<!-- TOC -->"
        );
    }

    #[test]
    fn anchors_drop_punctuation() {
        let mut seen = HashMap::new();
        assert_eq!(anchor("Hello, World!", &mut seen), "hello-world");
        assert_eq!(anchor("Hello, World!", &mut seen), "hello-world-1");
    }

    #[test]
    fn finds_cells() {
        assert_eq!(cells("| a | bc |"), vec![2..3, 6..8]);
    }
}
