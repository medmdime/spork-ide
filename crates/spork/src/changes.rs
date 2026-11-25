//! Working copy: unstaged / staged files, per-file diff, commit box (docs/specs/M07).

use crate::notices::Notification;
use crate::util::ResultExt as _;
use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::command::{CommandItem, CommandState};
use gpui_kit::component::diff::{DiffFile, DiffFileStatus, DiffSide};
use gpui_kit::component::input::{InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, resizable_panel,
    v_flex, v_resizable,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use spork_git::{GitError, PatchTarget, StatusEntry};
use spork_patch::{FilePatch, Side};
use spork_workbench::Spinner;

use crate::RepoView;

const ROW_HEIGHT: Rems = rems(1.75);

// The reference app's Commit tool window actions (docs/reference-app/actions.md), by their IntelliJ ids
actions!(
    changes,
    [
        /// ChangesView.Refresh
        Refresh,
        /// ChangesView.Revert
        Rollback,
        /// Diff.ShowDiff
        ShowDiff,
        /// ChangesView.SelectFile
        SelectOpenedFile,
        ExpandAll,
        CollapseAll,
        /// ChangesView.ShowIgnored
        ShowIgnored,
        /// ChangesView.GroupBy.Directory
        GroupByDirectory,
        /// ChangesView.GroupBy.Repository
        GroupByRepository,
        /// Vcs.ToggleAmendCommitMode
        ToggleAmend,
        /// Vcs.ShowMessageHistory
        MessageHistory,
        /// ChangesView.ShowCommitOptions
        ShowCommitOptions,
        /// Git.Commit.And.Push.Executor
        CommitAndPush,
        /// CheckinFiles
        CommitFile,
        /// ChangesView.AddUnversioned, Git.Add
        AddToVcs,
        /// ChangesView.EditSource
        JumpToSource,
        /// $Delete
        DeleteFile,
        /// ChangesView.CreatePatchToClipboard
        CopyPatch,
        /// ChangesView.CreatePatch
        CreatePatch,
        /// ChangesView.ShelveSilently
        ShelveSilently,
        /// ChangesView.SaveToShelve
        SaveToShelf,
        /// ChangesView.Shelve
        Shelve,
        /// Spork's own, reachable from Find Action
        Annotate,
        OpenFile,
        RevealFile,
        ExternalDiff,
        StashFiles,
        LfsLock,
        LfsUnlock,
        ToggleSpelling,
        ExternalMerge,
        /// Git.ResolveConflicts
        ResolveConflicts,
        /// Git.Stage.Add.Tracked
        StageAllTracked,
        /// Git.Stage.Add.All
        StageAll,
        /// Git.Stash.Silently
        StashSilently,
        /// Git.Stage.ToggleCommitAll
        ToggleCommitAll,
        /// Git.Stage.Enable
        UseStagingArea,
        /// Git.Stage.Disable
        UseChangelists,
        /// CommitView.ShowOnDoubleClick.EditorPreview
        ShowDiffOnDoubleClick,
        /// CommitView.ShowOnDoubleClick.Source
        ShowSourceOnDoubleClick,
        SelectPrevious,
        SelectNext,
    ]
);

/// The Commit tool window's keys, from the reference app's keymap; those of the changes tree apply while it
/// has the focus, the commit's anywhere in the project window (a text field keeps its own).
pub(crate) fn install(cx: &mut App) {
    let context = |name: &str| {
        KeyBindingContextPredicate::parse(name)
            .ok()
            .map(std::rc::Rc::new)
    };
    // an action's last binding is the one its menu item shows: the reference app's first shortcut goes last
    let tree: [(&str, Box<dyn Action>); 15] = [
        ("alt-secondary-z", Box::new(Rollback)),
        ("secondary-d", Box::new(ShowDiff)),
        ("shift-secondary-h", Box::new(ShelveSilently)),
        ("secondary-=", Box::new(ExpandAll)),
        ("secondary-+", Box::new(ExpandAll)),
        ("secondary--", Box::new(CollapseAll)),
        ("ctrl-p", Box::new(GroupByDirectory)),
        ("alt-secondary-a", Box::new(AddToVcs)),
        ("f4", Box::new(JumpToSource)),
        ("secondary-down", Box::new(JumpToSource)),
        ("secondary-backspace", Box::new(DeleteFile)),
        ("backspace", Box::new(DeleteFile)),
        ("delete", Box::new(DeleteFile)),
        ("up", Box::new(SelectPrevious)),
        ("down", Box::new(SelectNext)),
    ];
    let commit: [(&str, Box<dyn Action>); 4] = [
        ("ctrl-alt-m", Box::new(ToggleAmend)),
        ("ctrl-m", Box::new(MessageHistory)),
        ("ctrl-o", Box::new(ShowCommitOptions)),
        ("alt-secondary-k", Box::new(CommitAndPush)),
    ];
    let bindings: Vec<KeyBinding> = tree
        .into_iter()
        .map(|(keys, action)| (keys, action, "ChangesView"))
        .chain(
            commit
                .into_iter()
                .map(|(keys, action)| (keys, action, "Spork")),
        )
        .filter_map(|(keys, action, name)| {
            KeyBinding::load(keys, action, context(name), false, None, cx).log_err()
        })
        .collect();
    cx.bind_keys(bindings);
}

/// The Commit tool window's actions in Find Action, with the reference app's texts; Spork's own file
/// commands that the reference app's menus don't have are reached only from there.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    let commands: [(&str, Box<dyn Action>); 40] = [
        ("Create Patch from Local Changes…", Box::new(CreatePatch)),
        ("Shelve Silently", Box::new(ShelveSilently)),
        ("Save To Shelf", Box::new(SaveToShelf)),
        ("Shelve Changes…", Box::new(Shelve)),
        ("Refresh", Box::new(Refresh)),
        ("Rollback…", Box::new(Rollback)),
        ("Show Diff", Box::new(ShowDiff)),
        (
            "Select Opened File in Changes View",
            Box::new(SelectOpenedFile),
        ),
        ("Expand All", Box::new(ExpandAll)),
        ("Collapse All", Box::new(CollapseAll)),
        ("Ignored Files", Box::new(ShowIgnored)),
        ("Group By: Directory", Box::new(GroupByDirectory)),
        ("Group By: Repository", Box::new(GroupByRepository)),
        ("Amend Commit", Box::new(ToggleAmend)),
        ("Commit Message History", Box::new(MessageHistory)),
        ("Show Commit Options", Box::new(ShowCommitOptions)),
        ("Commit and Push…", Box::new(CommitAndPush)),
        ("Commit File…", Box::new(CommitFile)),
        ("Add to VCS", Box::new(AddToVcs)),
        ("Jump to Source", Box::new(JumpToSource)),
        ("Copy as Patch to Clipboard", Box::new(CopyPatch)),
        ("Delete…", Box::new(DeleteFile)),
        ("Annotate", Box::new(Annotate)),
        ("Open", Box::new(OpenFile)),
        ("Reveal in Finder", Box::new(RevealFile)),
        ("Open in External Diff Tool", Box::new(ExternalDiff)),
        ("Stash Changes of the Selected Files…", Box::new(StashFiles)),
        ("Lock (Git LFS)", Box::new(LfsLock)),
        ("Unlock (Git LFS)", Box::new(LfsUnlock)),
        (
            "Check Spelling in Commit Messages",
            Box::new(ToggleSpelling),
        ),
        ("Open in External Merge Tool", Box::new(ExternalMerge)),
        ("Resolve Conflicts…", Box::new(ResolveConflicts)),
        ("Stage All Tracked", Box::new(StageAllTracked)),
        ("Stage All", Box::new(StageAll)),
        ("Stash Silently", Box::new(StashSilently)),
        (
            "Commit All when Nothing Is Staged",
            Box::new(ToggleCommitAll),
        ),
        ("Staging Area", Box::new(UseStagingArea)),
        ("Changelists", Box::new(UseChangelists)),
        ("Show Diff on Double-Click", Box::new(ShowDiffOnDoubleClick)),
        (
            "Show Source on Double-Click",
            Box::new(ShowSourceOnDoubleClick),
        ),
    ];
    commands
        .into_iter()
        .map(|(label, action)| (SharedString::from(label), action))
        .collect()
}

/// The Commit tool window's own state.
pub(crate) struct CommitUi {
    /// The changes tree's: its shortcuts (⌘D, ⌥⌘Z…) apply while it has the focus.
    pub(crate) focus: FocusHandle,
    /// The Changes and Ignored Files nodes are open (Unversioned Files:
    /// `RepoView::unversioned_open`).
    changes_open: bool,
    ignored_open: bool,
    /// View Options › Group By.
    group_by_directory: bool,
    group_by_repository: bool,
    /// Directory nodes folded while grouped by directory.
    closed_dirs: std::collections::HashSet<String>,
    /// The Rollback Changes dialog's, while it is open.
    rollback: ChangesDialog,
    /// The Shelve Changes or Create Patch dialog's, while it is open.
    executor: ExecutorDialog,
    /// The Conflicts dialog's selected file, and its Group by directory.
    conflict_selected: Option<String>,
    conflicts_by_directory: bool,
    /// Display as Staging Area (Git.Stage.Enable) rather than Changelists, its nodes open, and
    /// Commit All when Nothing Is Staged; kept in the settings.
    staging: bool,
    stage_open: [bool; 5],
    commit_all: bool,
    /// Show on Double-Click ▸ Source rather than Diff; kept in the settings.
    double_click_source: bool,
    /// The commit options popup, while open: from its gear or ⌃O (ChangesView.ShowCommitOptions).
    options_menu: Option<Entity<PopupMenu>>,
    _options_dismiss: Option<Subscription>,
}

/// The Staging Area's nodes, in IntelliJ's order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StageNode {
    Unmerged,
    Staged,
    Unstaged,
    Untracked,
    /// Listed with View Options › Ignored Files (Git.Stage.ToggleIgnored).
    Ignored,
}

impl StageNode {
    const ALL: [StageNode; 5] = [
        Self::Unmerged,
        Self::Staged,
        Self::Unstaged,
        Self::Untracked,
        Self::Ignored,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Unmerged => "Unmerged",
            Self::Staged => "Staged",
            Self::Unstaged => "Unstaged",
            Self::Untracked => "Untracked",
            Self::Ignored => "Ignored",
        }
    }

    fn holds(self, e: &StatusEntry) -> bool {
        match self {
            Self::Unmerged => e.conflicted,
            Self::Staged => e.has_staged(),
            Self::Unstaged => !e.conflicted && !e.untracked() && e.worktree != '.',
            Self::Untracked => e.untracked() && !e.ignored(),
            Self::Ignored => e.ignored(),
        }
    }
}

impl CommitUi {
    pub(crate) fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            changes_open: true,
            ignored_open: false,
            group_by_directory: false,
            group_by_repository: false,
            closed_dirs: Default::default(),
            rollback: Default::default(),
            executor: Default::default(),
            conflict_selected: None,
            conflicts_by_directory: false,
            staging: setting("commit.stagingArea").as_deref() == Some("true"),
            stage_open: [true, true, true, false, false],
            commit_all: setting("commit.commitAll").as_deref() != Some("false"),
            double_click_source: setting("commit.showSourceOnDoubleClick").as_deref()
                == Some("true"),
            options_menu: None,
            _options_dismiss: None,
        }
    }
}

/// A dialog's tree of changes (Rollback Changes, Shelve Changes, Create Patch): the files
/// checked, the one selected, its grouping and folding; Rollback's Delete local copies of added
/// files.
#[derive(Default, Clone)]
struct ChangesDialog {
    included: Vec<String>,
    selected: Option<String>,
    delete_added: bool,
    by_directory: bool,
    open: bool,
    closed_dirs: std::collections::HashSet<String>,
}

/// The two commit dialogs that don't commit, IntelliJ's commit executors.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Executor {
    /// Shelve Changes… (ChangesView.Shelve)
    #[default]
    Shelve,
    /// Create Patch from Local Changes… (ChangesView.CreatePatch)
    Patch,
}

impl Executor {
    fn title(self) -> &'static str {
        match self {
            Self::Shelve => "Shelve Changes",
            Self::Patch => "Create Patch",
        }
    }

    fn button(self) -> &'static str {
        match self {
            Self::Shelve => "Shelve Changes",
            Self::Patch => "Create Patch…",
        }
    }
}

/// The Shelve Changes or Create Patch dialog while open: its changes, its Commit Message, its
/// Diff folded or not; then Patch File Settings' fields.
#[derive(Default)]
struct ExecutorDialog {
    executor: Executor,
    tree: ChangesDialog,
    /// The files its tree lists, as it opened.
    listed: Vec<String>,
    message: Option<Entity<TextareaState>>,
    diff_closed: bool,
    patch_file: Option<Entity<InputState>>,
    base_path: Option<Entity<InputState>>,
    to_clipboard: bool,
    reverse: bool,
    /// Why Patch File Settings' OK can't go on (its base path).
    patch_error: Option<String>,
}

/// The file chosen for the diff pane: its path and which side (staged or not) it was picked from.
#[derive(Clone, PartialEq, Eq)]
pub struct ChangeSelection {
    pub path: String,
    pub staged: bool,
}

/// The marked files after a click on `clicked` in `list`. `current` is the selected file when it
/// is in the same list; Shift marks the range from it, Cmd/Ctrl toggles the clicked file (the
/// selected one counts as marked), and a plain click (or another list) marks nothing.
fn next_marks(
    marked: &[String],
    current: Option<&str>,
    clicked: &str,
    list: &[String],
    shift: bool,
    toggle: bool,
) -> Vec<String> {
    let Some(current) = current else {
        return Vec::new();
    };
    let at = |p: &str| list.iter().position(|q| q == p);
    match (shift, toggle) {
        (true, _) => match (at(current), at(clicked)) {
            (Some(a), Some(b)) => list[a.min(b)..=a.max(b)].to_vec(),
            _ => Vec::new(),
        },
        (false, true) => {
            let mut marks = if marked.is_empty() {
                vec![current.to_owned()]
            } else {
                marked.to_vec()
            };
            match marks.iter().position(|p| p == clicked) {
                Some(i) => drop(marks.remove(i)),
                None => marks.push(clicked.to_owned()),
            }
            marks
        }
        (false, false) => Vec::new(),
    }
}

/// Status letter shown in the file lists, with the color of the file's name (IntelliJ's).
pub(crate) fn status_letter(entry: &StatusEntry, staged: bool, cx: &App) -> (char, Hsla) {
    use spork_workbench::FileStatus::*;
    let letter = match (entry.conflicted, entry.untracked(), staged) {
        (true, _, _) => 'U',
        (_, true, _) if entry.ignored() => '!',
        (_, true, _) => '?',
        (_, _, true) => entry.index,
        (_, _, false) => entry.worktree,
    };
    let status = match letter {
        'U' => Conflicted,
        '!' => Ignored,
        '?' => Unversioned,
        'A' | 'C' => Added,
        'D' => Deleted,
        'R' => Renamed,
        _ => Modified,
    };
    (letter, spork_workbench::file_status_color(status, cx))
}

/// Why git refused a commit, when Spork can help (M07 §5).
enum Refused {
    Identity,
    /// A commit hook said no, with what it printed.
    Hooks(String),
}

impl RepoView {
    /// The changed files a list shows: those the filter box lets through.
    pub(crate) fn status_entries(&self, staged: bool) -> Vec<&StatusEntry> {
        let mut entries = self.all_entries(staged);
        entries.retain(|e| e.path.to_lowercase().contains(&self.changes_query));
        if self.prefs.changes_tree {
            entries.sort_by(|a, b| split_dir(&a.path).cmp(&split_dir(&b.path)));
        }
        entries
    }

