//! IntelliJ's editor on Zed's (ROADMAP, phase 5): the editing actions Zed lacks, and what the app
//! draws into Zed's editor through its hooks.

mod documentation;
mod documentation_window;
mod editing;
mod folding;
mod inspection_widget;
mod intention_preview;
mod markdown;
mod markdown_styling;
mod paste_history;
mod popups;

pub use documentation_window::{
    DocumentationPanel, OpenInDocumentationToolWindow, QuickDocumentation,
    ToggleAutoUpdateFromSource, ToggleShowDocumentationDuringCompletion,
    ToggleShowDocumentationOnMouseMove, ToggleShowDocumentationPopupFirst, documentation_panel,
    e2e_state as documentation_e2e_state,
};
pub use editing::{
    MoveToCodeBlockEnd, MoveToCodeBlockStart, MoveToMatchingBrace, MoveToPageBottom, MoveToPageTop,
    SelectToCodeBlockEnd, SelectToCodeBlockStart, SelectToPageBottom, SelectToPageTop,
    ToggleColumnMode, column_mode,
};
pub use inspection_widget::{GoToNextError, GoToPreviousError};
pub use markdown::{
    CreateLink, GenerateTableOfContents, MarkdownEditorAndPreview, MarkdownEditorOnly,
    MarkdownPreviewOnly, MoveTableRowDown, MoveTableRowUp, SelectTableColumnCells, SelectTableRow,
    ToggleBold, ToggleCodeSpan, ToggleItalic, ToggleStrikethrough,
};
pub use markdown_styling::{
    ConvertToReference, CreateOrChangeList, DecreaseHeaderLevel, IncreaseHeaderLevel, InsertImage,
    InsertTable, SetHeaderStyle,
};
pub use paste_history::PasteFromHistory;

use editor::actions::Newline;
use editor::{Editor, SelectionEffects};
use gpui::{App, Context, Window};
use multi_buffer::MultiBufferRow;
use text::Point;

gpui::actions!(
    spork_editor,
    [
        /// Breaks the line at the caret, the caret staying before the break (IntelliJ's Split
        /// Line, ⌘⏎).
        SplitLine,
        /// Puts a caret at the end of each line a selection spans, the selections dropped
        /// (IntelliJ's Add Carets to Ends of Selected Lines, ⌥⇧G).
        AddCaretsToLineEnds,
    ]
);

pub fn init(cx: &mut App) {
    folding::init(cx);
    inspection_widget::init(cx);
    popups::init(cx);
    paste_history::init(cx);
    documentation_window::init(cx);
    markdown::init(cx);
    markdown_styling::init(cx);
    cx.observe_new(|editor: &mut Editor, _, cx: &mut Context<Editor>| {
        if !editor.mode().is_full() {
            return;
        }
        let handle = cx.entity().downgrade();
        inspection_widget::register(editor, handle.clone());
        editing::register(editor, handle.clone());
        paste_history::register(editor, handle.clone());
        markdown::register(editor, handle.clone());
        markdown_styling::register(editor, handle.clone());
        markdown_styling::watch_selections(editor, cx);
        editor
            .register_action({
                let handle = handle.clone();
                move |_: &QuickDocumentation, window, cx| {
                    handle
                        .update(cx, |editor, cx| {
                            documentation_window::quick_documentation(editor, window, cx)
                        })
                        .ok();
                }
            })
            .detach();
        editor
            .register_action({
                let handle = handle.clone();
                move |_: &SplitLine, window, cx| {
                    handle
                        .update(cx, |editor, cx| split_line(editor, window, cx))
                        .ok();
                }
            })
            .detach();
        editor
            .register_action(move |_: &AddCaretsToLineEnds, window, cx| {
                handle
                    .update(cx, |editor, cx| add_carets_to_line_ends(editor, window, cx))
                    .ok();
            })
            .detach();
    })
    .detach();
}

fn split_line(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    if editor.read_only(cx) {
        return;
    }
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    // anchored before the caret, so the line break goes after it
    let carets: Vec<_> = editor
        .selections
        .disjoint_anchors()
        .iter()
        .map(|selection| snapshot.anchor_before(selection.start))
        .collect();
    editor.transact(window, cx, |editor, window, cx| {
        editor.newline(&Newline, window, cx);
        editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
            selections.select_anchor_ranges(carets.into_iter().map(|caret| caret..caret))
        });
    });
}

fn add_carets_to_line_ends(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    let display = editor.display_snapshot(cx);
    let buffer = display.buffer_snapshot();
    let mut carets = Vec::new();
    for selection in editor.selections.all::<Point>(&display) {
        let mut last_row = selection.end.row;
        // a selection ending at a line's start leaves that line out, as IntelliJ's does
        if last_row > selection.start.row && selection.end.column == 0 {
            last_row -= 1;
        }
        for row in selection.start.row..=last_row {
            let end = Point::new(row, buffer.line_len(MultiBufferRow(row)));
            carets.push(end..end);
        }
    }
    editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
        selections.select_ranges(carets)
    });
}
