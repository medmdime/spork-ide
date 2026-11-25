//! App menus, keyboard shortcuts, the command palette and Quick Launch (docs/specs/M01 §4, §7).

use crate::notices::Notification;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandItem, CommandState};
use gpui_kit::component::input;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, GlobalState, Selectable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use spork_git::gitflow::Kind;
use spork_workbench::Typography as _;

use crate::util::ResultExt as _;
use crate::view_filters::Toggle;
use crate::{RemoteOp, RepoView, View};

actions!(
    spork,
    [
        Fetch,
        Pull,
        Push,
        Stash,
        NewBranch,
        AddRemote,
        ShowChanges,
        ShowHistory,
        ToggleProject,
        ToggleCommit,
        ToggleNotifications,
        ToggleFullScreen,
        ThemeDark,
        ThemeLight,
        ThemeSync,
        Refresh,
        OpenRepository,
        CloneRepository,
        NewRepository,
        QuickLaunch,
        QuickCheckout,
        DeleteSelectedBranch,
        RenameSelectedBranch,
        CommandPalette,
        RepositorySettings,
        RevealRepository,
        OpenTerminal,
        ActivityLog,
        AiSettings,
        Preferences,
        Statistics,
        NewWindow,
        SelectPreviousCommit,
        SelectNextCommit,
        FoldMerge,
        UnfoldMerge,
        CopyCommitSha,
        SshKeys,
        CustomCommands,
        RepositoryDetails,
        Notifications,
        AskGit,
        UndoLastChange,
        NewTab,
        CloseTab,
        NextTab,
        PreviousTab,
        ToggleCurrentBranch,
        ToggleFirstParent,
        ToggleLostCommits,
        ToggleHideTags,
        ToggleHideStashes,
        ToggleHideRemotes,
        NewTag,
        ApplyPatch,
        AddSubmodule,
        DeleteMergedBranches,
        ConnectAccount,
        Accounts,
        Minimize,
        Zoom,
        ZoomIn,
        ZoomOut,
        ActualSize,
        ToggleTerminal,
        OpenWebsite,
        ReportIssue,
        FindCommit,
        ReopenClosedTab,
        ToggleStageSelected,
        StartBranch,
        FinishBranch,
        About,
        GitFlowStartFeature,
        GitFlowStartBugfix,
        GitFlowStartRelease,
        GitFlowStartHotfix,
        LfsPull,
        LfsFetch,
        LfsTrack,
        LfsUntrack,
        LfsFiles,
        NewWorktree,
        ShowWorktrees,
        RunAnything,
        ShowPlugins,
        ShowBackupAndSync,
        CustomizeToolbar,
        OpenInEditor,
        Quit,
        GitPull,
        GitMerge,
        GitRebase,
        GitReset,
        Unshallow,
        CommitMerge,
        AbortMerge,
        AbortRebase,
        ContinueRebase,
        SkipCommit,
        ResolveConflicts,
        AbortRevert,
        ContinueCherryPick,
        AbortCherryPick,
        ManageRemotes,
        VcsOperationsPopup,
        CopyBranchName,
        ShowFileHistory,
        GetFromVersionControl,
        ShowLocalChanges,
        ShowConsoleTab,
        CheckoutRevision,
        CopyCommitSubject,
        QuickSwitchScheme,
        ContextHelp,
        ShowStagingArea,
    ]
);

const WEBSITE: &str = "https://github.com/medmdime/Spork";

/// What the Git menu shows besides its fixed items: the reference app shows an operation's Continue and
/// Abort only while git has it paused, Resolve Conflicts… only with conflicts, Unshallow only in a
/// shallow clone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GitMenu {
    pub merging: bool,
    pub rebasing: bool,
    pub cherry_picking: bool,
    pub reverting: bool,
    pub conflicts: bool,
    pub shallow: bool,
    /// Show Git Stash shows only with a stash to show, and Unstash Changes… is greyed without one.
    pub stashes: bool,
    /// The file Git ▸ Current File acts on (file_view.rs).
    pub file: Option<crate::file_view::CurrentFile>,
}

impl GitMenu {
    /// Every item, for the command palette.
    const ALL: Self = Self {
        merging: true,
        rebasing: true,
        cherry_picking: true,
        reverting: true,
        conflicts: true,
        shallow: true,
        stashes: true,
        file: Some(crate::file_view::CurrentFile {
            tracked: true,
            addable: true,
        }),
    };
}

