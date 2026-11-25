//! One file through time (docs/specs/M08 §5–6), as the reference app shows it: Show History opens the
//! file's history tab (its toolbar, View Options, the revisions' menu, the diff preview and the
//! details), Annotate shows each line with its commit.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::notices::Notification;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::DiffState;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::component::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::{Value, json};
use spork_git::file::BlameLine;
use spork_git::{Commit, GitError};
use spork_workbench::ActionButton;

use crate::history::{ROW_HEIGHT, relative_age};
use crate::util::ResultExt as _;
use crate::{RepoView, View};

// shortcut: fixed cap on a file's history; page it in when files with longer histories are common.
const MAX_FILE_COMMITS: usize = 5_000;

/// The key context of the history tab: its shortcuts apply while it has the focus.
const CONTEXT: &str = "FileHistory";

actions!(
    file_history,
    [
        /// Refresh (⌘R): reads the file's history again.
        Refresh,
        /// Show Diff (⌘D): the selected revision's change to the file, in its own tab.
        ShowDiff,
        /// Show All Affected Files (⌃⌘A): everything the selected revision changed.
        ShowAllAffected,
        /// Copy Revision Number (⌥⇧⌘C).
        CopyRevisionNumber,
        /// Spork's own: writes the selected revision of the file where the user picks.
        SaveRevisionAs,
        /// Compare with Revision… (`Compare.Selected`) on Git ▸ Current File's file.
        CompareWithRevision,
        /// Compare with Branch or Tag… (`Git.CompareWithBranch`) on Git ▸ Current File's file.
        CompareWithBranch,
    ]
);

/// The file Git ▸ Current File acts on: the change selected in the Commit tool window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CurrentFile {
    pub(crate) tracked: bool,
    /// It isn't tracked yet or has changes `git add` takes.
    pub(crate) addable: bool,
}

/// The reference app's `Git.MainMenu.FileActions`, Git ▸ Current File. Without a file every item is greyed
/// and Commit File reads Commit…; Annotate and Show History for Selection… need an editor, which
/// a selected change isn't, so the reference app leaves them out there.
pub(crate) fn current_file_menu(file: Option<CurrentFile>) -> Menu {
    let tracked = file.is_some_and(|f| f.tracked);
    let commit = match file {
        Some(_) => "Commit File…",
        None => "Commit…",
    };
    Menu::new("Current File").items([
        MenuItem::action(commit, crate::changes::CommitFile).disabled(file.is_none()),
        MenuItem::action("Add", crate::changes::AddToVcs)
            .disabled(!file.is_some_and(|f| f.addable)),
        MenuItem::separator(),
        MenuItem::action("Show Diff", crate::changes::ShowDiff).disabled(!tracked),
        MenuItem::action("Compare with Revision…", CompareWithRevision).disabled(!tracked),
        MenuItem::action("Compare with Branch or Tag…", CompareWithBranch).disabled(!tracked),
        MenuItem::action("Show History", crate::shell::ShowFileHistory).disabled(!tracked),
    ])
}

pub(crate) fn bind_keys(cx: &mut App) {
    let context = || {
        KeyBindingContextPredicate::parse(CONTEXT)
            .ok()
            .map(std::rc::Rc::new)
    };
    let bindings: Vec<KeyBinding> = [
        ("secondary-r", Box::new(Refresh) as Box<dyn Action>),
        ("secondary-d", Box::new(ShowDiff)),
        ("ctrl-secondary-a", Box::new(ShowAllAffected)),
        ("alt-shift-secondary-c", Box::new(CopyRevisionNumber)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| {
        KeyBinding::load(keys, action, context(), false, None, cx).log_err()
    })
    .collect();
    cx.bind_keys(bindings);
}

/// Spork's commands on a file's revision that the reference app's menus don't have, for Find Action.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    vec![("Save Revision As…".into(), Box::new(SaveRevisionAs))]
}

/// The history tab's View Options, kept for the session as the reference app keeps one set for every
/// file history.
// shortcut: not saved across launches; move into prefs when the log's own options settle there.
#[derive(Clone, Copy)]
struct Options {
    commit_time: bool,
    author: bool,
    date: bool,
    hash: bool,
    details: bool,
    diff_preview: bool,
    preview_bottom: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            commit_time: false,
            author: true,
            date: true,
            hash: false,
            details: true,
            diff_preview: true,
            preview_bottom: false,
        }
    }
}

impl Global for Options {}

fn options(cx: &App) -> Options {
    cx.try_global::<Options>().copied().unwrap_or_default()
}

fn update_options(cx: &mut App, change: impl FnOnce(&mut Options)) {
    let mut options = options(cx);
    change(&mut options);
    cx.set_global(options);
}

pub(crate) struct FileView {
    path: String,
    blame: bool,
    /// The revision its history goes back from: HEAD, or the log's History Up to Here.
    from: String,
    history: Option<Result<Vec<Commit>, GitError>>,
    pub(crate) selected: Option<usize>,
    lines: Option<Result<Vec<BlameLine>, GitError>>,
    /// Annotations as of this revision instead of the working copy.
    blame_rev: Option<String>,
    pub(crate) diff: Entity<DiffState>,
    /// What the diff preview compares instead of the revision's change: "1cf6d44 vs Local".
    comparison: Option<String>,
    /// The history tab's focus, for its shortcuts.
    focus: FocusHandle,
    /// How many times the history was read, for the end-to-end tests.
    loads: usize,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
}

