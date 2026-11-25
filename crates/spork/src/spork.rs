//! Spork desktop app. P1: repository sidebar (docs/specs/M03) and commit list with graph (M05).
//! The standalone app starts at [`main`]; the IDE shows the same views in its windows ([`ide`]).

mod ai;
mod askpass;
mod bisect;
mod branches;
mod branches_pane;
mod branches_popup;
mod changes;
mod commit_files;
mod commit_side;
mod custom;
mod diff_bar;
mod diff_tools;
mod e2e;
mod file_view;
mod git_menu;
mod gitflow;
mod history;
mod hosting;
pub mod ide;
mod image_diff;
mod lfs;
mod log_toolbar;
mod new_worktree;
mod notices;
mod operations;
mod prefs;
mod project_popup;
mod project_view;
mod push_dialog;
mod rebase;
mod remotes;
mod repos;
mod search_everywhere;
mod settings;
mod settings_behavior;
mod settings_code;
mod settings_editor;
mod settings_tools;
mod settings_vcs;
mod settings_window;
mod shell;
mod spell;
mod ssh;
mod ssh_view;
mod stash_view;
mod stats_view;
mod terminal_panel;
mod util;
mod view_filters;
mod welcome;
mod workspace;
mod worktrees;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::notices::Notification;
use crate::util::ResultExt as _;
use changes::ChangeSelection;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{DiffEvent, DiffFileStatus, DiffMode, DiffState};
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable_panel;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Theme, TitleBar, h_flex, v_flex};
use gpui_kit::component::{Selectable as _, Sizable as _, WindowExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use history::{History, ROW_HEIGHT};
use spork_git::{
    CommitDetails, GitError, OpState, Ref, RefKind, Stash, StatusEntry, Submodule, Worktree,
};
use spork_workbench::shell::island;
use spork_workbench::tree::{Tree, TreeItem};
use spork_workbench::{Spinner, Tokens};

// shortcut: fixed cap until the history loads incrementally (M05 §1 batches), then it becomes a setting.
const MAX_COMMITS: usize = 50_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Changes,
    History,
    /// One file's history or blame (file_view.rs).
    File,
    /// A stash's changes (stash_view.rs).
    Stash,
    /// The selected commit's diff, opened from the log's changed-files tree (commit_side.rs).
    CommitDiff,
}

/// The tool windows that open on the left: branches and the rest, or local changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LeftTool {
    Commit,
    Project,
    PullRequests,
}

/// The tool window with the focus: its stripe button shows in the accent color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTool {
    Left(LeftTool),
    Git,
    Terminal,
    Notifications,
}

/// What a drag on the gap beside the left tool window carries.
#[derive(Clone)]
struct LeftResize;

impl Render for LeftResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

pub struct RepoView {
    requested: PathBuf,
    /// Menus and shortcuts dispatch to the focused element, so the view keeps focus.
    focus: FocusHandle,
    view: View,
    /// `None` until the first load finishes.
    repo: Option<Result<Loaded, GitError>>,
    selected: Option<usize>,
    /// Details of the selected commit; `None` while loading.
    details: Option<Result<CommitDetails, GitError>>,
    diff: Entity<DiffState>,
    /// Scrolls the commit list to the commit chosen with the keyboard (shell.rs).
    history_scroll: UniformListScrollHandle,
    /// Quick filter over the commit list (view_filters.rs, M05 §5) and how many commits match.
    commit_filter: Entity<InputState>,
    /// The stash shown in View::Stash, and whether its diff loaded (stash_view.rs).
    stash_shown: Option<(spork_git::Stash, Option<Result<(), String>>)>,
    commit_matches: usize,
    /// The log toolbar's width at the last frame (log_toolbar.rs).
    log_toolbar_width: std::rc::Rc<std::cell::Cell<Pixels>>,
    /// The log toolbar's other filters (view_filters.rs) and git's search for the path one.
    log_filters: view_filters::LogFilters,
    _path_search: Option<Task<()>>,
    /// The find box searches changes or paths with git instead of the loaded commits' text
    /// (view_filters.rs), and git's matches once they arrive.
    search_in: Option<spork_git::log::SearchIn>,
    search_hits: Option<std::collections::HashSet<String>>,
    _search: Option<Task<()>>,
    _commit_filter_events: Subscription,
    sidebar_filter: Entity<InputState>,
    _sidebar_filter_events: Subscription,
    /// Each diff's patch text, by its state, for Find and the change map (diff_tools.rs).
    diff_texts: HashMap<EntityId, diff_tools::DiffInfo>,
    /// The find box in the diff bar, its text, and which match is current.
    diff_find: Entity<InputState>,
    diff_query: String,
    diff_find_ix: usize,
    _diff_find_events: Subscription,
    /// Text the local changes are narrowed to, in lower case (changes.rs); empty: all.
    changes_query: String,
    /// Unified or side by side, for every diff (diff_bar.rs).
    diff_mode: DiffMode,
    prefs: prefs::Prefs,
    /// View filters of the commit list (view_filters.rs).
    filters: spork_git::log::Filters,
    hide_tags: bool,
    /// Stashes stay out of the commit list (View menu).
    hide_stashes: bool,
    /// Local Changes also lists ignored files (changes.rs).
    show_ignored: bool,
    _load: Option<Task<()>>,
    _details_load: Option<Task<()>>,
    /// Watches the repository so external changes (terminal, IDE) show up without Refresh.
    _watcher: Option<(notify::RecommendedWatcher, Task<()>)>,
    /// A status-only refresh under way (`refresh_status`).
    _status_load: Option<Task<()>>,
    // working copy (changes.rs)
    change_selected: Option<ChangeSelection>,
    change_diff: Entity<DiffState>,
    /// Raw patch of the open file, for line staging (spork_patch).
    change_patch: Option<String>,
    _diff_events: Subscription,
    _change_diff_load: Option<Task<()>>,
    /// The commit message box (changes.rs): its first line is the subject.
    message: Entity<TextareaState>,
    /// Name field of the "New branch" dialog (branches.rs).
    branch_name: Entity<InputState>,
    /// Answer field of git's password / passphrase prompts (askpass.rs).
    prompt_input: Entity<InputState>,
    /// The SSH passphrase prompt's "Remember" (askpass.rs).
    remember_passphrase: bool,
    /// The tool window on the left, one at a time as in IntelliJ, and its width (dragged on the
    /// gap beside it).
    left_tools: Vec<LeftTool>,
    left_width: Pixels,
    /// The Project tool window's tree (project_view.rs).
    project: project_view::ProjectTree,
    /// The Commit tool window's Shelf and Stash tabs (stash_view.rs).
    saved: stash_view::Saved,
    /// The Commit tool window's Unversioned Files are open.
    unversioned_open: bool,
    /// The rest of the Commit tool window's state (changes.rs).
    commit_ui: changes::CommitUi,
    /// Amend on: the commit being amended (its subject, its files), shown in the Commit window
    /// as IntelliJ does; its node open or not.
    amend_commit: Option<(String, Vec<(char, String)>)>,
    amend_open: bool,
    /// The log's branches pane (branches_pane.rs): shown, tags in it, and a new tree id after
    /// Expand / Collapse All, with every node starting open or closed.
    branches_open: bool,
    branches_show_tags: bool,
    /// Group by Directory: `feature/login` in a `feature` folder.
    branches_group_dirs: bool,
    branches_tree_gen: usize,
    branches_expanded: bool,
    /// The log's changed-files tree: folders closed, the file picked (commit_side.rs).
    commit_tree_collapsed: std::collections::HashSet<String>,
    commit_file_selected: Option<String>,
    /// The log's toolbars and panes (log_toolbar.rs, commit_side.rs).
    log_ui: log_toolbar::LogUi,
    /// The Terminal tool window (terminal_panel.rs): its tabs, kept while hidden, the one shown,
    /// whether it shows, its height, maximized; why the last shell couldn't start.
    terminals: Vec<terminal_panel::TerminalTab>,
    active_terminal: usize,
    show_terminal: bool,
    /// The Git tool window (⌘9) at the bottom, in the terminal's place: its Console tab.
    show_console: bool,
    /// Shown in the IDE's tool windows: diffs open as editor tabs there (ide.rs).
    pub(crate) ide: bool,
    /// The commits on the current branch for the log's Current Branch highlight: for which HEAD
    /// and how many loaded commits they were found.
    branch_commits: (
        String,
        usize,
        std::rc::Rc<std::collections::HashSet<String>>,
    ),
    /// The tool window last clicked in or opened, None when it was the log.
    active_tool: Option<ActiveTool>,
    /// The toolbar's Git Branches popup, kept between openings so its folders stay as left.
    branches_popup: Option<(Entity<branches_popup::BranchesPopup>, Subscription)>,
    /// Compare with Branch or Tag…'s branches popup, while it shows (branches_popup.rs).
    compare_popup: Option<(Entity<branches_popup::BranchesPopup>, Subscription)>,
    branches_popup_open: bool,
    /// Where the branch widget is, for the popup to open under it.
    branches_chip: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// The project widget's popup, and where the widget is.
    project_popup: Option<(Entity<project_popup::ProjectPopup>, Subscription)>,
    project_popup_open: bool,
    project_chip: std::rc::Rc<std::cell::Cell<Bounds<Pixels>>>,
    /// The Git tool window's tab: Worktrees, or the console; the console's Soft-Wrap and
    /// Scroll to the End toggles, its scroll, and how many commands it showed last.
    worktrees_tab: bool,
    /// The IDE's Worktrees tab opened by Worktrees…: it shows without linked worktrees too.
    worktrees_opened: bool,
    /// The Worktrees tab closed with its ×, until Git ▸ Worktrees shows it again.
    worktrees_closed: bool,
    console_soft_wrap: bool,
    console_follow: bool,
    console_scroll: ScrollHandle,
    console_seen: usize,
    /// The row selected in the Worktrees tab.
    selected_worktree: Option<PathBuf>,
    /// The Notifications tool window on the right (notices.rs).
    show_notifications: bool,
    /// Set at each render: the window is in front; the log (not a text field) has the focus.
    window_active: bool,
    log_focused: bool,
    commit_focused: bool,
    terminal_height: Pixels,
    terminal_max: bool,
    terminal_error: Option<String>,
    /// Save the terminal tabs when Spork quits, and when this tab or its window closes.
    _save_terminals: [Subscription; 2],
    /// The Stash dialog's Message, Include untracked and Keep index (stash_view.rs).
    stash_message: Entity<TextareaState>,
    stash_untracked: bool,
    stash_keep: bool,
    /// "Also rename on the remote" in the Rename dialog (branches.rs).
    rename_on_remote: bool,
    rebase_plan: Option<rebase::RebasePlan>,
    file: Option<file_view::FileView>,
    /// Before / after of a changed image in Local Changes (image_diff.rs).
    change_images: Option<image_diff::ImagePair>,
    /// Files marked with Cmd/Ctrl- or Shift-click in the selected file's list (changes.rs).
    change_marked: Vec<String>,
    /// A moved submodule's path and commit summary in Local Changes (changes.rs).
    change_submodule: Option<(String, String)>,
    /// The open "AI settings" dialog and a running AI request (ai.rs).
    ai_form: Option<ai::AiForm>,
    /// The merge dialog's choice: how the branch comes in, and commit right away (branches.rs).
    merge_options: (spork_git::MergeKind, bool),
    /// The delete-branch dialog's "also delete it on the remote" (branches.rs).
    delete_remote_too: bool,
    /// Finish Branch's "delete it afterwards" (branches.rs).
    finish_delete: bool,
    /// Commit and stash diffs show whole files rather than the changes with a few lines around.
    whole_file: bool,
    /// Which parent a selected merge commit's changes are shown against (0: the first).
    merge_parent: usize,
    /// What the open merge / rebase dialog expects: `None` while checking, then git's answer
    /// (`None` inside when it can't tell) (branches.rs).
    conflicts: Option<Option<Vec<String>>>,
    _conflict_check: Option<Task<()>>,
    /// The fetch / pull / push under way, whose toolbar button spins (only while `busy`).
    remote_running: Option<RemoteOp>,
    /// The service picked in the Connect dialog, and a browser sign-in waiting for approval
    /// (hosting.rs).
    connect_as: spork_hosting::Service,
    _sign_in: Option<Task<()>>,
    /// The second commit when two are compared (`compare_with`).
    compare: Option<usize>,
    /// The commits the comparison spans, oldest first, when they are one line of history: what
    /// its Cherry-pick, Revert and Save as patches act on.
    compare_run: Vec<String>,
    /// The selected commit's File Tree, while that tab is open (commit_files.rs).
    commit_files: Option<commit_files::CommitFiles>,
    /// Commit options in Local Changes: add a Signed-off-by line; HEAD is already pushed (amend).
    signoff: bool,
    amend_pushed: bool,
    /// Title and description fields of the open "Create pull request" dialog (hosting.rs).
    pr_form: Option<(Entity<InputState>, Entity<TextareaState>)>,
    ai_busy: bool,
    _ai: Option<Task<()>>,
    /// Where the running bisect stands, once a step was taken in Spork (bisect.rs).
    bisect: Option<spork_git::bisect::Progress>,
    /// The repository on GitHub and its pull requests (hosting.rs).
    hosted: Option<hosting::Hosted>,
    _pulls_load: Option<Task<()>>,
    /// The open "Repository settings" dialog's state (settings.rs).
    settings: Option<settings::RepoSettings>,
    /// Recently opened repositories (repos.rs).
    recent: Vec<PathBuf>,
    /// Repositories found in the source folders (prefs.rs), for Quick Launch.
    scanned: Vec<PathBuf>,
    /// Files opened lately, newest first: Search Everywhere lists them before anything is typed.
    recent_files: Vec<String>,
    search_everywhere: Option<(Entity<search_everywhere::SearchEverywhere>, Subscription)>,
    _scan: Option<Task<()>>,
    clone_url: Entity<InputState>,
    tag_message: Entity<InputState>,
    /// Name field of the git-flow "Start" dialog (gitflow.rs).
    flow_name: Entity<InputState>,
    /// Field of the one-input dialogs (`input_dialog`, operations.rs).
    field: Entity<InputState>,
    /// Name field of the "Add remote" dialog (remotes.rs).
    remote_name: Entity<InputState>,
    _prompts: Option<Task<()>>,
    /// "Edit commit message" dialog (operations.rs).
    reword: Entity<TextareaState>,
    amend: bool,
    /// A git write (stage, commit…) is running.
    busy: bool,
    _op: Option<Task<()>>,
    /// Redraws while an operation runs, for its progress line (changes.rs `job_banner`).
    _job_poll: Option<Task<()>>,
    _appearance: Subscription,
    _commit_keys: [Subscription; 1],
    /// Background fetch every `prefs.fetch_minutes` (M09 §1).
    _auto_fetch: Task<()>,
    _fetching: Option<Task<()>>,
    last_fetch: std::time::Instant,
    fetch_failed: bool,
}

