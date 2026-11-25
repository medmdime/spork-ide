//! Show History and Put Label…, IntelliJ's dialogs (reference/idea/screenshots/dialogs/
//! localhistory-showhistory.jpg, localhistory-putlabel.png): the changes on the left, newest
//! first, with a search by content and Revert; on the right the selected change's "Before" beside
//! the "Current" content. The diff is Zed's side-by-side diff editor (`SplittableEditor` on a
//! `BufferDiff`), as its text diff view draws one: Spork's diff view isn't reachable outside the
//! version control's crate.

use std::path::{Path, PathBuf};

use buffer_diff::BufferDiff;
use editor::{MultiBuffer, SplittableEditor};
use gpui::{
    App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, TaskExt as _, Window, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use language::{Buffer, Capability};
use project::Project;
use settings::DiffViewStyle;
use spork_workbench::controls::{dialog_title, form_label};
use spork_workbench::{Tokens, Typography as _, popup};
use util::ResultExt as _;
use workspace::Workspace;

use crate::{Revision, shown_time, with_store};

/// Show History for `path`, a file or a folder.
pub fn show_history(
    path: PathBuf,
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let (project, workspace) = (workspace.project().clone(), cx.entity());
    let view = cx.new(|cx| HistoryView::new(path, project, workspace, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(dialog_title(format!("Local History: {name}")))
            .on_ok(|_, _, _| false)
            // wide enough for the editor's side-by-side diff (`minimum_split_diff_width`)
            .w(px(1240.))
            .p_0()
            .child(view.clone())
    });
}

/// Put Label…: names the project's state in its history.
pub fn put_label(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Some(root) = workspace
        .project()
        .read(cx)
        .visible_worktrees(cx)
        .next()
        .map(|worktree| worktree.read(cx).abs_path().to_path_buf())
    else {
        return;
    };
    let name = cx.new(|cx| InputState::new(window, cx));
    let ok = {
        let name = name.clone();
        move |window: &mut Window, cx: &mut App| {
            let label = name.read(cx).value().trim().to_owned();
            if label.is_empty() {
                return;
            }
            window.close_dialog(cx);
            crate::put_label_now(&root, &label, cx);
        }
    };
    window
        .subscribe(&name, cx, {
            let ok = ok.clone();
            move |_, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => ok(window, cx),
                InputEvent::Change => window.refresh(),
                _ => {}
            }
        })
        .detach();
    let field = name.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        let ready = !field.read(cx).value().trim().is_empty();
        let ok = ok.clone();
        dialog
            .title(dialog_title("Put Label"))
            .on_ok(|_, _, _| false)
            .w(px(360.))
            .close_button(false)
            .child(
                v_flex()
                    .gap(px(8.))
                    .child(form_label("Label name:"))
                    .child(spork_workbench::TextField::new(&field).aria_label("Label name:"))
                    .child(
                        h_flex()
                            .pt(px(8.))
                            .gap(px(8.))
                            .justify_end()
                            .child(
                                spork_workbench::Button::new("label-cancel", "Cancel")
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(
                                spork_workbench::Button::new("label-ok", "OK")
                                    .primary(true)
                                    .disabled(!ready)
                                    .on_click(move |_, window, cx| ok(window, cx)),
                            ),
                    ),
            )
    });
    name.update(cx, |name, cx| name.focus(window, cx));
}

struct HistoryView {
    path: PathBuf,
    revisions: Vec<Revision>,
    selected: Option<usize>,
    search: Entity<InputState>,
    /// The selected change's diff once built, and its left side's title.
    diff: Option<(Entity<SplittableEditor>, SharedString)>,
    project: Entity<Project>,
    workspace: Entity<Workspace>,
    _subscription: Subscription,
    _diff: Task<()>,
}