impl FileView {
    /// The file shown, relative to the repository.
    pub(crate) fn path(&self) -> &str {
        &self.path
    }

    /// Its editor tab's text: the reference app's history tab, or the annotated file's name.
    pub(crate) fn tab_label(&self) -> String {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        match self.blame {
            true => name.to_owned(),
            false => format!("History: {name}"),
        }
    }

    pub(crate) fn is_annotation(&self) -> bool {
        self.blame
    }

    pub(crate) fn set_comparison(&mut self, label: Option<String>) {
        self.comparison = label;
    }

    fn selected_commit(&self) -> Option<&Commit> {
        match &self.history {
            Some(Ok(history)) => history.get(self.selected?),
            _ => None,
        }
    }

    /// How many commits its history lists, once loaded.
    pub(crate) fn history_len(&self) -> Option<usize> {
        match &self.history {
            Some(Ok(history)) => Some(history.len()),
            _ => None,
        }
    }
}

impl RepoView {
    /// Git ▸ Current File's file, the change selected in the Commit tool window.
    pub(crate) fn current_file(&self) -> Option<CurrentFile> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let selected = self.change_selected.as_ref()?;
        let entry = repo.status.iter().find(|e| e.path == selected.path)?;
        Some(CurrentFile {
            tracked: !entry.untracked(),
            addable: entry.has_unstaged(),
        })
    }

    /// Git ▸ Current File's commands that act on the selected change.
    pub(crate) fn current_file_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        let Some(path) = self.change_selected.as_ref().map(|c| c.path.clone()) else {
            return el;
        };
        el.on_action(cx.listener({
            let path = path.clone();
            move |this, _: &CompareWithRevision, _, cx| this.open_file(path.clone(), false, cx)
        }))
        .on_action(cx.listener(move |this, _: &CompareWithBranch, window, cx| {
            this.compare_with_branch(path.clone(), window, cx)
        }))
    }

    /// Opens the history (or the annotations) of `path` in place of the commit list.
    pub(crate) fn open_file(&mut self, path: String, blame: bool, cx: &mut Context<Self>) {
        self.open_file_from(path, blame, "HEAD".into(), cx)
    }

    /// The history of `path` back from `rev` (the log's History Up to Here), or its annotations.
    pub(crate) fn open_file_from(
        &mut self,
        path: String,
        blame: bool,
        rev: String,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(_)) = &self.repo else { return };
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path.clone());
        self.recent_files.truncate(50);
        let mode = self.diff_mode;
        let diff = cx.new(|cx| DiffState::new([], cx).with_mode(mode));
        self.file = Some(FileView {
            path,
            blame,
            from: rev,
            history: None,
            selected: None,
            lines: None,
            blame_rev: None,
            diff,
            comparison: None,
            focus: cx.focus_handle(),
            loads: 0,
            scroll: UniformListScrollHandle::new(),
            _load: None,
        });
        self.load_file_history(cx);
        self.view = View::File;
        cx.notify();
    }

    /// Reads the file's history and annotations, keeping the selected revision when it's still
    /// there.
    fn load_file_history(&mut self, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some(file)) = (&self.repo, &mut self.file) else {
            return;
        };
        let root = repo.root.clone();
        let (path, at, from) = (file.path.clone(), file.blame_rev.clone(), file.from.clone());
        let keep = file.selected_commit().map(|c| c.id.clone());
        file._load = Some(cx.spawn(async move |this, cx| {
            let (history, lines) = cx
                .background_spawn(async move {
                    (
                        spork_git::file::history_from(&root, &from, &path, MAX_FILE_COMMITS),
                        spork_git::file::blame(&root, at.as_deref(), &path),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                let Some(file) = &mut this.file else { return };
                let ix = match (&history, keep) {
                    (Ok(history), Some(id)) => history.iter().position(|c| c.id == id),
                    _ => None,
                };
                file.history = Some(history);
                file.lines = Some(lines);
                file.loads += 1;
                this.select_file_commit(ix.unwrap_or(0), cx);
            })
            .ok();
        }));
    }

    pub(crate) fn select_file_commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some(file)) = (&self.repo, &mut self.file) else {
            return;
        };
        let Some(Ok(history)) = &file.history else {
            return;
        };
        let Some(commit) = history.get(ix) else {
            return;
        };
        file.selected = Some(ix);
        file.comparison = None;
        let ignore_ws = self.prefs.ignore_whitespace;
        let (root, id, path, diff) = (
            repo.root.clone(),
            commit.id.clone(),
            file.path.clone(),
            file.diff.clone(),
        );
        file._load = Some(cx.spawn(async move |this, cx| {
            let patch = cx
                .background_spawn(
                    async move { spork_git::file::diff_at(&root, &id, &path, ignore_ws) },
                )
                .await
                .unwrap_or_default();
            this.update(cx, |this, cx| this.set_patch(&diff, &patch, cx))
                .ok();
        }));
        cx.notify();
    }

    /// Compare with Local: the diff preview shows the selected revision against the working copy.
    fn compare_file_with_local(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some(file)) = (&self.repo, &mut self.file) else {
            return;
        };
        let Some(id) = file.selected_commit().map(|c| c.id.clone()) else {
            return;
        };
        let (root, path, diff) = (repo.root.clone(), file.path.clone(), file.diff.clone());
        file.comparison = Some(format!("{} vs Local", short(&id)));
        file._load = Some(cx.spawn_in(window, async move |this, cx| {
            let patch = cx
                .background_spawn(
                    async move { spork_git::file::diff_with_local(&root, &id, &path) },
                )
                .await;
            this.update_in(cx, |this, window, cx| match patch {
                Ok(patch) => this.set_patch(&diff, &patch, cx),
                Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
            })
            .ok();
        }));
        update_options(cx, |o| o.diff_preview = true);
        cx.notify();
    }

    /// The selected revision's commit in the log, selected there (None: the log doesn't hold it).
    fn select_in_log(&mut self, cx: &mut Context<Self>) -> Option<usize> {
        let id = self.file.as_ref()?.selected_commit()?.id.clone();
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let ix = repo.history.commits.iter().position(|c| c.id == id)?;
        self.select(ix, cx);
        Some(ix)
    }

    /// Show Diff: the revision's change to the file in its own tab.
    fn show_file_diff(&mut self, cx: &mut Context<Self>) {
        let path = self.file.as_ref().map(|f| f.path.clone());
        if self.select_in_log(cx).is_some() {
            self.open_commit_diff(path, cx);
        }
    }

    /// Show All Affected Files: every file the revision changed.
    fn show_all_affected(&mut self, cx: &mut Context<Self>) {
        if self.select_in_log(cx).is_some() {
            self.commit_file_selected = None;
            self.open_commit_diff(None, cx);
        }
    }

    /// Show in VCS Log: the log, at the revision's commit.
    fn show_in_log(&mut self, cx: &mut Context<Self>) {
        if let Some(ix) = self.select_in_log(cx) {
            self.view = View::History;
            self.history_scroll
                .scroll_to_item(ix, ScrollStrategy::Center);
            cx.notify();
        }
    }

    /// Annotate: the file as of the revision, each line with its commit.
    fn annotate_revision(&mut self, rev: Option<String>, cx: &mut Context<Self>) {
        if let Some(file) = &mut self.file {
            file.blame = true;
        }
        self.blame_at(rev, cx);
    }

    /// Fixup… / Squash Into…: the local changes committed as `fixup! <subject>` (or
    /// `squash!`), for a later rebase to fold into the revision's commit.
    fn commit_into_revision(&mut self, squash: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(subject) = self
            .file
            .as_ref()
            .and_then(|f| f.selected_commit())
            .map(|c| c.subject.clone())
        else {
            return;
        };
        let prefix = if squash { "squash!" } else { "fixup!" };
        self.set_message(format!("{prefix} {subject}"), window, cx);
        self.show_commit_window(window, cx);
    }

    /// Annotates the file as of `rev` (None: the working copy), e.g. just before a line's commit.
    fn blame_at(&mut self, rev: Option<String>, cx: &mut Context<Self>) {
        let (Some(Ok(repo)), Some(file)) = (&self.repo, &mut self.file) else {
            return;
        };
        let (root, path, at) = (repo.root.clone(), file.path.clone(), rev.clone());
        file.blame_rev = rev;
        file.lines = None;
        file._load = Some(cx.spawn(async move |this, cx| {
            let lines = cx
                .background_spawn(
                    async move { spork_git::file::blame(&root, at.as_deref(), &path) },
                )
                .await;
            this.update(cx, |this, cx| {
                if let Some(file) = &mut this.file {
                    file.lines = Some(lines);
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Writes the file as of `id` where the user picks (M08 §5).
    fn save_version(
        &mut self,
        id: String,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let target = cx.prompt_for_new_path(&repo.root, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(dest))) = target.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.run_op(
                    window,
                    cx,
                    move |root| {
                        let bytes = spork_git::file::blob(root, &format!("{id}:{path}"))?;
                        std::fs::write(&dest, bytes).map_err(|e| GitError::Other {
                            code: None,
                            stderr: e.to_string(),
                        })?;
                        Ok(dest)
                    },
                    |_, dest, window, cx| {
                        window.push_notification(
                            Notification::success(format!("Saved {}", dest.display())),
                            cx,
                        )
                    },
                )
            })
            .ok();
        })
        .detach();
    }

    /// Show File at Revision: the file as of `id`, from a temporary copy in its default app.
    pub(crate) fn open_version(
        &mut self,
        id: String,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.ide {
            return self.open_ide_revision(id, path, window, cx);
        }
        self.run_op(
            window,
            cx,
            move |root| {
                let bytes = spork_git::file::blob(root, &format!("{id}:{path}"))?;
                let name = path.rsplit('/').next().unwrap_or(&path);
                let dir = std::env::temp_dir().join(format!("spork-{}", &id[..7.min(id.len())]));
                let file = dir.join(name);
                std::fs::create_dir_all(&dir)
                    .and_then(|()| std::fs::write(&file, bytes))
                    .map_err(|e| GitError::Other {
                        code: None,
                        stderr: e.to_string(),
                    })?;
                Ok(file)
            },
            |_, file, _, cx| cx.open_with_system(&file),
        );
    }

    /// Get: the file put back as it was in `id`; the change shows in Local Changes.
    fn get_version(
        &mut self,
        id: String,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_op(
            window,
            cx,
            move |root| spork_git::file::restore(root, &path, &id).map(|()| path),
            |_, path, window, cx| {
                window.push_notification(
                    Notification::success(format!("Restored {path}: see Local Changes")),
                    cx,
                )
            },
        )
    }

    /// Jumps from an annotated line to its commit in the log.
    fn show_commit(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        if let Some(ix) = repo.history.commits.iter().position(|c| c.id == id) {
            self.view = View::History;
            self.select(ix, cx);
        }
    }

    /// Save Revision As… (Find Action): the history's selected revision of the file, saved.
    pub(crate) fn save_selected_revision(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let revision = self
            .file
            .as_ref()
            .filter(|f| !f.blame)
            .and_then(|f| Some((f.selected_commit()?.id.clone(), f.path.clone())));
        if let Some((id, path)) = revision {
            self.save_version(id, path, window, cx);
        }
    }

    fn copy_revision_number(&mut self, cx: &mut Context<Self>) {
        if let Some(commit) = self.file.as_ref().and_then(|f| f.selected_commit()) {
            cx.write_to_clipboard(ClipboardItem::new_string(commit.id.clone()));
        }
    }

    pub(crate) fn file_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(file) = &self.file else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let centered = |msg: String| {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(msg)
        };
        if file.blame {
            let header = file.blame_rev.clone().map(|rev| {
                h_flex()
                    .flex_none()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_xs()
                    .text_color(muted)
                    .child(format!("Annotated as of {}", short(&rev)))
                    .child(
                        Button::new("file-blame-current")
                            .xsmall()
                            .ghost()
                            .label("Current version")
                            .on_click(cx.listener(|this, _, _, cx| this.blame_at(None, cx))),
                    )
            });
            let body = match &file.lines {
                None => centered("Loading…".into()).into_any_element(),
                Some(Err(e)) => centered(e.to_string()).into_any_element(),
                Some(Ok(lines)) => div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::history::scrolled(
                        uniform_list(
                            "blame",
                            lines.len(),
                            cx.processor(|this, range, _, cx| this.blame_rows(range, cx)),
                        ),
                        &file.scroll,
                    ))
                    .into_any_element(),
            };
            return v_flex()
                .size_full()
                .children(header)
                .child(v_flex().flex_1().min_h_0().child(body))
                .into_any_element();
        }
        let body = match &file.history {
            None => centered("Loading…".into()).into_any_element(),
            Some(Err(e)) => centered(e.to_string()).into_any_element(),
            Some(Ok(_)) => self.history_panes(cx).into_any_element(),
        };
        let focus = self.file.as_ref().map(|f| f.focus.clone());
        h_flex()
            .id("file-history")
            .key_context(CONTEXT)
            .when_some(focus, |el, focus| el.track_focus(&focus))
            .size_full()
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.load_file_history(cx)))
            .on_action(cx.listener(|this, _: &ShowDiff, _, cx| this.show_file_diff(cx)))
            .on_action(cx.listener(|this, _: &ShowAllAffected, _, cx| this.show_all_affected(cx)))
            .on_action(
                cx.listener(|this, _: &CopyRevisionNumber, _, cx| this.copy_revision_number(cx)),
            )
            .child(self.history_toolbar(cx))
            .child(v_flex().flex_1().min_w_0().h_full().child(body))
            .into_any_element()
    }

    /// The reference app's `Vcs.FileHistory.Toolbar`, down the tab's left side.
    fn history_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.file.as_ref().and_then(|f| f.selected_commit());
        let picked = selected.is_some();
        let web = selected
            .and_then(|c| self.commit_web_url(&c.id))
            .map(|(url, service)| (url, open_in_browser_text(service)));
        let view = cx.entity();
        let separator = || {
            div()
                .w(px(16.))
                .h(px(1.))
                .my_1()
                .bg(spork_workbench::shell::toolbar_border(cx))
        };
        spork_workbench::shell::side_toolbar()
            .id("file-history-toolbar")
            .role(Role::Toolbar)
            .aria_label("Vcs.FileHistory.Toolbar")
            .child(
                ActionButton::new("file-history-refresh", "general/refresh")
                    .tooltip("Refresh (⌘R)")
                    .on_click(cx.listener(|this, _, _, cx| this.load_file_history(cx))),
            )
            .child(
                ActionButton::new("file-history-diff", "vcs/diff")
                    .tooltip("Show Diff (⌘D)")
                    .disabled(!picked)
                    .on_click(cx.listener(|this, _, _, cx| this.show_file_diff(cx))),
            )
            // hidden without a revision, as the reference app hides it
            .when(picked, |bar| {
                bar.child(
                    ActionButton::new("file-history-affected", "general/listFiles")
                        .tooltip("Show All Affected Files (⌃⌘A)")
                        .on_click(cx.listener(|this, _, _, cx| this.show_all_affected(cx))),
                )
            })
            .child(separator())
            .child(
                ActionButton::new("file-history-options", "general/show")
                    .popup_marker()
                    .tooltip("View Options")
                    .dropdown_menu(move |menu, window, cx| view_options(menu, &view, window, cx)),
            )
            // VcsHistoryActionsGroup.Toolbar: only for a repository on GitHub or GitLab
            .when_some(web, |bar, (url, text)| {
                bar.child(separator()).child(
                    ActionButton::new("file-history-web", "toolwindows/web")
                        .tooltip(text)
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            })
    }

    /// The table of revisions, the diff preview beside or under it, the details below.
    fn history_panes(&self, cx: &mut Context<Self>) -> Div {
        let Some(file) = &self.file else {
            return div();
        };
        let options = options(cx);
        let picked = file.selected_commit().is_some();
        let border = cx.theme().border;
        let corner = h_flex()
            .flex_none()
            .h(px(spork_workbench::tokens::metrics::CONTROL_HEIGHT + 4.))
            .px_1()
            .justify_end()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .id("file-history-corner")
                    .role(Role::Toolbar)
                    .aria_label("Vcs.FileHistory.Toolbar.RightCorner")
                    .child(
                        ActionButton::new("file-history-in-log", "general/vcs")
                            .tooltip("Show in VCS Log")
                            .disabled(!picked)
                            .on_click(cx.listener(|this, _, _, cx| this.show_in_log(cx))),
                    ),
            );
        let table = v_flex()
            .size_full()
            .child(corner)
            .child(div().flex_1().min_h_0().child(crate::history::scrolled(
                uniform_list(
                    "file-history",
                    match &file.history {
                        Some(Ok(history)) => history.len(),
                        _ => 0,
                    },
                    cx.processor(|this, range, _, cx| this.file_history_rows(range, cx)),
                ),
                &file.scroll,
            )))
            .when(options.details, |table| {
                table.children(self.revision_details(cx))
            });
        if !options.diff_preview {
            return div().size_full().child(table);
        }
        let preview = v_flex()
            .size_full()
            .border_color(border)
            .when(!options.preview_bottom, |p| p.border_l_1())
            .when(options.preview_bottom, |p| p.border_t_1())
            .children(file.comparison.clone().map(|label| {
                div()
                    .px_2()
                    .py_0p5()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label)
            }))
            .child(self.diff_bar("file", &file.diff, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.diff_view(&file.diff, cx)),
            );
        match options.preview_bottom {
            true => div().size_full().child(
                v_resizable("file-split-bottom")
                    .child(resizable_panel().child(table))
                    .child(resizable_panel().size(px(320.)).child(preview)),
            ),
            false => div().size_full().child(
                h_resizable("file-split")
                    .child(resizable_panel().size(px(520.)).child(table))
                    .child(resizable_panel().child(preview)),
            ),
        }
    }

    /// Show Details: the selected revision's subject, hash, author and date.
    fn revision_details(&self, cx: &mut Context<Self>) -> Option<Div> {
        let commit = self.file.as_ref()?.selected_commit()?;
        let theme = cx.theme();
        let time = match options(cx).commit_time {
            true => commit.commit_time,
            false => commit.time,
        };
        Some(
            v_flex()
                .flex_none()
                .max_h(px(160.))
                .p_2()
                .gap_1()
                .border_t_1()
                .border_color(theme.border)
                .text_sm()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(commit.subject.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(format!(
                            "{} {} <{}> on {}",
                            short(&commit.id),
                            commit.author,
                            commit.email,
                            crate::history::date_text(time, now())
                        )),
                ),
        )
    }

    fn file_history_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(FileView {
            history: Some(Ok(history)),
            selected,
            focus,
            ..
        }) = &self.file
        else {
            return Vec::new();
        };
        let view = cx.entity();
        let theme = cx.theme();
        let (hover, active) = (theme.list_hover, theme.list_active);
        let options = options(cx);
        let now = now();
        let focus = focus.clone();
        range
            .filter_map(|ix| history.get(ix).map(|c| (ix, c)))
            .map(|(ix, c)| {
                let time = match options.commit_time {
                    true => c.commit_time,
                    false => c.time,
                };
                let focus = focus.clone();
                h_flex()
                    .id(ix)
                    .role(Role::Row)
                    .aria_label(c.subject.clone())
                    .aria_selected(*selected == Some(ix))
                    .w_full()
                    .h(ROW_HEIGHT)
                    .px_2()
                    .gap_3()
                    .text_sm()
                    .when(*selected == Some(ix), |r| r.bg(active))
                    .hover(move |r| r.bg(hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            window.focus(&focus, cx);
                            this.select_file_commit(ix, cx)
                        }),
                    )
                    // a right click picks the row first, as the reference app's table does
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            if this.file.as_ref().and_then(|f| f.selected) != Some(ix) {
                                this.select_file_commit(ix, cx)
                            }
                        }),
                    )
                    .context_menu({
                        let view = view.clone();
                        move |menu, _, cx| revision_menu(menu, &view, ix, cx)
                    })
                    .child(div().flex_1().min_w_0().truncate().child(c.subject.clone()))
                    .when(options.author, |row| {
                        row.child(
                            div()
                                .w(px(150.))
                                .flex_none()
                                .truncate()
                                .child(c.author.clone()),
                        )
                    })
                    .when(options.date, |row| {
                        row.child(
                            div()
                                .w(px(124.))
                                .flex_none()
                                .truncate()
                                .child(crate::history::date_text(time, now)),
                        )
                    })
                    .when(options.hash, |row| {
                        row.child(div().w(px(64.)).flex_none().child(short(&c.id)))
                    })
                    .into_any_element()
            })
            .collect()
    }

    fn blame_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(FileView {
            lines: Some(Ok(lines)),
            ..
        }) = &self.file
        else {
            return Vec::new();
        };
        let view = cx.entity();
        let theme = cx.theme();
        let (muted, hover, border) = (theme.muted_foreground, theme.list_hover, theme.border);
        let mono = theme.mono_font_family.clone();
        let now = now();
        range
            .filter_map(|ix| lines.get(ix).map(|l| (ix, l)))
            .map(|(ix, l)| {
                // the commit is named once per run of consecutive lines from it
                let first = ix == 0 || lines[ix - 1].commit != l.commit;
                let id = l.commit.clone();
                h_flex()
                    .id(ix)
                    .w_full()
                    .h(ROW_HEIGHT)
                    .text_sm()
                    .hover(move |r| r.bg(hover))
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, _, _, cx| this.show_commit(&id, cx)
                    }))
                    .context_menu({
                        let view = view.clone();
                        move |menu, _, _| {
                            let (view, id) = (view.clone(), id.clone());
                            menu.item(PopupMenuItem::new("Blame the previous revision").on_click(
                                move |_, _, cx| {
                                    let before = format!("{id}^");
                                    view.update(cx, |this, cx| this.blame_at(Some(before), cx))
                                },
                            ))
                        }
                    })
                    .child(
                        h_flex()
                            .w(px(300.))
                            .flex_none()
                            .px_2()
                            .gap_2()
                            .h_full()
                            .border_r_1()
                            .border_color(border)
                            .text_xs()
                            .text_color(muted)
                            .when(first, |g| {
                                g.child(
                                    div()
                                        .font_family(mono.clone())
                                        .child(l.commit.chars().take(7).collect::<String>()),
                                )
                                .child(div().w(px(90.)).truncate().child(l.author.clone()))
                                .child(div().w(px(40.)).child(relative_age(l.time, now)))
                                .child(div().flex_1().min_w_0().truncate().child(l.summary.clone()))
                            }),
                    )
                    .child(
                        div()
                            .w(px(48.))
                            .flex_none()
                            .px_2()
                            .text_right()
                            .text_xs()
                            .text_color(muted)
                            .child(l.line.to_string()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(mono.clone())
                            .child(l.text.clone()),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

/// The menu of a file in a commit's File Tree tab (commit_files.rs): that version of the file.
pub(crate) fn version_menu(
    view: Entity<RepoView>,
    id: String,
    path: String,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, cx| {
        let action = |label: &str, f: fn(&mut RepoView, String, String, &mut Window, &mut Context<RepoView>)| {
            let (view, id, path) = (view.clone(), id.clone(), path.clone());
            PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                view.update(cx, |this, cx| f(this, id.clone(), path.clone(), window, cx))
            })
        };
        let sha = id.clone();
        // file commands see this version's commit too
        let extra = vec![
            ("sha".to_owned(), id.clone()),
            ("sha:short".to_owned(), id.chars().take(7).collect()),
        ];
        menu.item(action("Restore this version", RepoView::get_version))
            .item(action("Open this version", RepoView::open_version))
            .item(action("Save this version as…", RepoView::save_version))
            .separator()
            .item(
                PopupMenuItem::new("Copy commit SHA").on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(sha.clone()))
                }),
            )
            .map(|menu| {
                RepoView::custom_items_with(
                    menu,
                    &view,
                    crate::custom::Target::File,
                    Some(path.clone()),
                    extra,
                    cx,
                )
            })
    }
}

