//! The Code menu over the language servers: Reformat Code (⌥⌘L) and IntelliJ's Reformat File
//! dialog (⌥⇧⌘L), Optimize Imports (⌃⌥O, the server's organize imports), Generate… (⌘N, the
//! server's generating code actions), Surround With… (⌥⌘T, its wrapping ones) and Move Statement
//! Up/Down (⇧⌘↑/↓, over the file's syntax tree).

use std::collections::BTreeMap;

use collections::HashSet;
use editor::{Editor, SelectionEffects};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Div, Entity, InteractiveElement as _, IntoElement as _,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, TaskExt as _, Window, px,
};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use language::Point;
use project::CodeAction;
use project::lsp_store::{FormatTrigger, LspFormatTarget};
use spork_workbench::controls::{
    dialog_title, disabled_check, disabled_radio, labeled_check, labeled_radio,
};
use util::ResultExt as _;
use workspace::Workspace;

use crate::{
    Able, Row, Target, action_kind, code, kind_covers, request_code_actions, show_list_popup,
};

pub(crate) fn register(div: Div, able: &Able, cx: &mut Context<Workspace>) -> Div {
    div.when(able.editor && able.format, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::ReformatCode, window, cx| {
                reformat_code(workspace, window, cx)
            }),
        )
        .on_action(
            cx.listener(|workspace, _: &code::ReformatFile, window, cx| {
                reformat_file_dialog(workspace, window, cx)
            }),
        )
    })
    .when(able.editor && able.organize_imports, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::OptimizeImports, window, cx| {
                if let Some(target) = Target::of(workspace, cx) {
                    run(
                        target,
                        Steps {
                            optimize_imports: true,
                            ..Steps::default()
                        },
                        window,
                        cx,
                    );
                }
            }),
        )
    })
    .when(able.editor && (able.code_actions || able.generate), |div| {
        div.on_action(cx.listener(|workspace, _: &code::Generate, window, cx| {
            if let Some(target) = Target::of(workspace, cx)
                && let Some(rows) = crate::language_generate_rows(&target, window, cx)
            {
                let workspace = workspace.weak_handle();
                window
                    .spawn(cx, async move |cx| {
                        let rows = rows.await;
                        cx.update(|window, cx| {
                            show_list_popup(
                                &workspace,
                                &target.editor,
                                "Generate",
                                rows,
                                false,
                                window,
                                cx,
                            )
                        })
                    })
                    .detach_and_log_err(cx);
                return;
            }
            chooser(
                workspace,
                "Generate",
                |action| {
                    let kind = action_kind(action);
                    kind.is_empty()
                        || kind_covers("source.generate", &kind)
                        || action.lsp_action.title().starts_with("Generate")
                },
                window,
                cx,
            )
        }))
    })
    .when(able.editor && able.code_actions, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::SurroundWith, window, cx| {
                chooser(
                    workspace,
                    "Surround With",
                    |action| {
                        let title = action.lsp_action.title().to_lowercase();
                        title.starts_with("surround") || title.starts_with("wrap")
                    },
                    window,
                    cx,
                )
            }),
        )
    })
    .when(able.editor, |div| {
        div.on_action(
            cx.listener(|workspace, _: &code::MoveStatementDown, window, cx| {
                move_statement(workspace, true, window, cx)
            }),
        )
        .on_action(
            cx.listener(|workspace, _: &code::MoveStatementUp, window, cx| {
                move_statement(workspace, false, window, cx)
            }),
        )
    })
}

