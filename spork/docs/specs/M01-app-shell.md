# M01 — App shell, windows, workspaces, repository manager

Fork surfaces: WelcomeWindow, MainMenu, RepositoryManager, RepositoryManagerSidebar, QuickLaunchWindow,
OpenRepositoryAlert, RescanRepositoriesSheet, ConfigureWorkspaces, ActivityManager, NotificationBar,
NotificationView, ErrorWindow, About, LegalInfo, Feedback, Benchmark, DropdownPopover, AutoCompleteWindow.
Phase: P0 (shell, open), P4 (workspaces, quick launch, activity manager). Components: `Root`, `Dock`, `Tabs`,
`Sidebar`, `Toolbar`, `StatusBar`, `Command`, `Notification`, `Dialog`, `List`, `Tree`, `Progress`.

## 1. First run (Welcome)
| Field | Behaviour | Git |
|---|---|---|
| User name, Email | prefilled from global config; required before first commit, skippable here | `git config --global user.name|user.email` (write only if changed) |
| Default source directory | folder picker; becomes `source_dirs[0]`, triggers scan | — |
| (Spork) Import from Fork | shown only if Fork's `repositories.toml` exists | — |

## 2. Windows, tabs, workspaces
- Window = tab strip of repos. First tab of a new window = Repository Manager. `New Tab` opens Repository Manager in a tab; `Restore Closed Tab` reopens the last closed (stack of 20).
- **Workspace** = named set of windows+tabs. Switching saves the current one and restores the target. `/` in a name makes folders (`Work/Client A`). Stored in `session.json` (overview §5).
- Restore on launch: last workspace, window frames, tabs, dock layout, selected tab per repo (Changes / All Commits).
- Same repo in 2 tabs shares one `Entity<Repo>`.

## 3. Repository Manager
- Sidebar tree: source dirs → folders → repos (scan to `scan_depth`, skip `ignore`, skip `node_modules`, `.git` internals, bare repos listed separately). Scan runs in background, incremental.
- Recent list sorted by `opened`. Remove entry (not files). Filter box. Double-click opens in current tab.
- Rescan sheet: "rescan default folder" + "reset previously deleted entries" (clears the ignore list).
- Open a non-repo folder → alert "not under git" with **Initialize git repository here** → `git init` (then optional `.gitignore` template picker from github/gitignore, CC0).
- `File ▸ Create New Local Repository` → folder + name → `git init -b <default branch from init.defaultBranch or "main">`.

## 4. Quick Launch (`Cmd-P`)
`Command` palette over all known repos: fuzzy match on name then path, recent first; `Enter` open in current tab, `Cmd-Enter` new tab, `Cmd-Shift-Enter` new window. Second palette `Cmd-Shift-P` = every menu action of the current repo (all ops registered with name + keybinding).

## 5. Activity Manager (jobs)
List: job name, repo, started, duration, state (running/succeeded/failed/cancelled), User vs Background. Detail pane: full command lines + stdout/stderr (from runner). Cancel button for running jobs. Keep last 200 jobs in memory; "Copy log".

## 6. Notifications and errors
- Success toasts only for user-started network ops (`Notification`), auto-hide 4 s.
- Errors: inline banner in the repo view with friendly message from `GitError` + buttons **Show output**, contextual fixes (e.g. `IndexLocked` → "Remove index.lock"; `NonFastForward` → "Pull" / "Force push (with lease)…"; `HostKeyUnknown` → show fingerprint, "Trust" → append to `known_hosts`).
- Error window for unexpected failures: command, exit code, stderr, and (Fork has "Switch to bundled git") → **Try with bundled git** when system git is used and version-related.

## 7. Main menu (cross-platform: macOS menubar; Linux/Windows: app menu + palette)
File: New Window, New Tab, Open…, Open Recent, Clone…, Create New Local Repository, Quick Launch, Restore Closed Tab, Close.
Repository: Fetch, Quick Fetch, Pull, Quick Pull, Push, Quick Push, Save Stash, Quick Stash, Checkout Branch, New Branch, New Tag, New Worktree, Apply Patch, Bisect, Git Flow ▸, Git LFS ▸, Show in File Manager, Open in Terminal, Repository Treemap, Repository Statistics, Performance Benchmark, Settings for This Repository.
View: Show Uncommitted Changes, Show All Commits, Show HEAD, Hide Tags, Hide Stashes in Commit List, Show Lost Commits (Reflog), Collapse All Merges (First Parent), Filter Current Branch, Commit Details ▸ (Summary/Changes/File Tree/Hide), Refresh, Show Toolbar, Full Screen.
Help: Website, Issue Tracker, Report a Bug (opens GitHub issue with version/OS prefilled), Check for Updates, About, Licenses.
"Quick X" = run with remembered options, no dialog.

## 8. Benchmark (dev tool, keep)
Times: read branches+tags, read changed files, read N commits; "Copy report" (markdown with OS, git version, repo size). Used in bug reports about speed.

## 9. Deep links and CLI
- `spork://open?path=…`, `spork://clone?url=…`, `x-github-client://openRepo/<https-url>[?branch=…]` → clone dialog prefilled or open existing clone (match by remote URL).
- `spork [path]` → if running, IPC "open path" and focus; else launch. `spork --help`, `spork status`-like subcommands are out of scope.
- CLI install: macOS symlink into `/usr/local/bin` (ask for admin), Windows add to PATH via installer, Linux package puts it in `/usr/bin`.

## Edge cases
Repo moved/deleted while open → tab shows "Repository not found" with Locate…/Remove. Two Spork instances → second forwards args and exits. Network drive repos: watcher falls back to polling (5 s). Window off-screen after monitor change → clamp.

## Tests
Session round-trip (save → restore identical); scan of fixture tree with nested repos, submodules, ignored dirs; single-instance forwarding; deep-link parsing table.

## Open questions (analyst: trace/REA)
1. Exact scan rules: does Fork list submodules/worktrees as separate repos in the manager?
2. What triggers a Repository Manager rescan automatically (on launch? interval?).
3. Default keyboard shortcuts for every menu item (screenshot the menus; we decide our own defaults but want muscle-memory compatibility where sensible).