/// What the history tab knows of the revision a menu acts on.
struct Revision {
    id: String,
    path: String,
}

/// The reference app's `Vcs.FileHistory.ContextMenu` on the revision in row `ix`, with
/// `Dvcs.FileHistory.ContextMenu` and `Git.FileHistory.ContextMenu` in it. With no revision
/// only Copy Revision Number, Create Patch…, Show Diff and Show in VCS Log show, greyed.
fn revision_menu(
    menu: PopupMenu,
    view: &Entity<RepoView>,
    ix: usize,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = view.read(cx);
    let revision = this.file.as_ref().and_then(|f| match &f.history {
        Some(Ok(history)) => history.get(ix).map(|c| Revision {
            id: c.id.clone(),
            path: f.path.clone(),
        }),
        _ => None,
    });
    let changes = this.has_changes();
    let web = revision
        .as_ref()
        .and_then(|r| this.commit_web_url(&r.id))
        .map(|(url, service)| (url, open_in_browser_text(service)));
    let Some(revision) = revision else {
        return menu
            .item(PopupMenuItem::new("Copy Revision Number").disabled(true))
            .item(PopupMenuItem::new("Create Patch…").disabled(true))
            .separator()
            .item(PopupMenuItem::new("Show Diff").disabled(true))
            .item(PopupMenuItem::new("Show in VCS Log").disabled(true));
    };
    let item =
        |label: &str,
         icon: Option<&str>,
         run: fn(&mut RepoView, &Revision, &mut Window, &mut Context<RepoView>)| {
            let view = view.clone();
            let revision = Revision {
                id: revision.id.clone(),
                path: revision.path.clone(),
            };
            PopupMenuItem::new(label.to_owned())
                .when_some(icon, |item, icon| item.icon(spork_workbench::icon(icon)))
                .on_click(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        // the row may have been read again since the menu opened
                        this.select_revision(&revision.id, cx);
                        run(this, &revision, window, cx)
                    })
                })
        };
    let custom = {
        let (view, path, id) = (view.clone(), revision.path.clone(), revision.id.clone());
        move |menu: PopupMenu, cx: &mut Context<PopupMenu>| {
            let extra = vec![
                ("sha".to_owned(), id.clone()),
                ("sha:short".to_owned(), id.chars().take(7).collect()),
            ];
            RepoView::custom_items_with(
                menu,
                &view,
                crate::custom::Target::File,
                Some(path.clone()),
                extra,
                cx,
            )
        }
    };
    let menu = menu
        .item(
            item(
                "Copy Revision Number",
                Some("general/copy"),
                |this, _, _, cx| this.copy_revision_number(cx),
            )
            .action(Box::new(CopyRevisionNumber)),
        )
        .item(item(
            "Create Patch…",
            Some("vcs/patch"),
            |this, r, window, cx| this.save_patch_dialog(r.id.clone(), window, cx),
        ))
        .item(item(
            "Cherry-Pick",
            Some("dvcs/cherryPick"),
            |this, r, window, cx| {
                let id = r.id.clone();
                this.run_outcome(
                    window,
                    cx,
                    format!("Cherry-picked {}", short(&id)),
                    move |root| spork_git::cherry_pick(root, &id),
                )
            },
        ))
        .separator()
        .item(item("Get", None, |this, r, window, cx| {
            this.get_version(r.id.clone(), r.path.clone(), window, cx)
        }))
        .item(item(
            "Show File at Revision",
            None,
            |this, r, window, cx| this.open_version(r.id.clone(), r.path.clone(), window, cx),
        ))
        .item(item("Compare with Local", None, |this, _, window, cx| {
            this.compare_file_with_local(window, cx)
        }))
        .item(
            item("Show Diff", Some("vcs/diff"), |this, _, _, cx| {
                this.show_file_diff(cx)
            })
            .action(Box::new(ShowDiff)),
        )
        .item(item(
            "Show Diff in a New Tab or Window",
            None,
            |this, _, _, cx| this.show_file_diff(cx),
        ))
        .item(
            item(
                "Show All Affected Files",
                Some("general/listFiles"),
                |this, _, _, cx| this.show_all_affected(cx),
            )
            .action(Box::new(ShowAllAffected)),
        )
        .item(item("Annotate", None, |this, r, _, cx| {
            this.annotate_revision(Some(r.id.clone()), cx)
        }))
        .item(item(
            "Show in VCS Log",
            Some("general/vcs"),
            |this, _, _, cx| this.show_in_log(cx),
        ))
        .item(item("Revert Commit", None, |this, r, window, cx| {
            let id = r.id.clone();
            this.run_outcome(
                window,
                cx,
                format!("Reverted {}", short(&id)),
                move |root| spork_git::revert(root, &id),
            )
        }))
        // with nothing to commit there is nothing to fold into the revision
        .item(
            item("Fixup…", None, |this, _, window, cx| {
                this.commit_into_revision(false, window, cx)
            })
            .disabled(!changes),
        )
        .item(
            item("Squash Into…", None, |this, _, window, cx| {
                this.commit_into_revision(true, window, cx)
            })
            .disabled(!changes),
        )
        .separator()
        .item(item("New Tag…", None, |this, r, window, cx| {
            this.log_new_tag_dialog(r.id.clone(), window, cx)
        }))
        .separator()
        // VcsHistoryActionsGroup: only for a repository on GitHub or GitLab
        .when_some(web, |menu, (url, text)| {
            menu.item(PopupMenuItem::new(text).on_click(move |_, _, cx| cx.open_url(&url)))
        });
    custom(menu, cx)
}

