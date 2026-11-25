//! Go to Line:Column (⌘L), IntelliJ's `EditorGotoLineNumberDialog`: the caret's line and column in
//! a field, selected, OK moving the caret there; and File Path (⌥⌘F12), its `ShowFilePathAction`:
//! the active file and its folders up to the disk's root, one chosen opening in the system.

use editor::{Editor, SelectionEffects, scroll::Autoscroll};
use gpui::{
    App, AppContext as _, Focusable as _, InteractiveElement as _, ParentElement as _, Role,
    StatefulInteractiveElement as _, Styled as _, Window, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use language::Point;
use spork_workbench::controls::{dialog_title, form_label};
use workspace::Workspace;

use crate::list_popup::{Row, RowIcon, show_list_popup};
use crate::navigate;

const GO_TO_LINE_TITLE: &str = "Go to Line:Column";
const LINE_COLUMN_LABEL: &str = "[Line] [:column]:";

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &navigate::GoToLine, window, cx| {
        if let Some(editor) = workspace.active_item_as::<Editor>(cx) {
            go_to_line(editor, window, cx);
        }
    });
    workspace.register_action(|workspace, _: &navigate::ShowFilePath, window, cx| {
        let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
            return;
        };
        let Some(path) = crate::editor_path(editor.read(cx), cx)
            .and_then(|path| workspace.project().read(cx).absolute_path(&path, cx))
        else {
            return;
        };
        let rows = path
            .ancestors()
            .map(|ancestor| {
                let name = match ancestor.file_name() {
                    Some(name) => name.to_string_lossy().into_owned(),
                    None => ancestor.to_string_lossy().into_owned(),
                };
                let icon = match ancestor == path {
                    true => spork_workbench::file_icon(&name),
                    false => "nodes/folder",
                };
                let ancestor = ancestor.to_path_buf();
                Row::item(name, move |_, cx| cx.open_with_system(&ancestor))
                    .icon(Some(RowIcon::Colored(icon)))
            })
            .collect();
        // RevealFileAction's name on macOS
        let title = match cfg!(target_os = "macos") {
            true => "Reveal in Finder",
            false => "Open in File Manager",
        };
        let workspace = workspace.weak_handle();
        // the popup is the workspace's modal: shown once this update of it is over
        window.defer(cx, move |window, cx| {
            show_list_popup(&workspace, &editor, title, rows, false, window, cx)
        });
    });
}

/// Where `text` asks the caret to go, 0-based: IntelliJ's `line[:column]` (either may be left out,
/// `,` or a space also separating them, the line staying the caret's), or a line counted from the
/// caret's (`+3`, `-2`, at its start); none for anything else.
fn coordinates(text: &str, caret_row: u32, last_row: u32) -> Option<Point> {
    let text = text.trim();
    if let Some(sign) = text.chars().next().filter(|c| *c == '+' || *c == '-') {
        let rest = &text[1..];
        if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let lines: i64 = rest.parse().ok()?;
        let row = match sign {
            '+' => i64::from(caret_row) + lines,
            _ => i64::from(caret_row) - lines,
        };
        return Some(Point::new(row.clamp(0, i64::from(last_row)) as u32, 0));
    }
    let digits = |text: &str| -> (Option<Option<u32>>, usize) {
        let end = text
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len());
        match end {
            0 => (Some(None), 0),
            _ => (text[..end].parse().ok().map(Some), end),
        }
    };
    let (line, end) = digits(text);
    let rest = text[end..].trim_start();
    let rest = rest.strip_prefix([',', ':']).unwrap_or(rest).trim_start();
    let (column, end) = digits(rest);
    if !rest[end..].is_empty() {
        return None;
    }
    let line = line?.unwrap_or(caret_row + 1);
    let column = column?.unwrap_or(0);
    (line > 0).then(|| Point::new(line - 1, column.saturating_sub(1)))
}