/// The reference app's Git menu (`Git.MainMenu`), in its order; the actions Spork has beyond it are in
/// [`extras`].
pub(crate) fn git_menu(git: GitMenu) -> Menu {
    let mut items = vec![
        MenuItem::action("Commit…", ShowChanges),
        MenuItem::action("Push…", Push),
        // The reference app's own text, three dots and all
        MenuItem::action("Update Project...", Pull),
        MenuItem::action("Pull…", GitPull),
        MenuItem::action("Fetch", Fetch),
    ];
    if git.shallow {
        items.push(MenuItem::action("Unshallow repository", Unshallow));
    }
    items.extend([MenuItem::separator(), MenuItem::action("Merge…", GitMerge)]);
    if git.merging {
        items.extend([
            MenuItem::action("Commit merge", CommitMerge),
            MenuItem::action("Abort Merge", AbortMerge),
        ]);
    }
    items.push(MenuItem::action("Rebase…", GitRebase));
    if git.rebasing {
        items.push(MenuItem::submenu(Menu::new("Rebase").items([
            MenuItem::action("Abort Rebase", AbortRebase),
            MenuItem::action("Continue Rebase", ContinueRebase),
            MenuItem::action("Skip Commit", SkipCommit),
        ])));
    }
    if git.conflicts {
        items.push(MenuItem::action("Resolve Conflicts…", ResolveConflicts));
    }
    items.extend([
        MenuItem::separator(),
        MenuItem::action("Branches…", QuickCheckout),
        MenuItem::action("New Branch…", NewBranch),
        MenuItem::action("New Tag…", NewTag),
        MenuItem::action("Reset HEAD…", GitReset),
        MenuItem::separator(),
        MenuItem::action("New Worktree…", NewWorktree),
        MenuItem::action("Worktrees…", ShowWorktrees),
        MenuItem::separator(),
        MenuItem::action("Show Git Log", ShowHistory),
        // The reference app's Patch.MainMenu as it shows: its Apply Patch… stays hidden (Find Action has it)
        MenuItem::submenu(Menu::new("Patch").items([
            MenuItem::action(
                "Create Patch from Local Changes…",
                crate::changes::CreatePatch,
            ),
            MenuItem::action(
                "Apply Patch from Clipboard…",
                crate::stash_view::ApplyPatchFromClipboard,
            ),
        ])),
        MenuItem::submenu(
            Menu::new("Uncommitted Changes").items(
                [
                    Some(MenuItem::action("Shelve Changes…", crate::changes::Shelve)),
                    Some(MenuItem::action("Show Shelf", crate::stash_view::ShowShelf)),
                    git.stashes
                        .then(|| MenuItem::action("Show Git Stash", crate::stash_view::ShowStash)),
                    Some(MenuItem::action("Stash Changes…", Stash)),
                    Some(
                        MenuItem::action("Unstash Changes…", crate::stash_view::Unstash)
                            .disabled(!git.stashes),
                    ),
                    Some(MenuItem::action("Rollback…", crate::changes::Rollback)),
                ]
                .into_iter()
                .flatten(),
            ),
        ),
        MenuItem::submenu(crate::file_view::current_file_menu(git.file)),
        MenuItem::separator(),
        MenuItem::submenu(crate::hosting::gitlab_menu()),
        MenuItem::submenu(crate::hosting::github_menu()),
        MenuItem::action("Manage Remotes…", ManageRemotes),
        MenuItem::action("Clone…", CloneRepository),
        MenuItem::separator(),
        MenuItem::action("VCS Operations Popup…", VcsOperationsPopup),
    ]);
    if git.reverting || git.cherry_picking {
        items.push(MenuItem::separator());
    }
    if git.reverting {
        items.push(MenuItem::action("Abort Revert", AbortRevert));
    }
    if git.cherry_picking {
        items.extend([
            MenuItem::action("Continue Cherry-Pick", ContinueCherryPick),
            MenuItem::action("Abort Cherry-Pick", AbortCherryPick),
        ]);
    }
    Menu::new("Git").items(items)
}

/// Spork's commands that the reference app has no menu item for: Find Action still lists them.
fn extras() -> Vec<MenuItem> {
    vec![
        MenuItem::action("Create Git Repository…", NewRepository),
        MenuItem::action("Get from Version Control…", GetFromVersionControl),
        MenuItem::action("Add Remote…", AddRemote),
        MenuItem::action("Copy Branch Name", CopyBranchName),
        MenuItem::action("Copy Commit Subject", CopyCommitSubject),
        MenuItem::action("Checkout Revision", CheckoutRevision),
        MenuItem::action("Show Local Changes", ShowLocalChanges),
        MenuItem::action("Show VCS Console Tab", ShowConsoleTab),
        MenuItem::action("Quick Switch Scheme…", QuickSwitchScheme),
        MenuItem::action("Context Help", ContextHelp),
        MenuItem::action("Show Staging Area", ShowStagingArea),
        MenuItem::action("Start Feature…", GitFlowStartFeature),
        MenuItem::action("Start Bugfix…", GitFlowStartBugfix),
        MenuItem::action("Start Release…", GitFlowStartRelease),
        MenuItem::action("Start Hotfix…", GitFlowStartHotfix),
        MenuItem::action("Start Branch from Main…", StartBranch),
        MenuItem::action("Finish Branch into Main…", FinishBranch),
        MenuItem::action("Pull LFS Files", LfsPull),
        MenuItem::action("Fetch LFS Files", LfsFetch),
        MenuItem::action("Track Pattern…", LfsTrack),
        MenuItem::action("Untrack Pattern…", LfsUntrack),
        MenuItem::action("LFS Files…", LfsFiles),
        MenuItem::action("Apply Patch…", ApplyPatch),
        MenuItem::action("Add Submodule…", AddSubmodule),
        MenuItem::action("Delete Merged Branches…", DeleteMergedBranches),
        MenuItem::action("Repository Settings…", RepositorySettings),
    ]
}

thread_local! {
    /// The Git menu the menu bar shows now.
    static SHOWN_GIT_MENU: std::cell::Cell<Option<GitMenu>> = const { std::cell::Cell::new(None) };
}