struct Loaded {
    root: PathBuf,
    head: Option<String>,
    refs: Vec<Ref>,
    status: Vec<StatusEntry>,
    op: OpState,
    stashes: Vec<Stash>,
    /// The project's shelf (stash_view.rs).
    shelves: Vec<spork_git::shelf::Shelved>,
    worktrees: Vec<Worktree>,
    submodules: Vec<Submodule>,
    gitflow: Option<spork_git::gitflow::Gitflow>,
    /// A bisect is running (bisect.rs).
    bisecting: bool,
    /// A shallow clone: the Git menu offers Unshallow repository.
    shallow: bool,
    remotes: Vec<spork_git::remote::Remote>,
    /// Starred branches, shown in Favorites (`spork.favorite` in git config, M03 §1).
    favorites: Vec<String>,
    /// Branches checked out lately, newest first: the branches popup's Recent.
    recent_branches: Vec<String>,
    /// Branches left out of the graph (`spork.hidden`, M03 §3).
    hidden: Vec<String>,
    /// When not empty, the only branches the graph shows (`spork.only`).
    only: Vec<String>,
    /// Words of the commit subjects and changed paths, which spell check accepts (spell.rs).
    vocabulary: std::collections::HashSet<String>,
    /// Where `PROJ-123` keys in messages link to (`spork.issueTracker`, hosting.rs).
    tracker: Option<String>,
    /// Your email (`user.email`): IntelliJ shows your commits' author in bold.
    me: Option<String>,
    history: History,
}

impl RepoView {
    fn new(
        requested: PathBuf,
        prompts: Option<smol::channel::Receiver<askpass::Prompt>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // follow the OS light/dark setting live
        let prefs = prefs::Prefs::load();
        prefs.apply_appearance(window, cx);
        let diff_mode = if prefs.split_diffs {
            DiffMode::Split
        } else {
            DiffMode::Unified
        };
        // follow the OS light/dark setting live, unless the user picked one
        let _appearance = cx.observe_window_appearance(window, |this: &mut Self, window, cx| {
            this.prefs.apply_appearance(window, cx)
        });
        let change_diff = cx.new(|cx| DiffState::new([], cx).with_mode(diff_mode));
        // the selection bar above the diff follows the line selection
        let _diff_events = cx.subscribe(&change_diff, |_, _, event: &DiffEvent, cx| {
            if matches!(
                event,
                DiffEvent::SelectionChanged(_)
                    | DiffEvent::SelectionEnded(_)
                    | DiffEvent::ConflictResolved(..)
            ) {
                cx.notify();
            }
        });
        let message = cx.new(|cx| TextareaState::new(window, cx).placeholder("Commit Message"));
        let _commit_keys = [cx.subscribe_in(&message, window, Self::on_commit_input)];
        let commit_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Text or hash"));
        let _commit_filter_events = cx.subscribe_in(&commit_filter, window, Self::on_commit_filter);
        let sidebar_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Branch or tag"));
        let _sidebar_filter_events = cx.subscribe(&sidebar_filter, |_, _, e: &InputEvent, cx| {
            if matches!(e, InputEvent::Change) {
                cx.notify();
            }
        });
        let diff_find = cx.new(|cx| InputState::new(window, cx).placeholder("Find in diff"));
        let _diff_find_events = cx.subscribe(&diff_find, |this, state, e: &InputEvent, cx| {
            match e {
                InputEvent::Change => {
                    this.diff_query = state.read(cx).value().trim().to_owned();
                    this.diff_find_ix = 0;
                    this.find_again();
                    this.find_step(0, cx);
                }
                // Enter: the next match, Shift-Enter the one before
                InputEvent::PressEnter { shift, .. } => {
                    this.find_step(if *shift { -1 } else { 1 }, cx)
                }
                _ => {}
            }
        });
        let _auto_fetch = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(60))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.auto_fetch(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        // its window focuses it (workspace.rs); the IDE leaves the focus where it is
        let focus = cx.focus_handle();
        let mut view = Self {
            requested,
            focus,
            view: View::History,
            repo: None,
            selected: None,
            details: None,
            diff: cx.new(|cx| DiffState::new([], cx).with_mode(diff_mode)),
            diff_mode,
            history_scroll: UniformListScrollHandle::new(),
            prefs,
            filters: Default::default(),
            hide_tags: false,
            hide_stashes: false,
            show_ignored: false,
            commit_filter,
            commit_matches: 0,
            log_toolbar_width: std::rc::Rc::new(std::cell::Cell::new(px(f32::MAX))),
            log_filters: Default::default(),
            _path_search: None,
            search_in: None,
            search_hits: None,
            _search: None,
            stash_shown: None,
            _commit_filter_events,
            sidebar_filter,
            _sidebar_filter_events,
            diff_texts: HashMap::new(),
            diff_find,
            diff_query: String::new(),
            diff_find_ix: 0,
            _diff_find_events,
            changes_query: String::new(),
            _load: None,
            _details_load: None,
            _status_load: None,
            _watcher: None,
            change_selected: None,
            change_diff,
            _change_diff_load: None,
            change_patch: None,
            _diff_events,
            message,
            branch_name: cx.new(|cx| InputState::new(window, cx).placeholder("feature/my-change")),
            prompt_input: cx.new(|cx| InputState::new(window, cx)),
            remember_passphrase: false,
            left_tools: vec![LeftTool::Project],
            left_width: px(285.),
            project: Default::default(),
            saved: stash_view::Saved::new(window, cx),
            unversioned_open: false,
            commit_ui: changes::CommitUi::new(cx),
            amend_commit: None,
            amend_open: true,
            branches_open: true,
            branches_show_tags: true,
            branches_group_dirs: true,
            branches_tree_gen: 0,
            branches_expanded: true,
            commit_tree_collapsed: Default::default(),
            commit_file_selected: None,
            log_ui: Default::default(),
            terminals: Vec::new(),
            active_terminal: 0,
            show_terminal: false,
            show_console: false,
            ide: false,
            active_tool: None,
            branch_commits: Default::default(),
            branches_popup: None,
            compare_popup: None,
            branches_popup_open: false,
            branches_chip: Default::default(),
            project_popup: None,
            project_popup_open: false,
            project_chip: Default::default(),
            worktrees_tab: false,
            worktrees_opened: false,
            worktrees_closed: false,
            console_soft_wrap: false,
            console_follow: true,
            console_scroll: ScrollHandle::new(),
            console_seen: 0,
            selected_worktree: None,
            show_notifications: false,
            window_active: true,
            log_focused: false,
            commit_focused: false,
            terminal_height: px(280.),
            terminal_max: false,
            terminal_error: None,
            _save_terminals: [
                cx.on_app_quit(|this: &mut Self, cx| {
                    this.save_terminals(cx);
                    async {}
                }),
                cx.on_release(|this, cx| this.save_terminals(cx)),
            ],
            stash_message: cx.new(|cx| TextareaState::new(window, cx)),
            stash_untracked: false,
            stash_keep: false,
            rename_on_remote: false,
            rebase_plan: None,
            file: None,
            change_images: None,
            change_submodule: None,
            change_marked: Vec::new(),
            settings: None,
            hosted: None,
            bisect: None,
            ai_form: None,
            pr_form: None,
            merge_options: Default::default(),
            delete_remote_too: false,
            finish_delete: true,
            whole_file: false,
            commit_files: None,
            compare: None,
            connect_as: spork_hosting::Service::GitHub,
            _sign_in: None,
            remote_running: None,
            conflicts: None,
            _conflict_check: None,
            merge_parent: 0,
            compare_run: Vec::new(),
            signoff: false,
            amend_pushed: false,
            ai_busy: false,
            _ai: None,
            _pulls_load: None,
            recent: repos::recent(),
            scanned: Vec::new(),
            recent_files: Vec::new(),
            search_everywhere: None,
            _scan: None,
            clone_url: cx.new(|cx| {
                InputState::new(window, cx).placeholder("https://github.com/org/repo.git")
            }),
            tag_message: cx.new(|cx| InputState::new(window, cx).placeholder("Message (optional)")),
            flow_name: cx.new(|cx| InputState::new(window, cx)),
            field: cx.new(|cx| InputState::new(window, cx)),
            remote_name: cx.new(|cx| InputState::new(window, cx).placeholder("upstream")),
            _prompts: prompts.map(|rx| {
                cx.spawn_in(window, async move |this, cx| {
                    while let Ok((prompt, reply)) = rx.recv().await {
                        this.update_in(cx, |this, window, cx| this.ask(prompt, reply, window, cx))
                            .ok();
                    }
                })
            }),
            reword: cx.new(|cx| TextareaState::new(window, cx)),
            amend: false,
            busy: false,
            _op: None,
            _job_poll: None,
            _appearance,
            _commit_keys,
            _auto_fetch,
            _fetching: None,
            last_fetch: std::time::Instant::now(),
            fetch_failed: false,
        };
        view.reload(cx);
        view.scan_source_folders(cx);
        view
    }

