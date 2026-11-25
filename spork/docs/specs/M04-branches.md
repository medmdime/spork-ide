# M04 — Branch operations

Fork surfaces: CreateBranchWindow, CheckoutBranch, TrackRemoteBranch, RenameLocalBranchWindow, RemoveLocalBranch,
RemoveRemoteBranch, ResetBranchWindow, CheckoutAndSync. Phase: P2. Components: `Dialog`, `Form`, `Input`, `Select`, `Radio`, `Checkbox`.

## 0. Shared "Local changes" control (appears in 5 dialogs)
Shown only when the worktree is dirty **and** the target differs from HEAD.
| Choice | Behaviour |
|---|---|
| Stop if some local changes would be overwritten (default) | plain switch; on `LocalChangesWouldBeOverwritten{files}` show the files |
| Stash and reapply | `git stash push --include-untracked -m "Spork: auto-stash before checkout <target>"` → switch → `git stash pop`; if pop conflicts: leave stash, show conflict banner (M11) + "the stash was kept" |
| Discard local changes (--force) | confirm dialog listing counts → `git switch --discard-changes` (untracked files are kept; say so) |
Remembered as `checkout_local_changes`.

Branch-name input (all dialogs): live validation with `git check-ref-format --branch <name>` (debounced, or a Rust port of the rules), spaces replaced by the pref `branch_name_space_char`, `/` allowed (folders), duplicate check against refs.

## 1. Create branch
Fields: name · create at (ref/commit, prefilled from selection) · Check out after create (remembered) · local changes.
Git: without checkout `git branch <name> <start>`; with checkout `git switch -c <name> <start>`.
If start is a remote branch: ask "Track origin/x?" → `--track`.

## 2. Checkout branch
"Switch to" (local branch) · "Fast-forward to" (shown when the branch is behind its upstream and can ff: checkbox) · local changes.
Git: `git switch <b>`; then if ff chosen `git merge --ff-only @{u}`. If the branch is checked out in another worktree → disabled with "Open worktree" button.
After checkout: if pref `update_submodules` → `git submodule update --init --recursive`.

## 3. Track remote branch
Remote branch (fixed) · Local branch name (default: remote name without `<remote>/`) · local changes.
Git: `git switch -c <local> --track <remote>/<branch>`. If a local branch with that name exists and tracks it → just checkout.

## 4. Rename
New name · "Also rename remote branch" (only if upstream exists).
Git: `git branch -m <old> <new>`; remote: `git push <remote> <new>:refs/heads/<new>` → `git push <remote> --delete <old>` → `git branch -u <remote>/<new> <new>`. Do remote steps only after the local rename succeeds; on remote failure report partial state.

## 5. Delete local branch(es) (multi-select)
Table of branches with upstream · "Also delete corresponding remote branch" · "Also delete corresponding worktree".
Git: `git branch -d <b…>`; if "not fully merged" → second confirm → `-D`. Remote: `git push <remote> --delete <b>` per remote. Worktree: `git worktree remove <path>` before deleting the branch. Never delete the current branch (disable).

## 6. Delete remote branch
`git push <remote> --delete <branch>`; confirm shows the remote URL. Then prune the tracking ref locally.

## 7. Reset branch
Branch · Move to (commit) · type:
| Type | Copy for UI (ours) | Git |
|---|---|---|
| Soft | keep changes, staged | `git reset --soft <c>` |
| Mixed (default) | keep changes, unstaged | `git reset --mixed <c>` |
| Hard | discard everything (red) | `git reset --hard <c>` — first offer "Save snapshot" (M12) |
If the branch isn't HEAD: only "move" is possible → `git branch -f <b> <c>` (types hidden). Hard reset: show the count of files that will be lost.

## 8. Checkout & sync (from deep links / PR checkout / "remote has new commits")
Branch · Action: Checkout / Rebase / Merge / Reset (to remote) · Stash and reapply.
Git: Checkout = §2; Rebase = `git switch b && git rebase --autostash @{u}`; Merge = `git merge @{u}`; Reset = `git reset --hard @{u}` (confirm).

## Edge cases
Ambiguous names (branch `foo` + tag `foo`) → always pass full refs `refs/heads/foo` to plumbing and `--` separators. Case-only renames on case-insensitive FS (macOS/Windows): rename via temp name. Branch checked out in another worktree. Unborn HEAD.

## Tests
One fixture per dialog option; assert resulting refs + worktree; stash-and-reapply conflict path leaves stash in list; case-only rename on macOS/Windows CI.

## Open questions
1. Does Fork use `checkout` or `switch`, and `-u` or `--include-untracked` for auto-stash (trace)? 2. Exact rename-remote sequence and order. 3. How "Fast-Forward to" is computed and run.