/// Shows the Git menu for the front window's repository, rebuilding the menu bar only when
/// what it shows changes.
pub(crate) fn show_git_menu(git: GitMenu, cx: &mut App) {
    if SHOWN_GIT_MENU.get() == Some(git) {
        return;
    }
    SHOWN_GIT_MENU.set(Some(git));
    // asked during a render: AppKit's menu is rebuilt once GPUI is done with the frame
    cx.defer(move |cx| {
        cx.set_menus(menus(git));
        GlobalState::global_mut(cx)
            .set_app_menus(menus(git).into_iter().map(Menu::owned).collect());
    });
}

/// The Git menu set for `git` right away, outside a render: the end-to-end tests read the menu
/// bar of a window macOS may not draw while others cover it.
pub(crate) fn set_git_menu(git: GitMenu, cx: &mut App) {
    SHOWN_GIT_MENU.set(Some(git));
    cx.set_menus(menus(git));
    GlobalState::global_mut(cx).set_app_menus(menus(git).into_iter().map(Menu::owned).collect());
}

/// The menu bar with every item, for Settings › Keymap's Main Menu.
pub(crate) fn all_menus() -> Vec<Menu> {
    menus(GitMenu::ALL)
}

/// The menu bar; the command palette lists the same entries.
fn menus(git: GitMenu) -> Vec<Menu> {
    vec![
        Menu::new("Spork").items([
            MenuItem::action("About Spork", About),
            MenuItem::separator(),
            MenuItem::action("Settings…", Preferences),
            MenuItem::separator(),
            MenuItem::action("Quit Spork", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Window", NewWindow),
            MenuItem::action("Open…", OpenRepository),
            MenuItem::action("Clone Repository…", CloneRepository),
            MenuItem::separator(),
            MenuItem::action("Close Project", CloseTab),
            MenuItem::separator(),
            MenuItem::action("Reload All from Disk", Refresh),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Open In").items([
                MenuItem::action("Finder", RevealRepository),
                MenuItem::action("Terminal", OpenTerminal),
                MenuItem::action("Editor", OpenInEditor),
            ])),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", input::Undo, OsAction::Undo),
            MenuItem::os_action("Redo", input::Redo, OsAction::Redo),
            MenuItem::action("Undo Last Git Operation…", UndoLastChange),
            MenuItem::separator(),
            MenuItem::os_action("Cut", input::Cut, OsAction::Cut),
            MenuItem::os_action("Copy", input::Copy, OsAction::Copy),
            MenuItem::os_action("Paste", input::Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Select All", input::SelectAll, OsAction::SelectAll),
        ]),
        Menu::new("View").items([
            MenuItem::submenu(Menu::new("Tool Windows").items([
                MenuItem::action("Project", ToggleProject),
                MenuItem::action("Commit", ToggleCommit),
                MenuItem::action("Git", ActivityLog),
                MenuItem::action("Git Worktrees", ShowWorktrees),
                MenuItem::action("Terminal", ToggleTerminal),
                MenuItem::action("Notifications", ToggleNotifications),
            ])),
            MenuItem::submenu(Menu::new("Appearance").items([
                MenuItem::action("Zoom In", ZoomIn),
                MenuItem::action("Zoom Out", ZoomOut),
                MenuItem::action("Reset Zoom", ActualSize),
                MenuItem::separator(),
                MenuItem::action("Enter Full Screen", ToggleFullScreen),
            ])),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Git Log").items([
                MenuItem::action("Only Current Branch", ToggleCurrentBranch),
                MenuItem::action("First Parent Only", ToggleFirstParent),
                MenuItem::action("Show Lost Commits", ToggleLostCommits),
                MenuItem::action("Hide Tags", ToggleHideTags),
                MenuItem::action("Hide Stashes", ToggleHideStashes),
                MenuItem::action("Hide Remote Branches", ToggleHideRemotes),
            ])),
        ]),
        Menu::new("Navigate").items([
            MenuItem::action("Search Everywhere…", QuickLaunch),
            MenuItem::action("Go to Hash/Branch/Tag…", FindCommit),
        ]),
        Menu::new("Tools").items([
            MenuItem::action("External Tools…", CustomCommands),
            MenuItem::action("SSH Keys…", SshKeys),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("AI Assistant").items([
                MenuItem::action("Ask Git…", AskGit),
                MenuItem::action("AI Settings…", AiSettings),
            ])),
            MenuItem::separator(),
            MenuItem::action("Statistics…", Statistics),
            MenuItem::action("Repository Details…", RepositoryDetails),
        ]),
        git_menu(git),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::separator(),
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("Reopen Closed Tab", ReopenClosedTab),
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Previous Tab", PreviousTab),
        ]),
        Menu::new("Help").items([
            MenuItem::action("Find Action…", CommandPalette),
            MenuItem::separator(),
            MenuItem::action("Spork on GitHub", OpenWebsite),
            MenuItem::action("Report an Issue", ReportIssue),
        ]),
    ]
}