    /// Finds the repositories in the source folders in the background (M01 §3).
    pub(crate) fn scan_source_folders(&mut self, cx: &mut Context<Self>) {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        // `~/Projects` is how people write it; expand the tilde
        let roots: Vec<PathBuf> = self
            .prefs
            .source_paths()
            .into_iter()
            .map(|p| match (p.strip_prefix("~"), &home) {
                (Ok(rest), Some(home)) => PathBuf::from(home).join(rest),
                _ => p,
            })
            .collect();
        self._scan = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move { repos::scan(&roots, 3) })
                .await;
            this.update(cx, |this, _| this.scanned = found).ok();
        }));
    }

    /// Reads the repository on a background thread; the current view stays until the new data is in.
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.project.invalidate();
        let (path, filters, hide_tags) = (self.requested.clone(), self.filters, self.hide_tags);
        let (hide_stashes, show_ignored) = (self.hide_stashes, self.show_ignored);
        let needs_watcher = self._watcher.is_none();
        self._load = Some(cx.spawn(async move |this, cx| {
            let (watcher, loaded) = cx
                .background_spawn(async move {
                    // watching starts before the repository is read: what changes while it
                    // loads (a merge started from a terminal) reloads it once loaded
                    // not a repository: the load says so
                    let root = needs_watcher
                        .then(|| spork_git::toplevel(&path).ok())
                        .flatten();
                    let watcher = root.and_then(start_watcher);
                    let loaded = load(&path, filters, hide_tags, hide_stashes, show_ignored);
                    (watcher, loaded)
                })
                .await;
            this.update(cx, |this, cx| {
                // keep the same commit selected even if new commits shifted the rows
                let selected_id = this.selected_commit_id();
                let compare_id = match (&this.repo, this.compare) {
                    (Some(Ok(repo)), Some(c)) => repo.history.commits.get(c).map(|c| c.id.clone()),
                    _ => None,
                };
                let old_root = match &this.repo {
                    Some(Ok(repo)) => Some(repo.root.clone()),
                    _ => None,
                };
                let opened = matches!(&loaded, Ok(repo) if Some(&repo.root) != old_root.as_ref());
                // a remote added or changed may be on another hosting service
                let remotes_changed = match (&this.repo, &loaded) {
                    (Some(Ok(old)), Ok(new)) => old.remotes != new.remotes,
                    _ => false,
                };
                if let Ok(repo) = &loaded {
                    repos::remember(&repo.root);
                    this.recent = repos::recent();
                }
                this.repo = Some(loaded);
                this.relayout_graph();
                if opened {
                    this.load_pulls(cx);
                    this.restore_terminals(cx);
                } else if remotes_changed {
                    this.load_pulls(cx);
                }
                if let Some(Ok(repo)) = &this.repo {
                    let ix = selected_id
                        .and_then(|id| repo.history.commits.iter().position(|c| c.id == id));
                    if ix.is_none() {
                        this.selected = None;
                        this.details = None;
                    }
                    this.selected = ix;
                    // the compared commit is found again by id; gone with the selection or itself
                    this.compare = compare_id
                        .and_then(|id| repo.history.commits.iter().position(|c| c.id == id))
                        .filter(|_| ix.is_some());
                    // a stash that was applied and removed, or dropped, leaves its view
                    let stash_gone = !this
                        .stash_shown
                        .as_ref()
                        .is_some_and(|(s, _)| repo.stashes.iter().any(|t| t.id == s.id));
                    // the IDE's diff tab may show it while the view is another
                    if stash_gone && this.saved.shelf_shown.is_none() {
                        if this.view == View::Stash {
                            this.view = View::History;
                        }
                        this.stash_shown = None;
                    }
                    if this._watcher.is_none()
                        && let Some((watcher, changes)) = watcher
                    {
                        this.watch(watcher, changes, cx);
                    }
                }
                // refresh the open file diff (or clear it if the file is no longer changed)
                this.select_change(this.change_selected.clone(), cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Only the working tree changed (files edited, staged or unstaged): re-reads the status,
    /// not the history, which in a large repository is most of a reload's work.
    fn refresh_status(&mut self, cx: &mut Context<Self>) {
        self.project.invalidate();
        let Some(Ok(repo)) = &self.repo else { return };
        let (root, ignored) = (repo.root.clone(), self.show_ignored);
        self._status_load = Some(cx.spawn(async move |this, cx| {
            let status = cx
                .background_spawn(async move { spork_git::status_with(&root, ignored) })
                .await;
            this.update(cx, |this, cx| {
                if let (Some(Ok(repo)), Ok(status)) = (&mut this.repo, status) {
                    repo.status = status;
                }
                // refresh the open file diff (or clear it if the file is no longer changed)
                this.select_change(this.change_selected.clone(), cx);
                cx.notify();
            })
            .ok();
        }));
    }

    fn selected_commit_id(&self) -> Option<String> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        Some(repo.history.commits.get(self.selected?)?.id.clone())
    }

    /// Refreshes on the changes `start_watcher` reports, after 300 ms of quiet (once per burst):
    /// the status alone, or everything.
    fn watch(
        &mut self,
        watcher: notify::RecommendedWatcher,
        changes: smol::channel::Receiver<bool>,
        cx: &mut Context<Self>,
    ) {
        let task = cx.spawn(async move |this, cx| {
            while let Ok(mut full) = changes.recv().await {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(300))
                        .await;
                    if changes.is_empty() {
                        break;
                    }
                    while let Ok(more) = changes.try_recv() {
                        full |= more;
                    }
                }
                let refreshed = this.update(cx, |this, cx| {
                    if full {
                        this.reload(cx)
                    } else {
                        this.refresh_status(cx)
                    }
                });
                if refreshed.is_err() {
                    break;
                }
            }
        });
        self._watcher = Some((watcher, task));
    }
}

impl RepoView {
    /// Selects a commit and loads its details + patch in the background (M06).
    pub(crate) fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(commit) = repo.history.commits.get(ix) else {
            return;
        };
        if self.selected != Some(ix) {
            self.merge_parent = 0;
            self.commit_file_selected = None;
        }
        self.selected = Some(ix);
        self.compare = None;
        self.details = None;
        // the File Tree tab stays open for the newly selected commit
        let files_open = self.commit_files.is_some();
        let (root, id) = (repo.root.clone(), commit.id.clone());
        let ignore_ws = self.prefs.ignore_whitespace;
        let parent = commit
            .parents
            .get(self.merge_parent)
            .or(commit.parents.first())
            .cloned();
        // replacing the task drops (cancels) a load for a previously clicked commit
        self._details_load = Some(cx.spawn(async move |this, cx| {
            let details = cx
                .background_spawn(async move {
                    spork_git::commit_details(&root, &id, parent.as_deref(), ignore_ws)
                })
                .await;
            this.update(cx, |this, cx| {
                // shortcut: whole-patch parse; huge commits get a per-file lazy diff in M08
                let patch = match &details {
                    Ok(d) if d.patch.len() <= MAX_PATCH_BYTES => d.patch.as_str(),
                    _ => "",
                };
                let diff = this.diff.clone();
                this.set_patch(&diff, patch, cx);
                if this.whole_file {
                    diff.update(cx, |state, cx| state.expand_unchanged(cx));
                }
                this.details = Some(details);
                cx.notify();
            })
            .ok();
        }));
        if files_open {
            self.show_commit_files(cx);
        }
        cx.notify();
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemoteOp {
    Fetch,
    Pull,
    Push,
}

impl RepoView {
    /// Fetch / Pull / Push in the background; the outcome is a notification (docs/specs/M09).
    fn remote_op(&mut self, op: RemoteOp, window: &mut Window, cx: &mut Context<Self>) {
        self.remote_running = Some(op);
        self.run_op(
            window,
            cx,
            move |root| match op {
                RemoteOp::Fetch => {
                    spork_git::fetch(root).map(|()| Some("Fetched all remotes".into()))
                }
                RemoteOp::Pull => spork_git::pull(root).map(|()| Some("Pulled".into())),
                // rejected: the remote has commits we don't; asked about next, not an error
                RemoteOp::Push => match spork_git::push(root) {
                    Err(GitError::NonFastForward) => Ok(None),
                    other => other.map(Some),
                },
            },
            move |this, message, window, cx| {
                let Some(message) = message else {
                    return this.push_rejected(window, cx);
                };
                window.push_notification(Notification::success(message), cx);
                // a fetch is when new pull requests are expected
                if matches!(op, RemoteOp::Fetch) {
                    this.load_pulls(cx);
                }
            },
        );
    }

    /// Fetches quietly when the preference's interval has passed and nothing else runs.
    fn auto_fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let interval = Duration::from_secs(u64::from(self.prefs.fetch_minutes) * 60);
        let Some(Ok(repo)) = &self.repo else { return };
        if interval.is_zero() || self.busy || self.last_fetch.elapsed() < interval {
            return;
        }
        self.last_fetch = std::time::Instant::now();
        let root = repo.root.clone();
        self._fetching = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { spork_git::fetch(&root) })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(()) => {
                    this.fetch_failed = false;
                    this.reload(cx);
                    this.load_pulls(cx);
                }
                // one warning per streak of failures: background work must not nag
                Err(e) if !this.fetch_failed => {
                    this.fetch_failed = true;
                    window.push_notification(
                        Notification::warning(format!("Automatic fetch failed: {e}")),
                        cx,
                    );
                }
                Err(_) => {}
            })
            .ok();
        }));
    }

    fn remote_button(&self, op: RemoteOp, cx: &mut Context<Self>) -> AnyElement {
        let (id, icon, tip) = match op {
            RemoteOp::Fetch => ("fetch", "vcs/fetch", "Fetch all remotes"),
            RemoteOp::Pull => ("pull", "vcs/update", "Update Project"),
            RemoteOp::Push => ("push", "vcs/push", "Push…"),
        };
        // spinning while that operation runs (it lasts as long as `busy`)
        if self.busy && self.remote_running == Some(op) {
            return div()
                .size(px(spork_workbench::tokens::metrics::MAIN_TOOLBAR_BUTTON))
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new().small())
                .into_any_element();
        }
        spork_workbench::ActionButton::new(id, icon)
            .size(spork_workbench::ActionSize::MainToolbar)
            .tooltip(tip)
            .disabled(self.busy)
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                // ⌥ / Alt-click pulls straight away, as the dialog last would
                match op {
                    RemoteOp::Pull if !event.modifiers().alt => this.pull_dialog(window, cx),
                    RemoteOp::Push => this.push_commits_dialog(window, cx),
                    op => this.remote_op(op, window, cx),
                }
            }))
            .into_any_element()
    }
}

const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;

/// Watches the work tree and `.git` of the repository at `root`; the changes queue up on the
/// receiver, each true when a full reload is needed. Edited files and the index only need the
/// status re-read; anything else in `.git` (refs, HEAD, a merge or rebase starting) reloads
/// everything. What git ignores (build output like `target/`) and git's object store are left
/// out: a build or a fetch writes thousands of files there.
fn start_watcher(
    root: PathBuf,
) -> Option<(notify::RecommendedWatcher, smol::channel::Receiver<bool>)> {
    use notify::{EventKind, RecursiveMode, Watcher as _};
    let (tx, rx) = smol::channel::unbounded::<bool>();
    let git_dir = root.join(".git");
    let objects = git_dir.join("objects");
    let index = git_dir.join("index");
    let read_ignored = {
        let root = root.clone();
        move || -> Vec<PathBuf> {
            spork_git::ignored_paths(&root)
                .iter()
                .map(|p| root.join(p.trim_end_matches('/')))
                .collect()
        }
    };
    // looked up again now and then: a first build creates its folder after the watch starts
    let mut ignored = (read_ignored(), std::time::Instant::now());
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // reading files (including our own git reads) must not trigger reloads
        let Ok(event) = event else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if ignored.1.elapsed() > Duration::from_secs(20) {
            ignored = (read_ignored(), std::time::Instant::now());
        }
        let noise =
            |p: &PathBuf| p.starts_with(&objects) || ignored.0.iter().any(|i| p.starts_with(i));
        if event.paths.is_empty() || !event.paths.iter().all(noise) {
            let full = event.paths.is_empty()
                || event.paths.iter().any(|p| {
                    p.starts_with(&git_dir) && !p.starts_with(&objects) && !p.starts_with(&index)
                });
            tx.try_send(full).ok();
        }
    })
    .log_err()?;
    watcher.watch(&root, RecursiveMode::Recursive).log_err()?;
    Some((watcher, rx))
}

