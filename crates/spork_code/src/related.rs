//! Go to Test (⇧⌘T, IntelliJ's `GotoTestOrCodeHandler`) and Related Symbol… (⌃⌘↑,
//! `GotoRelatedSymbolAction` with its `GotoTestRelatedProvider`) over a file name convention, as
//! IntelliJ's `TestFinder`s name tests: `GreeterTest`, `TestGreeter`, `greeter_test`,
//! `test_greeter`, `greeter.test`, `greeter.spec`, or a file of a `tests` folder named as the file
//! it tests. One found is opened, several are chosen from a popup, none says so over the caret.

use gpui::prelude::FluentBuilder as _;
use gpui::{Context, Div, InteractiveElement as _, SharedString, TaskExt as _, Window};
use project::ProjectPath;
use util::ResultExt as _;
use workspace::Workspace;

use crate::{Able, Row, code, show_hint, show_list_popup};
use spork_navigate::list_popup::RowIcon;

pub(crate) fn register(div: Div, able: &Able, cx: &mut Context<Workspace>) -> Div {
    div.when(able.editor, |div| {
        div.on_action(cx.listener(|workspace, _: &code::GotoTest, window, cx| {
            go_to(workspace, false, window, cx)
        }))
        .on_action(cx.listener(|workspace, _: &code::GotoRelated, window, cx| {
            go_to(workspace, true, window, cx)
        }))
    })
}

/// The file a test file tests (its name without the test's), or none for a file that isn't one.
fn tested_name(stem: &str, in_tests_folder: bool) -> Option<&str> {
    let lower = stem.to_ascii_lowercase();
    for affix in [
        "_test", "_tests", "-test", ".test", "_spec", "-spec", ".spec",
    ] {
        if lower.len() > affix.len() && lower.ends_with(affix) {
            return stem.get(..stem.len() - affix.len());
        }
    }
    // CamelCase: GreeterTest, GreeterTests, GreeterSpec, TestGreeter
    for affix in ["Test", "Tests", "Spec"] {
        if let Some(name) = stem.strip_suffix(affix).filter(|name| !name.is_empty()) {
            return Some(name);
        }
    }
    if let Some(name) = stem
        .strip_prefix("Test")
        .filter(|name| name.starts_with(|c: char| c.is_ascii_uppercase()))
    {
        return Some(name);
    }
    if lower.len() > 5 && lower.starts_with("test_") {
        return stem.get(5..);
    }
    in_tests_folder.then_some(stem)
}

/// A file's name without its extension, its extension, and whether it's in a tests folder.
fn parts(path: &ProjectPath) -> Option<(&str, &str, bool)> {
    let name = path.path.file_name()?;
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, ""));
    let in_tests = path
        .path
        .components()
        .any(|folder| matches!(folder, "tests" | "test" | "__tests__"));
    Some((stem, extension, in_tests))
}

/// The tests of `path`, or the files it tests when it's a test, by their names: true with the
/// files it tests.
fn related_files(
    workspace: &Workspace,
    path: &ProjectPath,
    cx: &gpui::App,
) -> (bool, Vec<ProjectPath>) {
    let Some((stem, extension, in_tests)) = parts(path) else {
        return (false, Vec::new());
    };
    let tested = tested_name(stem, in_tests);
    let project = workspace.project().read(cx);
    let mut found: Vec<ProjectPath> = project
        .visible_worktrees(cx)
        .flat_map(|worktree| {
            let worktree = worktree.read(cx);
            let worktree_id = worktree.id();
            worktree
                .files(false, 0)
                .map(move |entry| ProjectPath {
                    worktree_id,
                    path: entry.path.clone(),
                })
                .collect::<Vec<_>>()
        })
        .filter(|other| other != path)
        .filter(|other| {
            let Some((other_stem, other_extension, other_in_tests)) = parts(other) else {
                return false;
            };
            if other_extension != extension {
                return false;
            }
            let other_tested = tested_name(other_stem, other_in_tests);
            match tested {
                Some(tested) => other_tested.is_none() && other_stem.eq_ignore_ascii_case(tested),
                None => other_tested.is_some_and(|name| name.eq_ignore_ascii_case(stem)),
            }
        })
        .collect();
    found.sort_by(|a, b| a.path.cmp(&b.path));
    (tested.is_some(), found)
}

/// Go to Test, or Related Symbol… with `related`.
fn go_to(
    workspace: &mut Workspace,
    related: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(editor) = workspace.active_item_as::<editor::Editor>(cx) else {
        return;
    };
    let Some(path) = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx))
    else {
        return;
    };
    let (is_test, found) = related_files(workspace, &path, cx);
    let handle = workspace.weak_handle();
    match found.as_slice() {
        [] => show_hint(
            &editor.downgrade(),
            match related {
                true => "No related symbols",
                false => "No test subjects found",
            },
            true,
            window,
            cx,
        ),
        [only] => {
            workspace
                .open_path(only.clone(), None, true, window, cx)
                .detach_and_log_err(cx);
        }
        _ => {
            let name = parts(&path).map(|(stem, _, _)| stem).unwrap_or_default();
            let title: SharedString = match (related, is_test) {
                (true, _) => "Choose Target".into(),
                (false, false) => format!("Choose Test for {name} ({} found)", found.len()).into(),
                (false, true) => {
                    format!("Choose Test Subject for {name} ({} found)", found.len()).into()
                }
            };
            let mut rows = Vec::new();
            if related {
                // GotoTestRelatedProvider's groups
                rows.push(Row::Separator(Some(
                    match is_test {
                        true => "Tested classes",
                        false => "Tests",
                    }
                    .into(),
                )));
            }
            rows.extend(found.into_iter().map(|file| {
                let name = file.path.file_name().unwrap_or_default().to_owned();
                let folder = file
                    .path
                    .parent()
                    .map(|folder| folder.as_unix_str().to_owned())
                    .unwrap_or_default();
                let handle = handle.clone();
                let icon = spork_workbench::file_icon(&name);
                Row::item(name, move |window, cx| {
                    handle
                        .update(cx, |workspace, cx| {
                            workspace
                                .open_path(file.clone(), None, true, window, cx)
                                .detach_and_log_err(cx)
                        })
                        .log_err();
                })
                .hint(Some(folder.into()))
                .icon(Some(RowIcon::Colored(icon)))
            }));
            // the popup is the workspace's modal: shown once this update of it is over
            window.defer(cx, move |window, cx| {
                show_list_popup(&handle, &editor, title, rows, false, window, cx)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tested_name;

    #[test]
    fn tells_tests_by_their_names() {
        assert_eq!(tested_name("GreeterTest", false), Some("Greeter"));
        assert_eq!(tested_name("GreeterTests", false), Some("Greeter"));
        assert_eq!(tested_name("TestGreeter", false), Some("Greeter"));
        assert_eq!(tested_name("greeting_test", false), Some("greeting"));
        assert_eq!(tested_name("test_greeting", false), Some("greeting"));
        assert_eq!(tested_name("greeting.spec", false), Some("greeting"));
        assert_eq!(tested_name("greeting", true), Some("greeting"));
        assert_eq!(tested_name("greeting", false), None);
        assert_eq!(tested_name("latest", false), None);
        assert_eq!(tested_name("Testing", false), None);
    }
}