/// Key bindings and menus; `secondary` is Cmd on macOS and Ctrl elsewhere.
pub fn install(cx: &mut App) {
    let bindings: Vec<(&str, Box<dyn Action>)> = vec![
        ("secondary-t", Box::new(Pull)),
        // The reference app's keymaps: ⌃V on macOS, Alt+` elsewhere, where Ctrl+V pastes
        #[cfg(target_os = "macos")]
        ("ctrl-v", Box::new(VcsOperationsPopup)),
        #[cfg(not(target_os = "macos"))]
        ("alt-`", Box::new(VcsOperationsPopup)),
        ("ctrl-`", Box::new(QuickSwitchScheme)),
        ("secondary-shift-k", Box::new(Push)),
        ("secondary-k", Box::new(ShowChanges)),
        ("secondary-shift-s", Box::new(Stash)),
        ("alt-secondary-n", Box::new(NewBranch)),
        ("secondary-1", Box::new(ToggleProject)),
        ("secondary-0", Box::new(ToggleCommit)),
        ("secondary-9", Box::new(ActivityLog)),
        ("secondary-2", Box::new(ShowHistory)),
        ("secondary-r", Box::new(Refresh)),
        // The reference app's Synchronize, which the Worktrees tab's Refresh shares
        ("alt-secondary-y", Box::new(Refresh)),
        ("shift-secondary-f1", Box::new(ContextHelp)),
        ("secondary-o", Box::new(OpenRepository)),
        ("secondary-p", Box::new(QuickLaunch)),
        ("secondary-b", Box::new(QuickCheckout)),
        ("secondary-shift-a", Box::new(CommandPalette)),
        ("secondary-shift-p", Box::new(CommandPalette)),
        ("secondary-q", Box::new(Quit)),
        ("secondary-,", Box::new(Preferences)),
        ("secondary-n", Box::new(NewWindow)),
        ("secondary-w", Box::new(CloseTab)),
        ("secondary-m", Box::new(Minimize)),
        ("secondary-f", Box::new(FindCommit)),
        ("secondary-shift-t", Box::new(ReopenClosedTab)),
        ("alt-f12", Box::new(ToggleTerminal)),
        ("ctrl-secondary-f", Box::new(ToggleFullScreen)),
        ("alt-shift-secondary-=", Box::new(ZoomIn)),
        ("alt-shift-secondary--", Box::new(ZoomOut)),
        ("alt-shift-secondary-0", Box::new(ActualSize)),
        ("secondary-shift-]", Box::new(NextTab)),
        ("secondary-shift-[", Box::new(PreviousTab)),
        ("ctrl-tab", Box::new(NextTab)),
        ("ctrl-shift-tab", Box::new(PreviousTab)),
    ];
    // `load` resolves `secondary` for this OS (`KeyBinding::new` can't)
    let bindings: Vec<KeyBinding> = bindings
        .into_iter()
        .filter_map(|(keys, action)| {
            KeyBinding::load(keys, action, None, false, None, cx).log_err()
        })
        .collect();
    cx.bind_keys(bindings);
    bind_view_keys(cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &OpenWebsite, cx| cx.open_url(WEBSITE));
    cx.on_action(|_: &About, cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, cx| {
                    window.open_alert_dialog(cx, |dialog, _, _| {
                        dialog
                            .title(format!("Spork {}", env!("CARGO_PKG_VERSION")))
                            .description(format!(
                                "A free, open-source git client.\nLicensed under the GPL, version 3 or later.\n{WEBSITE}"
                            ))
                    })
                })
                .ok();
        }
    });
    cx.on_action(|_: &ReportIssue, cx| cx.open_url(&format!("{WEBSITE}/issues/new")));
    // a new window starts on the welcome screen (home isn't a repository) with the recent list
    cx.on_action(|_: &NewWindow, cx| {
        crate::open_repo_window(Vec::new(), None, cx);
    });
    // Windows and Linux draw the same menus in the title bar (AppMenuBar)
    show_git_menu(GitMenu::default(), cx);
}

/// The keys of the repository view's parts, bound in their key contexts: the standalone app's
/// and the IDE's.
pub(crate) fn bind_view_keys(cx: &mut App) {
    // the commit list's keys only apply inside the repository view; text fields keep theirs
    let in_view = || {
        KeyBindingContextPredicate::parse("Spork")
            .ok()
            .map(std::rc::Rc::new)
    };
    let list_keys: Vec<KeyBinding> = [
        ("up", Box::new(SelectPreviousCommit) as Box<dyn Action>),
        ("down", Box::new(SelectNextCommit)),
        ("delete", Box::new(DeleteSelectedBranch)),
        ("backspace", Box::new(DeleteSelectedBranch)),
        ("space", Box::new(ToggleStageSelected)),
        ("secondary-c", Box::new(CopyCommitSha)),
        // text fields keep their own undo: this only applies outside them
        ("secondary-z", Box::new(UndoLastChange)),
    ]
    .into_iter()
    .filter_map(|(keys, action)| KeyBinding::load(keys, action, in_view(), false, None, cx).ok())
    .collect();
    cx.bind_keys(list_keys);
    crate::stash_view::install(cx);
    crate::operations::install(cx);
    crate::log_toolbar::install(cx);
    crate::file_view::bind_keys(cx);
    crate::diff_bar::bind_keys(cx);
}