fn load(
    path: &Path,
    filters: spork_git::log::Filters,
    hide_tags: bool,
    hide_stashes: bool,
    show_ignored: bool,
) -> Result<Loaded, GitError> {
    let root = spork_git::toplevel(path)?;
    let refs = spork_git::refs(&root)?;
    let status = spork_git::status_with(&root, show_ignored)?;
    let op = spork_git::op_state(&root)?;
    let stashes = spork_git::stashes(&root)?;
    let worktrees = spork_git::worktrees(&root)?;
    let submodules = spork_git::submodules(&root)?;
    let labelled: Vec<Ref> = refs
        .iter()
        .filter(|r| !(hide_tags && r.kind == RefKind::Tag))
        .filter(|r| !(filters.hide_remotes && r.kind == RefKind::Remote))
        .cloned()
        .collect();
    let hidden = spork_git::config::get_all(&root, "spork.hidden");
    let only = spork_git::config::get_all(&root, "spork.only");
    let mut commits = spork_git::log::filtered(&root, MAX_COMMITS, filters, &hidden, &only)?;
    let folded: HashMap<String, usize> = spork_git::config::get_all(&root, "spork.folded")
        .into_iter()
        .map(|merge| {
            let n = spork_git::log::fold_merge(&root, &mut commits, &merge);
            (merge, n)
        })
        .filter(|(_, n)| *n > 0)
        .collect();
    if !hide_stashes {
        spork_git::log::insert_stashes(&mut commits, &stashes);
    }
    let mut history = History::new(commits, &labelled);
    history.folded = folded;
    history.stashes = stashes
        .iter()
        .filter(|_| !hide_stashes)
        .map(|s| (s.id.clone(), s.clone()))
        .collect();
    let vocabulary = spell::vocabulary(
        history
            .commits
            .iter()
            .map(|c| c.subject.as_str())
            .chain(status.iter().map(|e| e.path.as_str())),
    );
    Ok(Loaded {
        head: spork_git::head_branch(&root)?,
        refs,
        status,
        op,
        stashes,
        shelves: spork_git::shelf::list(&root),
        worktrees,
        submodules,
        gitflow: spork_git::gitflow::read(&root),
        bisecting: spork_git::bisect::is_running(&root),
        shallow: spork_git::is_shallow(&root),
        remotes: spork_git::remote::remotes(&root),
        favorites: branches::favorites(&root),
        recent_branches: spork_git::branch::recent_checkouts(&root),
        hidden,
        only,
        vocabulary,
        tracker: spork_git::config::get(&root, "spork.issueTracker"),
        // bold only tells your commits apart when others have some too
        me: spork_git::config::get(&root, "user.email").filter(|me| {
            history
                .commits
                .iter()
                .any(|c| !c.email.eq_ignore_ascii_case(me))
        }),
        history,
        root,
    })
}

/// A branch row named `label`. `hidden`: the branch is left out of the graph (shown in its
/// badge).
fn branch_item(r: &Ref, view: &Entity<RepoView>, label: String, marks: &RefMarks) -> TreeItem {
    // IntelliJ's marks: ↙ commits to pull, in blue; ↗ commits to push, in green
    let incoming = (r.behind > 0).then(|| format!("↙ {}", r.behind.min(99)));
    let outgoing = (r.ahead > 0).then(|| format!("↗ {}", r.ahead.min(99)));
    let note = marks.hidden.contains(&r.name).then_some("hidden");
    let favorite = marks.favorites.contains(&r.name);
    let picked = marks
        .picked
        .is_some_and(|(kind, name)| *kind == r.kind && *name == r.name);
    let item = TreeItem::new(label)
        .active(picked)
        .on_click(branches_pane::ref_click(r, false, view.clone()))
        .context_menu(branches::ref_menu(r, view.clone()));
    let item = match (r.is_head, favorite) {
        (true, true) => item.colored_icon("dvcs/currentBranchFavoriteLabel"),
        (true, false) => item.colored_icon("dvcs/currentBranchLabel"),
        (false, true) => item.colored_icon("nodes/star"),
        (false, false) => item.icon(IconName::GitBranch),
    };
    if incoming.is_none() && outgoing.is_none() && note.is_none() {
        return item;
    }
    item.suffix(move |_, cx| {
        let dark = cx.theme().mode.is_dark();
        let (blue, green) = match dark {
            true => (rgb(0x548AF7), rgb(0x57965C)),
            false => (rgb(0x3574F0), rgb(0x369650)),
        };
        h_flex()
            .gap_1()
            .children(incoming.clone().map(|t| div().text_color(blue).child(t)))
            .children(outgoing.clone().map(|t| div().text_color(green).child(t)))
            .children(note.map(|t| div().child(t)))
    })
}

/// Groups `a/b/c` names into folders, as in M03 §1 (`feature/login` → `feature ▸ login`), the
/// first `depth` segments left out; `group` off (Group by Directory) lists them flat.
/// `open`: folders start expanded, as while the sidebar is filtered.
fn folder_tree(
    refs: &[&Ref],
    depth: usize,
    open: bool,
    group: bool,
    marks: &RefMarks,
    view: &Entity<RepoView>,
) -> Vec<TreeItem> {
    let mut folders: BTreeMap<&str, Vec<&Ref>> = BTreeMap::new();
    let mut leaves = Vec::new();
    for r in refs {
        match r.name.split('/').nth(depth) {
            Some(seg) if group && r.name.split('/').count() > depth + 1 => {
                folders.entry(seg).or_default().push(r)
            }
            _ => {
                // in a folder only the leaf shows; flat, all but the segments left out
                let label = match group {
                    true => r.name.rsplit('/').next(),
                    false => r.name.splitn(depth + 1, '/').last(),
                };
                leaves.push(branch_item(
                    r,
                    view,
                    label.unwrap_or(&r.name).to_owned(),
                    marks,
                ))
            }
        }
    }
    let mut items: Vec<TreeItem> = folders
        .into_iter()
        .map(|(name, inner)| {
            TreeItem::new(name.to_owned())
                .colored_icon("nodes/folder")
                .click_to_toggle(true)
                .default_open(open)
                .children(folder_tree(&inner, depth + 1, open, group, marks, view))
        })
        .collect();
    items.extend(leaves);
    items
}

/// What a branches pane row shows besides its name: left out of the graph, a favorite, the
/// row picked (by kind and name).
pub(crate) struct RefMarks<'a> {
    pub(crate) hidden: &'a [String],
    pub(crate) favorites: &'a [String],
    pub(crate) picked: Option<&'a (RefKind, String)>,
}

/// Right-click menu of a sidebar section header with its one "add" action.
fn header_menu(
    view: &Entity<RepoView>,
    label: &'static str,
    f: fn(&mut RepoView, &mut Window, &mut Context<RepoView>),
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    let view = view.clone();
    move |menu, _, _| {
        let view = view.clone();
        menu.item(
            PopupMenuItem::new(label)
                .on_click(move |_, window, cx| view.update(cx, |this, cx| f(this, window, cx))),
        )
    }
}

/// A spinner over a line of text, centred: something is coming.
fn loading(text: &'static str, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .size_full()
        .gap_3()
        .items_center()
        .justify_center()
        .child(Spinner::new().large().color(cx.theme().primary))
        .child(div().text_sm().text_color(muted).child(text))
}

impl Render for RepoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.note_focus(window, cx);
        // the front window's repository, or with Spork in the background the one drawn last
        if self.window_active || cx.active_window().is_none() {
            crate::shell::show_git_menu(self.git_menu_state(), cx);
        }
        let theme = cx.theme();
        // the window background (the workspace's, under the project's glow) shows between the
        // tool windows' rounded "islands"
        let root = v_flex()
            .size_full()
            .track_focus(&self.focus)
            .key_context("Spork")
            .text_color(theme.foreground);
        let root = self.with_actions(root, cx);
        self.sync_hosting_menus(cx);
        let status = self.status_bar(cx);
        let (stripe, right_stripe) = (self.stripe(cx), self.right_stripe(cx));
        let notifications = self.notifications_window(window, cx);
        let body = self.body(cx);
        let main = self.with_terminal(body, cx);
        let body = h_flex()
            .w_full()
            .child(stripe)
            .child(main.flex_1().min_w_0())
            .children(notifications)
            .child(right_stripe);
        root.child(body.flex_1().min_h_0())
            .child(status)
            .children(self.search_everywhere_overlay(window))
            .children(self.compare_popup_overlay(window))
    }
}

impl RepoView {
    /// What this frame shows depends on: the window in front, the log or the message focused.
    fn note_focus(&mut self, window: &Window, cx: &App) {
        self.window_active = window.is_window_active();
        self.log_focused = self.window_active && self.focus.is_focused(window);
        self.commit_focused = [self.message.read(cx).focus_handle(cx)]
            .iter()
            .any(|f| f.is_focused(window));
    }

