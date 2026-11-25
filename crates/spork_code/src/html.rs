//! HTML on tree-sitter: the language (tree-sitter-html, Zed's HTML extension's queries, which
//! the extension's language server joins when it is installed), and IntelliJ's HTML Tools on its
//! syntax tree: a table's cell navigation (`TableCellNavigator`), Add a New Column to the Table
//! Before/After the Current One (`TableColumnAdder`, in the Generate popup) and Surround with Tag
//! (the `T` live template).

use std::ops::Range;
use std::sync::Arc;

use editor::{Editor, MultiBufferOffset, SelectionEffects};
use futures::FutureExt as _;
use gpui::{App, Context, Entity, InteractiveElement as _, Window};
use language::{LanguageConfig, LanguageQueries, LanguageRegistry, LoadedLanguage, Node};
use language_core::{QueryFile, QueryFileContents};
use util::ResultExt as _;
use workspace::Workspace;

use crate::Row;

pub mod html_actions {
    gpui::actions!(
        code,
        [
            /// Moves the caret into the table cell on the right.
            NavigateToNextTableColumn,
            /// Moves the caret into the table cell on the left.
            NavigateToPreviousTableColumn,
            /// Moves the caret into the table cell below.
            NavigateToNextTableRow,
            /// Moves the caret into the table cell above.
            NavigateToPreviousTableRow,
            /// Adds an empty column to the table before the caret's.
            AddTableColumnBefore,
            /// Adds an empty column to the table after the caret's.
            AddTableColumnAfter,
            /// Surrounds the selection with a tag whose name is typed next.
            SurroundWithTag,
        ]
    );
}

use html_actions::*;

const CONFIG: &str = include_str!("../../../extensions/html/languages/html/config.toml");

const QUERIES: [(&str, &str); 6] = [
    (
        "highlights.scm",
        include_str!("../../../extensions/html/languages/html/highlights.scm"),
    ),
    (
        "brackets.scm",
        include_str!("../../../extensions/html/languages/html/brackets.scm"),
    ),
    (
        "indents.scm",
        include_str!("../../../extensions/html/languages/html/indents.scm"),
    ),
    (
        "injections.scm",
        include_str!("../../../extensions/html/languages/html/injections.scm"),
    ),
    (
        "outline.scm",
        include_str!("../../../extensions/html/languages/html/outline.scm"),
    ),
    (
        "overrides.scm",
        include_str!("../../../extensions/html/languages/html/overrides.scm"),
    ),
];

fn config() -> anyhow::Result<LanguageConfig> {
    Ok(LanguageConfig::from_toml(CONFIG)?)
}

fn queries() -> LanguageQueries {
    LanguageQueries::from_files(QUERIES.iter().filter_map(|(name, text)| {
        let file = name.parse::<QueryFile>().ok()?;
        Some(QueryFileContents::new(
            file,
            std::borrow::Cow::Borrowed(*text),
        ))
    }))
}

/// Registers HTML with the IDE's languages, so its files are HTML without the extension.
pub(crate) fn register_language(languages: &Arc<LanguageRegistry>) {
    let Some(config) = config().log_err() else {
        return;
    };
    languages.register_native_grammars([("html", tree_sitter_html::LANGUAGE)]);
    languages.register_language(
        config.name.clone(),
        config.grammar.clone(),
        config.matcher.clone(),
        config.hidden,
        None,
        Arc::new(move || {
            let config = config.clone();
            async move {
                Ok(LoadedLanguage {
                    config,
                    queries: queries(),
                    context_provider: None,
                    toolchain_provider: None,
                    manifest_name: None,
                })
            }
            .boxed()
        }),
    );
}

/// The table actions where the active editor is HTML's.
pub(crate) fn register(div: gpui::Div, html: bool, cx: &mut Context<Workspace>) -> gpui::Div {
    if !html {
        return div;
    }
    div.on_action(
        cx.listener(|workspace, _: &NavigateToNextTableColumn, window, cx| {
            navigate_active(workspace, (0, 1), window, cx)
        }),
    )
    .on_action(
        cx.listener(|workspace, _: &NavigateToPreviousTableColumn, window, cx| {
            navigate_active(workspace, (0, -1), window, cx)
        }),
    )
    .on_action(
        cx.listener(|workspace, _: &NavigateToNextTableRow, window, cx| {
            navigate_active(workspace, (1, 0), window, cx)
        }),
    )
    .on_action(
        cx.listener(|workspace, _: &NavigateToPreviousTableRow, window, cx| {
            navigate_active(workspace, (-1, 0), window, cx)
        }),
    )
    .on_action(
        cx.listener(|workspace, _: &AddTableColumnBefore, window, cx| {
            if let Some(editor) = workspace.active_item_as::<Editor>(cx) {
                add_column(&editor, true, window, cx);
            }
        }),
    )
    .on_action(
        cx.listener(|workspace, _: &AddTableColumnAfter, window, cx| {
            if let Some(editor) = workspace.active_item_as::<Editor>(cx) {
                add_column(&editor, false, window, cx);
            }
        }),
    )
    .on_action(cx.listener(|workspace, _: &SurroundWithTag, window, cx| {
        if let Some(editor) = workspace.active_item_as::<Editor>(cx) {
            editor.update(cx, |editor, cx| surround_with_tag(editor, window, cx));
        }
    }))
}