/// The reference app's text for opening a commit on its hosting service.
fn open_in_browser_text(service: &str) -> &'static str {
    match service {
        "GitHub" => "Open on GitHub",
        _ => "View in browser",
    }
}

/// The reference app's `Vcs.FileHistory.PresentationSettings`: what the table shows, then the layout.
fn view_options(
    menu: PopupMenu,
    view: &Entity<RepoView>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let now = options(cx);
    let toggle = |label: &str, on: bool, flip: fn(&mut Options)| option_item(view, label, on, flip);
    // The reference app's section titles are part of the separator, not items to pick
    let header = |title: &'static str| {
        PopupMenuItem::element(move |_, cx: &mut App| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(title)
        })
        .disabled(true)
    };
    menu.item(header("Show"))
        .item(toggle("Commit Timestamp", now.commit_time, |o| {
            o.commit_time ^= true
        }))
        .submenu("Columns", window, cx, {
            let view = view.clone();
            move |menu, _, cx| {
                let now = options(cx);
                menu.item(option_item(&view, "Author", now.author, |o| {
                    o.author ^= true
                }))
                .item(option_item(&view, "Date", now.date, |o| o.date ^= true))
                .item(option_item(&view, "Hash", now.hash, |o| o.hash ^= true))
            }
        })
        .separator()
        .item(header("Layout"))
        .item(toggle("Show Details", now.details, |o| o.details ^= true))
        .item(toggle("Show Diff Preview", now.diff_preview, |o| {
            o.diff_preview ^= true
        }))
        .submenu("Diff Preview Location", window, cx, {
            let view = view.clone();
            move |menu, _, cx| {
                let bottom = options(cx).preview_bottom;
                let place = |label: &str, at_bottom: bool| {
                    let view = view.clone();
                    PopupMenuItem::new(label.to_owned())
                        .checked(bottom == at_bottom)
                        .on_click(move |_, _, cx| {
                            update_options(cx, |o| o.preview_bottom = at_bottom);
                            view.update(cx, |_, cx| cx.notify());
                        })
                };
                menu.item(place("Bottom", true)).item(place("Right", false))
            }
        })
}

