# M11 — Merge, rebase, interactive rebase, conflict resolution

Fork surfaces: MergeWindow, RebaseController, RebaseInteractiveController, MergeConflictViewController, SideBySideMergeController,
MergerContentController, EditCustomMergeTool, helper `fork_ri`. Phase: P3.
Components: `Dialog`, `Select`, `Radio`, `List` (drag reorder), `Editor` ×3, `Resizable`, `StatusBar`, `Notification`.

## 0. Operation state machine (in `Entity<Repo>`)
Detect from `.git`: `MERGE_HEAD` → Merging; `rebase-merge/` → Rebasing{interactive, step=`msgnum`, total=`end`}; `rebase-apply/` → Rebasing/Am;
`CHERRY_PICK_HEAD` → CherryPicking; `REVERT_HEAD` → Reverting; `BISECT_LOG` → Bisecting.
Banner + status bar show state, conflicts count, and buttons: **Continue** (enabled when no unmerged paths) · **Skip** (rebase/cherry-pick/am) · **Abort**.
Git: `git <merge|rebase|cherry-pick|revert|am> --continue|--skip|--abort`; continue uses `GIT_EDITOR=spork-seq-editor --editor` so the message is taken from the in-app editor.

## 1. Merge
Merge (source ref) · Into (current) · Type (remembered `merge_type`):
| Type | Git |
|---|---|
| Default | `git merge <src>` (respects `merge.ff`) |
| Fast-forward if possible | `git merge --ff <src>` |
| No fast-forward | `git merge --no-ff <src>` |
| Squash | `git merge --squash <src>` (then commit box prefilled with squash message) |
| Merge without commit | `git merge --no-commit --no-ff <src>` |
Message editable before merge (Spork): `-m`.

## 2. Rebase
Rebase current onto (branch/commit) · Stash and reapply (`--autostash`) · (Spork) Rebase merges (`--rebase-merges`).
Git: `git rebase [--autostash] <onto>`. Warn when the current branch is already pushed (rewrites published history).

## 3. Interactive rebase
Opened from a commit ("Interactive rebase from here" → base = its parent) or branch.
List of commits base..HEAD oldest first; per row action **Pick · Edit · Reword · Squash · Fixup · Drop**, drag or Move up/down to reorder,
reword/squash messages editable inline. Options: **Backup current state with temporary branch** (`git branch spork-backup/<branch>-<yyyymmdd-hhmmss> HEAD` first), **Update dependent branches** (`--update-refs`).
Validation: first row can't be squash/fixup; warn when dropping commits contained in other branches.
Execution:
1. Write the todo to a temp file; run `git rebase -i [--update-refs] [--autostash] <base>` with `GIT_SEQUENCE_EDITOR="spork-seq-editor --todo <tmpfile>"` (copies our todo over git's).
2. Reword/squash messages: `GIT_EDITOR="spork-seq-editor --messages <tmpdir>"` — picks the prepared message for the commit being edited (matched by original oid via `rebase-merge/` state); if none prepared → asks the app over IPC (in-app editor).
3. `edit` stops → banner "Stopped at <c> for editing · Amend in commit box · Continue".
4. Conflicts → §4, then Continue.
Undo: banner after success "Rebase finished · Undo" → `git reset --keep ORIG_HEAD` (or the backup branch).

## 4. Conflict resolution
Conflicted files listed with their type and the actions allowed:
| Type (porcelain v2 `u` XY) | Actions |
|---|---|
| UU both modified | Use local (`git checkout --ours -- f`) · Use remote (`--theirs`) · Merge in Spork · Merge in external tool |
| AA both added | same as UU |
| DU deleted by them / UD deleted by us | Keep file (`git add f`) · Delete (`git rm f`) |
| DD, AU, UA | Keep/remove choices |
| Submodule conflict | pick local/remote commit (`git update-index --cacheinfo`) |
| Binary | Use local / Use remote only |
After choosing local/remote → `git add -- f` (stage). Mark resolved = stage. Labels in UI: during rebase "ours" = upstream being rebased onto, so show **branch names**, not "ours/theirs".

**Spork 3-way merger** (MergerContent / SideBySideMerge equivalent):
- Load stages: base `git show :1:f`, local `:2:f`, remote `:3:f` (missing stage = empty).
- `spork_patch::merge3` (imara-diff on base→local and base→remote) → chunks: unchanged / local-only / remote-only / both-same / **conflict**.
- Layout: Local (left) · Remote (right) · Result (bottom, editable `Editor`). Each conflict: buttons Take local · Take remote · Take both (local first) · Take both (remote first) · Edit. Non-conflicting changes auto-applied. Next/prev conflict navigation. Counter "2 conflicts left".
- Save → write result bytes (preserve EOL style + encoding of local) → `git add -- f`. "Resolved" state shown when no conflict chunks remain; refuse to stage while `<<<<<<<` markers remain unless confirmed.
- (AI) "Suggest resolution" per conflict → 10-ai.md.

## 5. External merge/diff tools
Preset list per OS (detect installed): VS Code, Meld, KDiff3, Beyond Compare, P4Merge, Kaleidoscope (mac), WinMerge (win), Araxis, IntelliJ, Sublime Merge, Zed.
Custom tool editor: name · executable path · arguments with variables `$LOCAL $REMOTE $BASE $MERGED` (merge) / `$LOCAL $REMOTE` (diff).
Run: write stages to temp files, launch, wait; on exit 0 and `$MERGED` changed → ask "Mark as resolved?". (Or `git mergetool --tool=<t> -- f` when the user has git-configured tools.)

## Edge cases
Rebase over merges, rebase with empty commits after conflict (`--empty=drop` prompt), autostash pop conflict at the end, rerere enabled (`rerere.enabled`: show "resolved by rerere" note), conflicts in renamed files, CRLF conflicts, huge conflicted files (> 5 MB → external tool only).

## Tests
Fixtures for every conflict type; interactive rebase scripts (reorder, squash, fixup, drop, edit, reword) compared to expected `log --format=%s`; abort restores exact pre-state (refs + worktree hash); merge3 property tests (both-same changes never conflict; result with all "take local" == local file).

## Open questions (REA candidates)
1. How `fork_ri` receives the todo and messages (transport + format, prose only). 2. Fork's backup branch naming. 3. How Fork labels local/remote during rebase. 4. Fork's built-in merger chunk actions (record UX).