fn navigate_active(
    workspace: &mut Workspace,
    direction: (isize, isize),
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if let Some(editor) = workspace.active_item_as::<Editor>(cx) {
        editor.update(cx, |editor, cx| navigate(editor, direction, window, cx));
    }
}

/// The Generate popup's HTML rows: the table's two column items, in a table cell.
pub(crate) fn generate_rows(
    target: &crate::Target,
    _: &mut Window,
    cx: &mut App,
) -> Option<gpui::Task<Vec<Row>>> {
    let buffer = target.buffer.read(cx);
    if buffer.language()?.name().as_ref() != "HTML" {
        return None;
    }
    let snapshot = buffer.snapshot();
    let caret = text::ToOffset::to_offset(&target.caret(cx), &snapshot);
    let root = snapshot.syntax_layers().next()?.node();
    let text = snapshot.text();
    let rows = match cell_at(root, &text, caret) {
        Some(_) => [
            ("Add a New Column to the Table Before the Current One", true),
            ("Add a New Column to the Table After the Current One", false),
        ]
        .into_iter()
        .map(|(label, before)| {
            let editor = target.editor.clone();
            Row::item(label, move |window, cx| {
                add_column(&editor, before, window, cx)
            })
        })
        .collect(),
        None => Vec::new(),
    };
    Some(gpui::Task::ready(rows))
}

/// The caret's offset, the buffer's text and syntax tree's root, for a singleton HTML editor.
fn caret_and_text(
    editor: &mut Editor,
    cx: &mut App,
) -> Option<(usize, String, language::BufferSnapshot)> {
    let buffer = editor.buffer().read(cx).as_singleton()?;
    let snapshot = buffer.read(cx).snapshot();
    let caret = editor
        .selections
        .newest::<MultiBufferOffset>(&editor.display_snapshot(cx))
        .head()
        .0;
    Some((caret, snapshot.text(), snapshot))
}

fn navigate(
    editor: &mut Editor,
    direction: (isize, isize),
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let Some((caret, text, snapshot)) = caret_and_text(editor, cx) else {
        return;
    };
    let Some(layer) = snapshot.syntax_layers().next() else {
        return;
    };
    let Some(target) = navigation_target(layer.node(), &text, caret, direction) else {
        return;
    };
    let target = MultiBufferOffset(target);
    editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
        selections.select_ranges([target..target])
    });
}

fn add_column(editor: &Entity<Editor>, before: bool, window: &mut Window, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        let Some((caret, text, snapshot)) = caret_and_text(editor, cx) else {
            return;
        };
        let Some(layer) = snapshot.syntax_layers().next() else {
            return;
        };
        let Some((edits, caret)) = column_edits(layer.node(), &text, caret, before) else {
            return;
        };
        editor.transact(window, cx, |editor, window, cx| {
            editor.edit(
                edits.into_iter().map(|(range, text)| {
                    (
                        MultiBufferOffset(range.start)..MultiBufferOffset(range.end),
                        text,
                    )
                }),
                cx,
            );
            let caret = MultiBufferOffset(caret);
            editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
                selections.select_ranges([caret..caret])
            });
        });
    });
}

/// IntelliJ's `T` template on the selection: `<tag>selection</tag>`, both names typed at once.
fn surround_with_tag(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let selection = editor
        .selections
        .newest::<MultiBufferOffset>(&editor.display_snapshot(cx));
    if selection.is_empty() {
        return;
    }
    let range = selection.range();
    let selected: String = editor
        .buffer()
        .read(cx)
        .snapshot(cx)
        .text_for_range(range.clone())
        .collect();
    let escaped = selected
        .replace('\\', "\\\\")
        .replace('$', "\\$")
        .replace('}', "\\}");
    let Some(snippet) = snippet::Snippet::parse(&format!("<$1>{escaped}</$1>")).log_err() else {
        return;
    };
    editor
        .insert_snippet(&[range], snippet, window, cx)
        .log_err();
}