/// A View Options toggle.
fn option_item(
    view: &Entity<RepoView>,
    label: &str,
    on: bool,
    flip: fn(&mut Options),
) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(label.to_owned())
        .checked(on)
        .on_click(move |_, _, cx| {
            update_options(cx, flip);
            view.update(cx, |_, cx| cx.notify());
        })
}

impl RepoView {
    /// Selects the revision `id` in the history tab, when it's there.
    fn select_revision(&mut self, id: &str, cx: &mut Context<Self>) {
        let ix = self.file.as_ref().and_then(|f| match &f.history {
            Some(Ok(history)) => history.iter().position(|c| c.id == id),
            _ => None,
        });
        let current = self.file.as_ref().and_then(|f| f.selected);
        if let Some(ix) = ix.filter(|ix| Some(*ix) != current) {
            self.select_file_commit(ix, cx);
        }
    }
}

/// What the history tab and the annotations show, for the end-to-end tests (e2e.rs).
pub(crate) fn e2e_state(view: &Entity<RepoView>, cx: &mut App) -> Value {
    let options = options(cx);
    let this = view.read(cx);
    let file = this.file.as_ref();
    let selected = file.and_then(|f| f.selected_commit());
    let log_selected = match (&this.repo, this.selected) {
        (Some(Ok(repo)), Some(ix)) => repo.history.commits.get(ix).map(|c| c.subject.clone()),
        _ => None,
    };
    let columns: Vec<&str> = [
        ("Author", options.author),
        ("Date", options.date),
        ("Hash", options.hash),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect();
    // the merge view's conflicts, each as it's resolved (null while it isn't)
    let merge = this.merge_file(cx).map(|(path, conflicts)| {
        let state = this.change_diff.read(cx);
        (0..conflicts.len())
            .map(|ix| {
                state
                    .conflict_resolution(&path, ix)
                    .map(|r| format!("{r:?}"))
            })
            .collect::<Vec<_>>()
    });
    let merge_result = this
        .merge_file(cx)
        .and_then(|(path, _)| this.change_diff.read(cx).resolved_text(&path));
    json!({
        "last_notification": crate::notices::last_text(),
        "merge_conflicts": merge,
        "merge_result": merge_result,
        "merge_selected": this.merge_selected(cx),
        "view": format!("{:?}", this.view),
        "file": file.map(|f| f.path.clone()),
        "mode": file.map(|f| if f.blame { "annotate" } else { "history" }),
        "revisions": file.and_then(|f| match &f.history {
            Some(Ok(history)) => Some(history.iter().map(|c| c.subject.clone()).collect::<Vec<_>>()),
            _ => None,
        }),
        "selected": selected.map(|c| c.subject.clone()),
        "annotated_at": file.and_then(|f| f.blame_rev.as_ref()).and_then(|rev| {
            let subject = file.and_then(|f| match &f.history {
                Some(Ok(history)) => history.iter().find(|c| &c.id == rev).map(|c| c.subject.clone()),
                _ => None,
            });
            subject.or_else(|| Some(rev.clone()))
        }),
        "annotated_lines": file.and_then(|f| f.lines.as_ref()).and_then(|l| l.as_ref().ok()).map(Vec::len),
        "comparison": file.and_then(|f| f.comparison.clone()),
        "preview_changes": file.map(|f| this.changed_lines(&f.diff)),
        "loads": file.map(|f| f.loads),
        "commit_timestamp": options.commit_time,
        "columns": columns,
        "details": options.details,
        "diff_preview": options.diff_preview,
        "diff_preview_location": if options.preview_bottom { "Bottom" } else { "Right" },
        "log_selected": log_selected,
        "commit_file": this.commit_file_selected.clone(),
        "change_file": this.change_selected.as_ref().map(|c| c.path.clone()),
        "diff_files": this.shown_diff_files(false, cx),
        "collapsed_files": this.shown_diff_files(true, cx),
        "commit_message": this.message(cx),
        "commit_window": this.left_tools.contains(&crate::LeftTool::Commit),
    })
}

/// A revision for display: the SHA cut to 7 characters, keeping a suffix such as `^`.
fn short(rev: &str) -> String {
    let (id, rest) = rev.split_at(rev.find('^').unwrap_or(rev.len()));
    format!("{}{rest}", &id[..7.min(id.len())])
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::short;

    #[test]
    fn shortens_revisions() {
        assert_eq!(short("9c4169f2ab01^"), "9c4169f^");
        assert_eq!(short("9c4169f2ab01"), "9c4169f");
        assert_eq!(short("abc"), "abc");
    }
}