/// `(label, action)` of every menu entry, for the command palette.
pub(crate) fn commands() -> Vec<(SharedString, Box<dyn Action>)> {
    fn walk(items: Vec<MenuItem>, out: &mut Vec<(SharedString, Box<dyn Action>)>) {
        for item in items {
            match item {
                // text-field editing (Edit menu) isn't a command to run from the palette
                MenuItem::Action {
                    name,
                    action,
                    os_action: None,
                    ..
                } => out.push((name, action)),
                MenuItem::Submenu(menu) => walk(menu.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus(GitMenu::ALL) {
        walk(menu.items, &mut out);
    }
    walk(extras(), &mut out);
    out.extend(crate::hosting::palette_commands());
    out.extend(crate::changes::commands());
    out.extend(crate::stash_view::commands());
    out.extend(crate::operations::log_commands());
    out.extend(crate::log_toolbar::commands());
    out.extend(crate::branches::commands());
    out.extend(crate::file_view::commands());
    out.extend(crate::diff_bar::commands());
    out
}

/// Builds the menu bar again, for menus that follow the project in front (hosting.rs).
pub(crate) fn refresh_menus(cx: &mut App) {
    let git = SHOWN_GIT_MENU.get().unwrap_or_default();
    cx.set_menus(menus(git));
    GlobalState::global_mut(cx).set_app_menus(menus(git).into_iter().map(Menu::owned).collect());
}

impl RepoView {
    /// Hooks every app action to this view; the view must hold focus for menus and keys to reach it.
    pub(crate) fn on_actions(&self, el: Div, cx: &mut Context<Self>) -> Div {
        let el = self.saved_actions(el, cx);
        el.on_action(cx.listener(|this, _: &Fetch, w, cx| this.remote_op(RemoteOp::Fetch, w, cx)))
            .on_action(cx.listener(|this, _: &Pull, w, cx| this.pull_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &Push, w, cx| this.push_commits_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &Stash, w, cx| this.stash_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &NewBranch, w, cx| {
                this.new_branch_dialog(this.log_new_branch_start(w), w, cx)
            }))
            .on_action(cx.listener(|this, _: &AddRemote, w, cx| this.add_remote_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &ShowChanges, window, cx| this.checkin_project(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleProject, w, cx| {
                this.toggle_left(crate::LeftTool::Project, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleCommit, w, cx| {
                this.toggle_left(crate::LeftTool::Commit, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowHistory, _, cx| {
                this.view = View::History;
                cx.emit(crate::ide::ToolWindowEvent::Show(
                    crate::ide::ToolWindow::Git,
                ));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.reload(cx)))
            .on_action(cx.listener(|this, _: &OpenRepository, w, cx| this.open_repo_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &CloneRepository, w, cx| this.clone_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &NewRepository, w, cx| this.new_repo_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &QuickLaunch, w, cx| {
                this.toggle_search_everywhere(crate::search_everywhere::Tab::All, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &QuickCheckout, w, cx| this.toggle_branches_popup(w, cx)),
            )
            .on_action(cx.listener(|this, _: &DeleteSelectedBranch, w, cx| {
                this.delete_selected_branch(w, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameSelectedBranch, w, cx| {
                this.rename_selected_branch(w, cx)
            }))
            .on_action(cx.listener(|this, _: &CommandPalette, w, cx| {
                this.toggle_search_everywhere(crate::search_everywhere::Tab::Actions, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &RepositorySettings, w, cx| this.settings_dialog(w, cx)),
            )
            .on_action(cx.listener(|this, _: &RevealRepository, w, cx| this.reveal_repo(w, cx)))
            .on_action(cx.listener(|this, _: &OpenTerminal, w, cx| this.open_terminal(w, cx)))
            .on_action(cx.listener(|this, _: &ActivityLog, _, cx| this.toggle_console(cx)))
            .on_action(cx.listener(|this, _: &RunAnything, w, cx| this.run_anything(w, cx)))
            .on_action(cx.listener(|this, _: &ShowPlugins, w, cx| {
                this.settings_page(Some("Plugins".into()), w, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowBackupAndSync, w, cx| {
                this.settings_page(Some("Backup and Sync".into()), w, cx)
            }))
            .on_action(cx.listener(|this, _: &CustomizeToolbar, w, cx| {
                this.settings_page(
                    Some("Appearance & Behavior/Menus and Toolbars".into()),
                    w,
                    cx,
                )
            }))
            .on_action(cx.listener(|this, _: &ShowWorktrees, _, cx| this.show_worktrees(cx)))
            .on_action(cx.listener(|this, _: &AiSettings, w, cx| this.ai_settings_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &Preferences, w, cx| this.preferences_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &ToggleTerminal, w, cx| this.toggle_terminal(w, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, w, cx| this.zoom(1, w, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, w, cx| this.zoom(-1, w, cx)))
            .on_action(cx.listener(|this, _: &ActualSize, w, cx| this.zoom(0, w, cx)))
            .on_action(cx.listener(|this, _: &Statistics, w, cx| this.stats_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &OpenInEditor, w, cx| this.open_in_editor(None, w, cx)),
            )
            .on_action(cx.listener(|this, _: &GitFlowStartFeature, w, cx| {
                this.gitflow_start(Kind::Feature, w, cx)
            }))
            .on_action(cx.listener(|this, _: &GitFlowStartBugfix, w, cx| {
                this.gitflow_start(Kind::Bugfix, w, cx)
            }))
            .on_action(cx.listener(|this, _: &GitFlowStartRelease, w, cx| {
                this.gitflow_start(Kind::Release, w, cx)
            }))
            .on_action(cx.listener(|this, _: &GitFlowStartHotfix, w, cx| {
                this.gitflow_start(Kind::Hotfix, w, cx)
            }))
            .on_action(cx.listener(|this, _: &LfsPull, w, cx| this.lfs_transfer(true, w, cx)))
            .on_action(cx.listener(|this, _: &LfsFetch, w, cx| this.lfs_transfer(false, w, cx)))
            .on_action(cx.listener(|this, _: &LfsTrack, w, cx| this.lfs_track_dialog(true, w, cx)))
            .on_action(
                cx.listener(|this, _: &LfsUntrack, w, cx| this.lfs_track_dialog(false, w, cx)),
            )
            .on_action(cx.listener(|this, _: &LfsFiles, w, cx| this.lfs_files_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &NewWorktree, w, cx| this.new_branch_worktree_dialog(w, cx)),
            )
            .on_action(cx.listener(|this, _: &StartBranch, w, cx| this.start_branch(w, cx)))
            .on_action(
                cx.listener(|this, _: &FinishBranch, w, cx| this.finish_branch_dialog(w, cx)),
            )
            .on_action(cx.listener(|this, _: &FindCommit, w, cx| this.log_go_to_ref_popup(w, cx)))
            .on_action(cx.listener(|this, _: &NewTag, w, cx| this.new_tag_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &ApplyPatch, w, cx| this.apply_patch_dialog(true, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &AddSubmodule, w, cx| this.add_submodule_dialog(w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &DeleteMergedBranches, w, cx| {
                    this.delete_merged_dialog(w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ConnectAccount, w, cx| this.connect_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &Accounts, w, cx| this.accounts_dialog(w, cx)))
            .on_action(|_: &Minimize, w, _| w.minimize_window())
            .on_action(|_: &ToggleFullScreen, w, _| w.toggle_fullscreen())
            .on_action(
                cx.listener(|this, _: &ToggleNotifications, _, cx| this.toggle_notifications(cx)),
            )
            .on_action(cx.listener(|this, _: &ThemeDark, w, cx| {
                this.set_appearance(crate::prefs::Appearance::Dark, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ThemeLight, w, cx| {
                this.set_appearance(crate::prefs::Appearance::Light, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ThemeSync, w, cx| {
                this.set_appearance(crate::prefs::Appearance::System, w, cx)
            }))
            .on_action(|_: &Zoom, w, _| w.zoom_window())
            .on_action(cx.listener(|this, _: &UndoLastChange, w, cx| this.undo_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &AskGit, w, cx| this.ask_git_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &Notifications, w, cx| this.notifications_dialog(w, cx)),
            )
            .on_action(cx.listener(|this, _: &RepositoryDetails, w, cx| this.details_dialog(w, cx)))
            .on_action(cx.listener(|this, _: &SshKeys, w, cx| this.ssh_keys_dialog(w, cx)))
            .on_action(
                cx.listener(|this, _: &CustomCommands, w, cx| this.edit_custom_commands(w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectPreviousCommit, _, cx| this.step_commit(-1, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectNextCommit, _, cx| this.step_commit(1, cx)))
            .on_action(cx.listener(|this, _: &FoldMerge, w, cx| this.fold_selected(true, w, cx)))
            .on_action(cx.listener(|this, _: &UnfoldMerge, w, cx| this.fold_selected(false, w, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleStageSelected, w, cx| {
                    this.toggle_stage_selected(w, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &CopyCommitSha, w, cx| this.copy_selected_sha(w, cx)))
            .on_action(cx.listener(|this, _: &ToggleCurrentBranch, _, cx| {
                this.toggle_filter(Toggle::CurrentBranch, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleFirstParent, _, cx| {
                this.toggle_filter(Toggle::FirstParent, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleLostCommits, _, cx| {
                this.toggle_filter(Toggle::LostCommits, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleHideRemotes, _, cx| {
                this.toggle_filter(Toggle::HideRemotes, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleHideTags, _, cx| {
                this.toggle_filter(Toggle::HideTags, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleHideStashes, _, cx| {
                this.toggle_filter(Toggle::HideStashes, cx)
            }))
            .map(|el| self.git_menu_actions(el, cx))
            .on_action(
                cx.listener(|this, _: &crate::file_view::SaveRevisionAs, window, cx| {
                    this.save_selected_revision(window, cx)
                }),
            )
    }

    /// ↑ / ↓ in the commit list (M05 §7); other views get the key.
    fn step_commit(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = match &self.repo {
            Some(Ok(repo)) => repo.history.commits.len(),
            _ => 0,
        };
        if self.view == View::Changes {
            return self.step_change(delta, cx);
        }
        if self.view != View::History || count == 0 {
            return cx.propagate();
        }
        let ix = self
            .selected
            .map_or(0, |i| i.saturating_add_signed(delta).min(count - 1));
        self.select(ix, cx);
        self.history_scroll
            .scroll_to_item(ix, ScrollStrategy::Nearest);
    }

    /// Shows All Commits with `commit` selected and scrolled into view.
    pub(crate) fn reveal_commit(
        &mut self,
        commit: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Ok(repo)) = &self.repo else { return };
        let Some(ix) = repo.history.commits.iter().position(|c| c.id == commit) else {
            return window.push_notification(
                Notification::info("That commit isn't in the list: check the View filters"),
                cx,
            );
        };
        self.view = View::History;
        self.select(ix, cx);
        self.history_scroll
            .scroll_to_item(ix, ScrollStrategy::Center);
    }

    /// Cmd/Ctrl-C with a commit selected copies its SHA.
    fn copy_selected_sha(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = match (&self.repo, self.selected) {
            (Some(Ok(repo)), Some(ix)) if self.view == View::History => {
                repo.history.commits.get(ix).map(|c| c.id.clone())
            }
            _ => None,
        };
        let Some(id) = id else { return cx.propagate() };
        cx.write_to_clipboard(ClipboardItem::new_string(id));
        window.push_notification(Notification::info("Commit SHA copied"), cx);
    }
}

pub(crate) fn palette(
    window: &mut Window,
    cx: &mut App,
    state: Entity<CommandState>,
    items: Vec<CommandItem>,
    placeholder: &'static str,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) {
    let on_pick = std::rc::Rc::new(on_pick);
    // typing searches at once, as in IntelliJ's list popups
    let focus = state.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialog, _, _| {
        let on_pick = on_pick.clone();
        dialog.p_0().child(
            Command::new(&state)
                .items(items.clone())
                .placeholder(placeholder)
                .bordered(false)
                .on_confirm(move |ix, window, cx| on_pick(ix.row, window, cx)),
        )
    });
    focus.focus(window, cx);
}

/// IntelliJ's "New" badge of the Worktrees tab (`Badge.new`).
pub(crate) fn new_badge(cx: &App) -> Div {
    let tokens = spork_workbench::Tokens::get(cx);
    div()
        .ml(px(6.))
        .px(px(5.))
        .rounded(px(8.))
        .bg(tokens.badge)
        .text_color(tokens.badge_text)
        .text_ui_xsmall()
        .child("New")
}

impl RepoView {
    /// Worktrees… (the Git menu, View ▸ Tool Windows): the Git tool window on its Worktrees tab.
    pub(crate) fn show_worktrees(&mut self, cx: &mut Context<Self>) {
        self.show_console = true;
        self.show_terminal = false;
        self.worktrees_closed = false;
        self.worktrees_opened = true;
        self.worktrees_tab = true;
        if self.ide {
            cx.emit(crate::ide::ToolWindowEvent::Show(
                crate::ide::ToolWindow::Git,
            ));
        }
        cx.notify();
    }

    /// The IDE's Worktrees tab shows, as IntelliJ's, while a repository has linked worktrees or
    /// once Worktrees… opened it, until its × closes it.
    pub(crate) fn worktrees_tab_shown(&self) -> bool {
        let linked = match &self.repo {
            Some(Ok(repo)) => repo.worktrees.iter().any(|worktree| !worktree.is_main),
            _ => false,
        };
        !self.worktrees_closed && (self.worktrees_opened || linked)
    }

    /// The Git tool window (⌘9), as in IntelliJ: its Console tab, the git commands Spork ran
    /// that changed something or failed with git's own output (M01 §5), and its Worktrees tab. A
    /// running fetch / pull / push / clone shows in the header with Cancel.
    pub(crate) fn git_window(&mut self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let running = spork_git::job_progress();
        let worktrees = self.worktrees_tab;
        let tab = |id: &'static str,
                   label: &'static str,
                   on: bool,
                   worktrees: bool,
                   cx: &mut Context<Self>| {
            spork_workbench::shell::header_tab(id, label, on, cx).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.worktrees_tab = worktrees;
                    cx.notify();
                },
            ))
        };
        let tokens = spork_workbench::Tokens::get(cx);
        // IntelliJ's "New" badge on the tab, and its × to close the tab
        let new_badge = new_badge(cx);
        let close_worktrees =
            spork_workbench::ActionButton::new("close-worktrees", "general/closeSmall")
                .tooltip("Close Tab")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.worktrees_tab = false;
                    this.worktrees_closed = true;
                    cx.notify();
                }));
        let header = h_flex()
            .w_full()
            .h(px(40.))
            .flex_none()
            .pl_3()
            .pr_1p5()
            .gap_1()
            .border_b_1()
            .border_color(tokens.separator)
            .child(div().mr_2().font_weight(FontWeight::SEMIBOLD).child("Git"))
            .child(tab("console-tab", "Console", !worktrees, false, cx))
            .when(!self.worktrees_closed, |header| {
                header
                    .child(tab("worktrees-tab", "Worktrees", worktrees, true, cx).child(new_badge))
                    .child(close_worktrees)
            })
            .child(self.log_tab_actions(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .truncate()
                    .text_color(muted)
                    .children(running.clone().map(|line| format!("Running: {line}"))),
            )
            .when(running.is_some(), |h| {
                h.child(
                    Button::new("console-cancel")
                        .small()
                        .ghost()
                        .label("Cancel")
                        .on_click(|_, _, _| spork_git::cancel_job()),
                )
            })
            .child(
                spork_workbench::shell::tool_window_actions().child(
                    spork_workbench::ActionButton::new("hide-console", "general/hide")
                        .tooltip("Hide (⌘9)")
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_console(cx))),
                ),
            );
        let body = match worktrees {
            true => self.worktrees_tab(cx),
            false => self.console_tab(cx),
        };
        v_flex()
            .size_full()
            .child(header)
            .child(h_flex().flex_1().min_h_0().items_start().child(body))
    }

    /// IntelliJ's VCS console: `time: [repo] git …` lines in the scheme's system color, git's
    /// output under each, a vertical toolbar on the left.
    pub(crate) fn console_tab(&mut self, cx: &mut Context<Self>) -> Div {
        let tokens = spork_workbench::Tokens::get(cx);
        let mono = cx.theme().mono_font_family.clone();
        let log = spork_git::activity();
        if self.console_follow && log.len() != self.console_seen {
            self.console_scroll.scroll_to_bottom();
        }
        self.console_seen = log.len();
        let (soft_wrap, empty) = (self.console_soft_wrap, log.is_empty());
        let text: String = log
            .iter()
            .map(|c| format!("{}\n{}\n", command_line(c), c.stderr.trim_end()))
            .collect();
        let lines = log.iter().enumerate().map(|(ix, c)| {
            let output = match c.code {
                Some(0) => tokens.console_text,
                _ => tokens.console_error,
            };
            v_flex()
                .child(
                    div()
                        .text_color(tokens.console_system)
                        .child(command_line(c)),
                )
                .when(!c.stderr.is_empty(), |d| {
                    // shortcut: long outputs are cut here; Copy All has them whole.
                    let text: String = c.stderr.chars().take(2000).collect();
                    d.child(div().text_color(output).child(linkified(
                        ("console-output", ix),
                        text,
                        tokens,
                    )))
                })
        });
        let toolbar = spork_workbench::shell::side_toolbar()
            .child(
                spork_workbench::ActionButton::new("console-wrap", "general/softWrap")
                    .tooltip("Soft-Wrap")
                    .selected(soft_wrap)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.console_soft_wrap = !this.console_soft_wrap;
                        cx.notify();
                    })),
            )
            .child(
                spork_workbench::ActionButton::new("console-end", "general/scrollDown")
                    .tooltip("Scroll to the End")
                    .selected(self.console_follow)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.console_follow = !this.console_follow;
                        if this.console_follow {
                            this.console_scroll.scroll_to_bottom();
                        }
                        cx.notify();
                    })),
            )
            .child(
                spork_workbench::ActionButton::new("console-clear", "general/delete")
                    .tooltip("Clear All")
                    .disabled(empty)
                    .on_click(cx.listener(|_, _, _, cx| {
                        spork_git::clear_activity();
                        cx.notify();
                    })),
            );
        let view = cx.entity();
        let console = div()
            .id("git-console")
            .size_full()
            .min_w_0()
            .px_2()
            .pb_1()
            .font_family(mono)
            .text_ui()
            .track_scroll(&self.console_scroll)
            .map(|d| match soft_wrap {
                true => d.overflow_y_scroll(),
                false => d.overflow_scroll().whitespace_nowrap(),
            })
            .children(lines)
            .vertical_scrollbar(&self.console_scroll)
            .context_menu(move |menu, _, _| {
                let (text, view) = (text.clone(), view.clone());
                menu.item(PopupMenuItem::new("Copy All").disabled(empty).on_click(
                    move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())),
                ))
                .item(
                    PopupMenuItem::new("Clear All")
                        .disabled(empty)
                        .on_click(move |_, _, cx| {
                            spork_git::clear_activity();
                            view.update(cx, |_, cx| cx.notify());
                        }),
                )
            });
        h_flex().size_full().child(toolbar).child(console)
    }
}

/// A command as IntelliJ's console prints it: `14:02:07.512: [/path/to/repo] git fetch origin`.
fn command_line(command: &spork_git::CommandLog) -> String {
    let time = chrono::DateTime::<chrono::Local>::from(command.time).format("%H:%M:%S%.3f");
    format!("{time}: [{}] git {}", command.dir.display(), command.args)
}

/// Git's output with its URLs as links, as IntelliJ's console shows them.
fn linkified(
    id: impl Into<ElementId>,
    text: String,
    tokens: &spork_workbench::Tokens,
) -> AnyElement {
    let urls = urls(&text);
    if urls.is_empty() {
        return text.into_any_element();
    }
    let link = HighlightStyle {
        color: Some(tokens.text_link),
        underline: Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(tokens.text_link),
            wavy: false,
        }),
        ..Default::default()
    };
    let targets: Vec<String> = urls.iter().map(|r| text[r.clone()].to_owned()).collect();
    let styled = StyledText::new(text).with_highlights(urls.iter().map(|r| (r.clone(), link)));
    InteractiveText::new(id, styled)
        .on_click(urls, move |ix, _, cx| {
            if let Some(url) = targets.get(ix) {
                cx.open_url(url)
            }
        })
        .into_any_element()
}

/// Where `http(s)://` links are in `text`, without the quote or punctuation that ends them.
fn urls(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find("http").map(|i| from + i) {
        let rest = &text[start..];
        let len = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '\'' | '"' | '<' | '>' | ')'))
            .unwrap_or(rest.len());
        let url = rest[..len].trim_end_matches(['.', ',', ':', ';']);
        if url.starts_with("https://") || url.starts_with("http://") {
            found.push(start..start + url.len());
        }
        from = start + len.max(4);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::urls;

    #[test]
    fn finds_urls_without_their_closing_quote() {
        let text =
            "fatal: Authentication failed for 'https://github.com/a/b.git/'\nsee http://x.y.";
        let found: Vec<&str> = urls(text).into_iter().map(|r| &text[r]).collect();
        assert_eq!(found, ["https://github.com/a/b.git/", "http://x.y"]);
        assert!(urls("no links, just httpish words").is_empty());
    }
}