    /// Every action of the view, handled on `el`: its root, or in the IDE the workspace's, so
    /// the menus and keys reach it wherever the focus is.
    pub fn with_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        let el = self.on_actions(el, cx);
        let el = self.on_hosting_actions(el, cx);
        let el = self.commit_actions(el, cx);
        let el = self.log_actions(el, cx);
        let el = self.log_toolbar_actions(el, cx);
        let el = self.branch_commands(el, cx);
        self.diff_actions(el, cx)
    }

    /// The tab's name: the repository folder, or "Welcome".
    /// The repository's folder, once it is open.
    pub(crate) fn tab_root(&self) -> Option<PathBuf> {
        match &self.repo {
            Some(Ok(repo)) => Some(repo.root.clone()),
            _ => None,
        }
    }

    /// The branch widget of the main toolbar: the branch you are on, with how far it is ahead of /
    /// behind its upstream; a click opens the Git Branches popup. Detached HEAD shows as a warning.
    fn current_branch_chip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (head, color, counts) = match &self.repo {
            Some(Ok(repo)) => match repo
                .refs
                .iter()
                .find(|r| r.is_head && r.kind == RefKind::Local)
            {
                Some(r) => (
                    r.name.clone(),
                    theme.foreground,
                    (r.behind > 0, r.ahead > 0),
                ),
                None => ("Detached HEAD".to_owned(), theme.warning, (false, false)),
            },
            _ => (String::new(), theme.foreground, (false, false)),
        };
        // IntelliJ's marks, no numbers: a blue ↙ to pull, a green ↗ to push
        let (incoming, outgoing) = counts;
        let dark = theme.mode.is_dark();
        let (blue, green) = match dark {
            true => (rgb(0x548AF7), rgb(0x57965C)),
            false => (rgb(0x3574F0), rgb(0x369650)),
        };
        // The reference app's VCS widget tooltip, "Git Branch: main"
        let tooltip_head = head.clone();
        let chip = spork_workbench::ToolbarDropdown::new("current-branch")
            .selected(self.branches_popup_open)
            .text_color(color)
            .child(spork_workbench::icon("general/vcs").size(px(16.)))
            .child(div().max_w(px(350.)).truncate().child(head))
            .when(incoming, |c| {
                c.child(
                    spork_workbench::icon("dvcs/incomingUpdate")
                        .size(px(12.))
                        .text_color(blue),
                )
            })
            .when(outgoing, |c| {
                c.child(
                    spork_workbench::icon("dvcs/outgoingPush")
                        .size(px(12.))
                        .text_color(green),
                )
            })
            .tooltip(format!("Git Branch: {tooltip_head}"))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_branches_popup(window, cx)));
        let bounds = self.branches_chip.clone();
        // IntelliJ opens it 1 px under the toolbar, its edge 5 px left of the widget's
        let popup = self
            .branches_popup
            .as_ref()
            .filter(|_| self.branches_popup_open)
            .map(|(popup, _)| {
                div().absolute().top_full().left_0().child(
                    deferred(
                        anchored()
                            .snap_to_window_with_margin(px(8.))
                            .child(div().mt(px(6.)).ml(px(-5.)).child(popup.clone())),
                    )
                    .with_priority(1),
                )
            });
        div()
            .relative()
            .child(chip)
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .children(popup)
    }

    /// The Git Branches popup, opened or closed (the widget's click, ⌘B).
    pub(crate) fn toggle_branches_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.branches_popup_open || !matches!(&self.repo, Some(Ok(_))) {
            self.branches_popup_open = false;
            return cx.notify();
        }
        let popup = match &self.branches_popup {
            Some((popup, _)) => popup.clone(),
            None => {
                let (view, trigger) = (cx.entity().downgrade(), self.branches_chip.clone());
                let popup =
                    cx.new(|cx| branches_popup::BranchesPopup::new(view, trigger, window, cx));
                let subscription = cx.subscribe_in(
                    &popup,
                    window,
                    |this, popup, _: &DismissEvent, window, cx| {
                        this.branches_popup_open = false;
                        // its search field had the focus: give it back, or the window's
                        // shortcuts find no one to answer them
                        if popup.read(cx).has_focus(window, cx) {
                            window.focus(&this.focus, cx);
                        }
                        cx.notify();
                    },
                );
                self.branches_popup = Some((popup.clone(), subscription));
                popup
            }
        };
        popup.update(cx, |popup, cx| popup.reset(window, cx));
        self.branches_popup_open = true;
        cx.notify();
    }

    /// What the window's main toolbar shows for this tab: the repository and branch widgets and
    /// the update / push / fetch / stash actions.
    pub(crate) fn main_toolbar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let Some(Ok(_)) = &self.repo else {
            return None;
        };
        Some(
            h_flex()
                .gap_1()
                .child(self.repo_switcher(self.tab_title(), cx))
                .child(self.vcs_group(cx)),
        )
    }

    /// The reference app's VCS Group: Update Project, Push…, the VCS widget and, while git has an operation
    /// paused, the Merge/Rebase widget.
    pub(crate) fn vcs_group(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        h_flex()
            .id("vcs-group")
            .role(Role::Toolbar)
            .aria_label("MainToolbarVCSGroup")
            .gap_1()
            .child(self.remote_button(RemoteOp::Pull, cx))
            .child(self.remote_button(RemoteOp::Push, cx))
            .child(self.current_branch_chip(cx))
            .children(self.merge_rebase_widget(cx))
    }

    /// The reference app's VCS Merge/Rebase Widget (`GitMergeRebaseWidgetGroup`): the paused operation's
    /// actions as colored buttons, green to go on, red to abort, in its order.
    fn merge_rebase_widget(&self, cx: &App) -> Option<Div> {
        let git = self.git_menu_state();
        let actions: [(bool, &'static str, Box<dyn Action>, bool); 8] = [
            (
                git.conflicts,
                "Resolve Conflicts…",
                Box::new(shell::ResolveConflicts),
                true,
            ),
            (
                git.rebasing,
                "Continue Rebase",
                Box::new(shell::ContinueRebase),
                true,
            ),
            (
                git.rebasing,
                "Abort Rebase",
                Box::new(shell::AbortRebase),
                false,
            ),
            (
                git.merging,
                "Commit merge",
                Box::new(shell::CommitMerge),
                true,
            ),
            (
                git.merging,
                "Abort Merge",
                Box::new(shell::AbortMerge),
                false,
            ),
            (
                git.cherry_picking,
                "Continue Cherry-Pick",
                Box::new(shell::ContinueCherryPick),
                true,
            ),
            (
                git.cherry_picking,
                "Abort Cherry-Pick",
                Box::new(shell::AbortCherryPick),
                false,
            ),
            (
                git.reverting,
                "Abort Revert",
                Box::new(shell::AbortRevert),
                false,
            ),
        ];
        let tokens = spork_workbench::Tokens::get(cx);
        let buttons: Vec<_> = actions
            .into_iter()
            .enumerate()
            .filter(|(_, (shown, ..))| *shown)
            .map(|(ix, (_, label, action, resolves))| {
                let (icon, fill) = match resolves {
                    true => ("dvcs/resolveContinue_stroke", tokens.toolbar_run_bg),
                    false => ("vcs/abort_stroke", tokens.toolbar_stop_bg),
                };
                spork_workbench::ActionButton::new(("merge-rebase", ix), icon)
                    .size(spork_workbench::ActionSize::MainToolbar)
                    .fill(fill)
                    .tooltip(label)
                    .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
            })
            .collect();
        (!buttons.is_empty()).then(|| h_flex().gap_1().children(buttons))
    }

    /// Opens `tool` on the left, or closes it when it shows; Commit and Project can be open
    /// together, Commit above.
    fn toggle_left(&mut self, tool: LeftTool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.left_tools.iter().position(|t| *t == tool) {
            self.left_tools.remove(ix);
            return cx.notify();
        }
        match tool {
            LeftTool::Commit => self.show_commit_window(window, cx),
            _ => self.open_left(tool, cx),
        }
    }

    /// A left tool window's − (Hide) button.
    pub(crate) fn hide_left_button(
        &self,
        tool: LeftTool,
        cx: &mut Context<Self>,
    ) -> spork_workbench::ActionButton {
        spork_workbench::ActionButton::new(("hide-left", tool as usize), "general/hide")
            .tooltip("Hide")
            .on_click(cx.listener(move |this, _, _, cx| {
                this.left_tools.retain(|t| *t != tool);
                if tool == LeftTool::Commit {
                    cx.emit(ide::ToolWindowEvent::Hide(ide::ToolWindow::Commit));
                }
                cx.notify();
            }))
    }

    pub(crate) fn open_left(&mut self, tool: LeftTool, cx: &mut Context<Self>) {
        if tool == LeftTool::Commit {
            cx.emit(ide::ToolWindowEvent::Show(ide::ToolWindow::Commit));
        } else {
            self.left_tools.retain(|t| *t == LeftTool::Commit);
        }
        if !self.left_tools.contains(&tool) {
            self.left_tools.push(tool);
            self.left_tools.sort();
        }
        cx.notify();
    }

    /// The Commit tool window, its message template filled in: local changes beside the log.
    pub(crate) fn show_commit_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_left(LeftTool::Commit, cx);
        self.fill_template(window, cx);
    }

    /// The tool window with the focus, after a click in it or opening it.
    fn set_active_tool(&mut self, tool: Option<ActiveTool>, cx: &mut Context<Self>) {
        if self.active_tool != tool {
            self.active_tool = tool;
            cx.notify();
        }
    }

    /// After a stripe click: the tool window opened takes the focus; closed, it loses it.
    fn stripe_toggled(&mut self, tool: ActiveTool, cx: &mut Context<Self>) {
        let open = match tool {
            ActiveTool::Left(left) => self.left_tools.contains(&left),
            ActiveTool::Git => self.show_console,
            ActiveTool::Terminal => self.show_terminal,
            ActiveTool::Notifications => self.show_notifications,
        };
        match open {
            true => self.set_active_tool(Some(tool), cx),
            false if self.active_tool == Some(tool) => self.set_active_tool(None, cx),
            false => {}
        }
    }

    fn stripe_state(&self, open: bool, tool: ActiveTool) -> spork_workbench::shell::StripeState {
        use spork_workbench::shell::StripeState;
        match (open, self.active_tool == Some(tool)) {
            (false, _) => StripeState::Hidden,
            (true, false) => StripeState::Open,
            (true, true) => StripeState::Active,
        }
    }

    /// A stripe button with its IntelliJ icon (`toolwindows/commit`) and tooltip.
    fn stripe_button(
        &self,
        id: &'static str,
        icon: &str,
        state: spork_workbench::shell::StripeState,
        tip: &'static str,
        cx: &App,
    ) -> Stateful<Div> {
        use spork_workbench::shell;
        let tokens = Tokens::get(cx);
        let color = shell::stripe_icon_color(state, self.window_active, tokens);
        shell::stripe_button(id, shell::stripe_icon(icon, color), state, tokens)
            .aria_label(tip)
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
    }

    /// The tool-window buttons down the left edge, in the reference app's order: Commit and Project at
    /// the top; Terminal, Git (the log) and Activity at the bottom.
    fn stripe(&self, cx: &mut Context<Self>) -> Div {
        let has_repo = matches!(&self.repo, Some(Ok(_)));
        let left = |id: &'static str,
                    icon: &str,
                    tool: LeftTool,
                    tip: &'static str,
                    cx: &mut Context<Self>| {
            let state = self.stripe_state(self.left_tools.contains(&tool), ActiveTool::Left(tool));
            self.stripe_button(id, icon, state, tip, cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_left(tool, window, cx);
                    this.stripe_toggled(ActiveTool::Left(tool), cx);
                }))
        };
        // GitHub's pull requests or GitLab's merge requests, once the remote is one of theirs
        let pulls = self.hosted.as_ref().and_then(|h| match h.service {
            spork_hosting::Service::GitHub => Some(("github/pullRequests", "Pull Requests")),
            spork_hosting::Service::GitLab => Some(("gitlab/gitLabToolWindow", "Merge Requests")),
            _ => None,
        });
        let more = self.more_tool_windows(cx);
        let top = has_repo.then(|| {
            let commit = left(
                "tw-commit",
                "toolwindows/commit",
                LeftTool::Commit,
                "Commit (⌘0)",
                cx,
            );
            let pulls =
                pulls.map(|(icon, tip)| left("tw-pulls", icon, LeftTool::PullRequests, tip, cx));
            let project = left(
                "tw-project",
                "toolwindows/project",
                LeftTool::Project,
                "Project (⌘1)",
                cx,
            );
            [commit.into_any_element()]
                .into_iter()
                .chain(pulls.map(IntoElement::into_any_element))
                .chain([
                    spork_workbench::shell::stripe_separator(Tokens::get(cx)).into_any_element(),
                    project.into_any_element(),
                    more.into_any_element(),
                ])
        });
        let git = has_repo.then(|| {
            self.stripe_button(
                "tw-git",
                "toolwindows/vcs",
                self.stripe_state(self.show_console, ActiveTool::Git),
                "Git (⌘9)",
                cx,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_console(cx);
                this.stripe_toggled(ActiveTool::Git, cx);
            }))
        });
        let bottom = [self
            .stripe_button(
                "tw-terminal",
                "terminal/toolwindow/terminal",
                self.stripe_state(self.show_terminal, ActiveTool::Terminal),
                "Terminal (⌥F12)",
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.toggle_terminal(window, cx);
                this.stripe_toggled(ActiveTool::Terminal, cx);
            }))]
        .into_iter()
        .chain(git);
        spork_workbench::shell::stripe(
            top.into_iter().flatten(),
            bottom,
            self.window_active,
            Tokens::get(cx),
        )
    }

    /// The stripe's "⋯": every tool window, a click opens or closes it.
    fn more_tool_windows(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let view = cx.entity();
        div().w_full().h(px(40.)).flex().items_center().justify_center().child(
            Button::new("tw-more")
                .ghost()
                .child(spork_workbench::icon("general/moreHorizontal@20x20").size(px(20.)))
                .tooltip("More Tool Windows")
                .dropdown_menu(move |menu, _, _| {
                    let item = |label: &str, f: fn(&mut RepoView, &mut Window, &mut Context<RepoView>)| {
                        let view = view.clone();
                        PopupMenuItem::new(label.to_owned())
                            .on_click(move |_, window, cx| view.update(cx, |this, cx| f(this, window, cx)))
                    };
                    menu.item(item("Commit", |this, w, cx| this.toggle_left(LeftTool::Commit, w, cx)))
                        .item(item("Project", |this, w, cx| this.toggle_left(LeftTool::Project, w, cx)))
                        .item(item("Terminal", RepoView::toggle_terminal))
                        .item(item("Git", |this, _, cx| this.toggle_console(cx)))
                        .item(item("Notifications", |this, _, cx| this.toggle_notifications(cx)))
                }),
        )
    }

    /// The right stripe: Notifications only, as in the reference app.
    fn right_stripe(&self, cx: &mut Context<Self>) -> Div {
        use spork_workbench::shell;
        let tokens = Tokens::get(cx);
        let state = self.stripe_state(self.show_notifications, ActiveTool::Notifications);
        let icon = shell::stripe_icon(
            "toolwindows/notifications",
            shell::stripe_icon_color(state, self.window_active, tokens),
        );
        // IntelliJ's dot while a notification is unread
        let icon = match notices::unread_badge(tokens) {
            Some(dot) => shell::stripe_badge(icon, dot, state, tokens).into_any_element(),
            None => icon.into_any_element(),
        };
        let bell = shell::stripe_button("tw-notifications", icon, state, tokens)
            .aria_label("Notifications")
            .tooltip(|window, cx| Tooltip::new("Notifications").build(window, cx))
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_notifications(cx);
                this.stripe_toggled(ActiveTool::Notifications, cx);
            }));
        spork_workbench::shell::stripe([bell], None::<Div>, self.window_active, Tokens::get(cx))
    }

    /// The editor tabs, as in the reference app: "Log" always first and not closable, then the tab of
    /// what the main area shows instead (a file's diff, its history, a stash, a rebase plan).
    fn editor_tab_row(&self, cx: &mut Context<Self>) -> Div {
        let tokens = Tokens::get(cx);
        let focused = self.log_focused;
        let editor_tab =
            |id: &'static str, icon: Option<AnyElement>, label: SharedString, selected: bool| {
                spork_workbench::shell::editor_tab(id, selected, focused, tokens)
                    .children(icon)
                    .child(label)
            };
        let tab_icon = |name: &str| spork_workbench::icon(name).size(px(16.));
        // the IDE's Git tool window shows its Console and Worktrees tabs in the log's place
        let worktrees = self.ide && self.show_console && self.worktrees_tab;
        let console = self.ide && self.show_console && !worktrees;
        let log = editor_tab(
            "editor-log",
            Some(tab_icon("general/vcs").into_any_element()),
            "Log".into(),
            self.view == View::History && !console && !worktrees,
        )
        // the IDE's tool window tabs, to accessibility as its editor tabs are
        .when(self.ide, |tab| {
            tab.role(Role::Tab)
                .aria_label("Log")
                .aria_selected(self.view == View::History && !console && !worktrees)
        })
        .on_click(cx.listener(|this, _, _, cx| {
            this.view = View::History;
            if this.ide {
                this.show_console = false;
            }
            cx.notify();
        }));
        let console_tab = self.ide.then(|| {
            editor_tab("editor-console", None, "Console".into(), console)
                .role(Role::Tab)
                .aria_label("Console")
                .aria_selected(console)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_console = true;
                    this.worktrees_tab = false;
                    cx.notify();
                }))
        });
        let worktrees_tab = (self.ide && self.worktrees_tab_shown()).then(|| {
            editor_tab("editor-worktrees", None, "Worktrees".into(), worktrees)
                .role(Role::Tab)
                .aria_label("Worktrees")
                .aria_selected(worktrees)
                .child(crate::shell::new_badge(cx))
                .child(
                    spork_workbench::ActionButton::new("close-worktrees", "general/closeSmall")
                        .tooltip("Close Tab")
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.worktrees_tab = false;
                            this.worktrees_opened = false;
                            this.worktrees_closed = true;
                            cx.notify();
                        })),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_console = true;
                    this.worktrees_tab = true;
                    cx.notify();
                }))
        });
        let other: Option<(SharedString, Option<&'static str>)> = match self.view {
            View::History => None,
            View::Changes => self.change_selected.as_ref().map(|sel| {
                let name = sel.path.rsplit('/').next().unwrap_or(&sel.path).to_owned();
                (name.into(), None)
            }),
            View::File => self.file.as_ref().map(|f| {
                let icon = (!f.is_annotation()).then_some("general/history");
                (f.tab_label().into(), icon)
            }),
            View::Stash => self
                .shelf_shown_title()
                .map(|title| (title, Some("vcs/patch")))
                .or_else(|| {
                    self.stash_shown.as_ref().map(|(st, _)| {
                        (format!("stash@{{{}}}", st.index).into(), Some("vcs/shelve"))
                    })
                }),
            View::CommitDiff => Some(match &self.commit_file_selected {
                Some(path) => (
                    path.rsplit('/').next().unwrap_or(path).to_owned().into(),
                    None,
                ),
                None => ("Changes".into(), Some("vcs/diff")),
            }),
        };
        let other = other.map(|(label, icon)| {
            let icon = match icon {
                Some(name) => tab_icon(name).into_any_element(),
                None => spork_workbench::image(spork_workbench::file_icon(&label), cx)
                    .into_any_element(),
            };
            editor_tab("editor-other", Some(icon), label, true).child(
                spork_workbench::shell::tab_button("close-editor-tab", "general/close", tokens)
                    .aria_label("Close")
                    .tooltip(|window, cx| Tooltip::new("Close").build(window, cx))
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.view = View::History;
                        cx.notify();
                    })),
            )
        });
        spork_workbench::shell::editor_tabs()
            .child(log)
            .children(console_tab)
            .children(worktrees_tab)
            .children(other)
    }

    /// The bar along the bottom: where you are, and the job running.
    fn status_bar(&self, cx: &mut Context<Self>) -> Div {
        let branch = match &self.repo {
            Some(Ok(repo)) => repo
                .refs
                .iter()
                .find(|r| r.is_head && r.kind == RefKind::Local)
                .map_or("Detached HEAD".to_owned(), |r| r.name.clone()),
            _ => String::new(),
        };
        let job = spork_git::job_progress().filter(|_| self.busy);
        let primary = cx.theme().primary;
        spork_workbench::shell::status_bar(Tokens::get(cx))
            .child(self.tab_title())
            .when(!branch.is_empty(), |bar| {
                bar.child(Icon::new(IconName::ChevronRight).xsmall())
                    .child(Icon::new(IconName::GitBranch).xsmall())
                    .child(branch)
            })
            .child(div().flex_1())
            .when(self.busy, |bar| {
                bar.child(Spinner::new().xsmall().color(primary))
                    .children(job.map(|line| div().max_w(px(420.)).truncate().child(line)))
            })
    }

    /// Whether the repository has uncommitted changes (ignored files aside), for the tab's badge.
    pub(crate) fn has_changes(&self) -> bool {
        matches!(&self.repo, Some(Ok(repo)) if repo.status.iter().any(|e| !e.ignored()))
    }

    pub(crate) fn tab_title(&self) -> String {
        match &self.repo {
            Some(Ok(repo)) => repo.root.file_name().map_or_else(
                || repo.root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            Some(Err(_)) => "Welcome".to_owned(),
            None => "Loading…".to_owned(),
        }
    }
}

