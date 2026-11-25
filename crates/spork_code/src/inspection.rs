//! Code ▸ Inspect Code…: IntelliJ's Specify Inspection Scope dialog (Whole project, Uncommitted
//! files, the current file), then the language servers' diagnostics over every file of the scope
//! (each opened for its servers and asked for its diagnostics) as IntelliJ's inspection results.
//!
//! The results go to the Problems tool window through [`InspectionResults`], which it sets: this
//! crate can't depend on the tool window's, which shows the inspection with [`rerun`].

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, Context, Div, Entity, Global, InteractiveElement as _, IntoElement as _,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, TaskExt as _,
    Window, px,
};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use language::{Buffer, Point};
use project::{Project, ProjectPath};
use spork_workbench::controls::{dialog_title, disabled_radio, labeled_radio};
use util::ResultExt as _;
use workspace::Workspace;

use crate::{Able, code};

/// A problem Inspect Code found: a language server's diagnostic.
#[derive(Clone)]
pub struct Problem {
    pub path: ProjectPath,
    pub buffer: Entity<Buffer>,
    pub range: Range<text::Anchor>,
    pub point: Point,
    pub severity: lsp::DiagnosticSeverity,
    pub message: String,
    /// The server or tool that reported it (`rustc`, `rust-analyzer`).
    pub source: Option<String>,
}

/// An Inspect Code run: what it looked at ("Project 'fixture'", "File 'main.rs'"), the scope's
/// files and what it found, in file order.
#[derive(Clone)]
pub struct Inspection {
    pub scope: SharedString,
    pub paths: Vec<ProjectPath>,
    pub problems: Vec<Problem>,
}

/// Where Inspect Code's results go. The Problems tool window sets it to show them in its
/// inspection tab (`cx.set_global(InspectionResults(Rc::new(…)))`); unset, they're only kept for
/// [`last_inspection`].
pub struct InspectionResults(
    pub Rc<dyn Fn(&mut Workspace, Inspection, &mut Window, &mut Context<Workspace>)>,
);

impl Global for InspectionResults {}

/// The last run's results, and the files it opened, kept open so their servers keep their
/// diagnostics.
#[derive(Default)]
struct LastInspection {
    inspection: Option<Inspection>,
    _open: Vec<project::lsp_store::OpenLspBufferHandle>,
}

impl Global for LastInspection {}

/// The last Inspect Code run's results.
pub fn last_inspection(cx: &App) -> Option<&Inspection> {
    cx.try_global::<LastInspection>()?.inspection.as_ref()
}

/// How many files a run opens at most.
// shortcut: a bounded number of files, opened all at once; open them in batches once a project
// is large enough to need more
const MAX_FILES: usize = 2000;
/// How long a server that doesn't answer diagnostics requests gets to publish them.
const PUBLISH_WAIT: Duration = Duration::from_secs(3);

/// Inspect Code needs no editor: the whole project is a scope.
pub(crate) fn register(div: Div, _: &Able, cx: &mut Context<Workspace>) -> Div {
    div.on_action(cx.listener(|workspace, _: &code::InspectCode, window, cx| {
        inspect_code_dialog(workspace, window, cx)
    }))
}

/// Rerun: Inspect Code again over the files of `inspection`'s scope.
pub fn rerun(
    workspace: &mut Workspace,
    inspection: &Inspection,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let project = workspace.project().clone();
    let handle = cx.entity().downgrade();
    inspect(
        &handle,
        &project,
        inspection.scope.clone(),
        inspection.paths.clone(),
        window,
        cx,
    );
}

#[derive(Clone, Copy, PartialEq)]
enum Scope {
    Project,
    Uncommitted,
    File,
}

