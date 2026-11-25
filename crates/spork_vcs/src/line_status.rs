//! IntelliJ's line-status popup (`LineStatusMarkerPopupPanel`): a click on a change marker in the
//! editor's gutter, or Next Change and Previous Change (⌃⌥⇧↓ and ⌃⌥⇧↑), shows the change's
//! former text under its line, its toolbar's Previous Change, Next Change, Rollback…, Show Diff
//! for Lines and Copy above it. The changes are those Zed's git layer feeds the editor
//! (`buffer_diff`, the buffer against HEAD), kept up to date as the buffer is edited.

use std::ops::Range;
use std::rc::Rc;

use collections::HashMap;
use editor::display_map::DisplayRow;
use editor::scroll::Autoscroll;
use editor::{
    Anchor, ChangeMarkerClick, Editor, EditorEvent, MultiBufferSnapshot, SelectionEffects,
    ToOffset as _, ToPoint as _,
};
use gpui::{
    AnyElement, AnyWindowHandle, App, ClipboardItem, Context, Global, HighlightStyle,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Styled as _,
    StyledText, Subscription, WeakEntity, Window, anchored, deferred, px,
};
use multi_buffer::MultiBufferDiffHunk;
use settings::Settings as _;
use spork_workbench::ActionButton;
use text::{Bias, BufferId};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;
use workspace::Workspace;

gpui::actions!(
    vcs,
    [
        /// Goes to the next change in the file and shows it (IntelliJ's Next Change).
        ShowNextChangeMarker,
        /// Goes to the previous change in the file and shows it (IntelliJ's Previous Change).
        ShowPrevChangeMarker,
    ]
);

/// The popup showing, at most one in the app as IntelliJ's `LineStatusMarkerPopupService` has it.
#[derive(Default)]
pub(crate) struct LineStatusPopup(Option<Popup>);

impl Global for LineStatusPopup {}

struct Popup {
    window: AnyWindowHandle,
    /// The text's top left in the window: under the change's line, lined up with its text.
    origin: Point<Pixels>,
    editor: WeakEntity<Editor>,
    buffer_id: BufferId,
    buffer_range: Range<text::Anchor>,
    /// The change's former lines, without their last line break, and their highlights.
    old_text: Option<(String, Vec<(Range<usize>, HighlightStyle)>)>,
    /// The former lines as they were, what Rollback puts back: none for added lines.
    restored: String,
    previous: Option<Anchor>,
    next: Option<Anchor>,
    _closes: Subscription,
}

pub(crate) fn init(cx: &mut App) {
    cx.set_global(LineStatusPopup::default());
    cx.set_global(ChangeMarkerClick(Rc::new(
        |editor, hunk, row, window, cx| show(editor, hunk.start, Some(row), window, cx),
    )));
    cx.observe_keystrokes(|event, _, cx| {
        if event.keystroke.key == "escape" {
            close(cx);
        }
    })
    .detach();
    cx.observe_new(|workspace: &mut Workspace, _, _| {
        workspace.register_action(|workspace, _: &ShowNextChangeMarker, window, cx| {
            go_to_change(workspace, true, window, cx)
        });
        workspace.register_action(|workspace, _: &ShowPrevChangeMarker, window, cx| {
            go_to_change(workspace, false, window, cx)
        });
    })
    .detach();
}

pub(crate) fn close(cx: &mut App) {
    if cx
        .try_global::<LineStatusPopup>()
        .is_some_and(|popup| popup.0.is_some())
    {
        cx.set_global(LineStatusPopup(None));
    }
}

/// Next Change and Previous Change: the caret to the change's last line, then its popup, as
/// IntelliJ's `ShowChangeMarkerAction` does; nothing past the first or the last change.
fn go_to_change(
    workspace: &mut Workspace,
    next: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
        return;
    };
    editor.update(cx, |editor, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let caret = editor
            .selections
            .newest_anchor()
            .head()
            .to_point(&snapshot)
            .row;
        let hunks: Vec<MultiBufferDiffHunk> = snapshot.diff_hunks().collect();
        let target = match next {
            true => hunks.iter().find(|hunk| hunk.row_range.start.0 > caret),
            false => hunks
                .iter()
                .rev()
                .find(|hunk| hunk.row_range.end.0 <= caret && hunk.row_range.start.0 < caret),
        };
        if let Some(hunk) = target {
            move_to(editor, hunk, window, cx);
        }
    });
}