    fn all_entries(&self, staged: bool) -> Vec<&StatusEntry> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        repo.status
            .iter()
            .filter(|e| {
                if staged {
                    e.has_staged()
                } else {
                    e.has_unstaged()
                }
            })
            .collect()
    }

    /// Runs a git write in the background, then reloads; failures become a notification.
    pub(crate) fn run_op<T: Send + 'static>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        op: impl FnOnce(&std::path::Path) -> Result<T, GitError> + Send + 'static,
        on_success: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
    ) {
        // no repository open (welcome screen): clone / init / settings ops don't need one
        let root = match &self.repo {
            Some(Ok(repo)) => repo.root.clone(),
            _ => self.requested.clone(),
        };
        self.busy = true;
        cx.notify();
        // network commands report progress (job_banner): redraw while the operation runs
        self._job_poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let busy = this
                    .update(cx, |this, cx| {
                        cx.notify();
                        this.busy
                    })
                    .unwrap_or(false);
                if !busy {
                    break;
                }
            }
        }));
        self._op = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx.background_spawn(async move { op(&root) }).await;
            this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(value) => on_success(this, value, window, cx),
                    Err(GitError::IndexLocked) => this.index_lock_dialog(window, cx),
                    Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                }
                this.reload(cx);
            })
            .ok();
        }));
    }

    /// While a fetch, pull, push or clone runs: git's latest progress line and Cancel (M09).
    pub(crate) fn job_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let line = spork_git::job_progress().filter(|_| self.busy)?;
        let theme = cx.theme();
        let primary = theme.primary;
        // a sliding bar under the strip: the operation is alive even while git prints nothing
        let bar = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(2.))
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .w(relative(0.3))
                    .bg(primary)
                    .with_animation(
                        "job-bar",
                        Animation::new(std::time::Duration::from_millis(1200))
                            .repeat()
                            .with_easing(ease_in_out),
                        |bar, t| bar.left(relative(-0.3 + 1.3 * t)),
                    ),
            );
        Some(
            h_flex()
                .relative()
                .flex_none()
                .w_full()
                .px_3()
                .py_1()
                .gap_2()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.secondary)
                .text_xs()
                .child(Spinner::new().xsmall().color(primary))
                .child(bar)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(if line.is_empty() {
                            "Working…".to_owned()
                        } else {
                            line
                        }),
                )
                .child(
                    Button::new("job-cancel")
                        .ghost()
                        .xsmall()
                        .label("Cancel")
                        .on_click(|_, _, _| spork_git::cancel_job()),
                ),
        )
    }

    /// A leftover `index.lock` blocks every write; offer to remove it (M01 §6).
    fn index_lock_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Ok(lock) = spork_git::branch::index_lock(&repo.root) else {
            return;
        };
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let lock = lock.clone();
            dialog
                .title("The repository is locked")
                .description(format!(
                    "{} exists: another git program is working in this repository, or one crashed. Remove the lock only if no other git program (terminal, editor, another client) is running.",
                    lock.display()
                ))
                .confirm()
                .ok_text("Remove index.lock")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    match std::fs::remove_file(&lock) {
                        Ok(()) => window.push_notification(Notification::success("Lock removed: try again"), cx),
                        Err(e) => window.push_notification(Notification::error(e.to_string()), cx),
                    }
                    true
                })
        });
    }

    /// ↑ / ↓ in Local Changes: the previous or next file of the list the selection is in (M07 §1).
    pub(crate) fn step_change(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(sel) = self.change_selected.clone() else {
            // nothing selected yet: start at the top of the unstaged list
            let first = self.status_entries(false).first().map(|e| e.path.clone());
            let Some(path) = first else { return };
            return self.select_change(
                Some(ChangeSelection {
                    path,
                    staged: false,
                }),
                cx,
            );
        };
        let paths: Vec<String> = self
            .status_entries(sel.staged)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        let Some(ix) = paths.iter().position(|p| *p == sel.path) else {
            return;
        };
        let next = ix.saturating_add_signed(delta).min(paths.len() - 1);
        let path = paths[next].clone();
        self.select_change(
            Some(ChangeSelection {
                path,
                staged: sel.staged,
            }),
            cx,
        );
    }

    /// A click in a file list: Cmd/Ctrl adds or removes the file from the marked ones, Shift marks
    /// the range from the selected file; a plain click selects only it (M07 §1).
    fn click_change(&mut self, sel: ChangeSelection, mods: Modifiers, cx: &mut Context<Self>) {
        let current = self
            .change_selected
            .as_ref()
            .filter(|s| s.staged == sel.staged);
        let paths: Vec<String> = self
            .status_entries(sel.staged)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        self.change_marked = next_marks(
            &self.change_marked,
            current.map(|c| c.path.as_str()),
            &sel.path,
            &paths,
            mods.shift,
            mods.secondary(cx),
        );
        self.select_change(Some(sel), cx);
    }

    /// The marked files of the staged or unstaged list that still have changes there.
    fn marked_in(&self, staged: bool) -> Vec<String> {
        if !self
            .change_selected
            .as_ref()
            .is_some_and(|s| s.staged == staged)
        {
            return Vec::new();
        }
        let present: Vec<&str> = self
            .status_entries(staged)
            .iter()
            .map(|e| e.path.as_str())
            .collect();
        self.change_marked
            .iter()
            .filter(|p| present.contains(&p.as_str()))
            .cloned()
            .collect()
    }

    /// A row's own button acts on every marked file when that row is one of them.
    fn acting_on(&self, path: &str, staged: bool) -> Vec<String> {
        let marked = self.marked_in(staged);
        if marked.iter().any(|p| p == path) {
            marked
        } else {
            vec![path.to_owned()]
        }
    }

    /// Space in Local Changes stages the selected file, or unstages it from the staged list.
    /// Only while no text field has focus: there, Space types a space.
    pub(crate) fn toggle_stage_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let in_tree = self.commit_ui.focus.is_focused(window);
        match (&self.view, self.change_selected.clone()) {
            (crate::View::Changes, Some(sel))
                if !self.busy && (in_tree || self.focus.is_focused(window)) =>
            {
                let paths = self.acting_on(&sel.path, sel.staged);
                self.toggle_stage(paths, sel.staged, window, cx)
            }
            // the changes tree's Space checks or unchecks the selected file, as IntelliJ's
            (_, Some(sel)) if !self.busy && in_tree => {
                let paths = self.acting_on(&sel.path, sel.staged);
                self.toggle_stage(paths, sel.staged, window, cx)
            }
            // in the commit list, Space previews the selected commit large
            (crate::View::History, _)
                if self.selected.is_some() && self.focus.is_focused(window) =>
            {
                self.preview_commit(window, cx)
            }
            _ => cx.propagate(),
        }
    }

    fn toggle_stage(
        &mut self,
        paths: Vec<String>,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // ignored files go in only when picked on purpose: `add --force` for those alone
        let ignored: Vec<String> = self
            .all_entries(false)
            .iter()
            .filter(|e| e.ignored() && paths.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        self.run_op(
            window,
            cx,
            move |root| {
                let (forced, paths): (Vec<&str>, Vec<&str>) = paths
                    .iter()
                    .map(String::as_str)
                    .partition(|p| !staged && ignored.iter().any(|i| i == p));
                if !forced.is_empty() {
                    spork_git::stage_ignored(root, &forced)?;
                    if paths.is_empty() {
                        return Ok(());
                    }
                }
                if staged {
                    spork_git::unstage(root, &paths)
                } else {
                    spork_git::stage(root, &paths)
                }
            },
            // the files moved to the other list
            |this, _, _, _| this.change_marked.clear(),
        );
    }

    /// Re-renders the subject counter as the user types; Cmd/Ctrl-Enter commits (M07 §5).
    pub(crate) fn on_commit_input<E>(
        &mut self,
        _: &Entity<E>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter {
                secondary: true, ..
            } if !self.busy => self.commit(false, window, cx),
            _ => {}
        }
    }

    fn commit(&mut self, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_as(push, false, window, cx)
    }

    /// `no_verify`: the user chose to skip the hooks after one refused the commit.
    fn commit_as(
        &mut self,
        push: bool,
        no_verify: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // the Staging Area's Commit All when Nothing Is Staged: the tracked changes go in
        let nothing_staged = self.all_entries(true).is_empty();
        let commit_all = nothing_staged
            && self.commit_ui.staging
            && self.commit_ui.commit_all
            && self.all_entries(false).iter().any(|e| !e.untracked());
        if !self.amend && nothing_staged && !commit_all {
            window.push_notification(Notification::error("Select files to commit"), cx);
            return;
        }
        // the first line is the subject: without one, IntelliJ's "Specify commit message"
        let message = self.message(cx).trim().to_owned();
        if message.is_empty() {
            window.push_notification(Notification::error("Specify commit message"), cx);
            return;
        }
        let opts = spork_git::CommitOptions {
            amend: self.amend,
            signoff: self.signoff,
            no_verify,
        };
        // Settings › Version Control › Commit › Clear initial commit message off: the next
        // commit starts from this one's message, as IntelliJ's
        let next_message = match setting("commit.clearInitialMessage") {
            Some(clear) if clear == "false" => message.clone(),
            _ => String::new(),
        };
        self.run_op(
            window,
            cx,
            move |root| {
                if commit_all {
                    spork_git::stage_tracked(root)?;
                }
                match spork_git::commit_with(root, &message, opts) {
                    // asked about next, then the commit is tried again
                    Err(e) if spork_git::is_identity_missing(&e) => {
                        return Ok(Err(Refused::Identity));
                    }
                    Err(spork_git::GitError::Other { stderr, .. })
                        if !no_verify && spork_git::has_commit_hooks(root) =>
                    {
                        return Ok(Err(Refused::Hooks(stderr)));
                    }
                    other => other?,
                }
                Ok(Ok(()))
            },
            move |this, committed, window, cx| {
                match committed {
                    Ok(()) => {}
                    Err(Refused::Identity) => return this.identity_dialog(push, window, cx),
                    Err(Refused::Hooks(why)) => return this.hooks_refused(push, why, window, cx),
                }
                this.set_message(next_message.clone(), window, cx);
                this.amend = false;
                this.amend_commit = None;
                this.fill_template(window, cx);
                // Commit and Push… goes on as Git ▸ Push… does, after the commit
                if push {
                    this.push_after_commit(window, cx);
                }
            },
        );
    }

    /// A commit hook refused the commit: show why, and offer to commit without hooks (M07 §5).
    fn hooks_refused(
        &mut self,
        push: bool,
        why: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity();
        let mono = cx.theme().mono_font_family.clone();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .title("The commit was refused")
                .description(
                    v_flex()
                        .gap_2()
                        .child("A commit hook stopped it. Fix what it reports, or commit without running hooks.")
                        .child(
                            div()
                                .id("hook-output")
                                .max_h(px(220.))
                                .text_xs()
                                .font_family(mono.clone())
                                .child(why.clone()).overflow_y_scrollbar(),
                        ),
                )
                .confirm()
                .ok_text("Commit without hooks")
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| this.commit_as(push, true, window, cx));
                    true
                })
        });
    }

    /// The commit message box's text: its first line is the subject.
    pub(crate) fn message(&self, cx: &App) -> String {
        self.message.read(cx).value().to_string()
    }

    pub(crate) fn set_message(
        &mut self,
        text: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text: String = text.into();
        self.message
            .update(cx, |message, cx| message.set_value(text, window, cx));
    }

    /// An empty message box starts from the repository's commit template, as `git commit` does.
    pub(crate) fn fill_template(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(template) = self
            .message(cx)
            .is_empty()
            .then(|| spork_git::commit_template(&repo.root))
            .flatten()
        else {
            return;
        };
        self.set_message(template, window, cx);
    }

    /// Git doesn't know who is committing: ask once, save it globally, commit again (M01 §1).
    fn identity_dialog(&mut self, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Your name"));
        let email = cx.new(|cx| InputState::new(window, cx).placeholder("you@example.com"));
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, name, email) = (view.clone(), name.clone(), email.clone());
            dialog
                .title("Who is committing?")
                .description(
                    v_flex()
                        .gap_2()
                        .child("Git records a name and email with every commit. They're saved in your global git config, for all repositories.")
                        .child(spork_workbench::TextField::new(&name))
                        .child(spork_workbench::TextField::new(&email)),
                )
                .confirm()
                .ok_text("Save and commit")
                .on_ok(move |_, window, cx| {
                    let name = name.read(cx).value().trim().to_owned();
                    let email = email.read(cx).value().trim().to_owned();
                    if name.is_empty() || email.is_empty() {
                        return false;
                    }
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |_| {
                                spork_git::config::set_global("user.name", &name)?;
                                spork_git::config::set_global("user.email", &email)
                            },
                            move |this, (), window, cx| this.commit(push, window, cx),
                        )
                    });
                    true
                })
        });
    }

    pub(crate) fn set_amend(&mut self, amend: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.amend = amend;
        self.amend_commit =
            match &self.repo {
                Some(Ok(repo)) if amend => spork_git::commit_files(&repo.root, "HEAD")
                    .log_err()
                    .map(|mut files| {
                        // IntelliJ lists them by file name, not by path
                        files.sort_by(|(_, a), (_, b)| {
                            let name =
                                |path: &str| path.rsplit('/').next().unwrap_or(path).to_owned();
                            name(a).cmp(&name(b))
                        });
                        let message = spork_git::head_message(&repo.root).unwrap_or_default();
                        let subject = message.lines().next().unwrap_or_default().to_owned();
                        (subject, files)
                    }),
                _ => None,
            };
        self.amend_pushed = match &self.repo {
            Some(Ok(repo)) if amend => spork_git::head_is_pushed(&repo.root),
            _ => false,
        };
        let empty = self.message(cx).is_empty();
        if let (true, true, Some(Ok(repo))) = (amend, empty, &self.repo) {
            // prefill with HEAD's message, like `git commit --amend` does
            if let Some(message) = spork_git::head_message(&repo.root) {
                self.set_message(message, window, cx);
            }
        }
        cx.notify();
    }

    /// Selects a changed file and loads its diff in the background.
    pub fn select_change(&mut self, selection: Option<ChangeSelection>, cx: &mut Context<Self>) {
        self.change_selected = selection.clone();
        let entry = selection.as_ref().and_then(|sel| {
            self.status_entries(sel.staged)
                .into_iter()
                .find(|e| e.path == sel.path)
                .cloned()
        });
        let (Some(sel), Some(entry), Some(Ok(repo))) = (selection, entry, &self.repo) else {
            self.change_selected = None;
            self.change_diff
                .update(cx, |s, cx| s.set_files(Vec::new(), cx));
            cx.notify();
            return;
        };
        self.change_images = None;
        self.change_submodule = None;
        if repo.submodules.iter().any(|s| s.path == entry.path) {
            let (root, path, staged) = (repo.root.clone(), entry.path, sel.staged);
            self._change_diff_load = Some(cx.spawn(async move |this, cx| {
                let log = cx
                    .background_spawn({
                        let path = path.clone();
                        async move { spork_git::submodule::change_log(&root, &path, staged) }
                    })
                    .await
                    .unwrap_or_else(|e| e.to_string());
                this.update(cx, |this, cx| {
                    this.change_submodule = Some((path, log));
                    cx.notify();
                })
                .ok();
            }));
            return cx.notify();
        }
        if let (Some(format), false) = (
            crate::image_diff::image_format(&entry.path),
            entry.conflicted,
        ) {
            return self.load_image_pair(entry.path.clone(), sel.staged, format, cx);
        }
        let root = repo.root.clone();
        self._change_diff_load = Some(cx.spawn(async move |this, cx| {
            let conflicted = entry.conflicted;
            let path = entry.path.clone();
            let text = cx
                .background_spawn(async move {
                    if conflicted {
                        // the working file with conflict markers, for the 3-way resolver (M11 §4)
                        std::fs::read_to_string(root.join(&entry.path)).map_err(|e| {
                            GitError::Other {
                                code: None,
                                stderr: e.to_string(),
                            }
                        })
                    } else {
                        spork_git::file_diff(&root, &entry, sel.staged)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                let state = this.change_diff.clone();
                match (&text, conflicted) {
                    (Ok(t), true) => {
                        let files = DiffFile::parse_conflicts(path, t).map(|f| vec![f]);
                        state.update(cx, |s, cx| s.set_files(files.unwrap_or_default(), cx));
                    }
                    (Ok(p), false) => this.set_patch(&state, p, cx),
                    _ => this.set_patch(&state, "", cx),
                }
                this.change_patch = if conflicted { None } else { text.ok() };
                state.update(cx, |s, cx| s.set_selected_lines(None, cx));
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The commit panel under the changes, as in the reference app: Amend, the message, then Commit and
    /// Commit and Push…, the commit options under the gear.
    fn commit_box(&mut self, cx: &mut Context<Self>) -> Div {
        let staged = self.all_entries(true);
        // what the commit takes, each count in its status color: "2 added   3 modified"
        let (mut added, mut modified, mut deleted) = (0, 0, 0);
        for e in &staged {
            match e.index {
                'A' | 'C' => added += 1,
                'D' => deleted += 1,
                _ => modified += 1,
            }
        }
        use spork_workbench::{FileStatus, file_status_color};
        let counts = [
            (added, "added", file_status_color(FileStatus::Added, cx)),
            (
                modified,
                "modified",
                file_status_color(FileStatus::Modified, cx),
            ),
            (
                deleted,
                "deleted",
                file_status_color(FileStatus::Deleted, cx),
            ),
        ];
        let spelling = self.spelling_hints(cx);
        let inspections = self.inspection_hints(cx);
        let theme = cx.theme();
        let (link, border, mono, warning) = (
            theme.link,
            theme.input,
            theme.mono_font_family.clone(),
            theme.warning,
        );
        let (view, menu) = (cx.entity(), self.commit_ui.options_menu.clone());
        // opened from its gear or ⌃O, so the popover's open state is the view's
        let options = Popover::new("commit-options")
            .appearance(false)
            .overlay_closable(false)
            .open(menu.is_some())
            .on_open_change(move |open, window, cx| {
                view.update(cx, |this, cx| this.show_commit_options(*open, window, cx))
            })
            .trigger(
                spork_workbench::ActionButton::new("commit-options-button", "general/settings")
                    .role(Role::Button)
                    .aria_label("Show Commit Options")
                    .tooltip("Show Commit Options"),
            )
            .content(move |_, _, _| div().children(menu.clone()));
        v_flex()
            .size_full()
            .px(px(11.))
            .pt_1()
            .pb_2()
            .gap(px(3.))
            // ChangesView.CommitToolbar: Amend Commit (⌃⌥M), Commit Message History (⌃M)
            .child(
                h_flex()
                    .id("commit-toolbar")
                    .role(Role::Toolbar)
                    .aria_label("ChangesView.CommitToolbar")
                    .gap_1()
                    .child(
                        h_flex()
                            .id("amend")
                            .role(Role::CheckBox)
                            .aria_label("Amend")
                            .aria_toggled(match self.amend {
                                true => Toggled::True,
                                false => Toggled::False,
                            })
                            .gap_1p5()
                            .mr_1()
                            .cursor_pointer()
                            .child(spork_workbench::controls::check_box("amend-box", Some(self.amend), cx))
                            .child("Amend")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_amend(!this.amend, window, cx)
                            })),
                    )
                    // shortcut: always the last commit; IntelliJ's popup of unpushed commits is not here yet
                    .child(
                        div()
                            .id("amend-target")
                            .role(Role::Link)
                            .aria_label("last commit")
                            .cursor_pointer()
                            .text_color(link)
                            .child("last commit")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.set_amend(!this.amend, window, cx)
                            })),
                    )
                    .child(
                        spork_workbench::ActionButton::new("message-history", "general/history")
                            .role(Role::Button)
                            .aria_label("Commit Message History")
                            .tooltip("Commit Message History")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.message_history_into(false, window, cx)
                            })),
                    )
                    .child(div().flex_1())
                    .children(
                        counts
                            .into_iter()
                            .filter(|(n, _, _)| *n > 0)
                            .map(|(n, what, color)| div().text_xs().text_color(color).child(format!("{n} {what}"))),
                    ),
            )
            // one "Commit Message" box in the code font, as IntelliJ's editor
            .child(
                v_flex()
                    .flex_1()
                    .min_h(px(64.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .font_family(mono)
                    .child(
                        Textarea::new(&self.message)
                            .appearance(false)
                            .px(px(6.))
                            .flex_1(),
                    ),
            )
            .children(spelling)
            .children(inspections)
            .when(self.amend && self.amend_pushed, |box_| {
                box_.child(div().text_xs().text_color(warning).child(
                    "This commit is already pushed: amending rewrites it, so pushing again needs a force push.",
                ))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        spork_workbench::Button::new(
                            "commit",
                            if self.amend { "Amend Commit" } else { "Commit" },
                        )
                        .role(Role::Button)
                        .aria_label(if self.amend { "Amend Commit" } else { "Commit" })
                        .primary(self.commit_focused)
                        .tooltip("Commit (⌘↩)")
                        .when(self.busy, |b| b.child(Spinner::new().xsmall()))
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.commit(false, window, cx))),
                    )
                    // as the reference app: Commit and Push… gives way first, the gear at the end
                    .child(
                        spork_workbench::Button::new("commit-push", "Commit and Push…")
                            .role(Role::Button)
                            .aria_label("Commit and Push…")
                            .shrink()
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.commit(true, window, cx))),
                    )
                    .child(div().flex_1())
                    .child(options)
                    .when(self.ai_busy, |row| row.child(Spinner::new().xsmall())),
            )
    }

    /// Opens or closes the commit options popup (the gear, ⌃O), its items built anew each time.
    fn show_commit_options(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ui = &mut self.commit_ui;
        if !open {
            ui.options_menu = None;
            ui._options_dismiss = None;
            return cx.notify();
        }
        let (signoff, view, ai_busy) = (self.signoff, cx.entity(), self.ai_busy);
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            let view = view.clone();
            let (ai_view, author_view) = (view.clone(), view.clone());
            menu.label("Git")
                .item(
                    PopupMenuItem::new("Sign-off commit")
                        .checked(signoff)
                        .on_click(move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.signoff = !this.signoff;
                                cx.notify();
                            })
                        }),
                )
                .item(
                    PopupMenuItem::new("Add Co-Author…").on_click(move |_, window, cx| {
                        author_view.update(cx, |this, cx| this.co_author_picker(window, cx))
                    }),
                )
                .separator()
                .item(
                    PopupMenuItem::new("Generate Commit Message with AI")
                        .disabled(ai_busy)
                        .on_click(move |_, window, cx| {
                            ai_view.update(cx, |this, cx| {
                                this.ai_start(crate::ai::AiTask::CommitMessage, window, cx)
                            })
                        }),
                )
        });
        // closed, the popup gives the focus back, as to the message after ⌃O
        let before = window.focused(cx);
        menu.focus_handle(cx).focus(window, cx);
        self.commit_ui._options_dismiss = Some(cx.subscribe_in(
            &menu,
            window,
            move |this, _, _: &DismissEvent, window, cx| {
                if let Some(before) = &before {
                    before.focus(window, cx);
                }
                this.show_commit_options(false, window, cx)
            },
        ));
        self.commit_ui.options_menu = Some(menu);
        cx.notify();
    }

    /// What Settings › Version Control › Commit's commit message inspections find in the
    /// message, as warnings under it.
    fn inspection_hints(&self, cx: &mut Context<Self>) -> Option<Div> {
        let text = self.message(cx);
        // an empty message has nothing to inspect, and reads no settings
        if text.trim().is_empty() {
            return None;
        }
        let settings = crate::settings_window::Store::load();
        let on = COMMIT_INSPECTIONS.map(|(key, _)| settings.get(key) == Some("true"));
        let problems = inspection_problems(&text, on);
        if problems.is_empty() {
            return None;
        }
        let warning = cx.theme().warning;
        Some(v_flex().text_xs().text_color(warning).children(problems))
    }

    /// Words of the message that look misspelt; a click adds one to the user's own words.
    fn spelling_hints(&self, cx: &mut Context<Self>) -> Option<Div> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        if !self.prefs.spell_check {
            return None;
        }
        let text = self.message(cx);
        // an empty message has nothing to check, and doesn't load the word list
        if text.trim().is_empty() {
            return None;
        }
        let typos = crate::spell::typos(&text, &repo.vocabulary);
        if typos.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_wrap()
                .gap_1()
                .text_xs()
                .text_color(theme.warning)
                .child("Check spelling:")
                .children(typos.into_iter().enumerate().map(|(i, word)| {
                    Button::new(("typo", i))
                        .ghost()
                        .xsmall()
                        .label(word.clone())
                        .tooltip("Spelt right: add it to my words")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            crate::spell::learn(&word);
                            cx.notify();
                        }))
                })),
        )
    }

    /// The repository's authors, searchable; the one picked is added as a `Co-authored-by:`
    /// trailer to the description.
    fn co_author_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let me = spork_git::config::get(&repo.root, "user.email").unwrap_or_default();
        let mut authors: Vec<(String, String)> = Vec::new();
        for c in &repo.history.commits {
            if !c.email.is_empty() && c.email != me && !authors.iter().any(|(_, e)| *e == c.email) {
                authors.push((c.author.clone(), c.email.clone()));
            }
        }
        let items = authors
            .iter()
            .map(|(name, email)| CommandItem::new().label(format!("{name} <{email}>")))
            .collect();
        let (view, state) = (cx.entity(), cx.new(|cx| CommandState::new(window, cx)));
        crate::shell::palette(
            window,
            cx,
            state,
            items,
            "Add a co-author…",
            move |ix, window, cx| {
                window.close_dialog(cx);
                let Some((name, email)) = authors.get(ix).cloned() else {
                    return;
                };
                view.update(cx, |this, cx| {
                    let message = this.message(cx);
                    let message =
                        add_trailer(&message, &format!("Co-authored-by: {name} <{email}>"));
                    this.set_message(message, window, cx);
                })
            },
        );
    }

    /// Selected diff lines → display indices in the open file's patch, if a selection exists.
    fn selected_patch_lines(
        &self,
        cx: &App,
    ) -> Option<(FilePatch, std::ops::RangeInclusive<usize>)> {
        let range = self.change_diff.read(cx).selected_lines()?;
        let patch = FilePatch::parse(self.change_patch.as_deref()?)?;
        let side = |s: DiffSide| match s {
            DiffSide::Original => Side::Old,
            DiffSide::Modified => Side::New,
        };
        let lines = patch.range(
            (side(range.side()), range.start()),
            (side(range.end_side()), range.end()),
        )?;
        Some((patch, lines))
    }

    /// Stage / unstage / discard only the selected lines (M07 §4).
    fn apply_selection(
        &mut self,
        target: PatchTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((patch, lines)) = self.selected_patch_lines(cx) else {
            return;
        };
        let reverse = target != PatchTarget::Stage;
        let Some(partial) = patch.partial(|i, _| lines.contains(&i), reverse) else {
            window.push_notification(
                Notification::error("The selection contains no changed lines."),
                cx,
            );
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| spork_git::apply_patch(root, &partial, target),
            |this, _, _, cx| {
                this.change_diff
                    .update(cx, |s, cx| s.set_selected_lines(None, cx))
            },
        );
    }

    /// Writes the file as resolved in the conflict view and marks it resolved (stages it).
    fn save_resolution(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.change_diff.read(cx).resolved_text(&path) else {
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| {
                std::fs::write(root.join(&path), text).map_err(|e| GitError::Other {
                    code: None,
                    stderr: e.to_string(),
                })?;
                spork_git::stage(root, &[&path])
            },
            |_, _, window, cx| {
                window.push_notification(Notification::success("Conflict resolved"), cx)
            },
        );
    }

    /// Bar above a conflicted file: progress of the per-conflict choices and "Save resolution".
    fn conflict_bar(&self, path: &str, cx: &mut Context<Self>) -> Option<Div> {
        let state = self.change_diff.read(cx);
        let file = state.files().iter().find(|f| f.path().as_ref() == path)?;
        if file.status() != DiffFileStatus::Conflicted {
            return None;
        }
        let total = file.conflicts().len();
        let done = (0..total)
            .filter(|&i| state.conflict_resolution(path, i).is_some())
            .count();
        let resolved = state.resolved_text(path);
        let theme = cx.theme();
        let (path, ready) = (path.to_owned(), resolved.is_some());
        let ai_path = path.clone();
        Some(
            h_flex()
                .flex_none()
                .h(px(36.))
                .px_3()
                .justify_between()
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(format!("{done} of {total} resolved"))
                .child(div().flex_1())
                .child(
                    Button::new("ai-conflict")
                        .small()
                        .ghost()
                        .label("✦ Suggest with AI")
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.ai_start(crate::ai::AiTask::Conflict(ai_path.clone()), window, cx)
                        })),
                )
                .child(
                    Button::new("save-resolution")
                        .small()
                        .primary()
                        .label("Save resolution")
                        .disabled(!ready || self.busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.save_resolution(path.clone(), window, cx)
                        })),
                ),
        )
    }

    /// A moved submodule: the commits it gained (+) or lost (−), and what to do with it (M08 §4).
    fn submodule_view(&self, cx: &mut Context<Self>) -> Div {
        let Some((path, log)) = &self.change_submodule else {
            return v_flex();
        };
        let theme = cx.theme();
        let (muted, mono) = (theme.muted_foreground, theme.mono_font_family.clone());
        let lines: Vec<String> = log
            .lines()
            .filter_map(|l| {
                let l = l.trim_start();
                l.strip_prefix("> ")
                    .map(|s| format!("+ {s}"))
                    .or_else(|| l.strip_prefix("< ").map(|s| format!("\u{2212} {s}")))
            })
            .collect();
        let (update_path, open_path) = (path.clone(), path.clone());
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Submodule {path}")),
            )
            .child(
                div().text_sm().text_color(muted).child(match lines.len() {
                    0 => "Its checked-out commit changed (no commits between the two to list)."
                        .to_owned(),
                    n => format!("{n} commit(s) between the recorded and the checked-out commit:"),
                }),
            )
            .child(
                v_flex()
                    .id("submodule-log")
                    .flex_1()
                    .min_h_0()
                    .text_sm()
                    .font_family(mono)
                    .children(lines)
                    .overflow_y_scrollbar(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("submodule-update")
                            .small()
                            .label("Update submodule")
                            .tooltip("Check out the commit this repository records")
                            .disabled(self.busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let path = update_path.clone();
                                this.run_op(
                                    window,
                                    cx,
                                    move |root| spork_git::submodule::update(root, &path),
                                    |_, (), window, cx| {
                                        window.push_notification(
                                            Notification::success("Submodule updated"),
                                            cx,
                                        )
                                    },
                                )
                            })),
                    )
                    .child(
                        Button::new("submodule-open")
                            .small()
                            .ghost()
                            .label("Open submodule")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some(Ok(repo)) = &this.repo else { return };
                                let target = repo.root.join(&open_path);
                                this.switch_repo(target, cx)
                            })),
                    ),
            )
    }

    /// Bar above the diff with actions for the selected lines.
    fn selection_bar(&self, staged: bool, cx: &mut Context<Self>) -> Option<Div> {
        let (patch, lines) = self.selected_patch_lines(cx)?;
        let changed = patch
            .lines()
            .enumerate()
            .filter(|(i, l)| lines.contains(i) && l.is_change())
            .count();
        let theme = cx.theme();
        let bar = h_flex()
            .flex_none()
            .h(px(36.))
            .px_3()
            .gap_2()
            .justify_between()
            .border_b_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground);
        if !patch.supports_partial() {
            return Some(bar.child("Deleted files can only be staged as a whole."));
        }
        let label = match changed {
            1 => "1 line selected".to_owned(),
            n => format!("{n} lines selected"),
        };
        let action = |id: &'static str, text: &'static str, target: PatchTarget| {
            Button::new(id)
                .small()
                .label(text)
                .disabled(changed == 0 || self.busy)
                .on_click(cx.listener(move |this, _, window, cx| match target {
                    PatchTarget::Discard => this.confirm_discard_lines(window, cx),
                    _ => this.apply_selection(target, window, cx),
                }))
        };
        let buttons = if staged {
            h_flex()
                .gap_2()
                .child(action("unstage-lines", "Unstage lines", PatchTarget::Unstage).primary())
        } else {
            h_flex()
                .gap_2()
                .child(action("discard-lines", "Discard lines…", PatchTarget::Discard).ghost())
                .child(action("stage-lines", "Stage lines", PatchTarget::Stage).primary())
        };
        Some(bar.child(label).child(buttons))
    }

    fn confirm_discard_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .title("Discard the selected lines?")
                .description("These changes are removed from the file and can't be recovered.")
                .confirm()
                .ok_text("Discard")
                .ok_variant(ButtonVariant::Danger)
                .on_ok(move |_, window, cx| {
                    view.update(cx, |this, cx| {
                        this.apply_selection(PatchTarget::Discard, window, cx)
                    });
                    true
                })
        });
    }

    /// The Commit tool window, as in the reference app (docs/reference-app/tool-windows.md): the Commit and Stash
    /// tabs, the changes toolbar, the changes tree with checkboxes (a checked file is staged, a
    /// partly checked one partly staged), the unversioned files folded below, then the commit
    /// panel.
    pub(crate) fn commit_window(&mut self, cx: &mut Context<Self>) -> Div {
        if !matches!(self.repo, Some(Ok(_))) {
            return v_flex();
        }
        let tabs = self.commit_tabs(cx);
        let header = h_flex()
            .w_full()
            .h(px(41.))
            .flex_none()
            .pl_2()
            .pr_2()
            .gap_1()
            .border_b_1()
            .border_color(spork_workbench::Tokens::get(cx).separator)
            .map(|h| match tabs {
                Some(tabs) => h.child(tabs),
                None => h.child(div().pl_1().font_weight(FontWeight::BOLD).child("Commit")),
            })
            .child(div().flex_1())
            .child(
                spork_workbench::shell::tool_window_actions()
                    .child(self.commit_options_button(cx))
                    .child(self.hide_left_button(crate::LeftTool::Commit, cx)),
            );
        if self.commit_tab() != crate::stash_view::CommitTab::Commit {
            return v_flex().size_full().child(header).child(self.saved_tab(cx));
        }
        let (rows, toolbar) = match self.commit_ui.staging {
            true => (self.staging_rows(cx), self.staging_toolbar(cx)),
            false => (self.changes_rows(cx), self.commit_changes_toolbar(cx)),
        };
        let tree = v_flex()
            .id("commit-tree")
            .role(Role::Tree)
            .aria_label("Changes Tree")
            .track_focus(&self.commit_ui.focus)
            .key_context("ChangesView")
            .size_full()
            .pt_0p5()
            .overflow_y_scrollbar()
            .children(rows);
        // a right-click below the rows: the menu of the selection, or of nothing selected
        let menu = tree_menu(cx.entity(), self.commit_ui.staging);
        let tree = div()
            .id("commit-tree-area")
            .size_full()
            .child(tree)
            .context_menu(move |m, window, cx| menu(m, window, cx));
        let commit = self.commit_box(cx);
        // the split takes what the header and toolbar leave
        v_flex()
            .size_full()
            .overflow_hidden()
            .child(header)
            .child(toolbar)
            .child(
                div().flex_1().min_h_0().child(
                    v_resizable("commit-split")
                        .child(resizable_panel().child(tree))
                        .child(
                            resizable_panel()
                                .size(px(210.))
                                .child(div().size_full().overflow_hidden().child(commit)),
                        ),
                ),
            )
    }

    /// The paths the changes tree's actions work on: the marked files, else the selected one.
    fn selected_paths(&self) -> Vec<String> {
        match &self.change_selected {
            Some(sel) => self.acting_on(&sel.path, sel.staged),
            None => Vec::new(),
        }
    }

    /// The entries of `paths`, as `git status` reports them.
    fn entries_of(&self, paths: &[String]) -> Vec<StatusEntry> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        repo.status
            .iter()
            .filter(|e| paths.contains(&e.path))
            .cloned()
            .collect()
    }

    /// The file shown in the main area (a file's history or blame), the reference app's opened editor.
    fn opened_file(&self) -> Option<String> {
        let file = self
            .file
            .as_ref()
            .filter(|_| self.view == crate::View::File)?;
        Some(file.path().to_owned())
    }

    /// The changes toolbar (ChangesViewToolbar.Shared): the reference app's buttons in its order; the
    /// commit actions it also holds show in the commit panel instead, as there.
    fn commit_changes_toolbar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let has_changes = !self.tracked_changes().is_empty();
        let selected = self.change_selected.is_some();
        let opened = self.opened_file();
        let button = |id: &'static str, icon: &str, label: &'static str| {
            spork_workbench::ActionButton::new(id, icon.to_owned())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(label)
        };
        let separator = div()
            .w(px(1.))
            .h(px(16.))
            .mx(px(3.))
            .bg(spork_workbench::shell::toolbar_border(cx));
        let view = cx.entity();
        let (directory, repository, ignored) = (
            self.commit_ui.group_by_directory,
            self.commit_ui.group_by_repository,
            self.show_ignored,
        );
        h_flex()
            .id("changes-toolbar")
            .role(Role::Toolbar)
            .aria_label("ChangesViewToolbar")
            .h(px(32.))
            .flex_none()
            .px_2()
            .gap_0p5()
            .child(
                button("changes-refresh", "general/refresh", "Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                button("changes-rollback", "vcs/revert", "Rollback…")
                    .disabled(!has_changes)
                    .on_click(cx.listener(|this, _, window, cx| this.rollback(window, cx))),
            )
            .child(
                button("changes-diff", "vcs/diff", "Show Diff")
                    .disabled(!selected)
                    .on_click(cx.listener(|this, _, _, cx| this.show_selected_diff(cx))),
            )
            .child(
                button("changes-shelve", "vcs/shelveSilent", "Shelve Silently")
                    .disabled(self.selected_changes().is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.shelve(None, window, cx))),
            )
            .child(
                button("changes-view-options", "general/show", "View Options")
                    .popup_marker()
                    .dropdown_menu(move |menu, window, cx| {
                        let group_view = view.clone();
                        menu.submenu("Group By", window, cx, move |menu, _, _| {
                            menu.item(view_option(&group_view, ViewOption::Directory, directory))
                                .item(view_option(&group_view, ViewOption::Repository, repository))
                        })
                        .separator()
                        .item(view_option(
                            &view,
                            ViewOption::Ignored,
                            ignored,
                        ))
                    }),
            )
            .child(separator)
            .when_some(opened, |bar, path| {
                bar.child(
                    button(
                        "changes-locate",
                        "general/locate",
                        "Select Opened File in Changes View",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.select_in_changes(&path, cx))),
                )
            })
            .child(
                button("changes-expand", "general/expandAll", "Expand All")
                    .on_click(cx.listener(|this, _, _, cx| this.expand_changes(true, cx))),
            )
            .child(
                button("changes-collapse", "general/collapseAll", "Collapse All")
                    .on_click(cx.listener(|this, _, _, cx| this.expand_changes(false, cx))),
            )
    }

    /// Turns a View Options choice on or off.
    fn toggle_view_option(&mut self, option: ViewOption, cx: &mut Context<Self>) {
        let ui = &mut self.commit_ui;
        match option {
            ViewOption::Directory => ui.group_by_directory = !ui.group_by_directory,
            // shortcut: one repository a window, so its node never shows (IntelliJ groups only
            // when there are several); show it once submodules are listed as repositories
            ViewOption::Repository => ui.group_by_repository = !ui.group_by_repository,
            ViewOption::Ignored => {
                self.show_ignored = !self.show_ignored;
                return self.reload(cx);
            }
        }
        cx.notify();
    }

    /// Expand All / Collapse All of the changes tree.
    fn expand_changes(&mut self, open: bool, cx: &mut Context<Self>) {
        self.commit_ui.changes_open = open;
        self.commit_ui.ignored_open = open;
        self.unversioned_open = open;
        self.amend_open = open;
        self.commit_ui.closed_dirs.clear();
        cx.notify();
    }

    /// Selects `path` in the changes tree, opening the nodes above it.
    fn select_in_changes(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.entries_of(&[path.to_owned()]).into_iter().next() else {
            return;
        };
        let staged = entry.has_staged() && !entry.has_unstaged();
        match entry.untracked() {
            true => self.unversioned_open = true,
            false => self.commit_ui.changes_open = true,
        }
        self.commit_ui
            .closed_dirs
            .retain(|dir| !path.starts_with(dir.as_str()));
        self.change_marked.clear();
        self.select_change(
            Some(ChangeSelection {
                path: path.to_owned(),
                staged,
            }),
            cx,
        );
    }

    /// Show Diff: the selected change's diff in the main area.
    fn show_selected_diff(&mut self, cx: &mut Context<Self>) {
        if self.change_selected.is_some() {
            self.open_diff_view(crate::View::Changes, cx);
            cx.notify();
        }
    }

    /// The changes Rollback puts back: the tracked ones (unversioned files aren't changes).
    fn tracked_changes(&self) -> Vec<StatusEntry> {
        match &self.repo {
            Some(Ok(repo)) => repo
                .status
                .iter()
                .filter(|e| !e.untracked())
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Rollback…: the reference app's Rollback Changes dialog, the selected changes checked.
    pub(crate) fn rollback(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tracked = self.tracked_changes();
        if tracked.is_empty() {
            return;
        }
        let selected = self.selected_paths();
        self.commit_ui.rollback = ChangesDialog {
            included: tracked
                .iter()
                .filter(|e| selected.contains(&e.path))
                .map(|e| e.path.clone())
                .collect(),
            by_directory: true,
            open: true,
            ..Default::default()
        };
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let (content, footer) = view.update(cx, |this, cx| this.rollback_dialog(cx));
            dialog
                .title("Rollback Changes")
                .w(px(480.))
                .child(content)
                .footer(footer)
        });
    }

    /// The Rollback Changes dialog's body (its toolbar, the changes, Delete local copies of added
    /// files) and its Close and Rollback buttons.
    fn rollback_dialog(&mut self, cx: &mut Context<Self>) -> (Stateful<Div>, Div) {
        let entries = self.tracked_changes();
        let toolbar = self.dialog_toolbar(|ui| &mut ui.rollback, &entries, false, cx);
        let rows = self.dialog_tree_rows(&entries, |ui| &mut ui.rollback, true, cx);
        let state = &self.commit_ui.rollback;
        let border = cx.theme().border;
        let included = |e: &StatusEntry| state.included.contains(&e.path);
        let added = entries
            .iter()
            .any(|e| included(e) && matches!(e.index, 'A' | 'C' | 'R'));
        let delete_added = state.delete_added;
        let content = v_flex()
            .id("rollback-dialog")
            .role(Role::Group)
            .aria_label("Rollback Changes")
            .gap_2()
            .child(toolbar)
            .child(
                v_flex()
                    .id("rollback-tree")
                    .role(Role::Tree)
                    .h(px(300.))
                    .py_0p5()
                    .border_1()
                    .border_color(border)
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .child(
                spork_workbench::controls::labeled_check(
                    "rollback-delete-added",
                    "Delete local copies of added files",
                    delete_added,
                    cx,
                )
                .role(Role::CheckBox)
                .aria_label("Delete local copies of added files")
                .aria_toggled(if delete_added {
                    Toggled::True
                } else {
                    Toggled::False
                })
                .when(!added, |check| check.opacity(0.5).cursor_default())
                .when(added, |check| {
                    check.on_click(cx.listener(|this, _, _, cx| {
                        let state = &mut this.commit_ui.rollback;
                        state.delete_added = !state.delete_added;
                        cx.notify();
                    }))
                })
                .map(|check| spork_workbench::controls::A11yDisabled::new(check, !added)),
            );
        let none = state.included.is_empty();
        let footer = h_flex()
            .w_full()
            .justify_end()
            .gap_2()
            .child(
                spork_workbench::Button::new("rollback-close", "Close")
                    .role(Role::Button)
                    .aria_label("Close")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                spork_workbench::Button::new("rollback-ok", "Rollback")
                    .role(Role::Button)
                    .aria_label("Rollback")
                    .primary(!none)
                    .disabled(none)
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.close_dialog(cx);
                        let state = &this.commit_ui.rollback;
                        let delete_added = state.delete_added;
                        let entries: Vec<StatusEntry> = this
                            .tracked_changes()
                            .into_iter()
                            .filter(|e| state.included.contains(&e.path))
                            .collect();
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::rollback(root, &entries, delete_added),
                            |this, (), _, _| this.change_marked.clear(),
                        )
                    })),
            );
        (content, footer)
    }

    /// The rows of a dialog's tree of `entries`, checked and folded as `tree` (that dialog's state)
    /// says: under a Changes node with `changes_node`, under their directories when grouped so.
    /// Rollback's selection stays in the dialog; the commit dialogs show the selected one's diff.
    fn dialog_tree_rows(
        &mut self,
        entries: &[StatusEntry],
        tree: fn(&mut CommitUi) -> &mut ChangesDialog,
        changes_node: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let state = tree(&mut self.commit_ui).clone();
        let theme = cx.theme();
        let (muted, list_active) = (theme.muted_foreground, theme.list_active);
        let included = |e: &StatusEntry| state.included.contains(&e.path);
        let checked = |list: &[&StatusEntry]| match list.iter().filter(|e| included(e)).count() {
            0 => Some(false),
            n if n == list.len() => Some(true),
            _ => None,
        };
        let check = |id: ElementId,
                     label: SharedString,
                     on: Option<bool>,
                     paths: Vec<String>,
                     cx: &mut Context<Self>| {
            spork_workbench::controls::check_box(id, on, cx)
                .role(Role::CheckBox)
                .aria_label(label)
                .aria_toggled(match on {
                    Some(true) => Toggled::True,
                    Some(false) => Toggled::False,
                    None => Toggled::Mixed,
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let included = &mut tree(&mut this.commit_ui).included;
                    included.retain(|p| !paths.contains(p));
                    if on != Some(true) {
                        included.extend(paths.iter().cloned());
                    }
                    cx.notify();
                }))
        };
        let row = |id: ElementId, depth: f32| {
            h_flex()
                .id(id)
                .role(Role::TreeItem)
                .h(ROW_HEIGHT)
                .pl(px(6. + 18. * depth))
                .pr_2()
                .gap_1p5()
                .rounded(px(4.))
        };
        let chevron = |open: bool| {
            div().flex_none().text_color(muted).child(
                spork_workbench::icon(match open {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                })
                .size(px(16.)),
            )
        };
        let all: Vec<&StatusEntry> = entries.iter().collect();
        let paths = |list: &[&StatusEntry]| list.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
        let files_label = |n: usize| match n {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        let id = |name: &str, ix: usize| ElementId::NamedInteger(name.to_owned().into(), ix as u64);
        let preview = !changes_node;
        let mut rows: Vec<AnyElement> = Vec::new();
        if changes_node {
            rows.push(
                row("dialog-changes".into(), 0.)
                    .aria_label("Changes")
                    .aria_expanded(state.open)
                    .cursor_pointer()
                    .child(chevron(state.open))
                    .child(check(
                        "dialog-changes-check".into(),
                        "Changes".into(),
                        checked(&all),
                        paths(&all),
                        cx,
                    ))
                    .child(div().font_weight(FontWeight::BOLD).child("Changes"))
                    .child(div().text_color(muted).child(files_label(all.len())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let state = tree(&mut this.commit_ui);
                        state.open = !state.open;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let file_row = |ix: usize, depth: f32, e: &StatusEntry, cx: &mut Context<Self>| {
            let staged_view = e.has_staged() && !e.has_unstaged();
            let (_, color) = status_letter(e, staged_view, cx);
            let name = e.path.rsplit('/').next().unwrap_or(&e.path).to_owned();
            let selected = state.selected.as_deref() == Some(e.path.as_str());
            let path = e.path.clone();
            row(id("dialog-file", ix), depth)
                .aria_label(name.clone())
                .aria_selected(selected)
                .when(selected, |r| r.bg(list_active))
                .child(check(
                    id("dialog-check", ix),
                    name.clone().into(),
                    Some(included(e)),
                    vec![e.path.clone()],
                    cx,
                ))
                .child(spork_workbench::image(
                    spork_workbench::file_icon(&name),
                    cx,
                ))
                .child(div().text_color(color).child(name))
                .on_click(cx.listener(move |this, _, _, cx| {
                    tree(&mut this.commit_ui).selected = Some(path.clone());
                    if preview {
                        let sel = ChangeSelection {
                            path: path.clone(),
                            staged: staged_view,
                        };
                        this.select_change(Some(sel), cx);
                    }
                    cx.notify();
                }))
                .into_any_element()
        };
        let top = if changes_node { 1. } else { 0. };
        if state.open {
            if state.by_directory {
                let mut dirs: Vec<&str> = all.iter().map(|e| split_dir(&e.path).0).collect();
                dirs.sort();
                dirs.dedup();
                for (dir_ix, dir) in dirs.into_iter().enumerate() {
                    let inside: Vec<&StatusEntry> = all
                        .iter()
                        .copied()
                        .filter(|e| split_dir(&e.path).0 == dir)
                        .collect();
                    let open = !state.closed_dirs.contains(dir);
                    // the repository's own folder has no name, as IntelliJ's
                    let label = match dir {
                        "" => files_label(inside.len()),
                        dir => dir.to_owned(),
                    };
                    let key = dir.to_owned();
                    rows.push(
                        row(id("dialog-dir", dir_ix), top)
                            .aria_label(label.clone())
                            .aria_expanded(open)
                            .cursor_pointer()
                            .child(chevron(open))
                            .child(check(
                                id("dialog-dir-check", dir_ix),
                                label.into(),
                                checked(&inside),
                                paths(&inside),
                                cx,
                            ))
                            .child(spork_workbench::image("nodes/folder", cx))
                            .when(!dir.is_empty(), |r| r.child(dir.to_owned()))
                            .child(div().text_color(muted).child(files_label(inside.len())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let closed = &mut tree(&mut this.commit_ui).closed_dirs;
                                if !closed.remove(&key) {
                                    closed.insert(key.clone());
                                }
                                cx.notify();
                            }))
                            .into_any_element(),
                    );
                    if open {
                        for e in inside {
                            let ix = all
                                .iter()
                                .position(|a| a.path == e.path)
                                .unwrap_or_default();
                            rows.push(file_row(ix, top + 1., e, cx));
                        }
                    }
                }
            } else {
                for (ix, e) in all.iter().enumerate() {
                    rows.push(file_row(ix, top, e, cx));
                }
            }
        }
        rows
    }

    /// A dialog's tree toolbar: Show Diff (with `commit`, the commit dialogs', Rollback… and
    /// Refresh), View Options, Expand All and Collapse All; theirs then ends with the Changelist.
    fn dialog_toolbar(
        &mut self,
        tree: fn(&mut CommitUi) -> &mut ChangesDialog,
        entries: &[StatusEntry],
        commit: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let state = tree(&mut self.commit_ui).clone();
        let dirs: Vec<String> = entries
            .iter()
            .map(|e| split_dir(&e.path).0.to_owned())
            .collect();
        let button = |id: &'static str, icon: &str, label: &'static str| {
            spork_workbench::ActionButton::new(id, icon.to_owned())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(label)
        };
        let view = cx.entity();
        let by_directory = state.by_directory;
        let separator = div()
            .w(px(1.))
            .h(px(16.))
            .mx(px(3.))
            .bg(spork_workbench::shell::toolbar_border(cx));
        h_flex()
            .id("dialog-toolbar")
            .role(Role::Toolbar)
            .aria_label("Dialog Changes Toolbar")
            .h(px(32.))
            .gap_0p5()
            .child(
                button("dialog-diff", "vcs/diff", "Show Diff")
                    .disabled(state.selected.is_none())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(path) = tree(&mut this.commit_ui).selected.clone() {
                            this.select_in_changes(&path, cx);
                            this.show_selected_diff(cx)
                        }
                    })),
            )
            .when(commit, |bar| {
                bar.child(
                    button("dialog-rollback", "vcs/revert", "Rollback…")
                        .disabled(state.selected.is_none())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(path) = tree(&mut this.commit_ui).selected.clone() {
                                this.select_in_changes(&path, cx);
                                this.rollback(window, cx)
                            }
                        })),
                )
                .child(
                    button("dialog-refresh", "general/refresh", "Refresh")
                        .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
                )
            })
            .child(separator)
            .child(
                button("dialog-view-options", "general/show", "View Options")
                    .popup_marker()
                    .dropdown_menu(move |menu, window, cx| {
                        let view = view.clone();
                        menu.submenu("Group By", window, cx, move |menu, _, _| {
                            let view = view.clone();
                            menu.item(
                                PopupMenuItem::new("Directory")
                                    .checked(by_directory)
                                    .action(Box::new(GroupByDirectory))
                                    .on_click(move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            let state = tree(&mut this.commit_ui);
                                            state.by_directory = !state.by_directory;
                                            cx.notify();
                                        })
                                    }),
                            )
                        })
                    }),
            )
            .child(div().flex_1())
            .child(
                button("dialog-expand", "general/expandAll", "Expand All").on_click(cx.listener(
                    move |this, _, _, cx| {
                        let state = tree(&mut this.commit_ui);
                        state.open = true;
                        state.closed_dirs.clear();
                        cx.notify();
                    },
                )),
            )
            .child(
                button("dialog-collapse", "general/collapseAll", "Collapse All").on_click(
                    cx.listener(move |this, _, _, cx| {
                        let state = tree(&mut this.commit_ui);
                        // the Rollback dialog's Changes node, the commit dialogs' folders
                        match commit {
                            true => state.closed_dirs.extend(dirs.iter().cloned()),
                            false => state.open = false,
                        }
                        cx.notify();
                    }),
                ),
            )
            .when(commit, |bar| {
                // shortcut: Spork has the one changelist, so the choice stays greyed on it, as
                // the reference app's with one; offer the others once Spork has changelists
                bar.child(crate::git_menu::form_label("Changelist:").ml_2().mr_1())
                    .child(
                        spork_workbench::controls::ComboBox::new("dialog-changelist", "Changes")
                            .disabled(true),
                    )
            })
    }

    /// The files a commit dialog lists: the tracked changes, and for a patch the selected
    /// unversioned files too (a shelf takes tracked files only).
    fn executor_entries(&self, executor: Executor, selected: &[String]) -> Vec<StatusEntry> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        repo.status
            .iter()
            .filter(|e| match e.untracked() {
                false => true,
                true => executor == Executor::Patch && !e.ignored() && selected.contains(&e.path),
            })
            .cloned()
            .collect()
    }

    /// Shelve Changes… or Create Patch from Local Changes…: IntelliJ's commit dialog for them,
    /// the selected changes checked (all of them when none is selected) and the first one's diff
    /// shown.
    fn open_executor(&mut self, executor: Executor, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_paths();
        let entries = self.executor_entries(executor, &selected);
        let chosen: Vec<String> = entries
            .iter()
            .filter(|e| selected.contains(&e.path))
            .map(|e| e.path.clone())
            .collect();
        let included = match chosen.is_empty() {
            true => entries.iter().map(|e| e.path.clone()).collect(),
            false => chosen,
        };
        let Some(first) = included.first().cloned() else {
            return;
        };
        let message = cx.new(|cx| TextareaState::new(window, cx));
        self.commit_ui.executor = ExecutorDialog {
            executor,
            tree: ChangesDialog {
                included,
                selected: Some(first.clone()),
                by_directory: true,
                open: true,
                ..Default::default()
            },
            listed: entries.into_iter().map(|e| e.path).collect(),
            message: Some(message),
            ..Default::default()
        };
        self.select_in_changes(&first, cx);
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let (content, footer) = view.update(cx, |this, cx| this.executor_dialog(cx));
            dialog
                .title(executor.title())
                .w(px(800.))
                .child(content)
                .footer(footer)
        });
    }

    /// The Shelve Changes or Create Patch dialog's body (the changes, how many of each kind are
    /// checked, the Commit Message, the selected change's Diff) and its Cancel and Shelve Changes
    /// or Create Patch… buttons.
    fn executor_dialog(&mut self, cx: &mut Context<Self>) -> (Stateful<Div>, Div) {
        let entries = self.entries_of(&self.commit_ui.executor.listed);
        let toolbar = self.dialog_toolbar(|ui| &mut ui.executor.tree, &entries, true, cx);
        let rows = self.dialog_tree_rows(&entries, |ui| &mut ui.executor.tree, false, cx);
        let diff_closed = self.commit_ui.executor.diff_closed;
        let diff = (!diff_closed).then(|| self.change_diff_view(cx));
        let dialog = &self.commit_ui.executor;
        let executor = dialog.executor;
        let theme = cx.theme();
        let (border, muted, mono) = (
            theme.border,
            theme.muted_foreground,
            theme.mono_font_family.clone(),
        );
        let included: Vec<&StatusEntry> = entries
            .iter()
            .filter(|e| dialog.tree.included.contains(&e.path))
            .collect();
        // IntelliJ's legend under the tree: "1 modified", each count in its status color
        use spork_workbench::{FileStatus, file_status_color};
        let kinds = [
            ("added", FileStatus::Added),
            ("modified", FileStatus::Modified),
            ("deleted", FileStatus::Deleted),
        ];
        let kind = |e: &StatusEntry| match (e.untracked(), e.index, e.worktree) {
            (true, _, _) | (_, 'A' | 'C', _) | (_, '.', 'A') => "added",
            (_, 'D', _) | (_, '.', 'D') => "deleted",
            _ => "modified",
        };
        let legend = kinds.into_iter().filter_map(|(name, status)| {
            let n = included.iter().filter(|e| kind(e) == name).count();
            (n > 0).then(|| {
                crate::git_menu::form_label(format!("{n} {name}"))
                    .text_color(file_status_color(status, cx))
            })
        });
        let line = || div().flex_1().h(px(1.)).bg(border);
        let message = dialog.message.clone();
        let content = v_flex()
            .id("executor-dialog")
            .role(Role::Group)
            .aria_label(executor.title())
            .gap_2()
            .child(toolbar)
            .child(
                v_flex()
                    .id("executor-tree")
                    .role(Role::Tree)
                    .h(px(160.))
                    .py_0p5()
                    .border_1()
                    .border_color(border)
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .child(h_flex().gap_3().children(legend))
            .child(
                h_flex()
                    .gap_2()
                    .child(crate::git_menu::form_label("Commit Message"))
                    .child(line())
                    .child(
                        spork_workbench::ActionButton::new("executor-history", "general/history")
                            .role(Role::Button)
                            .aria_label("Commit Message History")
                            .tooltip("Commit Message History")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.message_history_into(true, window, cx)
                            })),
                    ),
            )
            .child(
                v_flex()
                    .h(px(120.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .font_family(mono)
                    .children(message.map(|message| {
                        Textarea::new(&message)
                            .appearance(false)
                            .px(px(6.))
                            .flex_1()
                    })),
            )
            .child(
                h_flex()
                    .id("executor-diff-toggle")
                    .role(Role::Button)
                    .aria_label("Diff")
                    .aria_expanded(!diff_closed)
                    .gap_1()
                    .cursor_pointer()
                    .child(
                        div().text_color(muted).child(
                            spork_workbench::icon(match diff_closed {
                                true => "general/chevronRight",
                                false => "general/chevronDown",
                            })
                            .size(px(16.)),
                        ),
                    )
                    .child("Diff")
                    .child(line())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let dialog = &mut this.commit_ui.executor;
                        dialog.diff_closed = !dialog.diff_closed;
                        cx.notify();
                    })),
            )
            .children(diff.map(|diff| div().h(px(260.)).overflow_hidden().child(diff)));
        let none = dialog.tree.included.is_empty();
        let footer = h_flex()
            .w_full()
            .justify_end()
            .gap_2()
            .child(
                spork_workbench::Button::new("executor-cancel", "Cancel")
                    .role(Role::Button)
                    .aria_label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                spork_workbench::Button::new("executor-ok", executor.button())
                    .role(Role::Button)
                    .aria_label(executor.button())
                    .primary(!none)
                    .disabled(none)
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.close_dialog(cx);
                        match this.commit_ui.executor.executor {
                            Executor::Shelve => this.shelve_included(window, cx),
                            Executor::Patch => this.patch_settings(window, cx),
                        }
                    })),
            );
        (content, footer)
    }

    /// The commit dialog's message, trimmed.
    fn executor_message(&self, cx: &App) -> String {
        self.commit_ui
            .executor
            .message
            .as_ref()
            .map(|message| message.read(cx).value().trim().to_owned())
            .unwrap_or_default()
    }

    /// Shelve Changes: the checked changes to the shelf, described by the message (by their
    /// changelist's name without one), rolled back.
    fn shelve_included(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.entries_of(&self.commit_ui.executor.tree.included);
        let description = match self.executor_message(cx) {
            message if message.is_empty() => "Changes".to_owned(),
            message => message,
        };
        self.run_op(
            window,
            cx,
            move |root| shelve_paths(root, &description, &entries),
            |this, _, _, cx| {
                this.change_marked.clear();
                cx.notify()
            },
        );
    }

    /// Create Patch…'s Patch File Settings: where the patch goes (a file named after the
    /// message, else after the changelist, in the repository; or the clipboard), its base path,
    /// reversed or not.
    fn patch_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        let message = self.executor_message(cx);
        let name: String = match message.lines().next().unwrap_or_default().trim() {
            "" => "Changes".to_owned(),
            subject => subject
                .chars()
                .map(|c| match c.is_alphanumeric() || " -_.".contains(c) {
                    true => c,
                    false => '_',
                })
                .collect(),
        };
        let file = root.join(format!("{name}.patch")).display().to_string();
        let base = root.display().to_string();
        let dialog = &mut self.commit_ui.executor;
        dialog.patch_file = Some(cx.new(|cx| InputState::new(window, cx).default_value(file)));
        dialog.base_path = Some(cx.new(|cx| InputState::new(window, cx).default_value(base)));
        dialog.to_clipboard = false;
        dialog.reverse = false;
        dialog.patch_error = None;
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let (content, footer) = view.update(cx, |this, cx| this.patch_settings_dialog(cx));
            dialog
                .title("Patch File Settings")
                .w(px(560.))
                .child(content)
                .footer(footer)
        });
    }

    /// Patch File Settings' body (Save Patch to: To file or To clipboard, Base path, Reverse
    /// patch, Encoding) and its Cancel and OK buttons.
    fn patch_settings_dialog(&mut self, cx: &mut Context<Self>) -> (Stateful<Div>, Div) {
        let dialog = &self.commit_ui.executor;
        let (to_clipboard, reverse) = (dialog.to_clipboard, dialog.reverse);
        let label_width = px(90.);
        let field = |label: &'static str, input: Option<&Entity<InputState>>, disabled: bool| {
            h_flex()
                .gap_2()
                .child(crate::git_menu::form_label(label).w(label_width))
                .children(input.map(|input| {
                    spork_workbench::TextField::new(input)
                        .aria_label(label)
                        .disabled(disabled)
                        .flex_1()
                }))
        };
        let radio = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
            spork_workbench::controls::labeled_radio(id, label, on, cx).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.commit_ui.executor.to_clipboard = label == "To clipboard";
                    cx.notify();
                },
            ))
        };
        let error = spork_workbench::Tokens::get(cx).text_error;
        let content = v_flex()
            .id("patch-settings")
            .role(Role::Group)
            .aria_label("Patch File Settings")
            .gap_2()
            .child(crate::git_menu::form_label("Save Patch to"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().w(label_width).flex_none().child(radio(
                        "patch-to-file",
                        "To file:",
                        !to_clipboard,
                        cx,
                    )))
                    .children(dialog.patch_file.as_ref().map(|input| {
                        spork_workbench::TextField::new(input)
                            .aria_label("To file:")
                            .disabled(to_clipboard)
                            .flex_1()
                    })),
            )
            .child(radio(
                "patch-to-clipboard",
                "To clipboard",
                to_clipboard,
                cx,
            ))
            .child(field("Base path:", dialog.base_path.as_ref(), false))
            .child(
                spork_workbench::controls::labeled_check(
                    "patch-reverse",
                    "Reverse patch",
                    reverse,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    let dialog = &mut this.commit_ui.executor;
                    dialog.reverse = !dialog.reverse;
                    cx.notify();
                })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(crate::git_menu::form_label("Encoding:").w(label_width))
                    // shortcut: git writes the patch as the files are, UTF-8 for text; offer
                    // other encodings once Spork converts text
                    .child(
                        spork_workbench::controls::ComboBox::new("patch-encoding", "UTF-8")
                            .disabled(true),
                    ),
            )
            .children(
                dialog
                    .patch_error
                    .clone()
                    .map(|message| crate::git_menu::form_label(message).text_color(error)),
            );
        let footer = h_flex()
            .w_full()
            .justify_end()
            .gap_2()
            .child(
                spork_workbench::Button::new("patch-cancel", "Cancel")
                    .role(Role::Button)
                    .aria_label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                spork_workbench::Button::new("patch-ok", "OK")
                    .role(Role::Button)
                    .aria_label("OK")
                    .primary(true)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.write_patch(false, window, cx)),
                    ),
            );
        (content, footer)
    }

    /// Patch File Settings' OK: the checked changes as a patch, to the clipboard or the file
    /// (asking before overwriting one), from the base path, which must hold the repository.
    fn write_patch(&mut self, overwrite: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let root = repo.root.clone();
        let dialog = &self.commit_ui.executor;
        let value = |input: &Option<Entity<InputState>>| {
            input
                .as_ref()
                .map(|input| input.read(cx).value().trim().to_owned())
                .unwrap_or_default()
        };
        let (file, base) = (value(&dialog.patch_file), value(&dialog.base_path));
        let (to_clipboard, reverse) = (dialog.to_clipboard, dialog.reverse);
        let prefix = match patch_prefix(&root, std::path::Path::new(&base)) {
            Ok(prefix) => prefix,
            Err(message) => {
                self.commit_ui.executor.patch_error = Some(message);
                return cx.notify();
            }
        };
        let file = std::path::PathBuf::from(file);
        if !to_clipboard && file.exists() && !overwrite {
            let view = cx.entity();
            let name = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let shown = file.display().to_string();
            return window.open_alert_dialog(cx, move |alert, _, _| {
                let view = view.clone();
                alert
                    .title("Create Patch")
                    .description(format!(
                        "File {name} ({shown}) already exists.\nDo you want to overwrite it?"
                    ))
                    .confirm()
                    .ok_text("Yes")
                    .cancel_text("No")
                    .on_ok(move |_, window, cx| {
                        view.update(cx, |this, cx| this.write_patch(true, window, cx));
                        true
                    })
            });
        }
        window.close_dialog(cx);
        let entries = self.entries_of(&self.commit_ui.executor.tree.included);
        let target = (!to_clipboard).then_some(file);
        self.run_op(
            window,
            cx,
            move |root| {
                let patch = spork_git::local_patch_for(root, &entries, reverse, &prefix)?;
                match &target {
                    Some(file) => {
                        std::fs::write(file, patch)
                            .map(|()| None)
                            .map_err(|e| GitError::Other {
                                code: None,
                                stderr: format!("Cannot write patch to specified file: {e}"),
                            })
                    }
                    None => Ok(Some(patch)),
                }
                .map(|patch| (patch, target))
            },
            |this, (patch, file), window, cx| match (patch, file) {
                (Some(patch), _) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(patch));
                    window.push_notification(Notification::info("Patch copied to clipboard"), cx)
                }
                (None, Some(file)) => this.patch_created(file, window, cx),
                (None, None) => {}
            },
        );
    }

    /// After a patch file is written, what Settings › Version Control › Confirmation's When patch
    /// is created says: ask whether to show it in Finder (Don't ask again keeps the answer), show
    /// it, or nothing.
    fn patch_created(
        &mut self,
        file: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        const KEY: &str = "confirmation.patchCreated";
        match setting(KEY).as_deref() {
            Some("Do nothing") => return,
            Some("Show in Finder") => return cx.reveal_path(&file),
            _ => {}
        }
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let remember = std::rc::Rc::new(std::cell::Cell::new(false));
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let (shown, kept, file) = (remember.clone(), remember.clone(), file.clone());
            let checked = remember.get();
            let view = view.clone();
            alert
                .title("Create Patch")
                .description(
                    v_flex()
                        .gap_3()
                        .child(crate::git_menu::form_label(format!(
                            "Successfully created patch {name}"
                        )))
                        .child(
                            spork_workbench::controls::labeled_check(
                                "patch-dont-ask",
                                "Don't ask again",
                                checked,
                                cx,
                            )
                            .on_click(move |_, window, _| {
                                shown.set(!shown.get());
                                window.refresh();
                            }),
                        ),
                )
                .confirm()
                .ok_text("Show in Finder")
                .cancel_text("Cancel")
                .on_ok({
                    let kept = kept.clone();
                    move |_, _, cx| {
                        if kept.get() {
                            crate::settings_window::Store::save_one(KEY, "Show in Finder");
                        }
                        cx.reveal_path(&file);
                        true
                    }
                })
                .on_cancel(move |_, _, cx| {
                    if kept.get() {
                        crate::settings_window::Store::save_one(KEY, "Do nothing");
                    }
                    view.update(cx, |_, cx| cx.notify());
                    true
                })
        });
    }

    /// The changes tree's rows: Changes, then Unversioned Files, each node folding; the files
    /// under their directories when grouped by directory.
    fn changes_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (list_hover, list_active) = (theme.list_hover, theme.list_active);
        let entries: Vec<StatusEntry> = repo
            .status
            .iter()
            .filter(|e| self.show_ignored || !e.ignored())
            .cloned()
            .collect();
        let (untracked, tracked): (Vec<&StatusEntry>, Vec<&StatusEntry>) =
            entries.iter().partition(|e| e.untracked());
        let (ignored, untracked): (Vec<&StatusEntry>, Vec<&StatusEntry>) =
            untracked.into_iter().partition(|e| e.ignored());
        // git's own reading of the entry ("." or " " both mean unchanged on that side)
        let state = |e: &StatusEntry| match (e.has_staged(), e.has_unstaged()) {
            (true, false) => Some(true),
            (true, true) => None,
            _ => Some(false),
        };
        let group_state = |list: &[&StatusEntry]| {
            let checked = list.iter().filter(|e| state(e) == Some(true)).count();
            match checked {
                0 if list.iter().all(|e| state(e) == Some(false)) => Some(false),
                n if n == list.len() => Some(true),
                _ => None,
            }
        };
        let root_name = repo
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let files_label = |n: usize| match n {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        let by_directory = self.commit_ui.group_by_directory;
        let row = |id: ElementId, depth: f32| {
            h_flex()
                .id(id)
                .role(Role::TreeItem)
                .h(ROW_HEIGHT)
                .mx_1()
                .pl(px(6. + 18. * depth))
                .pr_2()
                .gap_1p5()
                .rounded(px(4.))
        };
        let chevron = |open: bool| {
            div().flex_none().text_color(muted).child(
                spork_workbench::icon(match open {
                    true => "general/chevronDown",
                    false => "general/chevronRight",
                })
                .size(px(16.)),
            )
        };
        let check = |id: ElementId, label: SharedString, checked: Option<bool>, cx: &App| {
            spork_workbench::controls::check_box(id, checked, cx)
                .role(Role::CheckBox)
                .aria_label(label)
                .aria_toggled(match checked {
                    Some(true) => Toggled::True,
                    Some(false) => Toggled::False,
                    None => Toggled::Mixed,
                })
        };
        let file_row = |ix: usize,
                        depth: f32,
                        e: &StatusEntry,
                        this: &RepoView,
                        cx: &mut Context<Self>| {
            let staged_view = e.has_staged() && !e.has_unstaged();
            let (_, color) = status_letter(e, staged_view, cx);
            let (dir, name) = match e.path.rsplit_once('/') {
                Some((dir, name)) => (dir.to_owned(), name.to_owned()),
                None => (root_name.clone(), e.path.clone()),
            };
            let selected = this
                .change_selected
                .as_ref()
                .is_some_and(|s| s.path == e.path)
                || this.change_marked.contains(&e.path);
            let checked = state(e);
            let (path, toggle_path, menu_path) = (e.path.clone(), e.path.clone(), e.path.clone());
            let menu = file_menu(
                cx.entity(),
                e.path.clone(),
                e.untracked(),
                staged_view,
                e.conflicted,
            );
            row(("commit-file", ix).into(), depth)
                .aria_label(name.clone())
                .aria_selected(selected)
                .when(selected, |r| r.bg(list_active))
                .when(!selected, |r| r.hover(move |r| r.bg(list_hover)))
                .child(
                    check(
                        ("commit-check", ix).into(),
                        name.clone().into(),
                        checked,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        // checked: unstage it; otherwise stage all of it
                        let staged = checked == Some(true);
                        this.toggle_stage(vec![toggle_path.clone()], staged, window, cx)
                    })),
                )
                .child(spork_workbench::image(
                    spork_workbench::file_icon(&name),
                    cx,
                ))
                .child(div().flex_none().text_color(color).child(name))
                .when(!by_directory, |r| {
                    r.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(muted)
                            .child(dir),
                    )
                })
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    window.focus(&this.commit_ui.focus, cx);
                    let sel = ChangeSelection {
                        path: path.clone(),
                        staged: staged_view,
                    };
                    this.click_change(sel, event.modifiers(), cx);
                    if event.click_count() == 2 {
                        this.open_on_double_click(window, cx);
                    }
                    cx.notify();
                }))
                // the right-clicked file is the one the menu acts on, as in IntelliJ
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        window.focus(&this.commit_ui.focus, cx);
                        if this
                            .change_selected
                            .as_ref()
                            .is_none_or(|s| s.path != menu_path)
                            && !this.change_marked.contains(&menu_path)
                        {
                            this.change_marked.clear();
                            let sel = ChangeSelection {
                                path: menu_path.clone(),
                                staged: staged_view,
                            };
                            this.select_change(Some(sel), cx);
                        }
                    }),
                )
                .context_menu(move |m, window, cx| menu(m, window, cx))
                .into_any_element()
        };
        let group_row = |id: &'static str,
                         label: &'static str,
                         bold: bool,
                         list: &[&StatusEntry],
                         open: bool,
                         cx: &mut Context<Self>| {
            let paths: Vec<String> = list.iter().map(|e| e.path.clone()).collect();
            let checked = group_state(list);
            row(id.into(), 0.)
                .aria_label(label)
                .aria_expanded(open)
                .child(chevron(open))
                .child(
                    check(
                        SharedString::from(format!("{id}-check")).into(),
                        label.into(),
                        checked,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.toggle_stage(paths.clone(), checked == Some(true), window, cx)
                    })),
                )
                .child(
                    div()
                        .when(bold, |d| d.font_weight(FontWeight::BOLD))
                        .child(label),
                )
                .child(div().text_color(muted).child(files_label(list.len())))
        };
        // the files of a node, under a node per directory when grouped by directory
        let files = |node: &'static str,
                     list: &[&StatusEntry],
                     first: usize,
                     this: &RepoView,
                     rows: &mut Vec<AnyElement>,
                     cx: &mut Context<Self>| {
            if !by_directory {
                for (ix, e) in list.iter().enumerate() {
                    rows.push(file_row(first + ix, 1., e, this, cx));
                }
                return;
            }
            let mut dirs: Vec<&str> = list.iter().map(|e| split_dir(&e.path).0).collect();
            dirs.sort();
            dirs.dedup();
            for (dir_ix, dir) in dirs.into_iter().enumerate() {
                let inside: Vec<(usize, &&StatusEntry)> = list
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| split_dir(&e.path).0 == dir)
                    .collect();
                let key = format!("{node}:{dir}");
                let open = !this.commit_ui.closed_dirs.contains(&key);
                let label = if dir.is_empty() {
                    root_name.clone()
                } else {
                    dir.to_owned()
                };
                rows.push(
                    row((node, first + dir_ix).into(), 1.)
                        .aria_label(label.clone())
                        .aria_expanded(open)
                        .cursor_pointer()
                        .child(chevron(open))
                        .child(spork_workbench::image("nodes/folder", cx))
                        .child(div().flex_none().child(label))
                        .child(div().text_color(muted).child(files_label(inside.len())))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let closed = &mut this.commit_ui.closed_dirs;
                            if !closed.remove(&key) {
                                closed.insert(key.clone());
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                );
                if open {
                    for (ix, e) in inside {
                        rows.push(file_row(first + ix, 2., e, this, cx));
                    }
                }
            }
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        // nothing to commit: IntelliJ still shows the "Changes" node, selected
        if tracked.is_empty() && (untracked.is_empty() || self.amend_commit.is_some()) {
            let selected = spork_workbench::Tokens::get(cx).selection_inactive;
            rows.push(
                row("commit-changes".into(), 0.)
                    .aria_label("Changes")
                    .bg(selected)
                    .child(
                        div()
                            .pl(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .child("Changes"),
                    )
                    .into_any_element(),
            );
        }
        if !tracked.is_empty() {
            let open = self.commit_ui.changes_open;
            rows.push(
                group_row("commit-changes", "Changes", true, &tracked, open, cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.commit_ui.changes_open = !this.commit_ui.changes_open;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            // the changelist's conflicts first, under IntelliJ's Merge Conflicts node
            let (conflicts, changes): (Vec<&StatusEntry>, Vec<&StatusEntry>) =
                tracked.iter().copied().partition(|e| e.conflicted);
            if open && !conflicts.is_empty() {
                let color =
                    spork_workbench::file_status_color(spork_workbench::FileStatus::Conflicted, cx);
                rows.push(
                    row("commit-conflicts".into(), 1.)
                        .aria_label("Merge Conflicts")
                        .child(div().pl(px(20.)).text_color(color).child("Merge Conflicts"))
                        .child(div().text_color(muted).child(files_label(conflicts.len())))
                        .child(
                            spork_workbench::controls::link("commit-resolve", "Resolve", cx)
                                .role(Role::Link)
                                .aria_label("Resolve")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.resolve_conflicts_dialog(window, cx)
                                })),
                        )
                        .into_any_element(),
                );
                for (ix, e) in conflicts.iter().enumerate() {
                    rows.push(file_row(30_000 + ix, 2., e, self, cx));
                }
            }
            if open {
                files("changes", &changes, 0, self, &mut rows, cx);
            }
        }
        if !untracked.is_empty() {
            let open = self.unversioned_open;
            rows.push(
                group_row(
                    "commit-unversioned",
                    "Unversioned Files",
                    false,
                    &untracked,
                    open,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.unversioned_open = !this.unversioned_open;
                    cx.notify();
                }))
                .into_any_element(),
            );
            if open {
                files("unversioned", &untracked, 10_000, self, &mut rows, cx);
            }
        }
        // View Options › Ignored Files: their own node, folded, as IntelliJ's
        if !ignored.is_empty() {
            let open = self.commit_ui.ignored_open;
            rows.push(
                row("commit-ignored".into(), 0.)
                    .aria_label("Ignored Files")
                    .aria_expanded(open)
                    .cursor_pointer()
                    .child(chevron(open))
                    .child(div().child("Ignored Files"))
                    .child(div().text_color(muted).child(files_label(ignored.len())))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.commit_ui.ignored_open = !this.commit_ui.ignored_open;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            if open {
                files("ignored", &ignored, 20_000, self, &mut rows, cx);
            }
        }
        if let Some((subject, files)) = &self.amend_commit {
            let open = self.amend_open;
            rows.push(
                row("amend-commit".into(), 1.)
                    .aria_label(subject.clone())
                    .aria_expanded(open)
                    .cursor_pointer()
                    .child(chevron(open))
                    .child(spork_workbench::icon("vcs/commit").size(px(16.)))
                    .child(div().min_w_0().truncate().child(subject.clone()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.amend_open = !this.amend_open;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            if open {
                for (ix, (status, path)) in files.iter().enumerate() {
                    let status = match status {
                        'A' | 'C' => spork_workbench::FileStatus::Added,
                        'D' => spork_workbench::FileStatus::Deleted,
                        _ => spork_workbench::FileStatus::Modified,
                    };
                    let (dir, name) = match path.rsplit_once('/') {
                        Some((dir, name)) => (dir.to_owned(), name.to_owned()),
                        None => (root_name.clone(), path.clone()),
                    };
                    rows.push(
                        row(("amend-file", ix).into(), 2.)
                            .aria_label(name.clone())
                            .child(spork_workbench::image(
                                spork_workbench::file_icon(&name),
                                cx,
                            ))
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(spork_workbench::file_status_color(status, cx))
                                    .child(name),
                            )
                            .child(div().min_w_0().truncate().text_color(muted).child(dir))
                            .into_any_element(),
                    );
                }
            }
        }
        rows
    }

    /// The Commit tool window's actions, for its shortcuts, menus and Find Action.
    pub(crate) fn commit_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        el.on_action(cx.listener(|this, _: &Refresh, _, cx| this.reload(cx)))
            .on_action(cx.listener(|this, _: &Rollback, window, cx| this.rollback(window, cx)))
            .on_action(cx.listener(|this, _: &ShowDiff, _, cx| this.show_selected_diff(cx)))
            .on_action(cx.listener(|this, _: &SelectOpenedFile, _, cx| {
                if let Some(path) = this.opened_file() {
                    this.select_in_changes(&path, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ExpandAll, _, cx| this.expand_changes(true, cx)))
            .on_action(cx.listener(|this, _: &CollapseAll, _, cx| this.expand_changes(false, cx)))
            .on_action(cx.listener(|this, _: &ShowIgnored, _, cx| {
                this.toggle_view_option(ViewOption::Ignored, cx)
            }))
            .on_action(cx.listener(|this, _: &GroupByDirectory, _, cx| {
                this.toggle_view_option(ViewOption::Directory, cx)
            }))
            .on_action(cx.listener(|this, _: &GroupByRepository, _, cx| {
                this.toggle_view_option(ViewOption::Repository, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleAmend, window, cx| {
                this.set_amend(!this.amend, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MessageHistory, window, cx| {
                this.message_history_into(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowCommitOptions, window, cx| {
                this.show_commit_options(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CommitAndPush, window, cx| {
                if !this.busy {
                    this.commit(true, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CommitFile, window, cx| this.commit_file(window, cx)))
            .on_action(cx.listener(|this, _: &AddToVcs, window, cx| this.add_to_vcs(window, cx)))
            .on_action(cx.listener(|this, _: &JumpToSource, window, cx| {
                let file = this.selected_file_on_disk();
                if file.is_some() {
                    this.open_in_editor(file, window, cx)
                }
            }))
            .on_action(
                cx.listener(|this, _: &DeleteFile, window, cx| this.confirm_delete(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CopyPatch, window, cx| this.copy_patch(window, cx)))
            .on_action(
                cx.listener(|this, _: &CreatePatch, window, cx| this.create_patch(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShelveSilently, window, cx| this.shelve(None, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SaveToShelf, window, cx| this.save_to_shelf(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Shelve, window, cx| this.shelve_dialog(window, cx)))
            .on_action(cx.listener(|this, _: &Annotate, _, cx| {
                if let Some(path) = this.selected_paths().into_iter().next() {
                    this.open_file(path, true, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &OpenFile, _, cx| {
                if let Some(file) = this.selected_file_on_disk() {
                    cx.open_with_system(&file)
                }
            }))
            .on_action(cx.listener(|this, _: &RevealFile, _, cx| {
                if let Some(file) = this.selected_file_on_disk() {
                    cx.reveal_path(&file)
                }
            }))
            .on_action(cx.listener(|this, _: &ExternalDiff, window, cx| {
                let Some(sel) = this.change_selected.clone() else {
                    return;
                };
                this.run_op(
                    window,
                    cx,
                    move |root| spork_git::file::external_diff(root, &sel.path, sel.staged),
                    |_, (), _, _| {},
                )
            }))
            .on_action(cx.listener(|this, _: &StashFiles, window, cx| {
                let paths: Vec<String> = this
                    .selected_changes()
                    .into_iter()
                    .map(|e| e.path)
                    .collect();
                if paths.is_empty() {
                    return;
                }
                this.input_dialog(
                    "Stash these files".into(),
                    format!(
                        "Stashes the staged and unstaged changes of {}. Message (optional):",
                        paths.join(", ")
                    ),
                    String::new(),
                    move |root, message| {
                        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                        spork_git::stash::save_paths(root, &message, &paths)
                            .map(|()| "Files stashed".to_owned())
                    },
                    window,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &StageAllTracked, window, cx| {
                this.run_op(window, cx, spork_git::stage_tracked, |_, (), _, _| {})
            }))
            .on_action(cx.listener(|this, _: &StageAll, window, cx| {
                this.run_op(window, cx, spork_git::stage_all, |_, (), _, _| {})
            }))
            .on_action(cx.listener(|this, _: &StashSilently, window, cx| {
                this.run_op(
                    window,
                    cx,
                    |root| spork_git::stash_save(root, "", false),
                    |_, (), window, cx| {
                        window.push_notification(Notification::success("Changes stashed"), cx)
                    },
                )
            }))
            .on_action(cx.listener(|this, _: &ToggleCommitAll, _, cx| {
                this.commit_ui.commit_all = !this.commit_ui.commit_all;
                let on = if this.commit_ui.commit_all {
                    "true"
                } else {
                    "false"
                };
                crate::settings_window::Store::save_one("commit.commitAll", on);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowDiffOnDoubleClick, _, cx| {
                this.set_double_click_source(false, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowSourceOnDoubleClick, _, cx| {
                this.set_double_click_source(true, cx)
            }))
            .on_action(cx.listener(|this, _: &UseStagingArea, _, cx| this.set_staging(true, cx)))
            .on_action(cx.listener(|this, _: &UseChangelists, _, cx| this.set_staging(false, cx)))
            .on_action(cx.listener(|this, _: &ResolveConflicts, window, cx| {
                this.resolve_conflicts_dialog(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ExternalMerge, window, cx| {
                let Some(path) = this.selected_paths().into_iter().find(|path| {
                    this.entries_of(std::slice::from_ref(path))
                        .iter()
                        .any(|e| e.conflicted)
                }) else {
                    return;
                };
                this.run_op(
                    window,
                    cx,
                    move |root| spork_git::file::merge_tool(root, &path),
                    |_, (), _, _| {},
                )
            }))
            .on_action(cx.listener(|this, _: &ToggleSpelling, _, cx| {
                this.prefs.spell_check = !this.prefs.spell_check;
                this.prefs.save();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &LfsLock, window, cx| this.lfs_lock(true, window, cx)))
            .on_action(
                cx.listener(|this, _: &LfsUnlock, window, cx| this.lfs_lock(false, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step_change(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step_change(1, cx)))
    }

    /// Merge…: the conflicted file in Spork's three-way resolver, in the main area.
    fn merge_conflict(&mut self, path: String, cx: &mut Context<Self>) {
        self.select_change(
            Some(ChangeSelection {
                path,
                staged: false,
            }),
            cx,
        );
        self.show_selected_diff(cx)
    }

    /// Revert Resolved: asks, then puts the resolved files' conflicts back.
    fn revert_resolved(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        let files = match paths.as_slice() {
            [one] => format!("file: {one}"),
            many => format!("files: {}", many.join(", ")),
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, paths) = (view.clone(), paths.clone());
            dialog
                .title("Revert Resolved Files")
                .description(format!("Do you want to revert resolved {files}?"))
                .confirm()
                .ok_text("Revert")
                .on_ok(move |_, window, cx| {
                    let paths = paths.clone();
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::revert_resolved(root, &paths),
                            |_, (), _, _| {},
                        )
                    });
                    true
                })
        });
    }

    /// Resolve Conflicts…: the reference app's Conflicts dialog.
    pub(crate) fn resolve_conflicts_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let conflicts: Vec<String> = self
            .tracked_changes()
            .into_iter()
            .filter(|e| e.conflicted)
            .map(|e| e.path)
            .collect();
        let Some(first) = conflicts.first() else {
            return window.push_notification(Notification::info("No conflicts to resolve"), cx);
        };
        self.commit_ui.conflict_selected = Some(first.clone());
        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let (content, footer) = view.update(cx, |this, cx| this.conflicts_dialog(cx));
            dialog
                .title("Conflicts")
                .w(px(560.))
                .child(content)
                .footer(footer)
        });
    }

    /// The Conflicts dialog's body (the conflicted files with each side's change, Accept Yours,
    /// Accept Theirs and Merge… for the selected one, Group by directory) and its Close button.
    fn conflicts_dialog(&mut self, cx: &mut Context<Self>) -> (Stateful<Div>, Div) {
        let conflicts: Vec<StatusEntry> = self
            .tracked_changes()
            .into_iter()
            .filter(|e| e.conflicted)
            .collect();
        let tokens = spork_workbench::Tokens::get(cx);
        let selected = self
            .commit_ui
            .conflict_selected
            .clone()
            .filter(|path| conflicts.iter().any(|e| &e.path == path));
        let by_directory = self.commit_ui.conflicts_by_directory;
        // git's two letters of an unmerged path: ours, then theirs ('D': that side deleted it)
        let side = |letter: char| if letter == 'D' { "Deleted" } else { "Modified" };
        let cells = |name: SharedString, yours: &'static str, theirs: &'static str| {
            h_flex()
                .w_full()
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(div().w(px(90.)).flex_none().child(yours))
                .child(div().w(px(90.)).flex_none().child(theirs))
        };
        let mut sorted: Vec<&StatusEntry> = conflicts.iter().collect();
        if by_directory {
            sorted.sort_by(|a, b| split_dir(&a.path).cmp(&split_dir(&b.path)));
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_dir = None;
        for (ix, e) in sorted.iter().enumerate() {
            let (dir, name) = split_dir(&e.path);
            if by_directory && last_dir != Some(dir) {
                last_dir = Some(dir);
                rows.push(
                    div()
                        .px_2()
                        .text_color(tokens.text_secondary)
                        .child(if dir.is_empty() { "/" } else { dir }.to_owned())
                        .into_any_element(),
                );
            }
            let shown = if by_directory { name } else { e.path.as_str() };
            let path = e.path.clone();
            let on = selected.as_deref() == Some(e.path.as_str());
            rows.push(
                cells(shown.to_owned().into(), side(e.index), side(e.worktree))
                    .id(("conflict-row", ix))
                    .role(Role::Row)
                    .aria_label(shown.to_owned())
                    .aria_selected(on)
                    .h(ROW_HEIGHT)
                    .px_2()
                    .when(on, |row| row.bg(tokens.selection))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.commit_ui.conflict_selected = Some(path.clone());
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let button = |id: &'static str, label: &'static str| {
            spork_workbench::Button::new(id, label)
                .role(Role::Button)
                .aria_label(label)
                .disabled(selected.is_none())
        };
        let (yours, theirs, merge) = (selected.clone(), selected.clone(), selected.clone());
        let description = match conflicts.len() {
            1 => "The following file has conflicts:",
            _ => "The following files have conflicts:",
        };
        let content = v_flex()
            .id("conflicts-dialog")
            .role(Role::Group)
            .aria_label("Conflicts")
            .gap_2()
            .child(description)
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        v_flex()
                            .id("conflicts-table")
                            .role(Role::Table)
                            .flex_1()
                            .h(px(220.))
                            .border_1()
                            .border_color(tokens.control_border)
                            .bg(tokens.control_bg)
                            .child(
                                cells("Name".into(), "Yours", "Theirs")
                                    .px_2()
                                    .h(ROW_HEIGHT)
                                    .text_color(tokens.text_muted)
                                    .border_b_1()
                                    .border_color(tokens.control_border),
                            )
                            .children(rows),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                button("conflicts-yours", "Accept Yours").on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if let Some(path) = yours.clone() {
                                            this.resolve_conflict(path, true, window, cx)
                                        }
                                    },
                                )),
                            )
                            .child(button("conflicts-theirs", "Accept Theirs").on_click(
                                cx.listener(move |this, _, window, cx| {
                                    if let Some(path) = theirs.clone() {
                                        this.resolve_conflict(path, false, window, cx)
                                    }
                                }),
                            ))
                            .child(button("conflicts-merge", "Merge…").on_click(cx.listener(
                                move |this, _, window, cx| {
                                    if let Some(path) = merge.clone() {
                                        window.close_dialog(cx);
                                        this.merge_conflict(path, cx)
                                    }
                                },
                            ))),
                    ),
            )
            .child(
                spork_workbench::controls::labeled_check(
                    "conflicts-by-directory",
                    "Group by directory",
                    by_directory,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.commit_ui.conflicts_by_directory = !this.commit_ui.conflicts_by_directory;
                    cx.notify();
                })),
            );
        let footer = h_flex().w_full().justify_end().child(
            spork_workbench::Button::new("conflicts-close", "Close")
                .role(Role::Button)
                .aria_label("Close")
                .on_click(|_, window, cx| window.close_dialog(cx)),
        );
        (content, footer)
    }

    /// Locks or unlocks the selected file with Git LFS (Spork's own, from Find Action).
    fn lfs_lock(&mut self, lock: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected_changes().into_iter().next().map(|e| e.path) else {
            return;
        };
        self.run_op(
            window,
            cx,
            move |root| match lock {
                true => spork_git::lfs::lock(root, &path).map(|()| format!("Locked {path}")),
                false => {
                    spork_git::lfs::unlock(root, &path, false).map(|()| format!("Unlocked {path}"))
                }
            },
            |_, message, window, cx| window.push_notification(Notification::success(message), cx),
        )
    }

    /// Commit… (⌘K): the Commit tool window, the cursor in its commit message.
    pub(crate) fn checkin_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_commit_window(window, cx);
        self.message
            .update(cx, |message, cx| message.focus(window, cx));
    }

    /// The selected file where it is on disk; None for a deleted one.
    fn selected_file_on_disk(&self) -> Option<std::path::PathBuf> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let path = self.change_selected.as_ref()?.path.clone();
        Some(repo.root.join(path)).filter(|file| file.exists())
    }

    /// Commit File…: the selected files go into the commit (staged), the message gets the focus.
    fn commit_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = self
            .entries_of(&self.selected_paths())
            .into_iter()
            .filter(|e| e.has_unstaged())
            .map(|e| e.path)
            .collect();
        let message = self.message.clone();
        self.open_left(crate::LeftTool::Commit, cx);
        message.update(cx, |message, cx| message.focus(window, cx));
        if !paths.is_empty() {
            self.toggle_stage(paths, false, window, cx);
        }
    }

    /// Add to VCS (Git ▸ Add): the selected unversioned files, or the selected files' changes.
    fn add_to_vcs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = self
            .entries_of(&self.selected_paths())
            .into_iter()
            .filter(|e| e.untracked())
            .map(|e| e.path)
            .collect();
        if !paths.is_empty() {
            self.toggle_stage(paths, false, window, cx)
        }
    }

    /// Delete…: asks, then moves the selected files to the Trash.
    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let paths: Vec<String> = self
            .selected_paths()
            .into_iter()
            .filter(|path| repo.root.join(path).exists())
            .collect();
        let what = match paths.as_slice() {
            [] => return,
            [one] => format!("Delete file \"{}\"?", one.rsplit('/').next().unwrap_or(one)),
            many => format!("Delete {} files?", many.len()),
        };
        let view = cx.entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let (view, paths) = (view.clone(), paths.clone());
            dialog
                .title("Delete")
                .description(what.clone())
                .confirm()
                .ok_text("Delete")
                .on_ok(move |_, window, cx| {
                    let paths = paths.clone();
                    view.update(cx, |this, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::trash_files(root, &paths),
                            |this, (), _, _| this.change_marked.clear(),
                        )
                    });
                    true
                })
        });
    }

    /// Copy as Patch to Clipboard: the selected changes against HEAD, as `git apply` takes them.
    fn copy_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.entries_of(&self.selected_paths());
        if entries.is_empty() {
            return;
        }
        self.run_op(
            window,
            cx,
            move |root| spork_git::local_patch(root, &entries),
            |_, patch, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(patch));
                window.push_notification(Notification::info("Patch copied to clipboard"), cx)
            },
        );
    }

    /// The tracked changes among the selected files: what Shelve takes.
    fn selected_changes(&self) -> Vec<StatusEntry> {
        self.entries_of(&self.selected_paths())
            .into_iter()
            .filter(|e| !e.untracked())
            .collect()
    }

    /// Shelve Silently, or Shelve Changes… with its name: the selected changes go to the shelf
    /// (`spork_git::shelf`), named after their changelist when no name is given.
    fn shelve(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.selected_changes();
        if entries.is_empty() {
            return;
        }
        // Spork has the one changelist, IntelliJ's default
        let name = name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Changes".to_owned());
        self.run_op(
            window,
            cx,
            move |root| shelve_paths(root, name.trim(), &entries),
            |this, _, window, cx| {
                this.change_marked.clear();
                window.push_notification(Notification::success("Changes shelved"), cx)
            },
        );
    }

    /// Save To Shelf: the selected changes copied to the shelf under their changelist's name,
    /// kept in the working tree.
    fn save_to_shelf(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.selected_changes();
        if entries.is_empty() {
            return;
        }
        self.run_op(
            window,
            cx,
            move |root| {
                let patch = spork_git::local_patch(root, &entries)?;
                spork_git::shelf::save(root, "Changes", &patch)
            },
            |_, _, window, cx| {
                window.push_notification(Notification::success("Changes shelved"), cx)
            },
        );
    }

    /// Shelve Changes…: the Shelve Changes dialog.
    fn shelve_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_executor(Executor::Shelve, window, cx)
    }

    /// Commit Message History: the messages of the last commits, searchable; the one picked
    /// replaces the message, the open commit dialog's with `dialog`.
    fn message_history_into(&mut self, dialog: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        // shortcut: a quick `git log -n 20` on the UI thread; IntelliJ keeps the messages typed
        // in it instead, move to a background load if it shows up in a profile
        let messages = spork_git::revision::recent_messages(&repo.root, 20);
        let items = messages
            .iter()
            .map(|message| {
                CommandItem::new().label(message.lines().next().unwrap_or_default().to_owned())
            })
            .collect();
        let target = self.commit_ui.executor.message.clone().filter(|_| dialog);
        let (view, state) = (cx.entity(), cx.new(|cx| CommandState::new(window, cx)));
        crate::shell::palette(
            window,
            cx,
            state,
            items,
            "Commit Message History",
            move |ix, window, cx| {
                window.close_dialog(cx);
                let Some(message) = messages.get(ix).cloned() else {
                    return;
                };
                match &target {
                    Some(target) => {
                        target.update(cx, |state, cx| state.set_value(message, window, cx))
                    }
                    None => view.update(cx, |this, cx| this.set_message(message, window, cx)),
                }
            },
        );
    }

    /// Create Patch from Local Changes…: the Create Patch dialog, then Patch File Settings.
    fn create_patch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_executor(Executor::Patch, window, cx)
    }

    /// Show Current Revision: the last commit that changed `path`, in VcsBundle's
    /// current.version.text (the file, its revision, author, date and message).
    fn show_current_revision(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            {
                let path = path.clone();
                move |root| {
                    let commit = spork_git::file::history(root, &path, 1)?.pop();
                    Ok(commit.map(|commit| {
                        let message = spork_git::commit_message(root, &commit.id)
                            .unwrap_or_else(|| commit.subject.clone());
                        (commit, message)
                    }))
                }
            },
            move |_, found, window, cx| {
                let Some((commit, message)) = found else {
                    return;
                };
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                let date = crate::history::date_text(commit.time, chrono::Local::now().timestamp());
                let lines = [
                    name,
                    String::new(),
                    format!("Current version is {}.", commit.id),
                    format!("Modified by {}", commit.author),
                    date,
                    message,
                ];
                window.open_dialog(cx, move |dialog, _, _| {
                    dialog.w(px(560.)).child(
                        v_flex()
                            .id("current-revision")
                            .role(Role::Group)
                            .aria_label("Show Current Revision")
                            .children(lines.iter().map(|line| match line.is_empty() {
                                true => div().h(px(12.)).into_any_element(),
                                false => {
                                    crate::git_menu::form_label(line.clone()).into_any_element()
                                }
                            })),
                    )
                });
            },
        )
    }

    /// Compare with Revision…: IntelliJ's File Revisions popup of the file's commits, then how
    /// the file differs from the one picked shows in the main area.
    fn compare_with_revision(&mut self, path: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        // shortcut: the file's `git log` on the UI thread, as Commit Message History's; load it in
        // the background if a long history shows up in a profile
        let revisions = match spork_git::file::history(&repo.root, &path, 500) {
            Ok(revisions) => revisions,
            Err(e) => {
                return window.push_notification(Notification::error(e.to_string()), cx);
            }
        };
        let now = chrono::Local::now().timestamp();
        let items = revisions
            .iter()
            .map(|c| {
                let date = crate::history::date_text(c.time, now);
                let short = c.id.get(..8).unwrap_or(&c.id);
                CommandItem::new().label(format!("{short}  {date}  {}  {}", c.author, c.subject))
            })
            .collect();
        let ids: Vec<String> = revisions.into_iter().map(|c| c.id).collect();
        let (view, state) = (cx.entity(), cx.new(|cx| CommandState::new(window, cx)));
        crate::shell::palette(
            window,
            cx,
            state,
            items,
            "File Revisions",
            move |ix, window, cx| {
                window.close_dialog(cx);
                let Some(rev) = ids.get(ix).cloned() else {
                    return;
                };
                let path = path.clone();
                view.update(cx, |this, cx| {
                    this.show_diff_with_ref(path, rev, window, cx)
                })
            },
        );
    }

    /// Compare with Branch or Tag…: pick one in a branches popup (branches_popup.rs), then how
    /// the file differs from it shows in the main area.
    pub(crate) fn compare_with_branch(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.compare_with_branch_popup(path, window, cx)
    }

    /// How the file at `path` differs from its version in `rev`, in the main area.
    pub(crate) fn show_diff_with_ref(
        &mut self,
        path: String,
        rev: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_op(
            window,
            cx,
            move |root| spork_git::file::diff_with_local(root, &rev, &path),
            |this, patch, _, cx| {
                let state = this.change_diff.clone();
                this.set_patch(&state, &patch, cx);
                // another version's diff isn't the working copy's: no line staging on it
                this.change_patch = None;
                this.open_diff_view(crate::View::Changes, cx);
                cx.notify();
            },
        )
    }

    /// A double-click on a change: its diff, or its file with Show on Double-Click ▸ Source.
    fn open_on_double_click(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.commit_ui.double_click_source {
            return self.open_diff_view(crate::View::Changes, cx);
        }
        let file = self.selected_file_on_disk();
        if file.is_some() {
            self.open_in_editor(file, window, cx)
        }
    }

    /// Show on Double-Click ▸ Diff or Source, kept in the settings.
    fn set_double_click_source(&mut self, source: bool, cx: &mut Context<Self>) {
        self.commit_ui.double_click_source = source;
        let value = if source { "true" } else { "false" };
        crate::settings_window::Store::save_one("commit.showSourceOnDoubleClick", value);
        cx.notify();
    }

    /// The Commit tool window's options (CommitView.GearActions): Show on Double-Click ▸ Diff or
    /// Source, Display as Staging Area or Changelists (Git.Stage.Configure), and in the Staging
    /// Area, Commit All when Nothing Is Staged.
    fn commit_options_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (view, staging, commit_all, source) = (
            cx.entity(),
            self.commit_ui.staging,
            self.commit_ui.commit_all,
            self.commit_ui.double_click_source,
        );
        spork_workbench::ActionButton::new("commit-gear", "general/moreVertical")
            .role(Role::Button)
            .aria_label("Options")
            .tooltip("Options")
            .dropdown_menu(move |menu, window, cx| {
                let item = |label: &str, on: bool, action: Box<dyn Action>| {
                    option_item(&view, label, on, action)
                };
                let secondary = spork_workbench::Tokens::get(cx).text_secondary;
                let submenu_view = view.clone();
                menu.submenu("Show on Double-Click", window, cx, move |menu, _, _| {
                    menu.item(option_item(
                        &submenu_view,
                        "Diff",
                        !source,
                        Box::new(ShowDiffOnDoubleClick),
                    ))
                    .item(option_item(
                        &submenu_view,
                        "Source",
                        source,
                        Box::new(ShowSourceOnDoubleClick),
                    ))
                })
                .separator()
                // a titled separator, not an item: no click, nothing for assistive technology
                .item(PopupMenuItem::element(move |_, _| {
                    div().text_xs().text_color(secondary).child("Display as")
                }))
                .item(item("Staging Area", staging, Box::new(UseStagingArea)))
                .item(item("Changelists", !staging, Box::new(UseChangelists)))
                .when(staging, |menu| {
                    menu.separator().item(item(
                        "Commit All when Nothing Is Staged",
                        commit_all,
                        Box::new(ToggleCommitAll),
                    ))
                })
            })
    }

    /// Whether the Commit tool window shows the Staging Area (Git ▸ Show Staging Area's target).
    pub(crate) fn staging_area_shown(&self) -> bool {
        self.commit_ui.staging
    }

    /// Display as Staging Area or Changelists, kept in the settings.
    fn set_staging(&mut self, staging: bool, cx: &mut Context<Self>) {
        self.commit_ui.staging = staging;
        let value = if staging { "true" } else { "false" };
        crate::settings_window::Store::save_one("commit.stagingArea", value);
        self.change_marked.clear();
        cx.notify();
    }

    /// The Staging Area's toolbar (Git.Stage.Toolbar): Show Diff, Refresh, Stage All Tracked,
    /// Stash Silently, View Options.
    fn staging_toolbar(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let entries = match &self.repo {
            Some(Ok(repo)) => repo.status.clone(),
            _ => Vec::new(),
        };
        let unstaged_tracked = entries.iter().any(|e| StageNode::Unstaged.holds(e));
        let changes = entries.iter().any(|e| !e.untracked());
        let button = |id: &'static str, icon: &str, label: &'static str| {
            spork_workbench::ActionButton::new(id, icon.to_owned())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(label)
        };
        let view = cx.entity();
        let (directory, repository, ignored) = (
            self.commit_ui.group_by_directory,
            self.commit_ui.group_by_repository,
            self.show_ignored,
        );
        h_flex()
            .id("stage-toolbar")
            .role(Role::Toolbar)
            .aria_label("Git.Stage.Toolbar")
            .h(px(32.))
            .flex_none()
            .px_2()
            .gap_0p5()
            .child(
                button("stage-diff", "vcs/diff", "Show Diff")
                    .disabled(self.change_selected.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.show_selected_diff(cx))),
            )
            .child(
                button("stage-refresh", "general/refresh", "Refresh")
                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
            )
            .child(
                button("stage-tracked", "general/add", "Stage All Tracked")
                    .disabled(!unstaged_tracked)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(StageAllTracked), cx)
                    }),
            )
            .child(
                button("stage-stash", "vcs/shelveSilent", "Stash Silently")
                    .disabled(!changes)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(StashSilently), cx)),
            )
            .child(
                button("stage-view-options", "general/show", "View Options")
                    .popup_marker()
                    .dropdown_menu(move |menu, window, cx| {
                        let group_view = view.clone();
                        menu.submenu("Group By", window, cx, move |menu, _, _| {
                            menu.item(view_option(&group_view, ViewOption::Directory, directory))
                                .item(view_option(&group_view, ViewOption::Repository, repository))
                        })
                        .separator()
                        .item(view_option(
                            &view,
                            ViewOption::Ignored,
                            ignored,
                        ))
                    }),
            )
    }

    /// The Staging Area's tree: Unmerged, Staged, Unstaged and Untracked, each with its files; a
    /// file staged and changed again is in both Staged and Unstaged, as in git.
    fn staging_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let entries: Vec<StatusEntry> = repo
            .status
            .iter()
            .filter(|e| self.show_ignored || !e.ignored())
            .cloned()
            .collect();
        let root_name = repo
            .root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let theme = cx.theme();
        let (muted, list_hover, list_active) =
            (theme.muted_foreground, theme.list_hover, theme.list_active);
        let mut rows = Vec::new();
        for (node_ix, node) in StageNode::ALL.into_iter().enumerate() {
            let files: Vec<&StatusEntry> = entries.iter().filter(|e| node.holds(e)).collect();
            if files.is_empty() {
                continue;
            }
            let open = self.commit_ui.stage_open[node_ix];
            let count = match files.len() {
                1 => "1 file".to_owned(),
                n => format!("{n} files"),
            };
            rows.push(
                h_flex()
                    .id(("stage-node", node_ix))
                    .role(Role::TreeItem)
                    .aria_label(node.label())
                    .aria_expanded(open)
                    .h(ROW_HEIGHT)
                    .mx_1()
                    .pl(px(6.))
                    .gap_1p5()
                    .rounded(px(4.))
                    .cursor_pointer()
                    .child(
                        div().flex_none().text_color(muted).child(
                            spork_workbench::icon(if open {
                                "general/chevronDown"
                            } else {
                                "general/chevronRight"
                            })
                            .size(px(16.)),
                        ),
                    )
                    .child(div().font_weight(FontWeight::BOLD).child(node.label()))
                    .child(div().text_color(muted).child(count))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let open = &mut this.commit_ui.stage_open[node_ix];
                        *open = !*open;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
            if !open {
                continue;
            }
            for (ix, e) in files.into_iter().enumerate() {
                let staged = node == StageNode::Staged;
                let (_, color) = status_letter(e, staged, cx);
                let (dir, name) = match e.path.rsplit_once('/') {
                    Some((dir, name)) => (dir.to_owned(), name.to_owned()),
                    None => (root_name.clone(), e.path.clone()),
                };
                let selected = self
                    .change_selected
                    .as_ref()
                    .is_some_and(|s| s.path == e.path && s.staged == staged);
                let (path, menu_path) = (e.path.clone(), e.path.clone());
                let menu = stage_menu(cx.entity(), e.clone(), node);
                rows.push(
                    h_flex()
                        .id(("stage-file", node_ix * 100_000 + ix))
                        .role(Role::TreeItem)
                        .aria_label(name.clone())
                        .aria_selected(selected)
                        .h(ROW_HEIGHT)
                        .mx_1()
                        .pl(px(6. + 18. + 16.))
                        .pr_2()
                        .gap_1p5()
                        .rounded(px(4.))
                        .when(selected, |r| r.bg(list_active))
                        .when(!selected, |r| r.hover(move |r| r.bg(list_hover)))
                        .child(spork_workbench::image(
                            spork_workbench::file_icon(&name),
                            cx,
                        ))
                        .child(div().flex_none().text_color(color).child(name))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(muted)
                                .child(dir),
                        )
                        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            window.focus(&this.commit_ui.focus, cx);
                            this.change_marked.clear();
                            let sel = ChangeSelection {
                                path: path.clone(),
                                staged,
                            };
                            this.select_change(Some(sel), cx);
                            if event.click_count() == 2 {
                                this.open_on_double_click(window, cx);
                            }
                            cx.notify();
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, _, window, cx| {
                                window.focus(&this.commit_ui.focus, cx);
                                this.change_marked.clear();
                                let sel = ChangeSelection {
                                    path: menu_path.clone(),
                                    staged,
                                };
                                this.select_change(Some(sel), cx);
                            }),
                        )
                        .context_menu(move |m, window, cx| menu(m, window, cx))
                        .into_any_element(),
                );
            }
        }
        rows
    }

    /// Rollback in the Staging Area (Git.Stage.Revert): throws the file's unstaged changes away
    /// at once, as the reference app does without asking.
    fn stage_rollback(&mut self, entry: StatusEntry, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            move |root| spork_git::discard(root, &[entry]),
            |_, (), _, _| {},
        )
    }

    /// Stash Files (Git.Stage.Stash.Files): stashes these files' changes at once.
    fn stash_files(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.run_op(
            window,
            cx,
            move |root| {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                spork_git::stash::save_paths(root, "", &paths)
            },
            |_, (), window, cx| {
                window.push_notification(Notification::success("Files stashed successfully"), cx)
            },
        )
    }

    /// The selected local change's diff, in the main area.
    pub(crate) fn change_diff_view(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        match (&self.change_selected, self.image_pair_view(cx)) {
            (Some(_), _) if self.change_submodule.is_some() => self.submodule_view(cx),
            (Some(_), Some(images)) => images,
            (Some(sel), None) => v_flex()
                .size_full()
                .children(self.conflict_bar(&sel.path, cx))
                .children(self.selection_bar(sel.staged, cx))
                .child(self.diff_bar("changes", &self.change_diff, cx))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .child(self.diff_view(&self.change_diff, cx)),
                ),
            (None, _) => v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(muted)
                .child(
                    if self.all_entries(false).is_empty() && self.all_entries(true).is_empty() {
                        "Working tree clean"
                    } else {
                        "Select a file to see its changes"
                    },
                ),
        }
    }
}

