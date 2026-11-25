//! A commit's File Tree (docs/specs/M06 §1): every file as of the selected commit, beside its
//! changes; a file shows in full, and its menu restores, opens or saves that version.
// shortcut: a flat, sorted path list; folders to fold come when long lists get in the way.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{
    ActiveTheme as _, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::RepoView;
use crate::history::ROW_HEIGHT;

pub(crate) struct CommitFiles {
    commit: String,
    paths: Option<Result<Vec<String>, String>>,
    selected: Option<usize>,
    /// The selected file in full; a message instead for a binary file.
    content: Entity<DiffState>,
    binary: bool,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
}

impl RepoView {
    /// The Changes / All files switch above a commit's diff; for a merge, which parent its
    /// changes are shown against (M06 §1).
    pub(crate) fn details_tabs(&self, cx: &mut Context<Self>) -> Div {
        let files = self.commit_files.is_some();
        let parents = match (&self.repo, self.selected) {
            (Some(Ok(repo)), Some(ix)) => repo.history.commits[ix].parents.clone(),
            _ => Vec::new(),
        };
        let muted = cx.theme().muted_foreground;
        let tab = |id: &'static str, label: &'static str, on: bool, show: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .selected(on)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if show {
                        this.show_commit_files(cx)
                    } else {
                        this.commit_files = None;
                        cx.notify();
                    }
                }))
        };
        h_flex()
            .flex_none()
            .px_2()
            .gap_1()
            .child(tab("details-changes", "Changes", !files, false))
            .child(tab("details-files", "All files", files, true))
            .when(parents.len() > 1 && !files, |bar| {
                bar.child(div().flex_1())
                    .child(div().text_xs().text_color(muted).child("Against parent"))
                    .children(parents.iter().enumerate().map(|(i, id)| {
                        Button::new(("merge-parent", i))
                            .ghost()
                            .xsmall()
                            .label(format!("{} {}", i + 1, &id[..7.min(id.len())]))
                            .selected(self.merge_parent == i)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.merge_parent = i;
                                if let Some(ix) = this.selected {
                                    this.select(ix, cx);
                                }
                            }))
                    }))
            })
    }

    /// Space in the commit list: the selected commit's message and changes in a large window.
    pub(crate) fn preview_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some(ix)) = (&self.repo, self.selected) else {
            return;
        };
        let commit = &repo.history.commits[ix];
        let message = match &self.details {
            Some(Ok(d)) => d.message.clone(),
            _ => commit.subject.clone(),
        };
        let title = format!(
            "{} · {}",
            &commit.id[..7.min(commit.id.len())],
            commit.author
        );
        let diff = self.diff.clone();
        let muted = cx.theme().muted_foreground;
        let height = (window.viewport_size().height * 0.75).max(px(300.));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.width(px(1100.)).title(title.clone()).child(
                v_flex()
                    .h(height)
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .max_h(px(140.))
                            .overflow_hidden()
                            .text_sm()
                            .text_color(muted)
                            .child(message.clone()),
                    )
                    .child(div().flex_1().min_h_0().child(Diff::new(&diff).size_full())),
            )
        });
    }

    /// Lists the files at the selected commit (again, when another commit is selected).
    pub(crate) fn show_commit_files(&mut self, cx: &mut Context<Self>) {
        let Some(commit) = self.selected_commit_id() else {
            return;
        };
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        let content = match &self.commit_files {
            Some(files) => files.content.clone(),
            None => cx.new(|cx| DiffState::new([], cx)),
        };
        content.update(cx, |s, cx| s.set_files([], cx));
        let rev = commit.clone();
        let load = cx.spawn(async move |this, cx| {
            let paths = cx
                .background_spawn(async move { spork_git::file::tree(&root, &rev) })
                .await
                .map_err(|e| e.to_string());
            this.update(cx, |this, cx| {
                if let Some(files) = &mut this.commit_files {
                    files.paths = Some(paths);
                }
                cx.notify();
            })
            .ok();
        });
        self.commit_files = Some(CommitFiles {
            commit,
            paths: None,
            selected: None,
            content,
            binary: false,
            scroll: UniformListScrollHandle::new(),
            _load: Some(load),
        });
        cx.notify();
    }

    fn show_commit_file(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some(files)) = (&self.repo, &mut self.commit_files) else {
            return;
        };
        let Some(Ok(paths)) = &files.paths else {
            return;
        };
        let Some(path) = paths.get(ix).cloned() else {
            return;
        };
        files.selected = Some(ix);
        let (root, spec) = (repo.root.clone(), format!("{}:{path}", files.commit));
        files._load = Some(cx.spawn(async move |this, cx| {
            let bytes = cx
                .background_spawn(async move { spork_git::file::blob(&root, &spec) })
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| {
                let Some(files) = &mut this.commit_files else {
                    return;
                };
                // the same test git uses: a NUL byte in the first 8000 means binary
                files.binary = bytes.iter().take(8000).any(|b| *b == 0);
                let shown = (!files.binary)
                    .then(|| DiffFile::unchanged(path, &String::from_utf8_lossy(&bytes)));
                files.content.update(cx, |s, cx| s.set_files(shown, cx));
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    pub(crate) fn commit_files_view(&self, cx: &mut Context<Self>) -> Div {
        let Some(files) = &self.commit_files else {
            return v_flex();
        };
        let theme = cx.theme();
        let (muted, hover, active, border) = (
            theme.muted_foreground,
            theme.list_hover,
            theme.list_active,
            theme.border,
        );
        let message = |msg: String| {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(msg)
        };
        let paths = match &files.paths {
            None => return message("Listing files…".into()),
            Some(Err(e)) => return message(e.clone()),
            Some(Ok(paths)) => paths.clone(),
        };
        let (view, commit, selected) = (cx.entity(), files.commit.clone(), files.selected);
        let list = uniform_list("commit-files", paths.len(), move |range, _, cx| {
            range
                .map(|ix| {
                    let path = paths[ix].clone();
                    let (name, dir) = path
                        .rsplit_once('/')
                        .map_or((path.clone(), String::new()), |(d, n)| {
                            (n.to_owned(), format!("{d}/"))
                        });
                    let click = view.clone();
                    h_flex()
                        .id(ix)
                        .h(ROW_HEIGHT)
                        .px_2()
                        .gap_2()
                        .text_xs()
                        .when(selected == Some(ix), |r| r.bg(active))
                        .hover(move |r| r.bg(hover))
                        .on_click(move |_, _, cx| {
                            click.update(cx, |this, cx| this.show_commit_file(ix, cx))
                        })
                        .context_menu(crate::file_view::version_menu(
                            view.clone(),
                            commit.clone(),
                            path,
                        ))
                        .child(spork_workbench::image(
                            spork_workbench::file_icon(&name),
                            cx,
                        ))
                        .child(div().min_w_0().truncate().child(name))
                        .child(div().min_w_0().truncate().text_color(muted).child(dir))
                        .into_any_element()
                })
                .collect()
        });
        let list = div()
            .w(px(280.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(border)
            .child(crate::history::scrolled(list, &files.scroll));
        let shown = match (files.selected, files.binary) {
            (None, _) => message("Pick a file to see it as of this commit.".into()),
            (Some(_), true) => message("Binary file".into()),
            (Some(_), false) => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(Diff::new(&files.content).size_full()),
        };
        h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .child(shown)
    }
}