/// The caret to `hunk`'s last line (the line after it, for deleted lines), then its popup once
/// the editor has scrolled there and drawn.
fn move_to(
    editor: &mut Editor,
    hunk: &MultiBufferDiffHunk,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let row = match hunk.row_range.is_empty() {
        true => hunk.row_range.start.0,
        false => hunk.row_range.end.0 - 1,
    };
    let point = text::Point::new(row, 0);
    editor.change_selections(
        SelectionEffects::scroll(Autoscroll::center()),
        window,
        cx,
        |selections| selections.select_ranges([point..point]),
    );
    let start = hunk.multi_buffer_range.start;
    let editor = cx.entity().downgrade();
    window.on_next_frame(move |window, cx| {
        editor
            .update(cx, |editor, cx| show(editor, start, None, window, cx))
            .ok();
    });
}

/// Shows the change starting at `start`, under the display row clicked or else under the caret's
/// row. Answers whether there was a change to show.
fn show(
    editor: &mut Editor,
    start: Anchor,
    row: Option<DisplayRow>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> bool {
    let snapshot = editor.buffer().read(cx).snapshot(cx);
    let hunks: Vec<MultiBufferDiffHunk> = snapshot.diff_hunks().collect();
    let offset = start.to_offset(&snapshot);
    let Some(ix) = hunks
        .iter()
        .position(|hunk| hunk.multi_buffer_range.start.to_offset(&snapshot) == offset)
    else {
        return false;
    };
    let hunk = &hunks[ix];
    let row = row.unwrap_or_else(|| {
        let display = editor.display_snapshot(cx);
        let caret = editor.selections.newest_anchor().head().to_point(&snapshot);
        display.point_to_display_point(caret, Bias::Left).row()
    });
    let Some(origin) = editor.text_origin_below_row(row) else {
        return false;
    };
    let _closes = cx.subscribe_self(|_, event: &EditorEvent, cx| match event {
        EditorEvent::Edited { .. }
        | EditorEvent::BufferEdited
        | EditorEvent::SelectionsChanged { .. }
        | EditorEvent::ScrollPositionChanged {
            autoscroll: false, ..
        } => close(cx),
        _ => {}
    });
    let popup = Popup {
        window: window.window_handle(),
        // the text field's 1 px border and 2 px inset before its text (`createEditorFragmentBorder`)
        origin: origin - gpui::point(px(3.), px(0.)),
        editor: cx.entity().downgrade(),
        buffer_id: hunk.buffer_id,
        buffer_range: hunk.buffer_range.clone(),
        old_text: old_text(&snapshot, hunk, cx),
        restored: snapshot
            .diff_for_buffer_id(hunk.buffer_id)
            .map(|diff| {
                let range = hunk.diff_base_byte_range.start.0..hunk.diff_base_byte_range.end.0;
                diff.base_text().text_for_range(range).collect()
            })
            .unwrap_or_default(),
        previous: ix
            .checked_sub(1)
            .and_then(|ix| hunks.get(ix))
            .map(|hunk| hunk.multi_buffer_range.start),
        next: hunks.get(ix + 1).map(|hunk| hunk.multi_buffer_range.start),
        _closes,
    };
    cx.set_global(LineStatusPopup(Some(popup)));
    true
}

/// The change's former lines with the file's syntax highlighting and IntelliJ's word diff over
/// them; `None` for added lines, which had none.
fn old_text(
    snapshot: &MultiBufferSnapshot,
    hunk: &MultiBufferDiffHunk,
    cx: &App,
) -> Option<(String, Vec<(Range<usize>, HighlightStyle)>)> {
    let range = hunk.diff_base_byte_range.start.0..hunk.diff_base_byte_range.end.0;
    if range.is_empty() {
        return None;
    }
    let diff = snapshot.diff_for_buffer_id(hunk.buffer_id)?;
    let base = diff.base_text();
    let syntax = cx.theme().syntax();
    let mut text = String::new();
    let mut syntax_highlights = Vec::new();
    let styling = language::LanguageAwareStyling {
        tree_sitter: true,
        diagnostics: false,
    };
    for chunk in base.chunks(range.clone(), styling) {
        let start = text.len();
        text.push_str(chunk.text);
        if let Some(style) = chunk.syntax_highlight_id.and_then(|id| syntax.get(id)) {
            syntax_highlights.push((start..text.len(), *style));
        }
    }
    let word_diffs = snapshot
        .buffer_for_id(hunk.buffer_id)
        .map(|buffer| {
            diff.hunks_intersecting_range(hunk.buffer_range.clone(), buffer)
                .find(|candidate| candidate.diff_base_byte_range == range)
                .map(|candidate| candidate.base_word_diffs)
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let changed = HighlightStyle {
        background_color: Some(cx.theme().colors().version_control_word_deleted),
        ..HighlightStyle::default()
    };
    let words = word_diffs.into_iter().map(|range| (range, changed));
    let highlights = gpui::combine_highlights(syntax_highlights, words).collect();
    let text = text.strip_suffix('\n').map(str::to_owned).unwrap_or(text);
    Some((text, highlights))
}

/// The popup for the window being drawn, drawn over everything.
pub(crate) fn render(window: &Window, cx: &App) -> Option<AnyElement> {
    let popup = cx.global::<LineStatusPopup>().0.as_ref()?;
    if popup.window != window.window_handle() {
        return None;
    }
    let tokens = spork_workbench::Tokens::get(cx);
    let theme = cx.theme();
    let settings = ThemeSettings::get_global(cx);
    let font_size = settings.buffer_font_size(cx);
    let editor = popup.editor.clone();
    let button = |id: &'static str, icon: &'static str, tooltip: &'static str| {
        ActionButton::new(id, icon).tooltip(tooltip)
    };
    let go = |target: Option<Anchor>| {
        let editor = editor.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            let Some(target) = target else { return };
            editor
                .update(cx, |editor, cx| {
                    let snapshot = editor.buffer().read(cx).snapshot(cx);
                    let offset = target.to_offset(&snapshot);
                    if let Some(hunk) = snapshot
                        .diff_hunks()
                        .find(|hunk| hunk.multi_buffer_range.start.to_offset(&snapshot) == offset)
                    {
                        move_to(editor, &hunk, window, cx);
                    }
                })
                .ok();
        }
    };
    let rollback = {
        let (editor, buffer_id, range, old) = (
            editor.clone(),
            popup.buffer_id,
            popup.buffer_range.clone(),
            popup.restored.clone(),
        );
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            let changes =
                HashMap::from_iter([(buffer_id, vec![(range.clone(), old.as_str().into())])]);
            close(cx);
            editor
                .update(cx, |editor, cx| editor.restore(changes, window, cx))
                .ok();
        }
    };
    let copy = popup.old_text.as_ref().map(|(text, _)| format!("{text}\n"));
    let show_diff = {
        let editor = editor.clone();
        move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
            close(cx);
            if let Some(editor) = editor.upgrade() {
                crate::revision_tabs::show_file_diff(&editor, window, cx);
            }
        }
    };
    let toolbar = gpui::div()
        .flex()
        .items_center()
        .gap(px(2.))
        // `VersionControl.MarkerPopup.borderInsets` in the New UI, and the toolbar's own 3 px
        .pt(px(6.))
        .pb(px(6.))
        .pl(px(8.))
        .pr(px(10.))
        .bg(tokens.island)
        .border_color(tokens.marker_popup_border)
        .border_t_1()
        .border_l_1()
        .border_r_1()
        .when(popup.old_text.is_none(), |toolbar| toolbar.border_b_1())
        .child(
            button("lst-previous", "general/up", "Previous Change (⌃⌥⇧↑)")
                .disabled(popup.previous.is_none())
                .on_click(go(popup.previous)),
        )
        .child(
            button("lst-next", "general/down", "Next Change (⌃⌥⇧↓)")
                .disabled(popup.next.is_none())
                .on_click(go(popup.next)),
        )
        .child(button("lst-rollback", "vcs/revert", "Rollback… (⌥⌘Z)").on_click(rollback))
        .child(button("lst-diff", "vcs/diff", "Show Diff for Lines (⌘D)").on_click(show_diff))
        .child(
            button("lst-copy", "general/copy", "Copy (⌘C)")
                .disabled(copy.is_none())
                .on_click(move |_, _, cx| {
                    if let Some(copy) = copy.clone() {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy));
                    }
                }),
        );
    let text = popup.old_text.as_ref().map(|(text, highlights)| {
        gpui::div()
            .p(px(2.))
            .border_1()
            .border_color(tokens.marker_popup_border)
            .bg(theme.colors().editor_background)
            .text_color(theme.colors().editor_foreground)
            .font(settings.buffer_font.clone())
            .text_size(font_size)
            .line_height(settings.line_height() * font_size)
            .whitespace_nowrap()
            .child(StyledText::new(text.clone()).with_highlights(highlights.clone()))
    });
    let panel = gpui::div()
        .id("line-status-popup")
        .occlude()
        .flex()
        .flex_col()
        .items_start()
        .on_mouse_down_out(|_, _, cx| close(cx))
        .child(toolbar)
        .children(text);
    let origin = popup.origin;
    Some(
        deferred(
            anchored()
                .position(origin)
                .snap_to_window_with_margin(px(8.))
                .child(crate::spork_look(cx).child(panel)),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

use gpui::prelude::FluentBuilder as _;