/// Settings › Version Control › Commit's commit message inspections, by setting key, in
/// the reference app's order.
pub(crate) const COMMIT_INSPECTIONS: [(&str, &str); 3] = [
    (
        "commit.inspection.blankLine",
        "Blank line between subject and body",
    ),
    ("commit.inspection.bodyLimit", "Limit body line"),
    ("commit.inspection.subjectLimit", "Limit subject line"),
];

/// IntelliJ's right margin for commit messages.
const COMMIT_MARGIN: usize = 72;

/// What the commit message inspections that are `on` (in `COMMIT_INSPECTIONS`' order) find in
/// `message`.
fn inspection_problems(message: &str, on: [bool; 3]) -> Vec<String> {
    let [blank_line, body_limit, subject_limit] = on;
    let mut lines = message.lines();
    let subject = lines.next().unwrap_or_default();
    let second = lines.next();
    let mut problems = Vec::new();
    if blank_line && second.is_some_and(|line| !line.trim().is_empty()) {
        problems.push("Missing blank line between subject and body".to_owned());
    }
    if subject_limit && subject.chars().count() > COMMIT_MARGIN {
        problems.push(format!(
            "Subject line is longer than {COMMIT_MARGIN} characters"
        ));
    }
    let body_too_long = message
        .lines()
        .skip(1)
        .any(|line| line.chars().count() > COMMIT_MARGIN);
    if body_limit && body_too_long {
        problems.push(format!(
            "Body line is longer than {COMMIT_MARGIN} characters"
        ));
    }
    problems
}