/// IntelliJ's Specify Inspection Scope dialog (`BaseAnalysisActionDialog`).
fn inspect_code_dialog(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let project = workspace.project().clone();
    let file = workspace
        .active_item(cx)
        .and_then(|item| item.project_path(cx));
    let uncommitted = uncommitted_files(&project, cx);
    let project_name = project
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).root_name_str().to_owned())
        .unwrap_or_default();
    let scope = cx.new(|_| Scope::Project);
    let workspace_handle = cx.entity().downgrade();
    let analyze = {
        let (scope, file, uncommitted, project_name) = (
            scope.clone(),
            file.clone(),
            uncommitted.clone(),
            project_name,
        );
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let (title, paths) = match *scope.read(cx) {
                Scope::Project => (
                    format!("Project '{project_name}'"),
                    project_files(&project, cx),
                ),
                Scope::Uncommitted => ("Uncommitted files".to_owned(), uncommitted.clone()),
                Scope::File => {
                    let Some(file) = file.clone() else { return };
                    (format!("File '{}'", file.path.as_unix_str()), vec![file])
                }
            };
            window.close_dialog(cx);
            inspect(&workspace_handle, &project, title.into(), paths, window, cx);
        })
    };
    window.open_dialog(cx, move |dialog, _, cx| {
        let chosen = *scope.read(cx);
        let choose = |choice: Scope| {
            let scope = scope.clone();
            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                scope.update(cx, |scope, _| *scope = choice);
                window.refresh();
            }
        };
        let analyze = analyze.clone();
        let file_label = file
            .as_ref()
            .map(|file| format!("File '{}'", file.path.as_unix_str()));
        dialog
            .title(dialog_title("Specify Inspection Scope"))
            .on_ok(|_, _, _| false)
            .w(px(480.))
            .close_button(false)
            .child(
                v_flex()
                    .id("inspection-scope-dialog")
                    .role(gpui::Role::Group)
                    .aria_label("Specify Inspection Scope")
                    .gap(px(8.))
                    .child(
                        h_flex().gap(px(8.)).child("Inspection Scope").child(
                            gpui::div()
                                .flex_1()
                                .h(px(1.))
                                .bg(spork_workbench::Tokens::get(cx).separator),
                        ),
                    )
                    .child(
                        v_flex()
                            .pl(px(16.))
                            .gap(px(8.))
                            .child(
                                labeled_radio(
                                    "inspect-project",
                                    "Whole project",
                                    chosen == Scope::Project,
                                    cx,
                                )
                                .on_click(choose(Scope::Project)),
                            )
                            .child(match uncommitted.is_empty() {
                                false => labeled_radio(
                                    "inspect-uncommitted",
                                    "Uncommitted files",
                                    chosen == Scope::Uncommitted,
                                    cx,
                                )
                                .on_click(choose(Scope::Uncommitted))
                                .into_any_element(),
                                true => disabled_radio(
                                    "inspect-uncommitted",
                                    "Uncommitted files",
                                    false,
                                    cx,
                                )
                                .into_any_element(),
                            })
                            .when_some(file_label, |group, label| {
                                group.child(
                                    labeled_radio("inspect-file", label, chosen == Scope::File, cx)
                                        .on_click(choose(Scope::File)),
                                )
                            }),
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
                                spork_workbench::Button::new("dialog-ok", "Analyze")
                                    .primary(true)
                                    .on_click(move |_, window, cx| analyze(window, cx)),
                            ),
                    ),
            )
    });
}

/// The project's files, ignored ones left out.
fn project_files(project: &Entity<Project>, cx: &App) -> Vec<ProjectPath> {
    project
        .read(cx)
        .visible_worktrees(cx)
        .flat_map(|worktree| {
            let worktree = worktree.read(cx);
            let id = worktree.id();
            worktree
                .files(false, 0)
                .map(move |entry| ProjectPath {
                    worktree_id: id,
                    path: entry.path.clone(),
                })
                .collect::<Vec<_>>()
        })
        .take(MAX_FILES)
        .collect()
}