/// An element's tag name, lowercase.
fn tag_name(element: Node, text: &str) -> Option<String> {
    if element.kind() != "element" {
        return None;
    }
    let mut cursor = element.walk();
    let tag = element
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "start_tag" | "self_closing_tag"))?;
    let mut cursor = tag.walk();
    let name = tag
        .named_children(&mut cursor)
        .find(|child| child.kind() == "tag_name")?;
    Some(text.get(name.byte_range())?.to_lowercase())
}

/// The child elements of `node`.
fn elements(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "element")
        .collect()
}

/// An attribute of an element: its value and the value's range.
fn attribute(element: Node, name: &str, text: &str) -> Option<(String, Range<usize>)> {
    let mut cursor = element.walk();
    let tag = element
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "start_tag" | "self_closing_tag"))?;
    let mut cursor = tag.walk();
    let attributes: Vec<Node> = tag
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "attribute")
        .collect();
    attributes.into_iter().find_map(|attribute| {
        let mut cursor = attribute.walk();
        let children: Vec<Node> = attribute.named_children(&mut cursor).collect();
        let named = children
            .iter()
            .find(|child| child.kind() == "attribute_name")?;
        if !text.get(named.byte_range())?.eq_ignore_ascii_case(name) {
            return None;
        }
        let value = children.iter().find_map(|child| match child.kind() {
            "attribute_value" => Some(*child),
            "quoted_attribute_value" => {
                let mut cursor = child.walk();
                let inner: Vec<Node> = child.named_children(&mut cursor).collect();
                inner
                    .into_iter()
                    .find(|inner| inner.kind() == "attribute_value")
            }
            _ => None,
        })?;
        Some((text.get(value.byte_range())?.to_owned(), value.byte_range()))
    })
}

/// The `td` or `th` element the caret is in.
fn cell_at<'a>(root: Node<'a>, text: &str, caret: usize) -> Option<Node<'a>> {
    let mut node = root.descendant_for_byte_range(caret, caret)?;
    loop {
        if tag_name(node, text).is_some_and(|name| name == "td" || name == "th") {
            return Some(node);
        }
        node = node.parent()?;
    }
}

/// A cell of a table: where the grid puts it, and how many columns it spans.
struct Cell<'a> {
    element: Node<'a>,
    row: usize,
    column: usize,
    colspan: usize,
}

/// A table row: its element, whether it is in the table's head, the cell in each column.
struct TableRow<'a> {
    element: Node<'a>,
    header: bool,
    slots: Vec<usize>,
}

struct Table<'a> {
    element: Node<'a>,
    cells: Vec<Cell<'a>>,
    rows: Vec<TableRow<'a>>,
}

fn span(element: Node, name: &str, text: &str) -> usize {
    attribute(element, name, text)
        .and_then(|(value, _)| value.trim().parse().ok())
        .filter(|span: &usize| *span > 0)
        .unwrap_or(1)
}

/// The table holding `cell`, its cells laid out on the grid as HTML's table model does (a cell
/// takes the first free column; `rowspan` and `colspan` cover more), as IntelliJ's
/// `getTableAndPosition`.
fn table_of<'a>(cell: Node<'a>, text: &str) -> Option<Table<'a>> {
    let mut element = cell.parent()?;
    while tag_name(element, text).as_deref() != Some("table") {
        element = element.parent()?;
    }
    let mut row_elements: Vec<(Node, bool)> = Vec::new();
    for child in elements(element) {
        match tag_name(child, text).as_deref() {
            Some("tr") => row_elements.push((child, false)),
            Some(section @ ("thead" | "tbody" | "tfoot")) => {
                let header = section == "thead";
                for row in elements(child) {
                    if tag_name(row, text).as_deref() == Some("tr") {
                        row_elements.push((row, header));
                    }
                }
            }
            _ => {}
        }
    }
    let mut table = Table {
        element,
        cells: Vec::new(),
        rows: Vec::new(),
    };
    // the grid: each row's columns, the index of the cell covering it
    let mut grid: Vec<Vec<Option<usize>>> = vec![Vec::new(); row_elements.len()];
    for (row, (row_element, _)) in row_elements.iter().enumerate() {
        let mut column = 0;
        for cell_element in elements(*row_element) {
            if !tag_name(cell_element, text).is_some_and(|name| name == "td" || name == "th") {
                continue;
            }
            while grid[row].get(column).is_some_and(Option::is_some) {
                column += 1;
            }
            let (colspan, rowspan) = (
                span(cell_element, "colspan", text),
                span(cell_element, "rowspan", text),
            );
            let index = table.cells.len();
            table.cells.push(Cell {
                element: cell_element,
                row,
                column,
                colspan,
            });
            for covered in grid.iter_mut().skip(row).take(rowspan) {
                if covered.len() < column + colspan {
                    covered.resize(column + colspan, None);
                }
                for slot in &mut covered[column..column + colspan] {
                    *slot = Some(index);
                }
            }
            column += colspan;
        }
    }
    for ((element, header), slots) in row_elements.into_iter().zip(grid) {
        table.rows.push(TableRow {
            element,
            header,
            // shortcut: a column no cell covers (a ragged row under a rowspan) is left out, as
            // IntelliJ's rows hold cells alone
            slots: slots.into_iter().flatten().collect(),
        });
    }
    Some(table)
}

