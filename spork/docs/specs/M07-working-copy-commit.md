# M07 — Working copy, staging, commit

Fork surfaces: StageFileViewController, FileListView, CommitController, CommitPreferences, DiffSelectionPopover,
AddGitIgnorePattern, AutoCompleteWindow. Phase: P2. **Highest-risk module for data loss; strictest tests.**
Components: `List`/`Tree`, `Checkbox`, `Input` (filter), `Diff`, `Textarea`, `Popover`, `Resizable`, `Badge`, `Menu`.

## 1. Layout ("Changes" tab)
Left: **Unstaged** list (top) and **Staged** list (bottom), each with filter, Stage/Unstage all buttons, count; tree or flat toggle.
Right: diff of the selected file (unstaged: index→worktree; staged: HEAD→index). Bottom-left: commit box.
Banner on top when an op is in progress (merge/rebase/cherry-pick/revert/am/bisect) with Continue/Abort/Skip.

## 2. Status
gix status (CLI fallback `git status --porcelain=v2 -z --branch --untracked-files=all --find-renames`).
Per entry: path, orig path (rename/copy), index status, worktree status, submodule flags, conflict stage (U*), LFS pointer flag.
Icons: Modified, Added, Deleted, Renamed, Copied, Type-changed, Untracked, Conflicted, Submodule-changed.
Large repos: respect `core.fsmonitor` / `core.untrackedCache`; offer to enable them when status > 1 s.

## 3. Whole-file actions
| Action | Git |
|---|---|
| Stage file(s) | `git add -- <paths>` (deleted: `git add -A -- <path>`), via stdin `--pathspec-from-file=- --pathspec-file-nul` for many files |
| Unstage | `git restore --staged -- <paths>`; before first commit: `git rm --cached -- <paths>` |
| Discard changes (unstaged, tracked) | confirm → `git restore --worktree -- <paths>` |
| Discard (untracked) | confirm → move to OS trash (`trash` crate), never `rm` |
| Discard all | confirm listing counts → `git restore --worktree .` + trash untracked; offer "Save snapshot first" |
| Resolve conflict | → M11 |
| Ignore… | §6 |
| Open / Reveal / Open in external editor / Copy path / File history / Blame | M08 |
| Custom commands (File target) | M16 |
Space toggles stage/unstage of the selection; Enter moves focus to diff; keyboard stage moves selection to the next file.

## 4. Hunk & line staging (DiffSelectionPopover: Stage / Unstage / Discard)
User selects lines (click, shift-click, drag) or a whole hunk (hunk header button). `spork_patch` builds a patch and applies it:
- **Stage selection** (unstaged diff, index→worktree): for each hunk keep header context; selected `+` lines stay `+`; unselected `+` lines are **dropped**; selected `-` stay `-`; unselected `-` become **context** (` `). Recompute `@@ -a,b +c,d @@` counts; old-side start = original; new-side start adjusted cumulatively by preceding hunks' net change. Apply via stdin: `git apply --cached --recount -` (we always keep context lines, so `--unidiff-zero` is never needed).
- **Unstage selection** (staged diff, HEAD→index): same construction on the staged diff, then `git apply --cached --reverse`.
- **Discard selection** (unstaged): same construction, `git apply --reverse` (worktree), after confirm.
- Line-ending preservation: build the patch from raw bytes; keep `\ No newline at end of file` markers attached correctly; never re-encode.
- Binary files, submodules, LFS pointers, renamed-and-modified files: whole-file only (disable line selection with tooltip why).
Property test (overview §6): random content + random selection → patch applies, and index content equals "old with exactly the selected changes".

## 5. Commit box
| Element | Behaviour |
|---|---|
| Subject input | counter; warn colour above `subject_low` (50), error colour above `subject_high` (72); page guide at `page_guide` in description |
| Description textarea | spellcheck (pref; platform spellchecker where available, else `spellbook`/hunspell dictionaries — P5) |
| Recent messages | dropdown of last 20 messages typed in this repo + `git log -n 20 --format=%B` |
| Autocomplete | `@`/`:` not needed; Tab-complete changed file names and branch names (AutoCompleteWindow) |
| Amend | toggle: loads HEAD message; commit runs `--amend`; warns if HEAD is already pushed (exists on upstream) |
| Commit template | prefilled from `commit.template` when empty |
| Sign-off / GPG | from repo settings (M02) |
| AI ✦ | generate message from staged diff (10-ai.md) |
| Commit button | label "Commit N files"; `Cmd-Enter`; disabled if nothing staged (unless amend or `--allow-empty` from menu) |
| Commit & Push | split button; also pref `push_after_commit` |
Git: `git commit [--amend] [--signoff] [-S] -F -` (message via stdin, `--cleanup=strip`). Hooks run normally; on `HookFailed` show hook output with "Commit without hooks (`--no-verify`)" button. If `user.name/email` missing → inline form (M01 §1).

## 6. Ignore (AddGitIgnorePattern)
Choices for the selected file: exact path · file name anywhere · `*.ext` · containing folder · custom pattern. Target file: root `.gitignore` / folder `.gitignore` / `.git/info/exclude`.
Preview list of files that will become ignored: `git ls-files --others --exclude-standard` before vs after (dry run in memory with `gix` exclude matching).
Note under preview: already tracked files stay tracked → offer "Also stop tracking" (`git rm --cached`).

## 7. Stash shortcuts from this view
"Stash selected files" (M12 partial stash), "Save snapshot" (M12).

## Edge cases
Unborn HEAD (first commit), file renamed + modified, mode-only change (`100644→100755`, respect `core.fileMode`), CRLF/`core.autocrlf` (diff with `--ignore-cr-at-eol` off by default, show EOL marker), case-only rename on case-insensitive FS, sparse checkout, `assume-unchanged`/`skip-worktree` files (show a section), 100k untracked files (cap list, warn, offer ignore), files with invalid UTF-8 names (`-z`, bytes paths).

## Tests
Matrix: each action × {normal, renamed, deleted, binary, CRLF, no-EOF-newline, unborn HEAD, submodule}; proptest for §4; hook failure path; amend of pushed commit warning.

## Open questions
1. Exact flags Fork uses for line staging (trace `git apply …`). 2. How Fork's recent messages list is sourced. 3. Whether Fork's "Discard" of untracked deletes or trashes.
