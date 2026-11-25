# M09 — Fetch, pull, push, remotes

Fork surfaces: FetchWindow, PullWindow, PushWindow, PushMultipleBranchesWindow, PushTag, PushMultipleTags,
EditRemote, AddCustomRefspec, Quick Fetch/Pull/Push. Phase: P2. Components: `Dialog`, `Select`, `Combobox`, `Checkbox`, `DataTable`, `Progress`, `Notification`.

All network ops: `Kind::Network` in the runner → `--progress`, askpass env, job with cancel, account credentials (M10).

## 1. Fetch
Remote (select) · Fetch all remotes (remembered `fetch_all_remotes`) · (pref) always include tags · (Spork) prune.
Git: `git fetch --progress [--all | <remote>] [--tags] [--prune]`. Also `--recurse-submodules=on-demand` if submodules exist.
**Auto-fetch** (pref `fetch.auto`, interval): background job per open repo, silent; pauses when the window is hidden > 1 h or on metered networks (best effort) and when another op runs. Failures shown as a small status-bar warning, not a dialog.
Quick Fetch = remembered options, no dialog.

## 2. Pull
Remote · Branch (default upstream) · Into (current, read-only) · Rebase instead of merge (remembered; default from `pull.rebase`) · Stash and reapply (remembered).
Git: `git pull --progress [--rebase | --no-rebase] [--autostash] <remote> <branch>`.
If no upstream: pick remote branch, offer "Set as upstream" (`git branch -u`). Conflicts → M11 banner. `Fast-forward only` when `pull.ff=only` config: show the error with "Merge" / "Rebase" choices.

## 3. Push
Branch (local, default current) · Remote · Remote branch (default upstream name or same name; "custom…" opens AddCustomRefspec: free-text destination) · Create tracking reference (`-u`, default on when no upstream) · Push all tags (remembered) · Force push.
Git: `git push --progress [-u] [--tags] [--force-with-lease=<ref>:<expected-oid>] <remote> <local>:refs/heads/<dest>`.
**Force push** uses `--force-with-lease` with the oid we last saw for the remote ref; plain `--force` only via a second "Force anyway" after a lease failure. Confirm dialog shows how many remote commits will be overwritten (`git rev-list --count <local>..<remote>/<b>`).
Rejected non-fast-forward → banner "Remote has N new commits · Pull · Force push…".
Pushing to a protected branch error from host → show host message verbatim.
Pref `push_after_commit` → push with remembered options after a successful commit.

## 4. Push multiple branches
`DataTable`: branch · remote · upstream name (editable) · checkbox. Git: one `git push` per remote with multiple refspecs.

## 5. Push tag(s)
Tag · Remote → `git push <remote> refs/tags/<t>`; multiple: table of tags (local-only tags pre-checked: `git ls-remote --tags <remote>` diff) → one push with N refspecs.

## 6. Remotes (add / edit / remove)
Name · URL · Account · Test Connection (`git ls-remote --heads <url>`, 15 s) · Sync submodule URL with parent (`git submodule sync --recursive`).
Git: `git remote add <n> <url>` / `git remote set-url <n> <url>` / `git remote rename` / `git remote remove`; push URL separate field (`--push`). After add → fetch it.

## Edge cases
Remote with multiple URLs, `pushurl` differs, remote HEAD missing, push of detached HEAD (ask for destination branch), push to a branch checked out on a non-bare remote, very slow networks (cancel must kill ssh child too), proxies (`http.proxy`), self-signed certs (`http.sslVerify` error → explain, never auto-disable).

## Tests
Local bare remote fixtures: ff push, rejected push, lease failure, push new branch with -u, pull rebase with autostash conflict, fetch prune; cancel mid-fetch leaves no lock files.

## Open questions
1. Does Fork use `--force` or `--force-with-lease` (trace)? 2. Auto-fetch interval default and behaviour when offline. 3. Whether Fork fetches submodules on fetch.
