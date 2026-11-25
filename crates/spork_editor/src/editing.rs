//! IntelliJ's editing actions Zed has no action for: Column Selection Mode (⇧⌘8), Move Caret to
//! Code Block Start/End (⌥⌘[, ⌥⌘], with selection ⌥⇧⌘[, ⌥⇧⌘]) and Move Caret to Page Top/Bottom
//! (⌘⇞, ⌘⇟, with selection ⇧⌘⇞, ⇧⌘⇟).

use std::any::Any;

use editor::display_map::{DisplayRow, ToDisplayPoint as _};
use editor::{Addon, DisplayPoint, Editor, SelectionEffects};
use gpui::{App, Context, KeyContext, Window};
use multi_buffer::{MultiBufferOffset, MultiBufferSnapshot};
use text::{Bias, SelectionGoal};

gpui::actions!(
    spork_editor,
    [
        /// Turns column (block) selection on or off in the editor: ⇧↑ and ⇧↓ grow the block by a
        /// line and a mouse drag selects a column (IntelliJ's Column Selection Mode, ⇧⌘8).
        ToggleColumnMode,
        /// Moves the caret after the brace opening the code block it is in (IntelliJ's Move Caret
        /// to Code Block Start, ⌥⌘[).
        MoveToCodeBlockStart,
        /// Moves the caret before the brace closing the code block it is in (IntelliJ's Move Caret
        /// to Code Block End, ⌥⌘]).
        MoveToCodeBlockEnd,
        /// Move Caret to Code Block Start with Selection (⌥⇧⌘[).
        SelectToCodeBlockStart,
        /// Move Caret to Code Block End with Selection (⌥⇧⌘]).
        SelectToCodeBlockEnd,
        /// Moves the caret to the first line shown whole, in its column (IntelliJ's Move Caret to
        /// Page Top, ⌘⇞).
        MoveToPageTop,
        /// Moves the caret to the last line shown whole (IntelliJ's Move Caret to Page Bottom, ⌘⇟).
        MoveToPageBottom,
        /// Move Caret to Page Top with Selection (⇧⌘⇞).
        SelectToPageTop,
        /// Move Caret to Page Bottom with Selection (⇧⌘⇟).
        SelectToPageBottom,
        /// Moves the caret to the bracket matching the one beside it, or to the bracket opening
        /// the pair it is in (IntelliJ's Move Caret to Matching Brace, ⌃M).
        MoveToMatchingBrace,
    ]
);

/// An editor in Column Selection Mode: its key context gets `column_mode`, where the keymap's
/// ⇧↑ and ⇧↓ add a line to the block, and a plain mouse drag selects a column.
struct ColumnMode;

impl Addon for ColumnMode {
    fn extend_key_context(&self, context: &mut KeyContext, _: &App) {
        context.add("column_mode");
    }

    fn columnar_mouse_selection(&self) -> bool {
        true
    }

    fn to_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) fn register(editor: &mut Editor, handle: gpui::WeakEntity<Editor>) {
    macro_rules! on {
        ($action:ty, $run:expr) => {
            editor
                .register_action({
                    let handle = handle.clone();
                    move |_: &$action, window, cx| {
                        handle
                            .update(cx, |editor, cx| $run(editor, window, cx))
                            .ok();
                    }
                })
                .detach();
        };
    }
    on!(ToggleColumnMode, toggle_column_mode);
    on!(MoveToMatchingBrace, matching_brace);
    on!(MoveToCodeBlockStart, |editor, window, cx| code_block(
        editor, true, false, window, cx
    ));
    on!(MoveToCodeBlockEnd, |editor, window, cx| code_block(
        editor, false, false, window, cx
    ));
    on!(SelectToCodeBlockStart, |editor, window, cx| code_block(
        editor, true, true, window, cx
    ));
    on!(SelectToCodeBlockEnd, |editor, window, cx| code_block(
        editor, false, true, window, cx
    ));
    on!(MoveToPageTop, |editor, window, cx| page_edge(
        editor, true, false, window, cx
    ));
    on!(MoveToPageBottom, |editor, window, cx| page_edge(
        editor, false, false, window, cx
    ));
    on!(SelectToPageTop, |editor, window, cx| page_edge(
        editor, true, true, window, cx
    ));
    on!(SelectToPageBottom, |editor, window, cx| page_edge(
        editor, false, true, window, cx
    ));
}

fn toggle_column_mode(editor: &mut Editor, _: &mut Window, cx: &mut Context<Editor>) {
    if editor.addon::<ColumnMode>().is_some() {
        editor.unregister_addon::<ColumnMode>();
    } else {
        editor.register_addon(ColumnMode);
    }
    cx.notify();
}

/// Whether Column Selection Mode is on in `editor` (the Edit menu's and the context menu's
/// check).
pub fn column_mode(editor: &Editor) -> bool {
    editor.addon::<ColumnMode>().is_some()
}

