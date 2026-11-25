# Spork — open-source, cross-platform Git client (Rust + GPUI Kit)

**Spork** (neither a spoon nor a fork, just a better tool) is an open-source Git client that matches the *UX* of the best
commercial clients (reference: Fork 2.71.1), with its own look, built in Rust on
[`gpui-kit = "0.9.1"`](https://gpui-kit.com/component/) for macOS, Linux and Windows, plus optional AI features.

| Doc | What it covers |
|---|---|
| `00-overview.md` (this) | rules, architecture, data model, git runner, settings, testing, schedule |
| `specs/M01…M16-*.md` | one spec per module: every dialog, every option → exact git command, edge cases, tests, open questions |
| `10-ai.md` | AI features, providers, privacy |
| `20-open-source.md` | license, clean-room contribution policy, CI, releases |
| `../inventory/` | evidence from the installed Fork bundle (`scripts/inventory.sh`, `scripts/ui-labels.sh`) |

---

## 1. How we use REA without inheriting Fork's copyright

Fork is proprietary. Rewriting its decompiled code into Rust, even line-by-line "in our own words",
is still a derivative work. In an open-source repo it would be a public, permanent liability: one DMCA notice
and the repo is taken down, and every contributor is exposed. Behaviour, algorithms, file formats, protocols
and UX are **not** copyrightable. So REA is used on everything; only its *output* is constrained:

```
Fork.app ──► REA / GIT_TRACE / using the app ──► specs/Mxx.md (prose, tables, git commands) ──► Rust code
              analyst only                        reviewed: no code, no pseudo-code from the binary      implementer reads specs only
```

| REA output that may go into a spec | Must never go into a spec or the repo |
|---|---|
| "Lanes are assigned left-to-right; a merge's 2nd parent takes the first free lane" | decompiled functions, pseudo-code, control flow mirroring the binary |
| config file formats, env vars the helpers use, IPC message shapes | Fork's strings beyond short option names, its icons, CSS, nibs |
| which git commands and flags run for an action | Fork's class/method names as our API |
| timings, limits, thresholds, defaults | |

Practical rule: **the person or agent who looked at decompiled code doesn't write the Rust for that module.**
With AI agents this means: the analysis session has REA; the implementation session does not, and only gets `specs/`.

Also free to read directly, because they are open source: **git-flow-avh** (BSD) inside the bundle, git, git-lfs.

---

## 2. Facts established from the bundle

| Fact | Consequence for Spork |
|---|---|
| Fork drives a **bundled git CLI** (2.50.1) for everything; no libgit2 | Same model: git CLI for writes/network. Add `gix` for fast reads |
| git-lfs 3.7.0, git-flow-avh 1.11.0 bundled | We call `git lfs`; we re-implement git-flow in Rust (works on Windows without bash) |
| helpers: `fork_askpasswd`, `fork_ri`, `fork_cli`, `gpg.sh` | `spork-askpass`, `spork-seq-editor`, `spork` CLI; no GPG wrapper |
| config: `repositories.toml` {source_dirs, scan_depth, ignore, repository[{path, opened}]}, `session.json` {activeWorkspaceIndex, lastWindowFrame, workspaces[{name, activeWindowIndex, windows[]}]}, `accounts.json` [{serverUrl, serviceType, authenticationType, username, avatarUrl}] (token in keychain) | Same shapes in our own files (§5); an importer can read Fork's files for migrating users |
| 22 user-default settings keys (e.g. `checkoutStashAndReapply`, `pullSheetRebase`, `mergeType`, `maxCommitCount`, `gitInstanceType`) | Dialogs remember their last options (§5 `[remember]`) |
| URL schemes `forkapp`, `sourcetree`, `x-github-client` | We register `spork://` and `x-github-client://` (GitHub's "Open in Desktop") |
| 139 UI controllers → 16 modules | `specs/M01…M16`; deliberately not reproduced: `LicensingController`, `PleaseBuyController` (paid licensing; Spork is free) |

---

## 3. Architecture

### 3.1 Crates

```
spork/
├─ crates/
│  ├─ spork_git/        runner (CLI), gix reader, parsers, error classifier, repo-state detection
│  ├─ spork_graph/      commit-graph lane layout (pure; no I/O)
│  ├─ spork_patch/      diff model, hunk/line patch builder, 3-way merge model (pure)
│  ├─ spork-ops/        one async fn per user action → OpOutcome; git-flow; custom commands engine
│  ├─ spork-auth/       IPC server for helpers, keyring, ssh keys, accounts
│  ├─ spork_hosting/    GitHub / GitHub Enterprise / GitLab / Bitbucket / Bitbucket Server / Gitea
│  ├─ spork_ai/         provider trait + OpenAI-compatible, Anthropic, Ollama clients
│  ├─ spork-settings/   settings.toml, repositories.toml, session.json, per-repo .spork/
│  └─ spork-ui/         gpui-kit views, one module per Mxx, theme, keymap
├─ bins/
│  ├─ spork/            app; `spork <path>` focuses a running instance over IPC (single-instance)
│  ├─ spork-askpass/    GIT_ASKPASS + SSH_ASKPASS helper
│  └─ spork-seq-editor/ GIT_SEQUENCE_EDITOR + GIT_EDITOR for interactive rebase / reword
├─ docs/  tests/fixtures/  xtask/ (packaging, fixtures, benchmarks)
```

Dependency direction: `ui → ops → {git, graph, patch, auth, hosting, ai, settings}`. `git`, `graph`, `patch` never depend on UI.
**The UI never runs git.** Every button calls one `spork-ops` function. That makes every action testable headless.

### 3.2 Crate choices (each has a reason; don't add others without one)

| Need | Crate |
|---|---|
| UI | `gpui-kit` 0.9.1 (gpui-component layer, own theme) |
| fast reads (revwalk, refs, trees, blobs, status) | `gix` |
| text diff / word diff / 3-way | `imara-diff` |
| process mgmt incl. killing process trees on Windows | `command-group` (or std + job objects) |
| async | gpui executors; `smol`/`async-process` where a future needs I/O |
| file watching | `notify` + `notify-debouncer-full` |
| IPC helpers ↔ app | `interprocess` (Unix sockets / Windows named pipes) |
| secrets | `keyring` |
| ssh keys | `ssh-key` |
| HTTP / hosting / AI | `ureq` (blocking, rustls; calls run on background threads like the git runner) + `serde_json`; tokens and keys live in the user's git credential helper. `oauth2` device flow later |
| config | `serde`, `toml`, `serde_json`, `directories` |
| logging | `tracing`, `tracing-appender` |
| notifications | `notify-rust` |
| open in file manager / browser | `open`, `opener` (reveal) |
| updates | `self_update` (GitHub Releases) behind a feature flag; packaging with `cargo-packager` |
| tests | `insta` (snapshots), `proptest` (patch builder), `criterion` (perf) |

### 3.3 Runtime model

```
                   ┌──────────── gpui main thread ────────────┐
 user input ──►    │ Workspace ─► Tab(RepoView) ─► Entity<Repo>│ ◄── notify watcher (debounced 150 ms)
                   └───────┬──────────────────────────▲───────┘
                           │ spawn ops                │ RepoEvent::{Refs,Index,Worktree,Config,OpState}
                   ┌───────▼──────── background ──────┴───────┐
                   │ spork-ops ─► spork_git runner ─► git child │──► spork-askpass ──IPC──► app (dialog)
                   │            gix reader (in-proc)          │──► spork-seq-editor ─IPC─► app
                   └──────────────────────────────────────────┘
```

- **`Entity<Repo>`** per open repo (shared by tabs/windows on the same path): `head`, `refs`, `status`, `op_state`
  (Merging / Rebasing{step,total} / CherryPicking / Reverting / Bisecting / None), `graph: Arc<GraphModel>`, `config`, `settings`.
- **Refresh**: the watcher watches `.git/` (HEAD, refs, packed-refs, index, MERGE_HEAD, rebase-*) and the work tree
  (respecting `.gitignore`). While a Spork op runs, events are held and replaced by one refresh when it ends.
  Each event type refreshes only its slice (refs ≠ status ≠ graph).
- **Jobs** (Activity Manager, M01): every op registers `Job{name, started, progress, log, cancel}`; user-started
  jobs show a progress bar; background jobs (auto-fetch) are silent unless they fail.

### 3.4 The git runner (one place for all git calls)

```rust
pub struct GitCmd { args: Vec<OsString>, cwd: PathBuf, stdin: Option<Vec<u8>>, kind: Kind /*Read|Write|Network*/ }
pub async fn run(cmd: GitCmd, job: &JobHandle) -> Result<Output, GitError>;
```

Always set:
- env `GIT_TERMINAL_PROMPT=0`, `GIT_ASKPASS`/`SSH_ASKPASS`=`spork-askpass`, `SSH_ASKPASS_REQUIRE=force`,
  `SPORK_IPC=<socket>`, `LC_ALL=C` (stable parsing; the UI translates), `GIT_OPTIONAL_LOCKS=0` for reads,
  `GIT_EDITOR=spork-seq-editor --editor` (never block on a terminal editor);
- flags `-c core.quotepath=false -c color.ui=false`, `--no-pager` (implicit with pipes), `-z` wherever a command supports it;
- Network ops: `--progress`; parse stderr lines split on `\r` with
  `^(?P<phase>[A-Za-z ]+):\s+(?P<pct>\d+)% \((?P<cur>\d+)/(?P<tot>\d+)\)`;
- verbose toggles (Git prefs): `GIT_TRACE`, `GIT_CURL_VERBOSE`, `GIT_TRACE_PACKET`, `GIT_TRACE_PERFORMANCE` → job log;
- Windows: `CREATE_NO_WINDOW`; kill = terminate job object; macOS/Linux: process group + SIGTERM, SIGKILL after 3 s.

`GitError` classification (stderr regexes + exit code) drives the friendly error banner + "Show output":
`AuthFailed`, `HostKeyUnknown`, `NonFastForward`, `Conflicts`, `LocalChangesWouldBeOverwritten{files}`,
`IndexLocked` (offer delete `index.lock` when no git process runs), `HookFailed{hook, output}`, `NotARepo`,
`Network`, `LfsMissing`, `Other`.

Minimum git: **2.38** (needed for `rebase --update-refs`); detect with `git version`; show a blocking notice below it.
Git instance (pref): *system git* (default on macOS/Linux) or *bundled* (default on Windows: MinGit + git-lfs shipped in the installer).

### 3.5 Read path: gix first, CLI fallback

| Data | Primary | Fallback |
|---|---|---|
| refs + upstream + ahead/behind | gix | `for-each-ref --format=…%(upstream:track)` |
| commit list for graph | gix revwalk (uses commit-graph file) | `log --format=%H%x00%P%x00%an%x00%ae%x00%at%x00%ct%x00%s -z` |
| status | gix status | `status --porcelain=v2 -z --branch --untracked-files=all` |
| diff | CLI `diff` (exact git semantics, rename detection, textconv, LFS) | — |
| blame | CLI `blame --porcelain` | — |
| tree / blob at commit | gix | `ls-tree` / `cat-file` |

Each pair has a parity test (fixture repo, compare outputs) so a gix bug can't silently show wrong data.

---

## 4. UX shell (layout patterns, our own look)

Same proven 3-pane UX, built only from gpui-kit components with a Spork theme:

```
┌Toolbar: [repo ▾][branch ▾]  Fetch  Pull  Push  Stash  Branch  │ Quick Launch (⌘P) │ AI ✦ ┐
├───────────────┬──────────────────────────────────────────────────────────────────────────┤
│ Sidebar(Tree) │  Tabs: [Changes] [All Commits] [Search] [Stats]                          │
│  ★ Favorites  │  ┌─ commit graph + list (VirtualList, custom-painted graph column) ────┐ │
│  Branches     │  └─────────────────────────────────────────────────────────────────────┘ │
│  Remotes      │  ┌─ details (Tabs: Summary | Changes | File Tree) ──┬─ Diff ────────────┐ │
│  Tags/Stashes │  │ file list (Tree)                                 │ Diff / Editor      │ │
│  Submodules   │  └──────────────────────────────────────────────────┴───────────────────┘ │
│  Worktrees    │                                                                          │
│  Pull Requests│                                                                          │
├───────────────┴──────────────────────────────────────────────────────────────────────────┤
│ StatusBar: op state (Rebasing 3/7 · Continue · Abort) · jobs spinner · branch ↑2 ↓1      │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

Built on `Root`, `Dock` (layout persisted per window), `Tabs`, `Sidebar`, `Tree`, `VirtualList`, `Resizable`,
`Diff`, `Editor`, `Dialog`, `Command`, `Notification`, `StatusBar`, `Toolbar`, `Settings`, `Chart`.

UX rules: every dialog remembers its last options; every destructive action names what it destroys and offers
the reversible variant first (stash before discard, `--force-with-lease` before `--force`); everything has a
keyboard path; every op is visible in the Activity Manager with full git output.

Keymap: defaults in `spork-ui/keymap.toml`, user-overridable. `Cmd` on macOS = `Ctrl` elsewhere.
Defaults (ours): Quick Launch `Cmd-P`, Command palette `Cmd-Shift-P`, Fetch `Cmd-Shift-F`, Pull `Cmd-Shift-L`,
Push `Cmd-Shift-K`, Commit `Cmd-Enter` (in message), Stage/Unstage selection `Space`/`Enter`… (full list in M01).

---

## 5. Settings and stored state

Paths via `directories`: macOS `~/Library/Application Support/Spork`, Linux `~/.config/spork`, Windows `%APPDATA%\Spork`.

```toml
# settings.toml (user)
[general]   appearance="system"  compact_branch_labels=false  commit_sort="date"|"topo"  max_commits=0  # 0 = unlimited (virtualized)
            syntax_highlighting=true  branch_name_space_char="-"  source_dirs=["~/code"]  diff_font="JetBrains Mono 12"
[fetch]     auto=true  interval_min=10  include_tags=true  prune=false
[commit]    spellcheck=true  page_guide=72  subject_low=50  subject_high=72  push_after_commit=false
[checkout]  update_submodules=true
[git]       instance="system"|"bundled"|"/custom/path"  verbose={trace=false, curl=false, packet=false, performance=false}
[integration] terminal="auto"  merge_tool="builtin"  diff_tool="builtin"  bugtracker_links=true  avatars="gravatar"|"github"
[updates]   check=true  download=false  channel="stable"|"beta"  interval="daily"
[ai]        provider="none"  model=""  endpoint=""  # key in keyring; see 10-ai.md
[remember]  # last-used dialog options (Fork keeps these as user defaults, see §2)
            checkout_local_changes="stash"  pull_rebase=false  pull_autostash=true  push_all_tags=false
            fetch_all_remotes=true  merge_type="default"  cherry_pick_append_origin=false  details_tab="summary"
```

`repositories.toml` = `{source_dirs, scan_depth=3, ignore=[…], repository=[{path, opened(unix), starred_branches=[…], hidden_branches=[…]}]}`
`session.json` = `{active_workspace, window_frame, workspaces=[{name, active_window, windows=[{frame, tabs=[path], active_tab, dock_layout}]}]}`
`accounts.json` = `[{id, server_url, service, auth_type, username, avatar_url}]` + token in keyring `spork/<id>`.
Per repo, versioned (shared with the team): `.spork/custom-commands.json`, `.spork/bugtracker.json`.
Per repo, private: in `.git/spork/` (`ui.json`: filters, layout; never committed).

Migration: "Import from Fork" reads Fork's three files (shapes in §2) and the repo list; no tokens are read.

---

## 6. Testing strategy (what "working to perfection" means, measurably)

| Layer | Tool | Gate |
|---|---|---|
| parsers (status, log, diff, blame, for-each-ref, progress) | unit + `insta` snapshots from real git output, all 3 OSes | every PR |
| `spork_patch` line staging | `proptest`: random file + random line selection → built patch applies with `git apply --cached --check` and index equals expected | every PR |
| `spork_graph` | snapshot of lane layout for 20 fixture topologies (octopus, criss-cross, orphan branches, 1-commit repo, stash commits) | every PR |
| ops | integration tests against real git in temp repos created by `tests/fixtures/*.sh` (fixed dates/authors) | every PR, 3 OSes |
| behaviour parity | for each spec'd action: run Fork (traced) and Spork on the same fixture, diff the **resulting repo state** (`for-each-ref`, `status`, `log --format=%H %T`, stash list, config) | per module before "done" |
| UI | `gpui_kit::test` headless: open every dialog, fill, submit, assert op called with expected args; focus order; a11y roles | every PR |
| performance | `criterion` + scripted UI benches on `linux.git` and a 300k-file monorepo | nightly; budget fails the build |

Performance budgets (M-series Mac / 8-core Linux / Windows equivalent):
first graph paint of linux.git < 400 ms · scroll ≥ 120 fps · status on 300k files < 700 ms (fsmonitor on) ·
switch commit → diff shown < 50 ms for files < 5k lines · RSS < 400 MB on linux.git.

**Definition of done for a feature**: spec merged → ops fn with integration test → UI with keyboard path →
error cases from spec handled → passes on macOS, Linux (X11 + Wayland), Windows → user docs page.

---

## 7. Schedule (2 full-time devs or 1 dev + agents; ~30 weeks to 1.0)

| Wk | Phase | Deliverables | Exit criteria |
|---|---|---|---|
| 1–2 | **P0 Foundations** | workspace, CI matrix, git runner + error classifier, `Entity<Repo>`, watcher, settings, single-instance + `spork` CLI, theme | opens a repo on 3 OSes, shows branches, refreshes on external `git commit` |
| 3–6 | **P1 Read-only** | M03 sidebar, M05 graph + list + search, M06 details (read), M08 text diff, blame, file history | browse linux.git within budgets; parity with Fork's commit list ordering |
| 7–10 | **P2 Daily driver** | M07 status/stage/hunk+line staging/commit/amend, M04 branches, M09 fetch/pull/push, M10 auth (all 3 OSes) | team dogfoods Spork for a week; zero data-loss bugs |
| 11–14 | **P3 History editing** | M11 merge, rebase, interactive rebase, conflict resolver; M06 cherry-pick/revert/reword/patches; M12 stash/tags | rebase-with-conflicts fixture suite green; 3-way merger handles all conflict types |
| 15–18 | **P4 Breadth** | M02 clone/init/repo settings, M12 submodules/worktrees, M13 git-flow + lean branching, M14 LFS, M08 image/binary diff, M01 workspaces/quick launch/activity manager | all 139 surfaces have a Spork equivalent or a documented "won't do" |
| 19–22 | **P5 Integrations** | M15 accounts, PRs, notifications; M16 custom commands, bug tracker rules, statistics, prefs complete | |
| 23–25 | **P6 AI** | `10-ai.md` features 1–6 | all AI actions previewable, no git op runs without confirmation |
| 26–30 | **P7 Ship 1.0** | installers (dmg universal, msi + MinGit, AppImage/deb/rpm/Flatpak), updater, signing/notarization, docs site, import-from-Fork, perf pass, a11y pass | public 1.0 |

Start the riskiest items in week 1 as spikes: graph layout at 1M commits, line staging builder,
askpass over IPC on Windows, gpui-kit `Editor` as 3-way merge result pane.