/// Where a cell's caret goes: before its end tag, at its end without one.
fn cell_end(cell: Node) -> usize {
    let mut cursor = cell.walk();
    cell.named_children(&mut cursor)
        .find(|child| child.kind() == "end_tag")
        .map_or(cell.end_byte(), |end| end.start_byte())
}

/// The offset IntelliJ's table navigation moves the caret to: the next cell other than the
/// caret's in `direction` (rows, columns), or nowhere at the table's edge.
fn navigation_target(
    root: Node,
    text: &str,
    caret: usize,
    direction: (isize, isize),
) -> Option<usize> {
    let cell = cell_at(root, text, caret)?;
    let table = table_of(cell, text)?;
    let current = table.cells.iter().position(|c| c.element == cell)?;
    let slot = |row: isize, column: isize| -> Option<usize> {
        let row = table.rows.get(usize::try_from(row).ok()?)?;
        row.slots.get(usize::try_from(column).ok()?).copied()
    };
    let (mut row, mut column) = (
        table.cells[current].row as isize,
        table.cells[current].column as isize,
    );
    loop {
        let (next_row, next_column) = (row + direction.0, column + direction.1);
        match slot(next_row, next_column) {
            Some(found) if found != current => return Some(cell_end(table.cells[found].element)),
            Some(_) => (row, column) = (next_row, next_column),
            None => return None,
        }
    }
}

/// The text between a cell and the one before it (or its row's start tag): how a new cell is
/// set beside it.
fn separator_before(cell: Node, text: &str) -> String {
    let start = cell
        .prev_sibling()
        .map_or(cell.start_byte(), |previous| previous.end_byte());
    let between = text.get(start..cell.start_byte()).unwrap_or_default();
    match between.rfind('\n') {
        Some(newline) => between[newline..].to_owned(),
        None => String::new(),
    }
}

/// Add a New Column to the Table Before/After the Current One: an empty cell in each row (a
/// `th` in the head), a spanning cell's `colspan` grown instead; the edits, and the caret in
/// the table's first empty cell after them.
fn column_edits(
    root: Node,
    text: &str,
    caret: usize,
    before: bool,
) -> Option<(Vec<(Range<usize>, String)>, usize)> {
    let cell = cell_at(root, text, caret)?;
    let table = table_of(cell, text)?;
    let current = table.cells.iter().find(|c| c.element == cell)?;
    let mut column = current.column as isize;
    if !before {
        column += current.colspan as isize;
    }
    let column = column - 1;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for (index, row) in table.rows.iter().enumerate() {
        let new_cell = if row.header { "<th></th>" } else { "<td></td>" };
        if row.slots.is_empty() {
            let at = cell_end(row.element);
            edits.push((at..at, new_cell.to_owned()));
            continue;
        }
        if column < 0 {
            if let Some(first) = row
                .slots
                .iter()
                .map(|ix| &table.cells[*ix])
                .find(|c| c.row == index)
            {
                let at = first.element.start_byte();
                let separator = separator_before(first.element, text);
                edits.push((at..at, format!("{new_cell}{separator}")));
            }
            continue;
        }
        let last = (column as usize).min(row.slots.len() - 1);
        for slot in (0..=last).rev() {
            let cell = &table.cells[row.slots[slot]];
            if let Some((value, range)) = attribute(cell.element, "colspan", text) {
                if cell.row == index
                    && let Ok(span) = value.trim().parse::<usize>()
                {
                    edits.push((range, (span + 1).to_string()));
                }
                break;
            }
            if cell.row == index {
                let at = cell.element.end_byte();
                let separator = separator_before(cell.element, text);
                edits.push((at..at, format!("{separator}{new_cell}")));
                break;
            }
        }
    }
    edits.sort_by_key(|(range, _)| range.start);
    // the text after the edits, for the first empty cell
    let mut edited = String::with_capacity(text.len());
    let mut taken = 0;
    let mut table_end = table.element.end_byte();
    for (range, new_text) in &edits {
        edited.push_str(&text[taken..range.start]);
        edited.push_str(new_text);
        taken = range.end;
        table_end = table_end + new_text.len() - range.len();
    }
    edited.push_str(&text[taken..]);
    let table_start = table.element.start_byte();
    let in_table = edited.get(table_start..table_end)?;
    let empty = ["<td></td>", "<th></th>"]
        .iter()
        .filter_map(|cell| in_table.find(cell))
        .min()?;
    Some((edits, table_start + empty + "<td>".len()))
}