/// The Shell Script plugin's Generate items (`ShGenerateGroup`): For Loop, While Loop and Until
/// Loop, its hidden live templates (`shell_for`, `shell_while`, `shell_until`) as snippets, on a
/// line of their own.
pub(crate) fn shell_rows(
    target: &crate::Target,
    _: &mut Window,
    cx: &mut App,
) -> Option<gpui::Task<Vec<crate::Row>>> {
    let language = target.buffer.read(cx).language()?;
    if language.name().as_ref() != "Shell Script" {
        return None;
    }
    let row = |label: &'static str, body: &'static str| {
        let editor = target.editor.clone();
        crate::Row::item(label, move |window, cx| {
            editor.update(cx, |editor, cx| {
                // IntelliJ's `moveAtNewLineIfNeeded`: a line with text gets one below it
                let caret = editor
                    .selections
                    .newest::<Point>(&editor.display_snapshot(cx))
                    .head();
                let snapshot = editor.buffer().read(cx).snapshot(cx);
                let end = snapshot.clip_point(Point::new(caret.row, u32::MAX), editor::Bias::Left);
                let blank = snapshot
                    .text_for_range(Point::new(caret.row, 0)..end)
                    .all(|chunk| chunk.trim().is_empty());
                if !blank {
                    editor.newline_below(&editor::actions::NewlineBelow, window, cx);
                }
                editor.insert_snippet_at_selections(
                    &editor::actions::InsertSnippet {
                        language: None,
                        name: None,
                        snippet: Some(body.to_owned()),
                    },
                    window,
                    cx,
                );
            });
        })
    };
    Some(gpui::Task::ready(vec![
        row(
            "For Loop",
            "for (( ${1:i} = ${2:0}; ${1:i} < ${3:n}; ${1:i}++ )); do\n    $0\ndone",
        ),
        row("While Loop", "while [ $1 ]; do\n    $0\ndone"),
        row("Until Loop", "until [ $1 ]; do\n    $0\ndone"),
    ]))
}

/// A popup of the server's code actions at the caret that `keep` keeps, titled `title`:
/// IntelliJ's Generate and Surround With popups; "Nothing here" without any.
fn chooser(
    workspace: &Workspace,
    title: &'static str,
    keep: impl Fn(&CodeAction) -> bool + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let request = request_code_actions(&target, "", keep, cx);
    let workspace = workspace.weak_handle();
    window
        .spawn(cx, async move |cx| {
            let actions = request.await;
            cx.update(|window, cx| {
                let rows = actions
                    .into_iter()
                    .map(|action| {
                        let target = target.clone();
                        Row::item(action.lsp_action.title().to_owned(), move |window, cx| {
                            crate::apply_code_action(&target, action.clone(), window, cx)
                        })
                    })
                    .collect();
                show_list_popup(&workspace, &target.editor, title, rows, false, window, cx);
            })
        })
        .detach_and_log_err(cx);
}

/// What Reformat File does, in IntelliJ's order (`ReformatCodeProcessor` after the optimizer).
#[derive(Default, Clone, Copy)]
struct Steps {
    optimize_imports: bool,
    cleanup: bool,
    /// What to reformat: the file (`Some(None)`), some of its ranges, or nothing.
    reformat: Option<Option<Scope>>,
}

#[derive(Clone, Copy, PartialEq)]
enum Scope {
    Uncommitted,
    Selection,
}

/// ⌥⌘L: the selection when there is one and the server formats ranges, the file otherwise.
fn reformat_code(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let able = Able::of(workspace, cx);
    let selection = able.range_format && target.range.start != target.range.end;
    run(
        target,
        Steps {
            reformat: Some(selection.then_some(Scope::Selection)),
            ..Steps::default()
        },
        window,
        cx,
    );
}