/// Shelves the tracked `entries` as one shelved changelist and rolls them back, in the shelf the
/// Shelf tab shows (spork_git::shelf).
fn shelve_paths(
    root: &std::path::Path,
    description: &str,
    entries: &[StatusEntry],
) -> Result<spork_git::shelf::Shelved, GitError> {
    let paths: Vec<&str> = entries
        .iter()
        .filter(|e| !e.untracked())
        .map(|e| e.path.as_str())
        .collect();
    spork_git::shelf::shelve(root, description, &paths)
}

/// What a patch's paths start with when it is based at `base` rather than at the repository
/// `root`: the repository's place under it; why not when `base` doesn't hold the repository.
fn patch_prefix(root: &std::path::Path, base: &std::path::Path) -> Result<String, String> {
    if base.as_os_str().is_empty() {
        return Err("Base path cannot be empty!".into());
    }
    if !base.is_dir() {
        return Err("Base dir doesn't exist".into());
    }
    let real = |path: &std::path::Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
    match real(root).strip_prefix(real(base)) {
        Ok(inside) if inside.as_os_str().is_empty() => Ok(String::new()),
        Ok(inside) => Ok(format!("{}/", inside.to_string_lossy().replace('\\', "/"))),
        Err(_) => Err(format!(
            "Base path doesn't contain all selected changes (use {})",
            root.display()
        )),
    }
}