#[cfg(test)]
mod tests {
    use super::{column_edits, config, navigation_target, queries};

    fn parse(text: &str) -> language::Tree {
        let mut parser = language::Parser::new();
        parser
            .set_language(&tree_sitter_html::LANGUAGE.into())
            .expect("HTML's grammar");
        parser.parse(text, None).expect("a tree")
    }

    const TABLE: &str = "<table>\n    <thead>\n    <tr>\n        <th>A</th>\n        <th>B</th>\n    </tr>\n    </thead>\n    <tr>\n        <td>1</td>\n        <td>2</td>\n    </tr>\n    <tr>\n        <td colspan=\"2\">3</td>\n    </tr>\n</table>\n";

    fn at(text: &str, needle: &str) -> usize {
        text.find(needle).expect("the needle") + needle.len()
    }

    #[test]
    fn the_queries_compile_against_the_grammar() {
        let language = language::Language::new(
            config().expect("HTML's config"),
            Some(tree_sitter_html::LANGUAGE.into()),
        )
        .with_queries(queries())
        .expect("HTML's queries");
        assert_eq!(language.name().as_ref(), "HTML");
    }

    #[test]
    fn navigation_moves_between_cells_and_stops_at_the_edges() {
        let tree = parse(TABLE);
        let root = tree.root_node();
        let one = at(TABLE, "<td>1");
        let right = navigation_target(root, TABLE, one, (0, 1)).expect("the next column");
        assert_eq!(right, at(TABLE, "<td>2"));
        assert_eq!(navigation_target(root, TABLE, right, (0, 1)), None);
        let up = navigation_target(root, TABLE, one, (-1, 0)).expect("the header");
        assert_eq!(up, at(TABLE, "<th>A"));
        // the spanning cell is below both columns
        let down = navigation_target(root, TABLE, right, (1, 0)).expect("the spanning cell");
        assert_eq!(down, at(TABLE, "\">3"));
        assert_eq!(navigation_target(root, TABLE, 3, (0, 1)), None);
    }

    #[test]
    fn a_column_goes_into_every_row_and_spans_grow() {
        let tree = parse(TABLE);
        let (edits, caret) =
            column_edits(tree.root_node(), TABLE, at(TABLE, "<td>1"), false).expect("edits");
        let mut text = TABLE.to_owned();
        for (range, new_text) in edits.iter().rev() {
            text.replace_range(range.clone(), new_text);
        }
        assert_eq!(
            text,
            "<table>\n    <thead>\n    <tr>\n        <th>A</th>\n        <th></th>\n        <th>B</th>\n    </tr>\n    </thead>\n    <tr>\n        <td>1</td>\n        <td></td>\n        <td>2</td>\n    </tr>\n    <tr>\n        <td colspan=\"3\">3</td>\n    </tr>\n</table>\n"
        );
        assert_eq!(caret, at(&text, "<th>A</th>\n        <th>"));
        let (edits, _) =
            column_edits(tree.root_node(), TABLE, at(TABLE, "<td>1"), true).expect("edits");
        let mut text = TABLE.to_owned();
        for (range, new_text) in edits.iter().rev() {
            text.replace_range(range.clone(), new_text);
        }
        assert!(text.contains("<tr>\n        <th></th>\n        <th>A</th>"));
        assert!(text.contains("<tr>\n        <td></td>\n        <td>1</td>"));
        assert!(text.contains("<tr>\n        <td></td>\n        <td colspan=\"2\">3</td>"));
    }
}
