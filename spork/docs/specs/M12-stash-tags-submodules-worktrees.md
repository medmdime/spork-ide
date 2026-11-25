# M12 — Stashes, snapshots, tags, submodules, worktrees

Fork surfaces: CreateStash, CreatePartialStash, ApplyStash, RemoveStash, RenameStash, SaveSnapshot, CreateTag, RemoveTag, PushTag(→M09),
TagDetails, AddSubmodule, SubmoduleDiffView(→M08), CreateWorktree, CheckoutBranchAsWorktree, CheckoutRemoteBranchAsWorktree, RemoveWorktree.
Phase: P3 (stash, tags), P4 (submodules, worktrees). Components: `Dialog`, `Form`, `Input`, `Checkbox`, `DescriptionList`.

## 1. Stashes
| Action | Fields | Git |
|---|---|---|
| Save stash | Message (optional) · Stage new files (untracked are ignored unless staged — say so) | if "stage new files": `git add -- <untracked>` first; `git stash push [-m msg]` · (Spork) "Include untracked" alt: `-u` |
| Quick stash | — | `git stash push -m "Quick stash <date>"` |
| Stash selected files (partial) | Message | `git stash push -m msg -- <paths>` (both staged + unstaged changes of those files; say so) |
| **Save snapshot** | Message · Stage new files | `oid=$(git stash create "msg")` → `git stash store -m "msg" $oid` (worktree untouched). If create returns nothing → "nothing to snapshot" |
| Apply | Stash · Delete after applying (remembered) | `git stash apply stash@{n}` / `git stash pop stash@{n}`; on conflict git keeps the stash — tell the user |
| Drop | confirm | `git stash drop stash@{n}` |
| Rename (experimental) | new message | `oid=$(git rev-parse stash@{n})` → `git stash drop stash@{n}` → `git stash store -m "<new>" $oid` (moves it to top; warn) |
| Create branch from stash | name | `git stash branch <name> stash@{n}` |
| View | — | commit-like details; diff vs its first parent; untracked part from `stash@{n}^3` |
Always address stashes by oid internally (indices shift); convert to `stash@{n}` right before running.

## 2. Tags
Create: name · at commit · Annotated (message) · Sign (`-s` if `tag.gpgsign`) · Push to remote after create (+ remote select).
Git: `git tag <name> <c>` / `git tag -a <name> -m <msg> <c>`; push → M09 §5.
Delete: "Delete tag from your repository" `git tag -d <t>` · "Delete from remote repositories" (checkbox per remote) `git push <remote> --delete refs/tags/<t>`.
Details: tagger, date, message, target, signature (`git for-each-ref refs/tags/<t> --format=…` / `git cat-file -p`).
Checkout tag → detached (M06).

## 3. Submodules
Add: Repository URL · Submodule folder (relative path) · Fetch submodules recursively.
Git: `git submodule add <url> <path>` then `git submodule update --init --recursive -- <path>` (if recursive).
Update: `git submodule update --init [--recursive] [-- <path>]`. Sync: `git submodule sync --recursive`. Open: new tab.
Remove (Spork): `git submodule deinit -f -- <p>` → `git rm -f <p>` → delete `.git/modules/<p>` (confirm).
Auto-update on checkout (pref). Status in sidebar (M03); pointer change view (M08 §4).

## 4. Worktrees
| Action | Fields | Git |
|---|---|---|
| Create worktree | Branch name (new) · Start at · Location | `git worktree add -b <name> <path> <start>` |
| Checkout branch as worktree | Local branch · Location | `git worktree add <path> <branch>` |
| Checkout remote branch as worktree | Remote branch · Location | `git worktree add --track -b <local> <path> <remote>/<b>` |
| Remove | confirm; force if dirty (second confirm) | `git worktree remove [--force] <path>`; then `git worktree prune` |
| Open / Reveal | — | new tab / file manager |
Default location: `<repo-parent>/<repo>.worktrees/<branch>` (Spork default; editable). Locked/prunable worktrees shown with state.

## Edge cases
Stash with conflicts on apply; stash of a repo with unborn HEAD (git refuses → explain); tag that exists remotely with a different target (push rejected → offer force); submodule URL relative (`../x.git`); worktree path exists; worktree on a different drive (Windows).

## Tests
Snapshot leaves worktree byte-identical and adds one stash; rename stash preserves tree; tag create/delete local+remote against bare fixture; submodule add/remove round trip leaves clean status; worktree create/remove each variant.

## Open questions
1. Does Fork's snapshot use `stash create/store` (trace)? 2. Partial stash flags. 3. Fork's default worktree location.
