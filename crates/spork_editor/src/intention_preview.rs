//! IntelliJ's intention preview beside the ⌥⏎ list (`IntentionPreviewPopupUpdateProcessor`,
//! `IntentionPreviewEditorsPanel`): the selected action's change, resolved by its language
//! server, shown as the changed lines of the file after it, with their numbers, the changed text
//! marked as IntelliJ's diff marks it. "Loading…" shows until the server answered; an action
//! that changes nothing in this file (a command, a run configuration) has no preview.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use editor::look::IntentionPreview;
use gpui::{
    AnyElement, App, AsyncApp, Entity, EntityId, FontWeight, Global, HighlightStyle,
    InteractiveElement as _, IntoElement as _, ParentElement as _, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, StyledText, Task, Window, div, px,
};
use language::{Buffer, Language};
use project::{CodeAction, LspAction};
use settings::Settings as _;
use spork_workbench::Typography as _;
use text::{Bias, PointUtf16, Unclipped};
use theme::ActiveTheme as _;
use theme_settings::ThemeSettings;
use util::ResultExt as _;

use crate::popups::tokens;

/// The preview's least width and tallest (`IntentionPreviewPopupUpdateProcessor.MIN_WIDTH`,
/// `MAX_HEIGHT`), and its editor's insets (`JBUI.Borders.empty(0, 10)`); a message's are 6, 10.
const MIN_WIDTH: f32 = 300.;
const MAX_HEIGHT: f32 = 300.;
const PADDING_X: f32 = 10.;
const MESSAGE_PADDING_Y: f32 = 6.;
/// The gap between the line numbers and the text.
const GUTTER_GAP: f32 = 12.;

#[derive(Clone)]
enum Preview {
    Loading,
    /// Nothing to show: the action changes nothing in this file.
    None,
    Change(Change, Option<Arc<Language>>),
}

/// The changed lines of the file after the action.
#[derive(Clone, PartialEq, Debug)]
struct Change {
    /// The first line's number, from 1.
    first_line: u32,
    text: String,
    /// What the action writes, in `text`: `true` where it only adds (IntelliJ's
    /// `DIFF_INSERTED`), `false` where it replaces (`DIFF_MODIFIED`).
    marks: Vec<(Range<usize>, bool)>,
}

/// The previews of one version of one file, by action.
#[derive(Default)]
struct Previews {
    of: Option<(EntityId, clock::Global)>,
    by_title: HashMap<SharedString, Preview>,
    tasks: Vec<Task<()>>,
}

impl Global for Previews {}

pub(crate) fn render(
    preview: IntentionPreview,
    _: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let title: SharedString = preview.action.lsp_action.title().to_owned().into();
    let version = (
        preview.buffer.entity_id(),
        preview.buffer.read(cx).version(),
    );
    let language = preview.buffer.read(cx).language().cloned();
    let previews = cx.default_global::<Previews>();
    if previews.of.as_ref() != Some(&version) {
        *previews = Previews {
            of: Some(version),
            ..Previews::default()
        };
    }
    let shown = match previews.by_title.get(&title) {
        Some(shown) => shown.clone(),
        None => {
            previews.by_title.insert(title.clone(), Preview::Loading);
            let task = cx.spawn(async move |cx| {
                let change = compute(preview, cx).await.log_err().flatten();
                cx.update(|cx| {
                    let previews = cx.default_global::<Previews>();
                    previews.by_title.insert(
                        title,
                        change.map_or(Preview::None, |change| Preview::Change(change, language)),
                    );
                    cx.refresh_windows();
                });
            });
            cx.default_global::<Previews>().tasks.push(task);
            Preview::Loading
        }
    };
    match shown {
        Preview::Loading => Some(message("Loading…", cx)),
        Preview::None => None,
        Preview::Change(change, language) => Some(change_view(change, language, cx)),
    }
}