impl RepoView {
    fn body(&mut self, cx: &mut Context<Self>) -> Div {
        // the gap beside the left tool window is its handle: drag it to resize the window
        let base = h_flex().w_full().on_drag_move(cx.listener(
            |this, event: &DragMoveEvent<LeftResize>, _, cx| {
                let width = event.event.position.x - event.bounds.left();
                let room = event.bounds.size.width - px(320.);
                this.left_width = width.min(room).max(px(240.));
                cx.notify();
            },
        ));
        let sidebar = match &self.repo {
            Some(Ok(_)) => self.left_tool_windows(cx),
            _ => None,
        };
        base.children(sidebar).child(self.main_panel(cx))
    }

    /// The left tool windows while one is open, and the gap beside them to resize them by.
    fn left_tool_windows(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let mut panes: Vec<AnyElement> = Vec::new();
        for tool in self.left_tools.clone() {
            let pane = match tool {
                LeftTool::Commit => self.commit_window(cx).into_any_element(),
                LeftTool::Project => self.project_window(cx).into_any_element(),
                LeftTool::PullRequests => self.pull_requests_window(cx).into_any_element(),
            };
            panes.push(
                div()
                    .size_full()
                    .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
                        this.set_active_tool(Some(ActiveTool::Left(tool)), cx)
                    }))
                    .child(pane)
                    .into_any_element(),
            );
        }
        // two open: stacked, the split dragged between them
        let left = match (panes.pop(), panes.pop()) {
            (None, _) => None,
            (Some(only), None) => Some(div().size_full().overflow_hidden().child(only)),
            (Some(lower), Some(upper)) => Some(
                div().size_full().child(
                    gpui_kit::component::v_resizable("left-split")
                        .child(
                            resizable_panel()
                                .child(div().size_full().overflow_hidden().child(upper)),
                        )
                        .child(
                            resizable_panel()
                                .size(px(260.))
                                .child(div().size_full().overflow_hidden().child(lower)),
                        ),
                ),
            ),
        };
        let handle = div()
            .id("left-resize")
            .w(px(6.))
            .h_full()
            .flex_none()
            .cursor_col_resize()
            .on_drag(LeftResize, |drag, _, _, cx| cx.new(|_| drag.clone()));
        left.map(|content| {
            h_flex()
                .h_full()
                .flex_none()
                .child(island(cx).w(self.left_width).child(content))
                .child(handle)
        })
    }

    /// The main area: the log under its editor tab, or what replaced it (a diff, a file's
    /// history, a stash, the commit's changes); the IDE's Git tool window shows it.
    fn main_panel(&mut self, cx: &mut Context<Self>) -> Div {
        let centered = |msg: String| {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .child(div().text_sm().child(msg))
        };

        let repo = match &self.repo {
            None => {
                let loading = loading("Loading repository…", cx);
                return island(cx).flex_1().child(loading);
            }
            Some(Ok(repo)) => repo,
            Some(Err(e)) => {
                let problem = match e {
                    GitError::NotARepo => None,
                    other => Some(format!("{}: {other}", self.requested.display())),
                };
                let panel = island(cx).flex_1().children(self.job_banner(cx));
                return panel.child(self.welcome(problem, cx));
            }
        };

        // the sidebar filter (M03 §3) narrows branches, remote branches and tags by name
        let query = self.sidebar_filter.read(cx).value().trim().to_lowercase();
        let filtering = !query.is_empty();
        let of = |kind| {
            repo.refs
                .iter()
                .filter(|r: &&Ref| r.kind == kind && r.name.to_lowercase().contains(&query))
                .collect::<Vec<_>>()
        };
        let stash_items = self.stash_items(&repo.stashes, cx);
        let worktree_items = self.worktree_items(&repo.worktrees, cx);
        let submodule_items = self.submodule_items(&repo.submodules, cx);
        let view = cx.entity();
        let locals = of(RefKind::Local);
        let open_all = filtering || self.branches_expanded;
        let remote_items = self.remote_items(&repo.remotes, &of(RefKind::Remote), open_all, cx);
        let tags: Vec<_> = of(RefKind::Tag)
            .iter()
            .map(|t| {
                TreeItem::new(t.name.clone())
                    .icon(IconName::Tag)
                    .active(self.branches_pane_picked(RefKind::Tag, &t.name))
                    .on_click(branches_pane::ref_click(t, false, view.clone()))
                    .context_menu(branches::ref_menu(t, view.clone()))
            })
            .collect();
        let group = |title: &str, children: Vec<TreeItem>| {
            TreeItem::new(title.to_owned())
                .default_open(open_all)
                .click_to_toggle(true)
                .children(children)
        };
        let head = repo
            .refs
            .iter()
            .find(|r| r.is_head && r.kind == RefKind::Local)
            .map(|r| {
                TreeItem::new("HEAD (Current Branch)")
                    .active(self.branches_pane_head_picked())
                    .on_click(branches_pane::ref_click(r, true, view.clone()))
            });
        // IntelliJ hides empty groups; the current branch and favorites first in Local
        let mut locals = locals;
        locals.sort_by_key(|r| (!r.is_head, !repo.favorites.contains(&r.name)));
        let marks = RefMarks {
            hidden: &repo.hidden,
            favorites: &repo.favorites,
            picked: self.log_ui.branch_pick.as_ref(),
        };
        let local = folder_tree(
            &locals,
            0,
            open_all,
            self.branches_group_dirs,
            &marks,
            &view,
        );
        let groups = [
            Some(group("Local", local)),
            repo.refs
                .iter()
                .any(|r| r.kind == RefKind::Remote)
                .then(|| group("Remote", remote_items)),
            (self.branches_show_tags && !tags.is_empty()).then(|| group("Tags", tags)),
            (!stash_items.is_empty()).then(|| group("Stashes", stash_items)),
            (!worktree_items.is_empty()).then(|| {
                group("Worktrees", worktree_items).context_menu(header_menu(
                    &view,
                    "New worktree…",
                    RepoView::new_branch_worktree_dialog,
                ))
            }),
            (!submodule_items.is_empty()).then(|| {
                group("Submodules", submodule_items).context_menu(header_menu(
                    &view,
                    "Add submodule…",
                    RepoView::add_submodule_dialog,
                ))
            }),
        ];
        let filter = spork_workbench::TextField::new(&self.sidebar_filter)
            .cleanable(true)
            .prefix(spork_workbench::icon("general/search").size(px(16.)));
        let branches_tree = v_flex()
            .size_full()
            .child(div().px_2().pt_1p5().pb_1().child(filter))
            .child(
                div()
                    .id("refs-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .pb_2()
                    .child(
                        Tree::new(format!("refs-{}", self.branches_tree_gen))
                            .children(head.into_iter().chain(groups.into_iter().flatten())),
                    ),
            );

        let count = repo.history.commits.len();
        let lanes = repo.history.graph_lanes;
        let panel = island(cx)
            .flex_1()
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.set_active_tool(None, cx)))
            .child(self.editor_tab_row(cx));
        let panel = panel
            .children(self.job_banner(cx))
            .children(self.op_banner(cx))
            .children(self.bisect_banner(cx));

        if self.ide && self.show_console {
            return match self.worktrees_tab && self.worktrees_tab_shown() {
                true => panel.child(self.worktrees_tab(cx)),
                false => panel.child(self.console_tab(cx)),
            };
        }
        if self.view == View::Stash {
            let stash = self.stash_view(cx);
            return panel.child(stash);
        }
        if self.view == View::File {
            let file = self.file_view(cx);
            return panel.child(file);
        }
        if self.view == View::CommitDiff && self.selected.is_some() {
            let tabs = self.details_tabs(cx);
            let diff = match self.commit_files.is_some() {
                true => self.commit_files_view(cx),
                false => self.diff_body(cx),
            };
            return panel.child(tabs).child(diff);
        }
        if self.view == View::Changes {
            let changes = self.change_diff_view(cx);
            return panel.child(changes);
        }
        if count == 0 {
            // the filter bar stays, so a filter that empties the list can be turned off
            let empty = panel
                .child(self.log_toolbar(cx))
                .child(centered("No commits yet".into()));
            return empty;
        }
        let list = v_flex()
            .size_full()
            .min_w_0()
            .child(self.log_toolbar(cx))
            .children(self.uncommitted_row(lanes, cx))
            .child(div().flex_1().min_h_0().child(history::scrolled(
                uniform_list(
                    "history",
                    count,
                    cx.processor(|this, range, _, cx| this.history_rows(range, cx)),
                ),
                &self.history_scroll,
            )));
        let list = self.with_branches(branches_tree, list, cx);
        // IntelliJ's log: the commits, and the selected one's files and details on the right
        let panel = panel.child(self.log_panes(list, cx));
        panel
    }

    /// "Uncommitted changes" above the newest commit while the working copy has any (M05 §1);
    /// a click opens Local Changes.
    fn uncommitted_row(&self, lanes: usize, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let Some(Ok(repo)) = &self.repo else {
            return None;
        };
        let n = repo.status.len();
        if n == 0 {
            return None;
        }
        let theme = cx.theme();
        let (muted, hover, accent) = (theme.muted_foreground, theme.list_hover, theme.warning);
        Some(
            h_flex()
                .id("uncommitted")
                .w_full()
                .h(history::ROW_HEIGHT)
                .px_3()
                .gap_3()
                .text_sm()
                .hover(move |row| row.bg(hover))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.show_commit_window(window, cx);
                }))
                // a hollow dot in the graph column, where the next commit will go
                .child(
                    div().w(history::graph_width(lanes)).flex_none().child(
                        div()
                            .ml(px(history::LANE_WIDTH / 2. - 5.))
                            .size(px(10.))
                            .rounded_full()
                            .border_2()
                            .border_color(accent),
                    ),
                )
                .child(div().text_color(muted).child(match n {
                    1 => "Uncommitted changes (1 file)".to_owned(),
                    n => format!("Uncommitted changes ({n} files)"),
                })),
        )
    }

    /// The diff bar, the changed files and the diff of the selected commit (or comparison).
    fn diff_body(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(self.diff_bar("details", &self.diff, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.changed_files(cx))
                    .child(self.diff_view(&self.diff, cx)),
            )
    }

    /// With one commit selected, Cmd/Ctrl- or Shift-click on another shows everything that
    /// changed from the older of the two to the newer (M06 §1).
    fn compare_with(&mut self, other: usize, cx: &mut Context<Self>) {
        let (Some(ix), Some(Ok(repo))) = (self.selected, &self.repo) else {
            return;
        };
        let commits = &repo.history.commits;
        // the list is newest first
        let (Some(newer), Some(older)) = (commits.get(ix.min(other)), commits.get(ix.max(other)))
        else {
            return;
        };
        let (root, newer, older) = (repo.root.clone(), newer.id.clone(), older.id.clone());
        let ignore_ws = self.prefs.ignore_whitespace;
        self.compare = Some(other);
        self.compare_run.clear();
        self.commit_files = None;
        self.details = None;
        self._details_load = Some(cx.spawn(async move |this, cx| {
            let (details, run) = cx
                .background_spawn(async move {
                    let run = spork_git::revision::run_between(&root, &older, &newer);
                    let details = spork_git::commit_details(&root, &newer, Some(&older), ignore_ws);
                    (details, run.unwrap_or_default())
                })
                .await;
            this.update(cx, |this, cx| {
                this.compare_run = run;
                let patch = match &details {
                    Ok(d) if d.patch.len() <= MAX_PATCH_BYTES => d.patch.as_str(),
                    _ => "",
                };
                let diff = this.diff.clone();
                this.set_patch(&diff, patch, cx);
                this.details = Some(details);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The commit's files beside its diff; a click scrolls the diff to that file (M06 §1).
    fn changed_files(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let files: Vec<(String, &'static str)> = self
            .diff
            .read(cx)
            .files()
            .iter()
            .map(|f| {
                let letter = match f.status() {
                    DiffFileStatus::Added => "A",
                    DiffFileStatus::Deleted => "D",
                    DiffFileStatus::Renamed => "R",
                    DiffFileStatus::Copied => "C",
                    _ => "M",
                };
                (f.path().to_string(), letter)
            })
            .collect();
        let theme = cx.theme();
        let (muted, hover, border) = (theme.muted_foreground, theme.list_hover, theme.border);
        v_flex()
            .id("changed-files")
            .w(px(240.))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(border)
            .py_1()
            .children(files.into_iter().enumerate().map(|(ix, (path, letter))| {
                let (dir, name) = path
                    .rsplit_once('/')
                    .map_or((String::new(), path.clone()), |(d, n)| {
                        (format!("{d}/"), n.to_owned())
                    });
                let diff = self.diff.clone();
                h_flex()
                    .id(("changed-file", ix))
                    .px_2()
                    .gap_2()
                    .h(px(24.))
                    .text_xs()
                    .hover(move |r| r.bg(hover))
                    .on_click(move |_, _, cx| diff.update(cx, |s, cx| s.scroll_to_file(&path, cx)))
                    .child(div().w(px(10.)).text_color(muted).child(letter))
                    .child(div().min_w_0().truncate().child(name))
                    .child(div().min_w_0().truncate().text_color(muted).child(dir))
            }))
            .overflow_y_scrollbar()
    }

    fn history_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(Ok(repo)) = &self.repo else {
            return Vec::new();
        };
        let h = &repo.history;
        let lanes = range
            .clone()
            .map(|ix| h.rows[ix].width)
            .max()
            .unwrap_or(1)
            .min(h.graph_lanes);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let view = cx.entity();
        let theme = cx.theme();
        let (muted, hover, active) = (theme.muted_foreground, theme.list_hover, theme.list_active);
        let drop_target = theme.drop_target;
        let query = self.commit_query(cx);
        let filtering = self.filtering(cx);
        let merge_text = spork_workbench::log_dim_color(cx);
        let me = repo.me.clone();
        let p = &self.prefs;
        let (compact, tag_names, commit_time, refs_left) = (
            p.log_compact_refs,
            p.log_tag_names,
            p.log_commit_time,
            p.log_refs_left,
        );
        let (show_author, show_date, show_hash) = (p.log_author, p.log_date, p.log_hash);
        let (mark_mine, mark_merges) = (p.log_mark_mine, p.log_mark_merges);
        let on_branch = match p.log_mark_branch {
            true => current_branch_commits(&mut self.branch_commits, repo),
            false => Default::default(),
        };
        let branch_bg = spork_workbench::Tokens::get(cx).log_current_branch;

        range
            .map(|ix| {
                let commit = &h.commits[ix];
                let labels = h.labels.get(&commit.id).cloned().unwrap_or_default();
                let stash = h.stashes.get(&commit.id).cloned();
                let dim = (filtering && !self.is_match(commit, &query))
                    || self.picked_elsewhere(&commit.id);
                let selected = self.selected == Some(ix) || self.compare == Some(ix);
                // IntelliJ dims merge commits and bolds your own name
                let is_merge = commit.parents.len() > 1;
                let mine = me.as_deref().is_some_and(|me| me.eq_ignore_ascii_case(&commit.email));
                let is_head = labels.iter().any(|l| l.is_head);
                h_flex()
                    .id(ix)
                    .role(Role::Row)
                    .aria_label(commit.subject.clone())
                    .aria_selected(selected)
                    // as in the reference app, a right click selects the row, unless it is one of the selection
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, _, cx| {
                            if this.selected != Some(ix) && this.compare != Some(ix) {
                                this.select(ix, cx);
                            }
                        }),
                    )
                    .when(dim, |row| row.opacity(0.35))
                    .w_full()
                    .h(ROW_HEIGHT)
                    .px_2()
                    .gap_3()
                    .overflow_hidden()
                    .rounded(px(4.))
                    .when(is_merge && mark_merges && !selected, |row| row.text_color(merge_text))
                    .when(!selected && on_branch.contains(&commit.id), |row| row.bg(branch_bg))
                    .when(selected, |row| row.bg(active))
                    .when(!selected, |row| row.hover(move |row| row.bg(hover)))
                    // drop a commit on another to move it right above that one (current branch)
                    .when(stash.is_none(), |row| {
                        let dragged = history::DraggedCommit {
                            id: commit.id.clone(),
                            subject: commit.subject.clone(),
                        };
                        let target = commit.id.clone();
                        row.on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()))
                            .drag_over::<history::DraggedCommit>(move |s, _, _, _| s.bg(drop_target))
                            .on_drop(cx.listener(move |this, d: &history::DraggedCommit, window, cx| {
                                let (moved, after) = (d.id.clone(), target.clone());
                                this.run_outcome(window, cx, "Commit moved".into(), move |root| {
                                    spork_git::move_commit(root, &moved, &after)
                                })
                            }))
                    })
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        // the list takes the keys (Space, Delete, F2, ← →), not a text field
                        window.focus(&this.focus, cx);
                        let m = event.modifiers();
                        let other = this.selected.is_some_and(|s| s != ix);
                        let stash = this.repo.as_ref().and_then(|r| r.as_ref().ok()).and_then(|r| {
                            r.history.stashes.get(&r.history.commits[ix].id).cloned()
                        });
                        match stash {
                            Some(stash) => this.show_stash(stash, cx),
                            None if other && (m.shift || m.secondary(cx)) => this.compare_with(ix, cx),
                            None => this.select(ix, cx),
                        }
                    }))
                    .child(history::graph_cell(&h.rows[ix], &h.color_ids, is_head, lanes, cx))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .gap_1p5()
                            .when(!refs_left, |row| {
                                row.child(div().flex_1().min_w_0().truncate().child(commit.subject.clone()))
                            })
                            // IntelliJ's labels: one per commit when compact, at most a third of the row
                            .when(!compact, |row| {
                                row.children(history::grouped(&labels).into_iter().map(|(l, text)| {
                                    let kind = match (l.is_head, l.kind) {
                                        (true, _) => spork_workbench::RefLabel::Head,
                                        (_, RefKind::Local) => spork_workbench::RefLabel::Local,
                                        (_, RefKind::Remote) => spork_workbench::RefLabel::Remote,
                                        (_, RefKind::Tag) => spork_workbench::RefLabel::Tag,
                                    };
                                    history::label_badge(&[kind], &text, cx).flex_none()
                                }))
                            })
                            .children(history::compact_label(&labels, tag_names).filter(|_| compact).map(|(text, kinds)| {
                                // the branch it names: drag it onto another to merge, rebase or
                                // push (M03 §3)
                                let named = labels
                                    .iter()
                                    .find(|l| l.is_head)
                                    .or_else(|| labels.iter().find(|l| l.kind != RefKind::Tag))
                                    .cloned();
                                let all: Vec<String> = labels.iter().map(|l| l.name.clone()).collect();
                                history::label_badge(&kinds, &text, cx)
                                    .id("ref-label")
                                    .max_w(relative(1. / 3.))
                                    .tooltip(move |window, cx| Tooltip::new(all.join(", ")).build(window, cx))
                                    .when_some(named, |badge, l| {
                                        let target = l.clone();
                                        badge
                                            .on_drag(l, |l, _, _, cx| cx.new(|_| l.clone()))
                                            .drag_over::<history::Label>(move |s, _, _, _| s.bg(drop_target))
                                            .on_drop(cx.listener(move |this, dragged: &history::Label, window, cx| {
                                                this.ref_dropped(dragged.clone(), target.clone(), window, cx)
                                            }))
                                    })
                            }))
                            .when_some(stash.as_ref(), |row, st| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px_1p5()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(muted)
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("stash@{{{}}}", st.index)),
                                )
                            })
                            .when_some(h.folded.get(&commit.id).copied(), |row, n| {
                                let id = commit.id.clone();
                                row.child(
                                    div()
                                        .id(("folded", ix))
                                        .flex_none()
                                        .px_1p5()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(muted)
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("+{n}"))
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(format!(
                                                "{n} commits of the merged branch are folded: click to show them"
                                            ))
                                            .build(window, cx)
                                        })
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.fold_merge(id.clone(), false, window, cx)
                                        })),
                                )
                            })
                            // References on the Left: the labels, then the subject
                            .when(refs_left, |row| {
                                row.child(div().flex_1().min_w_0().truncate().child(commit.subject.clone()))
                            }),
                    )
                    // in a narrow log the subject gives way; the author and the date stay whole
                    .when(show_author, |row| {
                        row.child(
                            div()
                                .w(px(150.))
                                .flex_none()
                                .truncate()
                                .when(mine && mark_mine, |d| d.font_weight(FontWeight::BOLD))
                                .child(commit.author.clone()),
                        )
                    })
                    .when(show_date, |row| {
                        let time = match commit_time {
                            true => commit.commit_time,
                            false => commit.time,
                        };
                        row.child(
                            div()
                                .w(px(124.))
                                .flex_none()
                                .truncate()
                                .child(history::date_text(time, now)),
                        )
                    })
                    .when(show_hash, |row| {
                        row.child(
                            div()
                                .w(px(64.))
                                .flex_none()
                                .truncate()
                                .child(commit.id.chars().take(8).collect::<String>()),
                        )
                    })
                    .tooltip({
                        // the full picture behind the row's abbreviations (M05 §6)
                        let (subject, who, id) = (
                            commit.subject.clone(),
                            format!("{} <{}>", commit.author, commit.email),
                            commit.id.clone(),
                        );
                        let parents = (commit.parents.len() > 1).then(|| {
                            let short: Vec<String> = commit
                                .parents
                                .iter()
                                .map(|p| p.chars().take(7).collect())
                                .collect();
                            format!("merge of {}", short.join(" and "))
                        });
                        move |window, cx| {
                            let (subject, who, id, parents) =
                                (subject.clone(), who.clone(), id.clone(), parents.clone());
                            Tooltip::element(move |_, cx| {
                                let muted = cx.theme().muted_foreground;
                                v_flex()
                                    .max_w(px(420.))
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(subject.clone()),
                                    )
                                    .child(div().text_xs().text_color(muted).child(who.clone()))
                                    .child(div().text_xs().text_color(muted).child(id.clone()))
                                    .children(
                                        parents
                                            .clone()
                                            .map(|p| div().text_xs().text_color(muted).child(p)),
                                    )
                            })
                            .build(window, cx)
                        }
                    })
                    .map(|row| match &stash {
                        Some(st) => row.context_menu({
                            let menu = operations::stash_menu(view.clone(), st.index, st.message.clone());
                            move |m, w, cx: &mut Context<PopupMenu>| menu(m, w, cx)
                        }),
                        None => row.context_menu(RepoView::commit_menu(view.clone(), commit.id.clone())),
                    })
                    .into_any_element()
            })
            .collect()
    }
}

