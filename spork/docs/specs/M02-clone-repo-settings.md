# M02 — Clone, init, repository settings & details

Fork surfaces: CloneController, RepositorySettingsController, RepositoryDetailsController, OpenRepositoryAlert (init, see M01).
Phase: P4. Components: `Dialog`, `Form`, `Input`, `Select`, `Checkbox`, `Progress`, `Settings`, `DescriptionList`.

## 1. Clone
| Field | Behaviour | Git |
|---|---|---|
| Repository URL | accepts https, ssh (`git@host:o/r`), `file://`, local path; paste from clipboard auto-filled if it looks like a git URL | — |
| Account | optional; filtered to accounts whose server matches the URL host; supplies credentials via askpass | — |
| Test Connection | async, 15 s timeout; ✓ / error text | `git ls-remote --heads <url>` |
| Location (parent folder) + Name | name defaults to repo name from URL; recent locations list (`recentCloneDirectories`) | — |
| Recurse submodules | default on | `--recurse-submodules` |
| (Spork) Branch, Shallow depth | advanced disclosure | `--branch`, `--depth N`, `--filter=blob:none` |

Run: `git clone --progress [--recurse-submodules] [--branch b] <url> <dir>`, progress phases: Counting/Compressing/Receiving/Resolving deltas/Updating files/Filtering content (LFS). On success open in a new tab; on cancel delete the partially created dir (only if we created it).
Errors: dir exists and not empty → block before running; auth → askpass flow (M10); host key → M01 banner.

## 2. Repository settings (per repo; most map to `git config --local`)
| Section | Option | Storage / git |
|---|---|---|
| User information | Use global identity (toggle) / name / email | unset or set `user.name`, `user.email` locally |
| Signing | Select GPG key (list from `gpg --list-secret-keys --with-colons`), or SSH key (`gpg.format=ssh`) | `user.signingkey`, `commit.gpgsign=true`, `tag.gpgsign` |
| Commit | Commit template file | `commit.template` |
| | Add Signed-off-by line | Spork setting → `--signoff` on commit |
| Merge | No fast-forward on merge | `merge.ff=false` |
| Lean branching | Main branch | `.git/spork/ui.json` (`lean.main`), default = remote HEAD (`git symbolic-ref refs/remotes/origin/HEAD`) |
| Diff | per-repo diff font / tab width override | `.git/spork/ui.json` |
| Issue tracker | rules (M16) | `.spork/bugtracker.json` (shared) or local |
| Custom commands | (M16) | `.spork/custom-commands.json` (shared) or local |
| Remotes | list + edit (M09) | `git remote …` |
| (Spork) Hooks | list `.git/hooks` / `core.hooksPath`, enabled state | read only |

All config writes go through `git config --local`, never by editing `.git/config` directly.

## 3. Repository details panel
Name, remote URL(s), size on disk (`git count-objects -vH` + worktree size), initial commit (`git rev-list --max-parents=0 HEAD`), last commit, changed files count, links: Website / Issues / Pull Requests derived from the remote URL via M15 host patterns.

## Edge cases
Clone into a path with spaces/unicode (Windows!), clone of empty repo (no HEAD), clone with LFS but git-lfs missing (warn, offer to continue), URL with embedded credentials (strip and warn: never store in remote URL).

## Tests
Clone from local bare fixture (with submodule, with LFS stub); cancel mid-clone removes dir; settings round-trip each option through `git config --get`.

## Open questions
1. Does Fork's Test Connection use `ls-remote` (trace it)? 2. Which config keys does Fork's "Use global git credentials" toggle touch? 3. Where does Fork store per-repo settings that aren't git config (check `.git/` for Fork files after changing each option)?
