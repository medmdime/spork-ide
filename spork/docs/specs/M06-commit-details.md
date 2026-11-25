# M06 — Commit details & commit actions

Fork surfaces: RevisionDetailsWindow, RevisionSummaryTab, RevisionChangesTab, RevisionFileTreeTab, RevisionDetailsTooltip,
CheckoutRevision, CherryPick, Revert, RewordPopover, SaveAsPatch, ApplyPatch, SaveSnapshot (→ M12).
Phase: P1 (read), P3 (actions). Components: `Tabs`, `DescriptionList`, `Tree`, `List`, `Diff`, `Popover`, `Dialog`, `TextView`.

## 1. Tabs (remembered: `details_tab`)
**Summary**: SHA (copy), parents (clickable), author + committer (if different) with avatars, dates, refs containing it
(lazy: `git branch -a --contains <c>` / `git tag --contains`, behind a "Show" link because it's slow), full message rendered
with bug-tracker links (M16) and URLs; changed files list with +/− counts (`git show --numstat -z --format= <c>`); "Expand all" shows all diffs inline.
**Changes**: file list (tree/flat, filter box) + diff pane (M08). Merge commits: selector "vs parent 1 / vs parent 2 / combined (`--cc`)".
**File Tree**: full tree at commit (gix), file preview (M08 viewer), actions: Save as…, Open (temp copy), Blame, File history, Copy path.
"Hide commit details" collapses the pane. Quick Look (`Space`) previews a file.

Multi-select 2 commits → Changes tab shows the range diff `git diff <a> <b>`.

## 2. Actions (context menu on a commit)
| Action | Dialog fields | Git |
|---|---|---|
| Checkout commit | commit, local-changes control (M04 §0) | `git switch --detach <c>`; explain detached HEAD in the banner with "Create branch here" |
| Create branch here | M04 §1 | |
| Create tag here | M12 | |
| Cherry-pick | commit · Commit the changes (default on) · Append origin (`-x`) · Parent to skip (merge commits only: 1/2) | `git cherry-pick [-x] [--no-commit] [-m N] <c…>` (multi-select keeps graph order, oldest first) |
| Revert | commit · Commit the changes · Parent (merges) | `git revert [--no-commit] [-m N] <c>` |
| Reset current branch to here | M04 §7 | |
| Reword | popover: subject + description, prefilled | HEAD: `git commit --amend --only -F -` (stdin). Older: interactive rebase with only `reword <c>` via `spork-seq-editor` (M11 §3), message supplied in advance; blocked if a merge commit lies between (offer `--rebase-merges`) |
| Edit / squash / fixup / drop from here | → M11 interactive rebase with base = parent of selection | |
| Save as patch | revision(s), folder | `git format-patch -o <dir> <c>^..<c>` (multi-select → series); single file option: `git format-patch --stdout <range> > file` |
| Copy SHA / subject / message / link | — | link via M15 host patterns |
| Open on host / Create PR from here | — | M15 |
| Custom commands (Commit target) | — | M16 |

## 3. Apply patch (Repository ▸ Apply Patch)
File · "Create commit" toggle: off = `git apply --3way <file>` (changes land in worktree/index), on = `git am --3way <file>` (mbox from format-patch). On `am` conflict → op-state banner with Continue (`git am --continue`) / Skip / Abort.

## 4. Commit tooltip / detail window
Hovering a row for 600 ms shows the summary card; double-click opens details in a separate window (multi-window via `Dock`).

## Edge cases
Root commit (diff against empty tree `4b825dc…`), merge commits (combined diff), commits touching 50k files (virtualize list, load numstat lazily), binary-only commits, submodule pointer changes (M08 submodule view), cherry-pick resulting in empty commit (`--allow-empty` prompt or skip).

## Tests
Fixtures: merge commit cherry-pick with -m 1; revert of revert; reword HEAD and HEAD~3; format-patch → am round trip yields same tree.

## Open questions
1. Fork's reword implementation for non-HEAD commits (trace: does it use `rebase -i` with `fork_ri`?). 2. Default parent for merge cherry-pick. 3. What "Commit the changes" off does for multiple commits.
