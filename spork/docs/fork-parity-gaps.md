# Fork parity: what is left to build

Compared on 2026-11-14 (ticked items done the same day) against Fork's own release notes (macOS 1.0.1–2.71, Windows 1.14–2.24), its
home page and its blog. Each line is a Fork feature that Spork lacks, or has only in part (marked
"partial"). Features Spork already has are not listed. Spork keeps its own look: these are
capabilities to match, not screens to copy.

Priority A: used every day; a Fork user would notice soon. B: useful, less frequent.
C: niche, platform-specific, or deliberately different.

## A. Daily use

### Commit list and search
- [x] Search commits by changed file path, and by diff content (`git log -S` / `-G`), listing the matching files; remember recent searches
- [x] Show only chosen branches (several at once, kept between launches); filter by a remote or a branch folder
- [x] Hide all remote branches (View menu), remembered per repository
- [x] Collapse / expand a merged branch in the graph (click the merge, ← / →, Collapse All / Expand All), remembered
- [x] Stashes drawn in the commit graph, with a toggle to hide them
- [ ] "To push" / "to pull" markers on the commits themselves (Spork shows counts only in the sidebar)
- [ ] ⌘0: select HEAD and reveal it in the sidebar
- [x] Space: quick-look popup of the selected commit or file diff
- [ ] Compare two branches (⌘-click two in the sidebar); compare local changes, or a stash, with a commit

