//! Select In… (⌥F1), IntelliJ's `SelectInAction`: the places the active file can be shown in, in
//! IntelliJ's order (`StandardTargetWeights`), numbered, greyed where they can't show it; choosing
//! one shows the file there. The targets IntelliJ has and Spork IDE has no engine for (Coverage,
//! Web Browser, Project Structure) aren't listed.

use editor::Editor;
use gpui::{Action as _, Context, Window};
use workspace::Workspace;

use crate::list_popup::{Row, RowIcon, show_list_popup};
use crate::navigate;

pub(crate) fn register(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &navigate::SelectIn, window, cx| {
        let Some(editor) = workspace.active_item_as::<Editor>(cx) else {
            return;
        };
        let Some(path) = crate::editor_path(editor.read(cx), cx) else {
            return;
        };
        let project = workspace.project().clone();
        let Some(abs_path) = project.read(cx).absolute_path(&path, cx) else {
            return;
        };
        let entry = project
            .read(cx)
            .entry_for_path(&path, cx)
            .map(|entry| entry.id);
        let has_outline = editor
            .read(cx)
            .buffer()
            .read(cx)
            .as_singleton()
            .is_some_and(|buffer| buffer.read(cx).language().is_some());
        let handle = workspace.weak_handle();
        let on_workspace =
            |run: fn(&mut Workspace, &Target, &mut Window, &mut Context<Workspace>),
             target: Target| {
                let handle = handle.clone();
                move |window: &mut Window, cx: &mut gpui::App| {
                    handle
                        .update(cx, |workspace, cx| run(workspace, &target, window, cx))
                        .ok();
                }
            };
        let target = Target {
            path,
            abs_path: abs_path.clone(),
            entry,
        };
        let tool = |icon| Some(RowIcon::Tinted(icon));
        let rows = vec![
            Row::item(
                "Project View",
                on_workspace(
                    |workspace, target, _, cx| {
                        if let Some(entry) = target.entry {
                            workspace.project().update(cx, |_, cx| {
                                cx.emit(project::Event::RevealInProjectPanel(entry))
                            });
                        }
                    },
                    target.clone(),
                ),
            )
            .icon(tool("toolwindows/project"))
            .enabled(entry.is_some()),
            Row::item(
                "Bookmarks",
                on_workspace(
                    |workspace, target, window, cx| {
                        crate::bookmarks::select_in_bookmarks(
                            workspace,
                            target.path.clone(),
                            window,
                            cx,
                        )
                    },
                    target.clone(),
                ),
            )
            .icon(tool("toolwindows/bookmarks"))
            .enabled(crate::bookmarks::has_bookmarks(workspace, &abs_path, cx)),
            Row::item(
                "File Structure",
                on_workspace(
                    |workspace, _, window, cx| {
                        crate::show_tool_window(workspace, "Outline Panel", window, cx);
                    },
                    target.clone(),
                ),
            )
            .icon(tool("toolwindows/structure"))
            .enabled(has_outline),
            Row::item("Navigation Bar", {
                let handle = handle.clone();
                move |window, cx| {
                    if let Some(workspace) = handle.upgrade() {
                        crate::navigation_bar::jump(&workspace, false, window, cx)
                    }
                }
            }),
            Row::item(
                "Commit",
                on_workspace(
                    |workspace, target, window, cx| {
                        spork_vcs::select_in_commit(workspace, &target.abs_path, window, cx)
                    },
                    target,
                ),
            )
            .icon(tool("toolwindows/commit"))
            .enabled(spork_vcs::has_local_change(workspace, &abs_path, cx)),
            Row::item(
                if cfg!(target_os = "macos") {
                    "Reveal in Finder"
                } else {
                    "Open in File Manager"
                },
                {
                    let abs_path = abs_path.clone();
                    move |_, cx| cx.reveal_path(&abs_path)
                },
            ),
            Row::item("Terminal", {
                let directory = abs_path.parent().map(|dir| dir.to_path_buf());
                move |window, cx| {
                    if let Some(working_directory) = directory.clone() {
                        window.dispatch_action(
                            workspace::OpenTerminal {
                                working_directory,
                                local: false,
                            }
                            .boxed_clone(),
                            cx,
                        );
                    }
                }
            })
            .icon(tool("terminal/toolwindow/terminal")),
        ];
        // the popup is the workspace's modal: shown once this update of it is over
        window.defer(cx, move |window, cx| {
            show_list_popup(&handle, &editor, "Select In", rows, true, window, cx)
        });
    });
}

/// The file Select In shows.
#[derive(Clone)]
struct Target {
    path: project::ProjectPath,
    abs_path: std::path::PathBuf,
    entry: Option<project::ProjectEntryId>,
}