/// Resolves the action with its language server, then applies its edits of this file to a copy.
async fn compute(preview: IntentionPreview, cx: &mut AsyncApp) -> anyhow::Result<Option<Change>> {
    let IntentionPreview {
        editor,
        buffer,
        action,
    } = preview;
    let resolve = editor.update(cx, |editor, cx| {
        let project = editor.project()?.clone();
        Some(project.update(cx, |project, cx| {
            project.lsp_store().update(cx, |lsp_store, cx| {
                lsp_store.resolve_code_action(&buffer, action, cx)
            })
        }))
    })?;
    let Some(resolve) = resolve else {
        return Ok(None);
    };
    let action = resolve.await?;
    Ok(cx.update(|cx| change_of(&action, &buffer, cx)))
}

/// The action's edits of `buffer`'s file, applied to its text.
fn change_of(action: &CodeAction, buffer: &Entity<Buffer>, cx: &App) -> Option<Change> {
    let LspAction::Action(lsp_action) = &action.lsp_action else {
        return None;
    };
    let edit = lsp_action.edit.as_ref()?;
    let path = buffer.read(cx).file()?.as_local()?.abs_path(cx);
    let uri = lsp::Uri::from_file_path(&path).ok()?;
    let mut edits: Vec<lsp::TextEdit> = edit
        .changes
        .as_ref()
        .and_then(|changes| changes.get(&uri).cloned())
        .unwrap_or_default();
    let document_edits = match &edit.document_changes {
        Some(lsp::DocumentChanges::Edits(edits)) => edits.iter().collect::<Vec<_>>(),
        Some(lsp::DocumentChanges::Operations(operations)) => operations
            .iter()
            .filter_map(|operation| match operation {
                lsp::DocumentChangeOperation::Edit(edit) => Some(edit),
                lsp::DocumentChangeOperation::Op(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    for document in document_edits {
        if document.text_document.uri == uri {
            edits.extend(document.edits.iter().map(|edit| match edit {
                lsp::Edit::Plain(edit) => edit.clone(),
                lsp::Edit::Annotated(annotated) => annotated.text_edit.clone(),
                // the snippet's text as it is inserted, its tab stops left as written
                lsp::Edit::Snippet(snippet) => lsp::TextEdit {
                    range: snippet.range,
                    new_text: snippet.snippet.value.clone(),
                },
            }));
        }
    }
    let snapshot = buffer.read(cx).snapshot();
    let offset = |position: lsp::Position| {
        let point = snapshot.clip_point_utf16(
            Unclipped(PointUtf16::new(position.line, position.character)),
            Bias::Left,
        );
        snapshot.point_utf16_to_offset(point)
    };
    let edits = edits
        .into_iter()
        .map(|edit| {
            (
                offset(edit.range.start)..offset(edit.range.end),
                edit.new_text,
            )
        })
        .collect::<Vec<_>>();
    apply(&snapshot.text(), edits)
}

/// `text` with `edits` (ranges of `text`, none overlapping) applied: the lines they touch, and
/// where they wrote.
fn apply(text: &str, mut edits: Vec<(Range<usize>, String)>) -> Option<Change> {
    if edits.is_empty() {
        return None;
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut new_text = String::with_capacity(text.len());
    let mut marks = Vec::new();
    let mut copied = 0;
    for (range, inserted) in edits {
        let start = range.start.max(copied);
        new_text.push_str(text.get(copied..start)?);
        let at = new_text.len();
        new_text.push_str(&inserted);
        if !inserted.is_empty() {
            marks.push((at..new_text.len(), range.is_empty()));
        }
        copied = range.end.max(start);
    }
    let changed_start = marks
        .first()
        .map_or(new_text.len(), |(range, _)| range.start);
    let changed_end = marks.last().map_or(new_text.len(), |(range, _)| range.end);
    new_text.push_str(text.get(copied..)?);
    // whole lines around what changed
    let line_start = new_text[..changed_start].rfind('\n').map_or(0, |ix| ix + 1);
    let line_end = new_text[changed_end..]
        .find('\n')
        .map_or(new_text.len(), |ix| changed_end + ix);
    let first_line = new_text[..line_start].matches('\n').count() as u32 + 1;
    let marks = marks
        .into_iter()
        .map(|(range, added)| (range.start - line_start..range.end - line_start, added))
        .collect();
    Some(Change {
        first_line,
        text: new_text[line_start..line_end].to_owned(),
        marks,
    })
}

/// A message in the preview's place ("Loading…").
fn message(text: &'static str, cx: &App) -> AnyElement {
    let tokens = tokens(cx);
    surface(cx)
        .px(px(PADDING_X))
        .py(px(MESSAGE_PADDING_Y))
        .font(ThemeSettings::get_global(cx).ui_font.clone())
        .text_ui()
        .text_color(tokens.text_secondary)
        .child(text)
        .into_any_element()
}

fn surface(cx: &App) -> gpui::Stateful<gpui::Div> {
    let tokens = tokens(cx);
    div()
        .id("intention-preview")
        .role(Role::Group)
        .aria_label("Intention Preview")
        .occlude()
        .min_w(px(MIN_WIDTH))
        .max_h(px(MAX_HEIGHT))
        .overflow_hidden()
        .bg(cx.theme().colors().editor_background)
        .border_1()
        .border_color(tokens.popup_border)
        .rounded(px(8.))
        .shadow_lg()
}

/// The changed lines in an editor's look: the file's syntax colours, the line numbers in the
/// gutter, what the action writes on IntelliJ's diff colours.
fn change_view(change: Change, language: Option<Arc<Language>>, cx: &App) -> AnyElement {
    let tokens = tokens(cx);
    let settings = ThemeSettings::get_global(cx);
    let colors = cx.theme().colors();
    let syntax = language
        .map(|language| {
            language::highlight_ranges_from_text(&change.text, &language, &cx.theme().syntax())
        })
        .unwrap_or_default();
    let marks = change.marks.iter().map(|(range, added)| {
        let background = match added {
            true => tokens.diff_inserted,
            false => tokens.diff_modified,
        };
        (
            range.clone(),
            HighlightStyle {
                background_color: Some(background),
                ..HighlightStyle::default()
            },
        )
    });
    let highlights = gpui::combine_highlights(syntax, marks).collect::<Vec<_>>();
    let mut lines = Vec::new();
    let mut offset = 0;
    for (ix, line) in change.text.split('\n').enumerate() {
        let range = offset..offset + line.len();
        let line_highlights = highlights
            .iter()
            .filter_map(|(mark, style)| {
                let start = mark.start.max(range.start);
                let end = mark.end.min(range.end);
                (start < end).then(|| (start - range.start..end - range.start, *style))
            })
            .collect::<Vec<_>>();
        lines.push(
            div()
                .flex()
                .gap(px(GUTTER_GAP))
                .child(
                    div()
                        .flex_none()
                        .min_w(px(16.))
                        .text_color(colors.editor_line_number)
                        .child((change.first_line + ix as u32).to_string()),
                )
                .child(
                    StyledText::new(SharedString::from(line.to_owned()))
                        .with_highlights(line_highlights),
                ),
        );
        offset = range.end + 1;
    }
    surface(cx)
        .px(px(PADDING_X))
        .py(px(MESSAGE_PADDING_Y))
        .font(settings.buffer_font.clone())
        .text_size(settings.buffer_font_size(cx))
        .font_weight(FontWeight::NORMAL)
        .text_color(colors.editor_foreground)
        .whitespace_nowrap()
        .children(lines)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{Change, apply};

    #[test]
    fn an_edit_s_preview_is_the_lines_it_changes_after_it() {
        let text = "fn main() {\n    let s = \"three\";\n}\n";
        let start = text.find('"').unwrap();
        let end = text.rfind('"').unwrap() + 1;
        let change = apply(text, vec![(start..end, "r\"three\"".to_owned())]).unwrap();
        assert_eq!(
            change,
            Change {
                first_line: 2,
                text: "    let s = r\"three\";".to_owned(),
                marks: vec![(12..20, false)],
            }
        );
        let inserted = apply(text, vec![(start..start, "x".to_owned())]).unwrap();
        assert_eq!(inserted.marks, vec![(12..13, true)]);
        assert_eq!(apply(text, Vec::new()), None);
    }
}