/// A by-name setting of the Settings window (settings_window::Store).
fn setting(key: &str) -> Option<String> {
    crate::settings_window::Store::load()
        .get(key)
        .map(str::to_owned)
}

/// An item of the Commit tool window's options: checked while `on`, running `action` on the view.
fn option_item(
    view: &Entity<RepoView>,
    label: &str,
    on: bool,
    action: Box<dyn Action>,
) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(label.to_owned())
        .checked(on)
        .on_click(move |_, window, cx| {
            let action = action.boxed_clone();
            view.update(cx, |_, cx| {
                window.dispatch_action(action, cx);
                cx.notify()
            })
        })
}

/// A View Options choice of the changes tree.
#[derive(Clone, Copy)]
enum ViewOption {
    Directory,
    Repository,
    Ignored,
}

/// A View Options item: checked while on, with its action's shortcut.
fn view_option(view: &Entity<RepoView>, option: ViewOption, on: bool) -> PopupMenuItem {
    let (label, action): (&str, Box<dyn Action>) = match option {
        ViewOption::Directory => ("Directory", Box::new(GroupByDirectory)),
        ViewOption::Repository => ("Repository", Box::new(GroupByRepository)),
        ViewOption::Ignored => ("Ignored Files", Box::new(ShowIgnored)),
    };
    let view = view.clone();
    PopupMenuItem::new(label)
        .checked(on)
        .action(action)
        .on_click(move |_, _, cx| view.update(cx, |this, cx| this.toggle_view_option(option, cx)))
}