### Quick actions and keys
- [x] ⌘B quick checkout: a searchable branch picker
- [x] Quick Launch also finds branches (checkout), files (file history), create worktree, create pull request (partial: Spork's palette has menu commands and repositories)
- [x] Hold ⌥ to fetch / pull / push without the dialog (Pull; Fetch and Push have none)
- [x] Delete key deletes the selected branch; F2 renames it
- [ ] Tab / Shift-Tab moves focus between sidebar, commit list and details

### History editing
- [x] Squash, fixup or drop the selected commits straight from the commit menu, without opening interactive rebase
- [x] Reorder or squash commits of the current branch by drag and drop in the commit list
- [ ] Interactive rebase from the root commit; p / r / s / f / d / e keys in the rebase list
- [x] "Rebase current branch onto this commit" in the commit menu; merge or rebase onto a tag (tag menu)
- [ ] `--update-refs` option for rebase and interactive rebase
- [x] Merge, rebase, cherry-pick and revert dialogs say beforehand whether it will conflict (`git merge-tree`) (partial: merge and rebase only)
- [ ] Cherry-pick / revert a merge commit choosing the parent (partial: always parent 1)

### Diffs
- [x] Find text inside the diff (⌘F in the diff), kept when switching files
- [x] Word wrap, show whitespace characters, tab width
- [ ] Context-lines dropdown (partial: Spork expands collapsed lines, or shows the entire file)
- [x] Change map / minimap beside the diff scrollbar
- [x] Open the file in the editor at the diff line
- [ ] "Copy as patch" from the diff's context menu

### Local Changes and committing
- [x] Drag files between Unstaged and Staged; double-click to stage / unstage
- [x] Spell check in the commit message (with an off switch) (partial: hints under the message, system word list; none on Windows)
- [x] Co-authored-by autocomplete from the repository's authors
- [ ] Subject length limit as a preference (partial: fixed 50 / 72 colors); ruler and "wrap paragraph at ruler" in the description
- [ ] Preference: push automatically after every commit (partial: "Commit & Push" button)
- [ ] Warning before committing on a detached HEAD
- [x] Show ignored files; hide untracked files (remembered, shown in the list header)
- [x] Ignore with a custom pattern and a preview; .gitignore templates; ignoring a tracked file also untracks it (partial: custom pattern only)
- [ ] Save the selected changes as a patch
- [ ] Hook output shown live while a commit runs (partial: shown when the hook refuses)

### Fetch, pull, push
- [ ] Fetch dialog: one remote or all, prune, fetch all tags (partial: Fetch always does all remotes with prune)
- [ ] Push several selected branches at once (`--atomic`)
- [ ] Create Tag with a "Push to remote" checkbox; push or delete several tags at once
- [ ] Warning in Push when submodule commits aren't pushed
- [ ] Fast-forward several branches at once (partial: one at a time)

### Accounts and hosting
- [x] GitHub sign-in in the browser (OAuth or device flow) instead of pasting a token (needs a GitHub OAuth app's client ID)
- [x] Several accounts on one service
- [x] Self-hosted servers on any domain: let the user say which service a host runs (partial: only hosts named `github.*`, `gitlab.*`, `gitea.*`, `forgejo.*`, codeberg.org)
- [x] Bitbucket Server / Data Center
- [x] Azure DevOps (pull requests, clone list)
- [x] Pull request templates filled in when creating a pull request
- [x] Pull request review state (approved, changes requested)
- [ ] Issues and Pull Requests tabs with filters by author / assignee
- [ ] Clone dialog: choose SSH or HTTPS URL for account repositories

### Tabs and repositories
- [x] Tab context menu: rename, close others / all, open in terminal / file manager, copy path
- [x] Badge on a tab when its repository has uncommitted changes
- [ ] Ctrl-Tab exists; also ⌘⇧[ / ⌘⇧] to switch tabs

## B. Less frequent

### Repository Manager and workspaces
- [ ] Repository Manager: folders / categories, rename repositories, color tags, multi-select remove, README preview, remote details (partial: recent repositories and source folders)
- [ ] Workspaces: named groups of tabs, per window, switchable from Quick Launch
- [ ] Vertical tabs
- [ ] Drop a repository folder on the window or Dock icon to open it
- [ ] Copy repository path (sidebar repository menu)

### Statistics
- [ ] Commits by weekday and by hour; pick a time range (partial: per month and per author, all time)
- [ ] Activity treemap; repository benchmark

### File history and blame
- [ ] History of a folder
- [ ] Compare two commits in file history; external diff for that pair
- [ ] History of a selected range of lines (`git log -L`), from the diff and File Tree menus
- [ ] Blame timeline, inverted blame, back / forward, go to line (partial: "Blame the previous revision")
- [ ] File history and blame in their own windows

### Commit details
- [ ] Collapse the commit details panel (⌘D)
- [ ] Open a commit in its own window
- [ ] Expand one file's diff inline in the summary
- [ ] Show file mode / permission changes, renamed files' old and new names, binary size difference
- [ ] More image types in image diffs (HEIC, PDF, SVG, TGA, WebP), exact pixel-difference highlight, checkerboard background

### Branches, tags, remotes
- [ ] Sidebar sort: alphabetical, recently used, folders first
- [ ] "Select stale local branches" (upstream gone); upstream-status icons (local only / gone / invalid)
- [ ] Multi-select branches or tags in the sidebar to delete several (partial: Delete Merged Branches)
- [ ] Move a branch into a branch folder; branch-name autocomplete; spaces become hyphens
- [ ] When deleting a branch, also remove its worktree
- [ ] Switch a remote between HTTPS and SSH in Edit Remote

### Stashes and patches
- [ ] Delete several stashes; save a stash as a patch
- [ ] Stash only the selected files (partial: one file at a time)
- [ ] Apply a patch from the clipboard (⌘V) or by drag and drop; option to apply without creating commits
- [ ] Several commits into one patch file (partial: one numbered file per commit)

### Merge conflicts
- [ ] `--allow-unrelated-histories` option
- [ ] Three-column resolver layout; branch names instead of current / incoming
- [ ] Resolve images, LFS files and submodules in the resolver
- [ ] Undo a wrong resolution of one file

### Submodules, LFS, git-flow
- [ ] Move a submodule; update several selected submodules
- [ ] LFS prune; LFS locks dialog with filter and multi-unlock; LFS progress
- [ ] git-flow: custom prefixes at initialization, de-initialize, option not to merge a release back into develop, support branches

### Preferences and tools
- [ ] Choose the git executable; verbose git output
- [ ] Customizable toolbar
- [x] UI zoom (⌘= / ⌘- / ⌘0, View menu)
- [ ] Commit list layout options (compact branch labels, highlight one author)
- [ ] .mailmap applied in the commit list (partial: statistics only)
- [ ] SSH configuration dialog; choose among several SSH keys

### Custom commands
- [x] Dialog controls: text field, branch picker, file picker, checkbox
- [x] URL commands (open a web page built from variables)
- [x] `${repo:name}`-style variables, nested menus with "/", per-OS commands, reorder (groups show as headings, not submenus)
- [x] Custom commands in file history

### CLI
- [ ] `spork log -- <file>` opens file history

## C. Niche or deliberately different
- [ ] Gravatar / GitHub avatars (Spork uses initials; photos send email hashes to a third party, so opt-in only)
- [ ] Auto-update with stable / develop channels (needs signing certificates)
- [ ] Menu-bar notifications on macOS (Spork has a Notifications window)
- [ ] `github-mac://` / `sourcetree://` "Open in Desktop" URL handlers
- [ ] Little Snitch rules file
- Windows-only details (middle-click close, accent colors, Windows 11 styling) apply once Spork ships on Windows
- AI: Fork drives Claude Code / Codex / Cursor; Spork calls Anthropic or a local model directly and already covers commit messages and reviews, so no gap