/// The files changed since the last commit, as git reports them.
fn uncommitted_files(project: &Entity<Project>, cx: &App) -> Vec<ProjectPath> {
    let git_store = project.read(cx).git_store().read(cx);
    git_store
        .repositories()
        .values()
        .flat_map(|repository| {
            let repository = repository.read(cx);
            repository
                .cached_status()
                .filter(|entry| !entry.status.is_deleted())
                .filter_map(|entry| repository.repo_path_to_project_path(&entry.repo_path, cx))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Opens each file of the scope for its language servers, asks them for its diagnostics, and
/// shows what they found.
fn inspect(
    workspace: &gpui::WeakEntity<Workspace>,
    project: &Entity<Project>,
    scope: SharedString,
    paths: Vec<ProjectPath>,
    window: &mut Window,
    cx: &mut App,
) {
    let languages = project.read(cx).languages().clone();
    let opens: Vec<_> = paths
        .iter()
        // only the files a language knows: a server may inspect them
        .filter(|path| {
            languages
                .language_for_file_path(path.path.as_std_path())
                .is_some()
        })
        .map(|path| project.update(cx, |project, cx| project.open_buffer(path.clone(), cx)))
        .collect();
    let (project, workspace) = (project.clone(), workspace.clone());
    window
        .spawn(cx, async move |cx| {
            let mut buffers = Vec::new();
            for open in opens {
                if let Some(buffer) = open.await.log_err() {
                    buffers.push(buffer);
                }
            }
            let handles = cx.update(|_, cx| {
                buffers
                    .iter()
                    .map(|buffer| {
                        project.update(cx, |project, cx| {
                            project.register_buffer_with_language_servers(buffer, cx)
                        })
                    })
                    .collect::<Vec<_>>()
            })?;
            // the servers start on the files they get, then publish their diagnostics
            cx.background_executor().timer(PUBLISH_WAIT).await;
            let pulls = cx.update(|_, cx| {
                buffers
                    .iter()
                    .map(|buffer| {
                        project.read(cx).lsp_store().update(cx, |store, cx| {
                            store.pull_diagnostics_for_buffer(buffer.clone(), cx)
                        })
                    })
                    .collect::<Vec<_>>()
            })?;
            for pull in pulls {
                pull.await.log_err();
            }
            cx.update(|window, cx| {
                let inspection = Inspection {
                    scope,
                    paths,
                    problems: problems(&buffers, cx),
                };
                cx.set_global(LastInspection {
                    inspection: Some(inspection.clone()),
                    _open: handles,
                });
                let Some(show) = cx
                    .try_global::<InspectionResults>()
                    .map(|hook| hook.0.clone())
                else {
                    return;
                };
                workspace
                    .update(cx, |workspace, cx| show(workspace, inspection, window, cx))
                    .log_err();
            })
        })
        .detach_and_log_err(cx);
}

/// The diagnostics of `buffers`, in file order, the most severe first within a file.
fn problems(buffers: &[Entity<Buffer>], cx: &App) -> Vec<Problem> {
    let mut problems = Vec::new();
    for buffer in buffers {
        let Some(file) = buffer.read(cx).file() else {
            continue;
        };
        let path = ProjectPath {
            worktree_id: file.worktree_id(cx),
            path: file.path().clone(),
        };
        let snapshot = buffer.read(cx).snapshot();
        for entry in snapshot.diagnostics_in_range::<_, text::Anchor>(0..snapshot.len(), false) {
            if !entry.diagnostic.is_primary {
                continue;
            }
            let point = text::ToPoint::to_point(&entry.range.start, &snapshot);
            problems.push(Problem {
                path: path.clone(),
                buffer: buffer.clone(),
                range: entry.range.clone(),
                point,
                severity: entry.diagnostic.severity,
                message: entry.diagnostic.message.to_string(),
                source: entry.diagnostic.source.clone(),
            });
        }
    }
    problems.sort_by(|a, b| {
        (a.path.path.as_unix_str(), a.severity, a.point).cmp(&(
            b.path.path.as_unix_str(),
            b.severity,
            b.point,
        ))
    });
    problems
}