/// Right-click menu of a Staging Area file (Git.Stage.Tree.Menu), what the reference app shows for the node
/// it is in.
fn stage_menu(
    view: Entity<RepoView>,
    entry: StatusEntry,
    node: StageNode,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, _, cx| {
        let on =
            |label: &str,
             action: Option<Box<dyn Action>>,
             enabled: bool,
             f: fn(&mut RepoView, StatusEntry, &mut Window, &mut Context<RepoView>)| {
                let (view, entry) = (view.clone(), entry.clone());
                PopupMenuItem::new(label.to_owned())
                    .when_some(action, |item, action| item.action(action))
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| f(this, entry.clone(), window, cx))
                    })
            };
        let on_disk = match &view.read(cx).repo {
            Some(Ok(repo)) => repo.root.join(&entry.path).exists(),
            _ => false,
        };
        let tracked = !entry.untracked();
        let menu = match node {
            StageNode::Unmerged => menu
                .item(on("Merge…", None, true, |this, e, _, cx| {
                    this.merge_conflict(e.path, cx)
                }))
                .item(on("Accept Theirs", None, true, |this, e, window, cx| {
                    this.resolve_conflict(e.path, false, window, cx)
                }))
                .item(on("Accept Yours", None, true, |this, e, window, cx| {
                    this.resolve_conflict(e.path, true, window, cx)
                })),
            StageNode::Staged => menu
                .item(on(
                    "Unstage",
                    Some(Box::new(Rollback)),
                    true,
                    |this, e, window, cx| this.toggle_stage(vec![e.path], true, window, cx),
                ))
                .item(on("Stash Files", None, true, |this, e, window, cx| {
                    this.stash_files(vec![e.path], window, cx)
                })),
            StageNode::Unstaged => menu
                .item(on(
                    "Stage",
                    Some(Box::new(AddToVcs)),
                    true,
                    |this, e, window, cx| this.toggle_stage(vec![e.path], false, window, cx),
                ))
                .item(on("Stash Files", None, true, |this, e, window, cx| {
                    this.stash_files(vec![e.path], window, cx)
                }))
                .item(on(
                    "Rollback",
                    Some(Box::new(Rollback)),
                    true,
                    |this, e, window, cx| this.stage_rollback(e, window, cx),
                )),
            StageNode::Untracked => menu
                .item(on(
                    "Stage",
                    Some(Box::new(AddToVcs)),
                    true,
                    |this, e, window, cx| this.toggle_stage(vec![e.path], false, window, cx),
                ))
                .item(on(
                    "Stage Without Content",
                    None,
                    true,
                    |this, e, window, cx| {
                        this.run_op(
                            window,
                            cx,
                            move |root| spork_git::stage_without_content(root, &[e.path.as_str()]),
                            |_, (), _, _| {},
                        )
                    },
                )),
            StageNode::Ignored => menu,
        };
        menu.item(on(
            "Delete…",
            Some(Box::new(DeleteFile)),
            on_disk,
            |this, _, window, cx| this.confirm_delete(window, cx),
        ))
        .separator()
        .item(
            on(
                "Show Diff",
                Some(Box::new(ShowDiff)),
                true,
                |this, _, _, cx| this.show_selected_diff(cx),
            )
            .icon(spork_workbench::icon("vcs/diff")),
        )
        .item(on(
            "Jump to Source",
            Some(Box::new(JumpToSource)),
            on_disk,
            |this, _, window, cx| {
                let file = this.selected_file_on_disk();
                this.open_in_editor(file, window, cx)
            },
        ))
        .item(on(
            "Create Patch…",
            None,
            tracked,
            |this, _, window, cx| this.create_patch(window, cx),
        ))
        .item(on(
            "Copy as Patch to Clipboard",
            None,
            tracked,
            |this, _, window, cx| this.copy_patch(window, cx),
        ))
        .separator()
        .item(on("Show History", None, tracked, |this, e, _, cx| {
            this.open_file(e.path, false, cx)
        }))
    }
}