/// Runs `steps` on the target's file, then saves it.
fn run(target: Target, steps: Steps, window: &mut Window, cx: &mut App) {
    let project = target.project.clone();
    let buffer = target.buffer.clone();
    let ranges = steps
        .reformat
        .flatten()
        .map(|scope| format_ranges(&target, scope, cx));
    window
        .spawn(cx, async move |cx| {
            let buffers = HashSet::from_iter([buffer.clone()]);
            for (asked, kind) in [
                (steps.optimize_imports, "source.organizeImports"),
                (steps.cleanup, "source.fixAll"),
            ] {
                if asked {
                    project
                        .update(cx, |project, cx| {
                            project.apply_code_action_kind(
                                buffers.clone(),
                                lsp::CodeActionKind::from(kind.to_owned()),
                                true,
                                cx,
                            )
                        })
                        .await
                        .log_err();
                }
            }
            if steps.reformat.is_some() {
                let target = match ranges {
                    Some(ranges) => {
                        let id = cx.update(|_, cx| buffer.read(cx).remote_id())?;
                        LspFormatTarget::Ranges(BTreeMap::from_iter([(id, ranges)]))
                    }
                    None => LspFormatTarget::Buffers,
                };
                project
                    .update(cx, |project, cx| {
                        project.format(buffers.clone(), target, true, FormatTrigger::Manual, cx)
                    })
                    .await?;
            }
            project
                .update(cx, |project, cx| project.save_buffers(buffers, cx))
                .await
        })
        .detach_and_log_err(cx);
}

/// The ranges Reformat File's scope names: the selection, or the lines changed since the last
/// commit (the editor's change markers).
fn format_ranges(target: &Target, scope: Scope, cx: &App) -> Vec<std::ops::Range<text::Anchor>> {
    match scope {
        Scope::Selection => vec![target.range.clone()],
        Scope::Uncommitted => {
            let snapshot = target.editor.read(cx).buffer().read(cx).snapshot(cx);
            snapshot
                .diff_hunks()
                .map(|hunk| hunk.buffer_range)
                .collect()
        }
    }
}

/// Whether the file has lines changed since the last commit.
fn has_changes(editor: &Entity<Editor>, cx: &App) -> bool {
    let snapshot = editor.read(cx).buffer().read(cx).snapshot(cx);
    snapshot.diff_hunks().next().is_some()
}