/// IntelliJ's Go to Line:Column dialog over `editor`.
fn go_to_line(editor: gpui::Entity<Editor>, window: &mut Window, cx: &mut App) {
    let (caret, last_row) = editor.update(cx, |editor, cx| {
        let snapshot = editor.buffer().read(cx).snapshot(cx);
        let head = editor
            .selections
            .newest::<Point>(&editor.display_snapshot(cx))
            .head();
        let column = snapshot
            .text_for_range(Point::new(head.row, 0)..head)
            .flat_map(str::chars)
            .count();
        ((head.row, column), snapshot.max_point().row)
    });
    let text = format!("{}:{}", caret.0 + 1, caret.1 + 1);
    let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
    let go = {
        let (input, editor) = (input.clone(), editor.downgrade());
        std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
            let Some(point) = coordinates(&input.read(cx).value(), caret.0, last_row) else {
                return;
            };
            window.close_dialog(cx);
            editor
                .update(cx, |editor, cx| {
                    let snapshot = editor.buffer().read(cx).snapshot(cx);
                    let row = point.row.min(snapshot.max_point().row);
                    // the column counts characters, as IntelliJ's logical column does
                    let line_start = Point::new(row, 0);
                    let line_end =
                        snapshot.clip_point(Point::new(row, u32::MAX), editor::Bias::Left);
                    let offset: u32 = snapshot
                        .text_for_range(line_start..line_end)
                        .flat_map(str::chars)
                        .take(point.column as usize)
                        .map(|c| c.len_utf8() as u32)
                        .sum();
                    let target = Point::new(row, offset);
                    editor.change_selections(
                        SelectionEffects::scroll(Autoscroll::center()),
                        window,
                        cx,
                        |selections| selections.select_ranges([target..target]),
                    );
                    window.focus(&editor.focus_handle(cx), cx);
                })
                .ok();
        })
    };
    window
        .subscribe(&input, cx, {
            let go = go.clone();
            move |_, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    go(window, cx)
                }
            }
        })
        .detach();
    let field = input.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let go = go.clone();
        dialog
            .title(dialog_title(GO_TO_LINE_TITLE))
            .on_ok(|_, _, _| false)
            .w(px(340.))
            .close_button(false)
            .child(
                v_flex()
                    .id("go-to-line-dialog")
                    .role(Role::Group)
                    .aria_label(GO_TO_LINE_TITLE)
                    .gap(px(16.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(form_label(LINE_COLUMN_LABEL))
                            .child(
                                gpui::div().flex_1().child(
                                    spork_workbench::TextField::new(&field)
                                        .aria_label(LINE_COLUMN_LABEL),
                                ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("dialog-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                spork_workbench::Button::new("dialog-ok", "OK")
                                    .primary(true)
                                    .on_click(move |_, window, cx| go(window, cx)),
                            ),
                    ),
            )
    });
    input.update(cx, |input, cx| {
        input.focus(window, cx);
        input.select_all(window, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::coordinates;
    use language::Point;

    #[test]
    fn reads_line_and_column_as_intellij_does() {
        assert_eq!(coordinates("6:14", 0, 99), Some(Point::new(5, 13)));
        assert_eq!(coordinates(" 6 , 14 ", 0, 99), Some(Point::new(5, 13)));
        assert_eq!(coordinates("6 14", 0, 99), Some(Point::new(5, 13)));
        assert_eq!(coordinates("6", 0, 99), Some(Point::new(5, 0)));
        assert_eq!(coordinates("6:", 0, 99), Some(Point::new(5, 0)));
        assert_eq!(coordinates(":3", 9, 99), Some(Point::new(9, 2)));
        assert_eq!(coordinates("", 9, 99), Some(Point::new(9, 0)));
        assert_eq!(coordinates("0", 9, 99), None);
        assert_eq!(coordinates("6:x", 0, 99), None);
        assert_eq!(coordinates("+3", 9, 99), Some(Point::new(12, 0)));
        assert_eq!(coordinates("-20", 9, 99), Some(Point::new(0, 0)));
        assert_eq!(coordinates("+200", 9, 99), Some(Point::new(99, 0)));
    }
}