/// `body` with `trailer` added in its own block at the end, or to the trailers already there.
fn add_trailer(body: &str, trailer: &str) -> String {
    let body = body.trim_end();
    let in_trailers = body
        .lines()
        .last()
        .is_some_and(|l| l.split_once(": ").is_some_and(|(k, _)| !k.contains(' ')));
    match (body.is_empty(), in_trailers) {
        (true, _) => trailer.to_owned(),
        (false, true) => format!("{body}\n{trailer}"),
        (false, false) => format!("{body}\n\n{trailer}"),
    }
}

fn split_dir(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

/// One item per pattern, adding it to `.gitignore` or, `private`, to `.git/info/exclude`.
fn ignore_items(
    menu: PopupMenu,
    view: &Entity<RepoView>,
    patterns: &[String],
    private: bool,
) -> PopupMenu {
    patterns.iter().fold(menu, |menu, pattern| {
        let (view, pattern) = (view.clone(), pattern.clone());
        menu.item(
            PopupMenuItem::new(pattern.clone()).on_click(move |_, window, cx| {
                let pattern = pattern.clone();
                view.update(cx, |this, cx| {
                    this.run_op(
                        window,
                        cx,
                        move |root| {
                            spork_git::ignore::add(root, &pattern, private).map(|_| pattern)
                        },
                        move |_, pattern, window, cx| {
                            let file = if private {
                                ".git/info/exclude"
                            } else {
                                ".gitignore"
                            };
                            window.push_notification(
                                Notification::success(format!("Added {pattern} to {file}")),
                                cx,
                            )
                        },
                    )
                })
            }),
        )
    })
}

/// The changes tree's menu for a right-click below its rows: the selected file's, or with none
/// selected what the reference app shows then (ChangesViewPopupMenu, the Staging Area's
/// Git.Stage.Tree.Menu): what acts on every change enabled, Show History the opened file's.
fn tree_menu(
    view: Entity<RepoView>,
    staging: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        let this = view.read(cx);
        let selected = this.change_selected.clone().and_then(|sel| {
            let entry = this.entries_of(std::slice::from_ref(&sel.path)).pop()?;
            Some((sel, entry))
        });
        let opened = this.opened_file();
        match (selected, staging) {
            (Some((sel, entry)), true) => {
                let node = StageNode::ALL
                    .into_iter()
                    .find(|node| node.holds(&entry) && (*node == StageNode::Staged) == sel.staged)
                    .unwrap_or(StageNode::Unstaged);
                return stage_menu(view.clone(), entry, node)(menu, window, cx);
            }
            (Some((sel, entry)), false) => {
                let (untracked, conflicted) = (entry.untracked(), entry.conflicted);
                let build = file_menu(view.clone(), entry.path, untracked, sel.staged, conflicted);
                return build(menu, window, cx);
            }
            (None, _) => {}
        }
        let item =
            |label: &str,
             action: Option<Box<dyn Action>>,
             run: Option<fn(&mut RepoView, &mut Window, &mut Context<RepoView>)>| {
                let view = view.clone();
                PopupMenuItem::new(label.to_owned())
                    .when_some(action, |item, action| item.action(action))
                    .disabled(run.is_none())
                    .when_some(run, |item, run| {
                        item.on_click(move |_, window, cx| {
                            view.update(cx, |this, cx| run(this, window, cx))
                        })
                    })
            };
        let history = match opened {
            Some(path) => {
                let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                let view = view.clone();
                PopupMenuItem::new(format!("Show History for {name}")).on_click(move |_, _, cx| {
                    let path = path.clone();
                    view.update(cx, |this, cx| this.open_file(path, false, cx))
                })
            }
            None => item("Show History", None, None),
        };
        let diff = || item("Show Diff", Some(Box::new(ShowDiff)), None);
        if staging {
            return menu
                .item(item("Delete…", Some(Box::new(DeleteFile)), None))
                .separator()
                .item(diff().icon(spork_workbench::icon("vcs/diff")))
                .item(item("Create Patch…", None, None))
                .item(item("Copy as Patch to Clipboard", None, None))
                .separator()
                .item(history);
        }
        let rollback = || {
            item(
                "Rollback…",
                Some(Box::new(Rollback)),
                Some(|this, window, cx| this.rollback(window, cx)),
            )
        };
        menu.item(item("Commit…", Some(Box::new(CommitFile)), None))
            .item(rollback().icon(spork_workbench::icon("vcs/revert")))
            .item(diff().icon(spork_workbench::icon("vcs/diff")))
            .separator()
            .item(item("Delete…", Some(Box::new(DeleteFile)), None))
            .item(item("Add to VCS", Some(Box::new(AddToVcs)), None))
            .separator()
            .item(item(
                "Create Patch from Local Changes…",
                None,
                Some(|this, window, cx| this.create_patch(window, cx)),
            ))
            .item(item("Copy as Patch to Clipboard", None, None))
            .item(
                item(
                    "Shelve Changes…",
                    None,
                    Some(|this, window, cx| this.shelve_dialog(window, cx)),
                )
                .icon(spork_workbench::icon("vcs/shelve")),
            )
            .separator()
            .item(item("Refresh", None, Some(|this, _, cx| this.reload(cx))))
            .separator()
            // the Git group (Git.ContextMenu), as the reference app shows it inline
            .item(item("Commit…", None, None))
            .item(item("Add", Some(Box::new(AddToVcs)), None))
            .separator()
            .item(item("Show Diff", None, None))
            .item(item("Compare with Revision…", None, None))
            .item(item("Compare with Branch or Tag…", None, None))
            .item(history)
            .item(item("Show Current Revision", None, None))
            .item(rollback())
    }
}