/// The standalone app: installs Spork's themes (JetBrains New UI palette) as the app's light and
/// dark modes and opens its windows.
pub fn main() {
    // git runs this same binary as GIT_ASKPASS / SSH_ASKPASS: answer and exit, no UI
    if std::env::var_os(spork_git::askpass::HELPER_ENV).is_some() {
        std::process::exit(spork_git::askpass::helper_main());
    }
    // apps opened from Finder get a minimal PATH; add Homebrew's so git-lfs, gpg and git
    // credential helpers installed there are found, as they are in a terminal
    #[cfg(target_os = "macos")]
    {
        let path = std::env::var("PATH").unwrap_or_default();
        let extra: Vec<&str> = ["/opt/homebrew/bin", "/usr/local/bin"]
            .into_iter()
            .filter(|dir| !path.split(':').any(|p| p == *dir))
            .collect();
        if !extra.is_empty() {
            // SAFETY: first thing in main, before any thread exists to read the environment
            unsafe { std::env::set_var("PATH", format!("{path}:{}", extra.join(":"))) };
        }
    }
    // a path given: that repository. Otherwise the tabs of last time; failing that, the current
    // folder if it's a repository (started from a terminal), else the Welcome window.
    let paths: Vec<PathBuf> = match std::env::args_os().nth(1) {
        Some(path) => vec![PathBuf::from(path)],
        None => {
            // Settings › System Settings › Reopen projects on startup
            let reopen =
                settings_window::Store::load().get("system.reopenProjects") != Some("false");
            let session = match reopen {
                true => workspace::last_session(),
                false => Vec::new(),
            };
            if session.is_empty() {
                let cwd = std::env::current_dir().unwrap_or_default();
                match spork_git::toplevel(&cwd) {
                    Ok(_) => vec![cwd],
                    Err(_) => Vec::new(),
                }
            } else {
                session
            }
        }
    };

    // shortcut: embeds the full Lucide icon set; switch to a trimmed asset source when binary size matters (P7).
    let app = gpui_kit::application().with_assets(spork_workbench::Assets);
    // the Dock icon clicked with no window left: the Welcome window
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            welcome::open(None, cx)
        }
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        spork_workbench::install(cx);
        notices::install(cx);
        shell::install(cx);
        changes::install(cx);
        e2e::start(cx);
        match paths.is_empty() {
            true => welcome::open(askpass::start(), cx),
            false => {
                open_repo_window(paths, askpass::start(), cx);
            }
        }
        cx.activate(true);
    });
}

