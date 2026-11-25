# Spork

**Neither a spoon nor a fork: just a better Git client.**

Spork is a free, open-source Git client for **macOS, Windows and Linux**. It's built for people who love what
polished desktop clients like Fork and GitKraken offer (a clear commit graph, painless staging, safe history
editing) but want it open, cross-platform, and owned by its community instead of a license key.

> 🚧 **Spork is in early development.** It isn't ready for daily use yet. Today it opens repositories in tabs,
> clones and creates them, draws the full commit graph with details, diffs (also side by side), blame and file
> history, stages files or single lines, commits (also signed), pushes, merges, rebases (also interactively),
> resolves conflicts, runs git-flow, Git LFS and bisect, works with GitHub, GitLab and Gitea pull requests, manages
> SSH keys, runs your custom commands, and can write commit messages and reviews with your own AI provider.
> Everything else below is being built in the open, step by step. Star or watch the repo to follow along, and see
> [Status & roadmap](#status--roadmap).

## What Spork is

- **An open-source alternative to Fork and GitKraken.** The same kind of fast, visual, keyboard-friendly Git
  workflow, without a subscription or a paywall.
- **Open source, always.** Spork's code is public and will stay public, for everyone: individuals, teams and
  companies. No paid tier, no feature locked behind an account, no telemetry.
- **Cross-platform for real.** One Rust codebase, native builds for macOS, Windows and Linux, with the same
  features on every OS.
- **Independent.** Spork is written from scratch. It doesn't contain code from any other Git client and is not
  affiliated with Fork, GitKraken, or any other product.

## Why another Git client?

Today the nicest Git GUIs are closed source, paid, and often missing one of the three major platforms.
The open-source ones tend to be slower or harder to use. Spork aims to close that gap: commercial-grade UX
in an open project that anyone can use, audit, fix and extend.

## Features (the plan)

Each item has a detailed spec in [`docs/specs/`](docs/specs) and is built in the order of the roadmap.

| Area | What you'll get |
|---|---|
| History | Fast commit graph for very large repos, search by message/author/content, file history, blame, reflog view |
| Working copy | Stage files, hunks or **single lines**, discard safely, amend, commit templates, signing |
| Branching | Create / checkout / rename / delete / reset, drag-and-drop merge & rebase, ahead/behind counters |
| History editing | Merge, rebase, **interactive rebase** (reorder, squash, fixup, reword, drop), cherry-pick, revert, with undo |
| Conflicts | Built-in **3-way merge** editor, or your favourite external tool |
| Remotes | Fetch / pull / push with progress, force-with-lease by default, multiple remotes |
| More git | Stashes & snapshots, tags, submodules, worktrees, git-flow, Git LFS, bisect, patches |
| Hosting | GitHub, GitLab, Bitbucket and Gitea accounts, clone from your repo list, pull requests |
| Power tools | Custom commands, bug-tracker links, repository statistics, quick launch, workspaces |
| **AI (optional)** | Commit messages, code review, PR descriptions, conflict suggestions, using **your own provider** (Claude, any OpenAI-compatible API, or a local Ollama model). Off by default, you can see exactly what is sent, and it never runs git on its own. |
| Look & feel | Light and dark themes that follow your system (JetBrains-inspired palette), keyboard shortcuts for everything |

## The stack

| Layer | Technology | Why |
|---|---|---|
| Language | [Rust](https://www.rust-lang.org) | fast, memory-safe, one codebase for every OS |
| UI | [GPUI](https://www.gpui.rs) via [GPUI Kit](https://gpui-kit.com) | GPU-rendered at 120 fps, from the team behind the Zed editor, with 75+ components (virtual lists, trees, diff and code editor, docking, charts) |
| Git (reads) | [gitoxide](https://github.com/GitoxideLabs/gitoxide) (planned) | in-process reads for speed on huge repositories |
| Git (writes & network) | the real `git` CLI | your hooks, credential helpers, SSH keys, signing and LFS work exactly as in your terminal |
| Platform | OS keychain, native notifications, file watching | credentials never stored in plain text |

Design rule: the UI never runs git directly. Every action goes through one tested operations layer, which keeps
behaviour predictable and testable without a window. Details: [docs/00-overview.md](docs/00-overview.md).

## Status & roadmap

| Phase | Scope | State |
|---|---|---|
| P0 Foundations | git runner, error handling, repo view with branches/remotes/tags, themes, auto-refresh, CI on 3 OSes | ✓ done |
| **P1 Read-only client** | commit graph ✓, commit details ✓, diffs ✓, blame ✓, file history ✓, find commit ✓, compare commits ✓, file tree ✓, syntax highlighting ✓, merge diffs against either parent ✓ | **in progress** |
| P2 Daily driver | staging incl. single lines ✓, discard ✓, commit ✓, amend ✓, fetch/pull/push ✓ (with progress and cancel), branches ✓, password prompts ✓ (SSH passphrases can be remembered), commit options ✓, file filter and folder view ✓ | ✓ done |
| P3 History editing | merge ✓, rebase ✓, interactive rebase ✓, 3-way conflict resolver ✓, cherry-pick ✓, revert ✓, stashes ✓, tags ✓, drag-and-drop merge / rebase ✓, undo last change ✓, several commits at once ✓, signed tags ✓ | ✓ done |
| **P4 Breadth** | open / clone / new ✓ (branch, shallow), remotes with push URLs ✓, recent repos ✓, submodules ✓, worktrees ✓, git-flow ✓ (incl. bugfix), LFS ✓, image diffs ✓ (side by side, overlay, swipe) | **in progress** |
| **P5 Integrations** | GitHub, GitLab, Gitea & Bitbucket accounts, pull / merge requests ✓ with CI status, clone from your account ✓, notifications ✓, custom commands ✓, statistics ✓ | **in progress** |
| **P6 AI** | commit messages ✓, code review ✓, PR descriptions ✓, explain a commit ✓, release notes ✓, conflict suggestions ✓, rebase planner ✓, "Ask Git" ✓ (Anthropic or a local / OpenAI-compatible model), semantic search | **in progress** |
| P7 1.0 | downloads for every OS on each version tag ✓ (unsigned), signed installers, auto-update, docs site | |

## Build from source

Requirements: Rust (stable) and git ≥ 2.38.
- **macOS:** Xcode (for Metal).
- **Linux:** the [GPUI system libraries](https://github.com/zed-industries/zed/blob/main/docs/src/development/linux.md).
- **Windows:** the MSVC build tools.

Spork's crates are in the Spork IDE monorepo's workspace; paths below are from the monorepo's root. Open a
repository with Spork:

```bash
cargo run -p spork -- /path/to/a/repo
```

On macOS, `spork/scripts/package-macos.sh` builds `target/Spork.app` (signed ad hoc, so it runs on the Mac that built
it) to drop in /Applications. The app icon is `crates/spork/icon/spork.svg`; `spork/scripts/make-icon.sh` renders it
again. `spork/scripts/package.sh` builds this OS's download into `target/dist` (a zipped app, a Linux tarball with a
`.desktop` entry, or a Windows zip); pushing a `v*` tag runs it on all three and publishes a GitHub release.

Run the checks (format, lint and tests):

```bash
spork/scripts/check.sh
```

## Project layout

```
crates/spork_ai      optional AI with your own provider: commit messages, reviews, PR text (tested offline)
crates/spork_git     git runner, error classification, refs and commit log (tested against real git)
crates/spork_graph   commit-graph lane layout (pure, no I/O)
crates/spork_hosting GitHub, GitLab and Gitea REST clients: pull requests, repositories (tested offline)
crates/spork_patch   partial patches for line-level staging (pure, no I/O)
crates/spork           the desktop app (GPUI Kit), themes in crates/spork/assets/themes
spork/docs/          architecture overview, 16 feature specs, AI and open-source policy
spork/scripts/       helpers for writing behaviour specs
```

## Contributing

Spork is built in the open and contributions of every size are welcome: code, specs, testing on your OS, design,
docs, or simply opening an issue with what you need from a Git client.

1. Pick a module in [`docs/specs/`](docs/specs) (each lists its open questions and tests).
2. Open an issue or draft PR to say what you're working on.
3. Follow [CONTRIBUTING.md](CONTRIBUTING.md): Conventional Commit messages, one branch per change, and a DCO sign-off (`git commit -s`).

**Clean-room rule:** Spork is an independent implementation. Don't submit code, pseudo-code or text derived
from decompiling or disassembling proprietary software. Specs describe observable behaviour only, and code is
written from the specs. This keeps Spork safe to use and to build on for everyone.
More in [docs/20-open-source.md](docs/20-open-source.md).

## License

Spork is free software, licensed under the **[GNU General Public License v3.0 or later](LICENSE)**.
Anyone can use, study, change and share it, and every distributed version, including modified ones, must stay
open source under the same terms. That's how Spork stays open for everyone, for good.

Bundled third-party assets, each under its own license (files in `crates/spork_workbench/assets`):
IntelliJ Platform icons and theme colors from [intellij-community](https://github.com/JetBrains/intellij-community)
(Apache-2.0, with its NOTICE), and the [Inter](https://github.com/rsms/inter) and
[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono) fonts (SIL Open Font License 1.1).

*Spork is not affiliated with or endorsed by Fork, GitKraken, JetBrains, or any other Git client or IDE maker. Product names belong to their owners.*