/// IntelliJ's code block moves (`CodeBlockUtil`) over the structural braces `{ }`: to the start,
/// after the `{` of the innermost block around the caret, or before its `{` when the caret is just
/// after its `}`; to the end, before its `}`, or after it when the caret is just before its `{`.
/// The brace next to the caret is stepped over, so pressing again goes to the outer block.
fn code_block(
    editor: &mut Editor,
    start: bool,
    select: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let moved = |map: &editor::display_map::DisplaySnapshot, head: DisplayPoint| {
        let buffer = map.buffer_snapshot();
        let offset = head.to_offset(map, Bias::Left);
        let target = if start {
            code_block_start(buffer, offset)
        } else {
            code_block_end(buffer, offset)
        };
        target.map(|target| target.to_display_point(map))
    };
    editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
        selections.move_with(&mut |map, selection| {
            if let Some(target) = moved(map, selection.head()) {
                if select {
                    selection.set_head(target, SelectionGoal::None);
                } else {
                    selection.collapse_to(target, SelectionGoal::None);
                }
            }
        })
    });
}

/// The `{ }` pairs around `offset`, the brackets themselves counted as around it.
fn braces_around(
    buffer: &MultiBufferSnapshot,
    offset: MultiBufferOffset,
) -> Vec<(MultiBufferOffset, MultiBufferOffset)> {
    let Some(pairs) = buffer.enclosing_bracket_ranges(offset..offset) else {
        return Vec::new();
    };
    pairs
        .filter(|(open, _)| buffer.chars_at(open.start).next() == Some('{'))
        .map(|(open, close)| (open.start, close.start))
        .collect()
}

fn code_block_start(
    buffer: &MultiBufferSnapshot,
    offset: MultiBufferOffset,
) -> Option<MultiBufferOffset> {
    let pairs = braces_around(buffer, offset);
    // just after a block's `}`: its `{`
    if let Some((open, _)) = pairs.iter().find(|(_, close)| close.0 + 1 == offset.0) {
        return Some(*open);
    }
    pairs
        .into_iter()
        .filter(|(open, close)| open.0 + 1 < offset.0 && offset <= *close)
        .max_by_key(|(open, _)| *open)
        .map(|(open, _)| MultiBufferOffset(open.0 + 1))
}

fn code_block_end(
    buffer: &MultiBufferSnapshot,
    offset: MultiBufferOffset,
) -> Option<MultiBufferOffset> {
    let pairs = braces_around(buffer, offset);
    // just before a block's `{`: after its `}`
    if let Some((_, close)) = pairs.iter().find(|(open, _)| *open == offset) {
        return Some(MultiBufferOffset(close.0 + 1));
    }
    pairs
        .into_iter()
        .filter(|(open, close)| open.0 < offset.0 && offset < *close)
        .min_by_key(|(_, close)| *close)
        .map(|(_, close)| close)
}

/// IntelliJ's Move Caret to Matching Brace (`MatchBraceAction`, the brace matcher's navigation
/// offset): from beside a bracket, after its pair's closing bracket when it opens, before its
/// pair's opening bracket when it closes (just after a closing bracket first, then just before an
/// opening one, just after an opening one, just before a closing one); elsewhere, before the
/// bracket opening the pair the caret is in.
fn matching_brace(editor: &mut Editor, window: &mut Window, cx: &mut Context<Editor>) {
    editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
        selections.move_offsets_with(&mut |buffer, selection| {
            let offset = selection.head();
            let Some(pairs) = buffer.enclosing_bracket_ranges(offset..offset) else {
                return;
            };
            let pairs: Vec<_> = pairs.collect();
            let beside = |touches: &dyn Fn(
                &(
                    std::ops::Range<MultiBufferOffset>,
                    std::ops::Range<MultiBufferOffset>,
                ),
            ) -> bool| { pairs.iter().find(|pair| touches(pair)) };
            let target = if let Some((open, _)) = beside(&|(_, close)| close.end == offset) {
                open.start
            } else if let Some((_, close)) = beside(&|(open, _)| open.start == offset) {
                close.end
            } else if let Some((_, close)) = beside(&|(open, _)| open.end == offset) {
                close.end
            } else if let Some((open, _)) = beside(&|(_, close)| close.start == offset) {
                open.start
            } else {
                let Some((open, _)) = pairs
                    .iter()
                    .filter(|(open, close)| open.end <= offset && offset <= close.start)
                    .max_by_key(|(open, _)| open.start)
                else {
                    return;
                };
                open.start
            };
            selection.collapse_to(target, SelectionGoal::None);
        })
    });
}

/// IntelliJ's Move Caret to Page Top/Bottom (`EditorActionUtil.moveCaretPageTop`): the first or
/// last line shown whole, the caret's column kept; the editor doesn't scroll.
fn page_edge(
    editor: &mut Editor,
    top: bool,
    select: bool,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let Some(lines) = editor.visible_line_count() else {
        return;
    };
    let scroll_top = editor.scroll_position(cx).y;
    let first = scroll_top.ceil();
    let last = ((scroll_top + lines).floor() - 1.).max(first);
    let row = if top { first } else { last };
    editor.change_selections(SelectionEffects::no_scroll(), window, cx, |selections| {
        selections.move_with(&mut |map, selection| {
            let row = DisplayRow((row as u32).min(map.max_point().row().0));
            let column = selection.head().column();
            let target = map.clip_point(DisplayPoint::new(row, column), Bias::Left);
            if select {
                selection.set_head(target, SelectionGoal::None);
            } else {
                selection.collapse_to(target, SelectionGoal::None);
            }
        })
    });
}