/// Right-click menu of a changed file (ChangesViewPopupMenu), the reference app's items in its order with
/// what it hides or greys for this file: no changelists (Spork has the one), its Git group
/// inline. Spork's own file commands are in Find Action (`commands`).
fn file_menu(
    view: Entity<RepoView>,
    path: String,
    untracked: bool,
    staged: bool,
    conflicted: bool,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        // an item running `f` on the view, with its action's shortcut
        let on =
            |label: &str,
             action: Option<Box<dyn Action>>,
             enabled: bool,
             f: fn(&mut RepoView, String, &mut Window, &mut Context<RepoView>)| {
                let (view, path) = (view.clone(), path.clone());
                PopupMenuItem::new(label.to_owned())
                    .when_some(action, |item, action| item.action(action))
                    .disabled(!enabled)
                    .on_click(move |_, window, cx| {
                        view.update(cx, |this, cx| f(this, path.clone(), window, cx))
                    })
            };
        let ignore_view = view.clone();
        let suggestions = spork_git::ignore::suggestions(&path);
        // the file on disk; a deleted one has nothing to open
        // an added file has no revision yet: IntelliJ's fileInVcsByFileStatus
        let (root, on_disk, added) = match &view.read(cx).repo {
            Some(Ok(repo)) => (
                Some(repo.root.clone()),
                repo.root.join(&path).exists(),
                repo.status
                    .iter()
                    .any(|e| e.path == path && e.index == 'A' && !e.conflicted),
            ),
            _ => (None, false, false),
        };
        let copy = |label: &str, text: String| {
            PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                window.push_notification(Notification::info("Copied"), cx);
            })
        };
        let absolute = root
            .as_ref()
            .map_or_else(|| path.clone(), |r| r.join(&path).display().to_string());
        let file_name = path.rsplit('/').next().unwrap_or(&path).to_owned();
        let relative = path.clone();
        let tracked = !untracked;
        let ignore = move |menu: PopupMenu, window: &mut Window, cx: &mut Context<PopupMenu>| {
            let (view, mine) = (ignore_view.clone(), suggestions.clone());
            let (custom_view, first) = (
                ignore_view.clone(),
                suggestions.first().cloned().unwrap_or_default(),
            );
            let suggestions = suggestions.clone();
            let ignore_view = ignore_view.clone();
            menu.submenu("Add to .gitignore", window, cx, move |menu, window, cx| {
                let (view, mine) = (view.clone(), mine.clone());
                let (custom_view, first) = (custom_view.clone(), first.clone());
                ignore_items(menu, &ignore_view, &suggestions, false)
                    .separator()
                    .submenu("Only in This Clone", window, cx, move |menu, _, _| {
                        ignore_items(menu, &view, &mine, true)
                    })
                    .item(PopupMenuItem::new("Custom Pattern…").on_click(move |_, window, cx| {
                        let first = first.clone();
                        custom_view.update(cx, |this, cx| {
                            this.input_dialog(
                                "Ignore pattern".into(),
                                "One line of .gitignore: * matches within a folder, ** across folders, a / at the end means a folder, a / at the start means the top of the repository.".into(),
                                first,
                                |root, pattern| {
                                    spork_git::ignore::add(root, &pattern, false)
                                        .map(|_| format!("Added {} to .gitignore", pattern.trim()))
                                },
                                window,
                                cx,
                            )
                        })
                    }))
            })
        };
        // a merge's conflicts and those resolved since, which Revert Resolved puts back
        let (merging, resolved) = match &view.read(cx).repo {
            Some(Ok(repo)) => {
                let conflicts = repo.status.iter().any(|e| e.conflicted);
                // shortcut: asks git on the UI thread as the menu opens, during a merge only;
                // keep the list in Loaded if it ever shows in a profile
                let resolved = (repo.op != spork_git::OpState::None)
                    .then(|| spork_git::resolved_conflicts(&repo.root).log_err())
                    .flatten()
                    .unwrap_or_default();
                (conflicts || !resolved.is_empty(), resolved.contains(&path))
            }
            _ => (false, false),
        };
        let revert_resolved = |menu: PopupMenu| {
            menu.when(resolved && !conflicted, |menu| {
                menu.item(on(
                    "Revert Resolved",
                    None,
                    true,
                    |this, path, window, cx| this.revert_resolved(vec![path], window, cx),
                ))
            })
        };
        // Git.ChangesView.Conflicts
        let menu = menu.when(conflicted, |menu| {
            menu.item(
                on("Merge…", None, true, |this, path, _, cx| {
                    this.merge_conflict(path, cx)
                })
                .icon(spork_workbench::icon("vcs/merge")),
            )
            .item(on("Accept Theirs", None, true, |this, path, window, cx| {
                this.resolve_conflict(path, false, window, cx)
            }))
            .item(on("Accept Yours", None, true, |this, path, window, cx| {
                this.resolve_conflict(path, true, window, cx)
            }))
            .separator()
        });
        let menu = revert_resolved(menu);
        let menu = menu
            .item(on(
                "Commit File…",
                Some(Box::new(CommitFile)),
                true,
                |this, _, window, cx| this.commit_file(window, cx),
            ))
            .item(
                on(
                    "Rollback…",
                    Some(Box::new(Rollback)),
                    tracked,
                    |this, _, window, cx| this.rollback(window, cx),
                )
                .icon(spork_workbench::icon("vcs/revert")),
            )
            .item(
                on(
                    "Show Diff",
                    Some(Box::new(ShowDiff)),
                    true,
                    move |this, path, _, cx| {
                        let sel = ChangeSelection {
                            path,
                            staged: this.change_selected.as_ref().is_some_and(|s| s.staged),
                        };
                        this.select_change(Some(sel), cx);
                        this.show_selected_diff(cx)
                    },
                )
                .icon(spork_workbench::icon("vcs/diff")),
            )
            .item(on(
                "Jump to Source",
                Some(Box::new(JumpToSource)),
                on_disk,
                |this, _, window, cx| {
                    let file = this.selected_file_on_disk();
                    this.open_in_editor(file, window, cx)
                },
            ))
            .submenu("Copy Path/Reference…", window, cx, move |menu, _, _| {
                menu.item(copy("Absolute Path", absolute.clone()))
                    .item(copy("File Name", file_name.clone()))
                    .separator()
                    .item(copy("Path From Repository Root", relative.clone()))
            })
            .separator()
            .item(on(
                "Delete…",
                Some(Box::new(DeleteFile)),
                on_disk,
                |this, _, window, cx| this.confirm_delete(window, cx),
            ))
            .item(on(
                "Add to VCS",
                Some(Box::new(AddToVcs)),
                untracked,
                |this, _, window, cx| this.add_to_vcs(window, cx),
            ));
        // only untracked files can be ignored; tracked ones stay tracked whatever .gitignore says
        let menu = match untracked {
            true => ignore(menu, window, cx),
            false => menu,
        };
        menu.separator()
            .item(on(
                "Create Patch from Local Changes…",
                None,
                true,
                |this, _, window, cx| this.create_patch(window, cx),
            ))
            .item(on(
                "Copy as Patch to Clipboard",
                None,
                true,
                |this, _, window, cx| this.copy_patch(window, cx),
            ))
            .item(
                on("Shelve Changes…", None, tracked, |this, _, window, cx| {
                    this.shelve_dialog(window, cx)
                })
                .icon(spork_workbench::icon("vcs/shelve")),
            )
            .separator()
            .item(on("Refresh", None, true, |this, _, _, cx| this.reload(cx)))
            .separator()
            // the Git group (Git.ContextMenu), as the reference app shows it inline
            .item(on("Commit File…", None, true, |this, _, window, cx| {
                this.commit_file(window, cx)
            }))
            .item(on(
                "Add",
                Some(Box::new(AddToVcs)),
                untracked || !staged,
                |this, path, window, cx| this.toggle_stage(vec![path], false, window, cx),
            ))
            .map(|menu| match untracked {
                true => ignore(menu, window, cx),
                false => menu,
            })
            .separator()
            .item(on("Show Diff", None, tracked, |this, _, _, cx| {
                this.show_selected_diff(cx)
            }))
            .item(on(
                "Compare with Revision…",
                None,
                tracked,
                |this, path, window, cx| this.compare_with_revision(path, window, cx),
            ))
            .item(on(
                "Compare with Branch or Tag…",
                None,
                tracked,
                |this, path, window, cx| this.compare_with_branch(path, window, cx),
            ))
            .item(on("Show History", None, tracked, |this, path, _, cx| {
                this.open_file(path, false, cx)
            }))
            .item(on(
                "Show Current Revision",
                None,
                tracked && !added,
                |this, path, window, cx| this.show_current_revision(path, window, cx),
            ))
            .item(on(
                "Rollback…",
                Some(Box::new(Rollback)),
                tracked,
                |this, _, window, cx| this.rollback(window, cx),
            ))
            // Git.ContextMenu's merge part, while a merge has conflicts or resolved ones
            .when(merging && (conflicted || resolved), |menu| {
                let menu = menu.separator().when(conflicted, |menu| {
                    menu.item(on(
                        "Resolve Conflicts…",
                        None,
                        true,
                        |this, _, window, cx| this.resolve_conflicts_dialog(window, cx),
                    ))
                });
                revert_resolved(menu)
            })
            .map(|menu| {
                RepoView::custom_items(
                    menu,
                    &view,
                    crate::custom::Target::File,
                    Some(path.clone()),
                    cx,
                )
            })
    }
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::{add_trailer, inspection_problems, next_marks, patch_prefix};

    #[test]
    fn bases_patches_above_the_repository_only() {
        let base = std::env::temp_dir().join(format!("spork-patch-prefix-{}", std::process::id()));
        let root = base.join("repo");
        std::fs::create_dir_all(root.join("src")).unwrap();
        assert_eq!(patch_prefix(&root, &root).unwrap(), "");
        assert_eq!(patch_prefix(&root, &base).unwrap(), "repo/");
        assert!(patch_prefix(&root, &root.join("src")).is_err());
        assert!(patch_prefix(&root, &root.join("missing")).is_err());
        assert!(patch_prefix(&root, std::path::Path::new("")).is_err());
        std::fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn inspects_commit_messages() {
        let long = "x".repeat(73);
        let all = [true, true, true];
        assert!(inspection_problems("fix: a typo\n\nWhy.", all).is_empty());
        assert_eq!(inspection_problems("fix: a typo\nWhy.", all).len(), 1);
        assert_eq!(inspection_problems(&long, all).len(), 1);
        assert_eq!(
            inspection_problems(&format!("fix\n\n{long}"), all),
            vec!["Body line is longer than 72 characters"]
        );
        // an inspection turned off finds nothing
        assert!(inspection_problems(&format!("{long}\nx\n{long}"), [false; 3]).is_empty());
    }

    #[test]
    fn adds_trailers() {
        let t = "Co-authored-by: Ann <a@x>";
        assert_eq!(add_trailer("", t), t);
        assert_eq!(
            add_trailer("Why it changed.\n", t),
            format!("Why it changed.\n\n{t}")
        );
        let signed = "Body.\n\nSigned-off-by: Me <m@x>";
        assert_eq!(add_trailer(signed, t), format!("{signed}\n{t}"));
    }

    #[test]
    fn marks_ranges_and_toggles() {
        let list: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            next_marks(&[], Some("b"), "d", &list, true, false),
            s(&["b", "c", "d"])
        );
        assert_eq!(
            next_marks(&[], Some("c"), "a", &list, true, false),
            s(&["a", "b", "c"])
        );
        // Cmd/Ctrl-click: the selected file and the clicked one; again removes it
        let two = next_marks(&[], Some("a"), "c", &list, false, true);
        assert_eq!(two, s(&["a", "c"]));
        assert_eq!(
            next_marks(&two, Some("c"), "c", &list, false, true),
            s(&["a"])
        );
        // a plain click, or nothing selected in this list, marks nothing
        assert!(next_marks(&two, Some("a"), "b", &list, false, false).is_empty());
        assert!(next_marks(&two, None, "b", &list, true, true).is_empty());
    }
}