/// A window with a tab per path (one empty tab when none), and its workspace. Only the first
/// window gets `prompts`: git's password questions from any window are asked there.
// shortcut: one prompt window; route prompts to the window whose git asked once windows are common.
pub(crate) fn open_repo_window(
    paths: Vec<PathBuf>,
    prompts: Option<smol::channel::Receiver<askpass::Prompt>>,
    cx: &mut App,
) -> Option<(AnyWindowHandle, Entity<workspace::Workspace>)> {
    // The reference app's first frame fills the screen, leaving room for the log's graph and table beside
    // the tool windows; restored, it covers two thirds of it
    let screen = cx
        .primary_display()
        .map_or(size(px(1650.), px(1080.)), |d| d.bounds().size);
    let restored = size(
        (screen.width * 0.67).max(px(1100.)).min(screen.width),
        (screen.height * 0.76).max(px(720.)).min(screen.height),
    );
    // the end-to-end tests' screenshots need the same size whatever the screen: `1400x900`
    let fixed = std::env::var("SPORK_WINDOW_SIZE").ok().and_then(|size| {
        let (width, height) = size.split_once('x')?;
        Some(gpui_kit::size(
            px(width.parse().ok()?),
            px(height.parse().ok()?),
        ))
    });
    let bounds = match fixed {
        Some(fixed) => WindowBounds::Windowed(Bounds::centered(None, fixed, cx)),
        None => WindowBounds::Maximized(Bounds::centered(None, restored, cx)),
    };
    // app-drawn title bar, so it matches the theme (and draws window buttons on Linux/Windows)
    let options = WindowOptions {
        window_bounds: Some(bounds),
        // the window buttons centered in the 40 px main toolbar
        titlebar: Some(TitlebarOptions {
            traffic_light_position: Some(point(px(13.), px(13.))),
            ..TitleBar::title_bar_options()
        }),
        ..TitleBar::window_options()
    };
    // open_window wraps the view in gpui-kit's Root (themes, dialogs, notifications)
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
        cx.new(|cx| workspace::Workspace::new(paths, prompts, window, cx))
    });
    opened
        .map_err(|e| eprintln!("failed to open a window: {e}"))
        .ok()
}

/// The loaded commits reachable from HEAD, found again only when HEAD or the history changed.
fn current_branch_commits(
    cache: &mut (
        String,
        usize,
        std::rc::Rc<std::collections::HashSet<String>>,
    ),
    repo: &Loaded,
) -> std::rc::Rc<std::collections::HashSet<String>> {
    let commits = &repo.history.commits;
    let head = repo
        .refs
        .iter()
        .find(|r| r.is_head)
        .map(|r| r.commit.clone())
        .unwrap_or_default();
    if cache.0 != head || cache.1 != commits.len() {
        let parents: std::collections::HashMap<&str, &[String]> = commits
            .iter()
            .map(|c| (c.id.as_str(), c.parents.as_slice()))
            .collect();
        let mut found = std::collections::HashSet::new();
        let mut next = vec![head.clone()];
        while let Some(id) = next.pop() {
            if let Some(up) = parents.get(id.as_str())
                && found.insert(id.clone())
            {
                next.extend(up.iter().cloned());
            }
        }
        *cache = (head, commits.len(), std::rc::Rc::new(found));
    }
    cache.2.clone()
}

#[cfg(test)]
mod tests {
    // explicit: `super::*` would pull in gpui's `test` attribute, which shadows #[test]
    use super::start_watcher;
    use std::time::{Duration, Instant};

    // a merge started while the repository loads changes `.git` before the view takes the
    // watcher's changes: they wait for it (the Git menu missed such a merge)
    #[test]
    fn keeps_changes_until_they_are_taken() {
        let root = std::env::temp_dir().join(format!("spork-watch-{}", std::process::id()));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let root = root.canonicalize().unwrap();
        let (_watcher, changes) = start_watcher(root.clone()).unwrap();
        std::fs::write(root.join(".git").join("MERGE_HEAD"), "0\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut reload = false;
        while !reload && Instant::now() < deadline {
            match changes.try_recv() {
                Ok(full) => reload |= full,
                Err(_) => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        std::fs::remove_dir_all(&root).unwrap();
        assert!(reload, "no full reload asked for after `.git` changed");
    }
}