/// IntelliJ's Reformat File dialog (`LayoutCodeDialog`): the scope (Only changes uncommitted to
/// VCS, Selected text, Whole file) and options (Optimize imports, Rearrange code, Code cleanup,
/// Do not keep line breaks), each greyed where the server can't.
fn reformat_file_dialog(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(target) = Target::of(workspace, cx) else {
        return;
    };
    let able = Able::of(workspace, cx);
    let name = target
        .buffer
        .read(cx)
        .file()
        .map(|file| file.file_name(cx).to_owned())
        .unwrap_or_default();
    let can_select = able.range_format && target.range.start != target.range.end;
    let can_uncommitted = able.range_format && has_changes(&target.editor, cx);
    // the dialog's choices: the scope, Optimize imports, Code cleanup
    let state = cx.new(|_| (None::<Scope>, false, false));
    let run_steps = {
        let (state, target) = (state.clone(), target);
        move |window: &mut Window, cx: &mut App| {
            let (scope, optimize_imports, cleanup) = *state.read(cx);
            window.close_dialog(cx);
            run(
                target.clone(),
                Steps {
                    optimize_imports,
                    cleanup,
                    reformat: Some(scope),
                },
                window,
                cx,
            );
        }
    };
    let run_steps = std::rc::Rc::new(run_steps);
    window.open_dialog(cx, move |dialog, _, cx| {
        let (scope, optimize_imports, cleanup) = *state.read(cx);
        let set = |change: fn(&mut (Option<Scope>, bool, bool))| {
            let state = state.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                state.update(cx, |state, _| change(state));
                window.refresh();
            }
        };
        let radio = |id: &'static str, label: &'static str, on: bool, enabled: bool, cx: &App| {
            match enabled {
                true => labeled_radio(id, label, on, cx).into_any_element(),
                false => disabled_radio(id, label, on, cx).into_any_element(),
            }
        };
        let run_steps = run_steps.clone();
        dialog
            .title(dialog_title(format!("Reformat File: {name}")))
            .on_ok(|_, _, _| false)
            .w(px(440.))
            .close_button(false)
            .child(
                v_flex()
                    .id("reformat-file-dialog")
                    .role(gpui::Role::Group)
                    .aria_label("Reformat File")
                    .gap(px(8.))
                    .child(section("Scope", cx))
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .pl(px(4.))
                            .child(match can_uncommitted {
                                true => labeled_radio(
                                    "reformat-uncommitted",
                                    "Only changes uncommitted to VCS",
                                    scope == Some(Scope::Uncommitted),
                                    cx,
                                )
                                .on_click(set(|state| state.0 = Some(Scope::Uncommitted)))
                                .into_any_element(),
                                false => radio(
                                    "reformat-uncommitted",
                                    "Only changes uncommitted to VCS",
                                    false,
                                    false,
                                    cx,
                                ),
                            })
                            .child(match can_select {
                                true => labeled_radio(
                                    "reformat-selection",
                                    "Selected text",
                                    scope == Some(Scope::Selection),
                                    cx,
                                )
                                .on_click(set(|state| state.0 = Some(Scope::Selection)))
                                .into_any_element(),
                                false => {
                                    radio("reformat-selection", "Selected text", false, false, cx)
                                }
                            })
                            .child(
                                labeled_radio("reformat-whole", "Whole file", scope.is_none(), cx)
                                    .on_click(set(|state| state.0 = None)),
                            ),
                    )
                    .child(section("Options", cx))
                    .child(
                        h_flex()
                            .pl(px(4.))
                            .gap(px(24.))
                            .items_start()
                            .child(
                                v_flex()
                                    .gap(px(8.))
                                    .child(match able.organize_imports {
                                        true => labeled_check(
                                            "reformat-optimize-imports",
                                            "Optimize imports",
                                            optimize_imports,
                                            cx,
                                        )
                                        .on_click(set(|state| state.1 = !state.1))
                                        .into_any_element(),
                                        false => disabled_check(
                                            "reformat-optimize-imports",
                                            "Optimize imports",
                                            false,
                                            cx,
                                        )
                                        .into_any_element(),
                                    })
                                    .child(match able.fix_all {
                                        true => labeled_check(
                                            "reformat-cleanup",
                                            "Code cleanup",
                                            cleanup,
                                            cx,
                                        )
                                        .on_click(set(|state| state.2 = !state.2))
                                        .into_any_element(),
                                        false => disabled_check(
                                            "reformat-cleanup",
                                            "Code cleanup",
                                            false,
                                            cx,
                                        )
                                        .into_any_element(),
                                    }),
                            )
                            .child(
                                v_flex()
                                    .gap(px(8.))
                                    // no server rearranges code or reflows its line breaks
                                    .child(disabled_check(
                                        "reformat-rearrange",
                                        "Rearrange code",
                                        false,
                                        cx,
                                    ))
                                    .child(disabled_check(
                                        "reformat-line-breaks",
                                        "Do not keep line breaks",
                                        false,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .pt(px(12.))
                            .gap(px(8.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("dialog-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                spork_workbench::Button::new("dialog-ok", "Run")
                                    .primary(true)
                                    .on_click(move |_, window, cx| run_steps(window, cx)),
                            ),
                    ),
            )
    });
}

/// A titled group of a dialog (IntelliJ's `TitledSeparator`): its title, then a line to the
/// right.
fn section(title: &'static str, cx: &App) -> gpui::AnyElement {
    let tokens = spork_workbench::Tokens::get(cx);
    h_flex()
        .gap(px(8.))
        .child(title)
        .child(div_line(tokens.separator))
        .into_any_element()
}

fn div_line(color: gpui::Hsla) -> Div {
    gpui::div().flex_1().h(px(1.)).bg(color)
}

/// Move Statement Down/Up: the statement on the caret's line (the outermost node starting there
/// whose parent starts above) changes places with its next or previous sibling, the lines
/// between them staying between them.
fn move_statement(
    workspace: &mut Workspace,
    down: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
        return;
    };
    editor.update(cx, |editor, cx| {
        let Some(buffer) = editor.buffer().read(cx).as_singleton() else {
            return;
        };
        let snapshot = buffer.read(cx).snapshot();
        let caret = editor
            .selections
            .newest::<Point>(&editor.display_snapshot(cx))
            .head();
        let Some((first, second, caret_row)) = statements(&snapshot, caret.row, down) else {
            return;
        };
        let line_start = |row: u32| Point::new(row, 0);
        let line_end = |row: u32| Point::new(row, snapshot.line_len(row));
        let text = |rows: &std::ops::Range<u32>| -> String {
            snapshot
                .text_for_range(line_start(rows.start)..line_end(rows.end - 1))
                .collect()
        };
        let between: String = snapshot
            .text_for_range(line_end(first.end - 1)..line_start(second.start))
            .collect();
        let replaced = format!("{}{between}{}", text(&second), text(&first));
        let range = line_start(first.start)..line_end(second.end - 1);
        // where the caret's statement starts once moved
        let moved_start = match down {
            true => first.start + (second.end - second.start) + (second.start - first.end),
            false => first.start,
        };
        let caret_target = Point::new(moved_start + caret_row, caret.column);
        editor.transact(window, cx, |editor, window, cx| {
            editor.edit([(range, replaced)], cx);
            editor.change_selections(SelectionEffects::default(), window, cx, |selections| {
                selections.select_ranges([caret_target..caret_target])
            });
        });
    });
}

/// The rows of the two statements that change places, the earlier first, and the caret's row in
/// the statement it is in; `None` when the statement has no sibling that way.
fn statements(
    snapshot: &language::BufferSnapshot,
    row: u32,
    down: bool,
) -> Option<(std::ops::Range<u32>, std::ops::Range<u32>, u32)> {
    let indent = snapshot.indent_size_for_line(row).len;
    let at = snapshot.point_to_offset(Point::new(row, indent.min(snapshot.line_len(row))));
    // the line's first character: an empty range there finds the block around the line
    let mut node = snapshot.syntax_ancestor(at..at + 1)?;
    while let Some(parent) = node.parent() {
        if parent.parent().is_none() || parent.start_position().row != node.start_position().row {
            break;
        }
        node = parent;
    }
    let mut sibling = match down {
        true => node.next_named_sibling(),
        false => node.prev_named_sibling(),
    }?;
    // a comment goes along with the statement it sits by
    while sibling.kind().contains("comment") {
        sibling = match down {
            true => sibling.next_named_sibling(),
            false => sibling.prev_named_sibling(),
        }?;
    }
    let rows = |node: &language::Node| {
        node.start_position().row as u32..node.end_position().row as u32 + 1
    };
    let (statement, other) = (rows(&node), rows(&sibling));
    if statement.start < other.end && other.start < statement.end {
        return None;
    }
    let caret_row = row - statement.start;
    Some(match down {
        true => (statement, other, caret_row),
        false => (other, statement, caret_row),
    })
}

#[cfg(test)]
mod tests {
    use super::statements;
    use gpui::AppContext as _;
    use language::Buffer;

    #[gpui::test]
    fn a_statement_changes_places_with_its_sibling(cx: &mut gpui::TestAppContext) {
        let text = "fn main() {\n    let a = 1;\n\n    let b = 2;\n    // two\n}\n";
        let buffer = cx.new(|cx| Buffer::local(text, cx).with_language(language::rust_lang(), cx));
        cx.run_until_parked();
        let snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot());
        assert_eq!(statements(&snapshot, 1, true), Some((1..2, 3..4, 0)));
        assert_eq!(statements(&snapshot, 3, false), Some((1..2, 3..4, 0)));
        // nothing before the first statement, and a comment is no statement to swap with
        assert_eq!(statements(&snapshot, 1, false), None);
        assert_eq!(statements(&snapshot, 3, true), None);
    }
}
