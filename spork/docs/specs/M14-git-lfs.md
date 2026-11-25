# M14 — Git LFS

Fork surfaces: GitLfsFetch, GitLfsPull, GitLfsStatus, GitLfsTrack (+ menu Repository ▸ Git LFS). Phase: P4.
Components: `Dialog`, `Select`, `DataTable`, `Input`, `List`, `Button`.

Detection: `.gitattributes` contains `filter=lfs` or `git lfs env` reports a configured endpoint. If LFS is used but `git lfs` is
missing → persistent banner with install link (bundled on Windows).

| Action | Fields | Git |
|---|---|---|
| Install hooks | — | `git lfs install --local` (offered when LFS is used but hooks are missing) |
| Fetch | Remote | `git lfs fetch <remote>` (progress via `GIT_LFS_PROGRESS=<tmpfile>`, tail the file) |
| Pull | Remote | `git lfs pull <remote>` |
| Status | — | `git lfs ls-files --long` + `git lfs status` + locks `git lfs locks --json` → `DataTable`: path, oid, downloaded?, locked by |
| Lock / Unlock | selected files · (unlock) force | `git lfs lock <path>` / `git lfs unlock [--force] <path>` |
| Track | pattern(s) (prefilled `*.<ext>` from selection) | `git lfs track "<pattern>"` → stage `.gitattributes`; preview = tracked files matching the pattern (in-memory glob over `git ls-files`) |
| Untrack | pattern | `git lfs untrack "<pattern>"` |

Integration elsewhere: LFS files marked in status/diff lists; diff shows real content when the object is local (M08 §3); clone progress shows the "Filtering content" phase; push shows LFS upload progress from the progress file.

Edge cases: locked file modified by someone else (warn before commit: `git lfs locks --verify`), missing objects on checkout (`smudge` error → offer `git lfs pull`), `GIT_LFS_SKIP_SMUDGE` set by user.

Tests: local LFS test server (`lfs-test-server` or `git lfs` against file:// with standalone transfer) in CI on Linux; track preview; lock table parsing from JSON fixtures.

Open questions: 1. Does Fork show LFS progress for push/pull (trace env). 2. When Fork prompts to install LFS hooks.