impl HistoryView {
    fn new(
        path: PathBuf,
        project: Entity<Project>,
        workspace: Entity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = match path.is_dir() {
            true => "Search by file name",
            false => "Search by content",
        };
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let _subscription = cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });
        let mut this = Self {
            revisions: with_store(cx, |store| store.revisions(&path)).unwrap_or_default(),
            path,
            selected: None,
            search,
            diff: None,
            project,
            workspace,
            _subscription,
            _diff: Task::ready(()),
        };
        if !this.revisions.is_empty() {
            this.select(0, window, cx);
        }
        this
    }

    /// The rows the search lets through: by content for a file, by name for a folder.
    fn shown(&self, cx: &App) -> Vec<usize> {
        let query = self.search.read(cx).value().trim().to_lowercase();
        (0..self.revisions.len())
            .filter(|&ix| {
                let revision = &self.revisions[ix];
                query.is_empty()
                    || match self.path.is_dir() {
                        true => revision
                            .path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query),
                        false => revision
                            .content
                            .as_deref()
                            .and_then(|hash| with_store(cx, |store| store.content(hash)).flatten())
                            .is_some_and(|text| text.to_lowercase().contains(&query)),
                    }
            })
            .collect()
    }

    fn select(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(revision) = self.revisions.get(ix).cloned() else {
            return;
        };
        self.selected = Some(ix);
        let current = current_text(&revision.path, &self.project, cx);
        let before = revision
            .content
            .as_deref()
            .and_then(|hash| with_store(cx, |store| store.content(hash)).flatten())
            .unwrap_or_else(|| current.clone());
        let title: SharedString = format!("Before {}", shown_time(revision.time)).into();
        let (project, workspace) = (self.project.clone(), self.workspace.clone());
        let languages = project.read(cx).languages().clone();
        let path = revision.path;
        self._diff = cx.spawn_in(window, async move |this, cx| {
            let language = languages.load_language_for_file_path(&path).await.ok();
            let buffer = cx.new(|cx| {
                let mut buffer = Buffer::local(current, cx);
                buffer.set_language(language.clone(), cx);
                buffer.set_capability(Capability::ReadOnly, cx);
                buffer
            });
            let snapshot = buffer.read_with(cx, |buffer, _| buffer.text_snapshot());
            let diff = cx.new(|cx| BufferDiff::new(&snapshot, language, None, cx));
            diff.update(cx, |diff, cx| {
                diff.set_base_text(Some(before.into()), snapshot.clone(), cx)
            })
            .await;
            this.update_in(cx, |this, window, cx| {
                let multibuffer = cx.new(|cx| {
                    let mut multibuffer = MultiBuffer::singleton(buffer, cx);
                    multibuffer.add_diff(diff, cx);
                    multibuffer
                });
                let editor = cx.new(|cx| {
                    SplittableEditor::new(
                        DiffViewStyle::Split,
                        multibuffer,
                        project,
                        workspace,
                        window,
                        cx,
                    )
                });
                this.diff = Some((editor, title));
                cx.notify();
            })
            .log_err();
        });
        cx.notify();
    }

    /// Revert: the file goes back to the selected change's "Before", saved from its editor's
    /// buffer, which records the revert as a change of its own.
    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(revision) = self.selected.and_then(|ix| self.revisions.get(ix)).cloned() else {
            return;
        };
        let Some(before) = revision
            .content
            .as_deref()
            .and_then(|hash| with_store(cx, |store| store.content(hash)).flatten())
        else {
            return;
        };
        let project = self.project.clone();
        let open = project.update(cx, |project, cx| {
            project.open_local_buffer(&revision.path, cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            let buffer = open.await?;
            buffer.update(cx, |buffer, cx| {
                buffer.set_text(before, cx);
            });
            project
                .update(cx, |project, cx| project.save_buffer(buffer, cx))
                .await?;
            this.update_in(cx, |this, window, cx| {
                this.revisions =
                    with_store(cx, |store| store.revisions(&this.path)).unwrap_or_default();
                this.diff = None;
                if !this.revisions.is_empty() {
                    this.select(0, window, cx);
                }
            })?;
            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn row(&self, ix: usize, selected: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tokens = Tokens::get(cx);
        let revision = &self.revisions[ix];
        let time = shown_time(revision.time);
        // a folder's rows say which file changed
        let file = self
            .path
            .is_dir()
            .then(|| revision.path.strip_prefix(&self.path).ok())
            .flatten()
            .map(|file| file.to_string_lossy().into_owned());
        let title = revision
            .title
            .clone()
            .or(file)
            .unwrap_or_else(|| time.clone());
        let second = (title != time).then(|| time.clone());
        popup::row(("revision", ix), selected, cx)
            .role(Role::Row)
            .aria_label(title.clone())
            .h_auto()
            .py(px(4.))
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().truncate().child(title))
                    .children(second.map(|time| {
                        div()
                            .text_ui_xsmall()
                            .text_color(tokens.text_secondary)
                            .child(time)
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.select(ix, window, cx)))
    }
}

/// The content Show History compares with: the open editor's, else the disk's.
fn current_text(path: &Path, project: &Entity<Project>, cx: &App) -> String {
    let project = project.read(cx);
    project
        .find_project_path(path, cx)
        .and_then(|project_path| project.buffer_store().read(cx).get_by_path(&project_path))
        .map(|buffer| buffer.read(cx).text())
        .or_else(|| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}

impl Render for HistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = Tokens::get(cx);
        let shown = self.shown(cx);
        let rows: Vec<_> = shown
            .iter()
            .map(|&ix| {
                self.row(ix, self.selected == Some(ix), cx)
                    .into_any_element()
            })
            .collect();
        let name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let empty = rows.is_empty().then(|| {
            div()
                .p(px(12.))
                .text_color(tokens.text_secondary)
                .child(format!("No activity in {name} detected"))
        });
        let has_selection = self.selected.is_some();
        let list = v_flex()
            .w(px(320.))
            .h_full()
            .flex_none()
            .border_r_1()
            .border_color(tokens.separator)
            .child(
                h_flex()
                    .p(px(6.))
                    .gap(px(4.))
                    .child(
                        div().flex_1().child(
                            spork_workbench::TextField::new(&self.search)
                                .in_popup()
                                .aria_label("Search")
                                .prefix(spork_workbench::icon("general/search").size(px(16.))),
                        ),
                    )
                    .child(
                        spork_workbench::ActionButton::new("history-revert", "general/undo")
                            .tooltip("Revert Selected and Later Changes")
                            .disabled(!has_selection)
                            .on_click(cx.listener(|this, _, window, cx| this.revert(window, cx))),
                    ),
            )
            .child(
                v_flex()
                    .id("revisions")
                    .role(Role::List)
                    .aria_label("Changes")
                    .flex_1()
                    .overflow_y_scroll()
                    .children(rows)
                    .children(empty),
            );
        let diff = match &self.diff {
            Some((editor, before)) => v_flex()
                .size_full()
                .child(
                    h_flex()
                        .h(px(24.))
                        .px(px(8.))
                        .text_ui_small()
                        .child(div().flex_1().child(before.clone()))
                        .child(div().flex_1().child("Current")),
                )
                .child(div().flex_1().min_h_0().child(editor.clone()))
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(tokens.text_secondary)
                .child(match self.revisions.is_empty() {
                    true => "No changes detected",
                    false => "Select activity to view changes",
                })
                .into_any_element(),
        };
        h_flex()
            .id("local-history")
            .h(px(560.))
            .border_t_1()
            .border_color(tokens.separator)
            .child(list)
            .child(div().flex_1().min_w_0().h_full().child(diff))
    }
}
