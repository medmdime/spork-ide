# Open-source project plan

## 1. Name & identity
**Spork** (neither a spoon nor a fork, just a better tool). Check before launch: GitHub org `spork_git` (or similar), domain, crates.io prefix `spork-*` (`spork_git` was free on 2026-11-13; bare `spork` is taken),
and a trademark search in your jurisdiction. Own logo and theme; nothing visual taken from Fork.
Bare `spork` is taken on crates.io (an unrelated crate); the app does not need it. Publish internal crates as `spork-*`.

## 2. License
Recommendation: **GPL-3.0-or-later** for the app, so improvements (including by companies redistributing it) stay open.
Compatible with dependencies: gpui/gpui-kit (Apache-2.0), gix (MIT/Apache-2.0), most crates MIT/Apache. Bundled MinGit (GPL-2.0) and git-lfs (MIT) ship as separate programs alongside (aggregate), with their licenses in the installer.
If you'd rather maximise company adoption/embedding: Apache-2.0. Pick once; changing later needs every contributor's consent.
Run `cargo deny` in CI for license and advisory checks.

## 3. Clean-room contribution policy (CONTRIBUTING.md)
This is what protects the project; put it at the top of CONTRIBUTING.md:
- Contributions must be original work. **Do not submit code, pseudo-code, or text derived from decompiling or disassembling proprietary software** (including Fork). Specs may describe observable behaviour only.
- `specs/` PRs (behaviour descriptions) and `crates/` PRs (code) are separate; code PRs cite the spec they implement.
- People/agents who did binary analysis of a module don't author that module's code.
- Every commit carries a DCO sign-off (`git commit -s`) — enforced by the DCO GitHub app.
- PR template checkbox: "I have not used decompiled proprietary code or output from reverse-engineering tools in this PR."

## 4. Repo & workflow
- Monorepo layout from overview §3.1. Branching and commit rules are in [CONTRIBUTING.md](../CONTRIBUTING.md): `main` = production (tagged releases), `develop` = integration, one `feat/<topic>` / `fix/<topic>` branch per change, Conventional Commit messages with no co-author/attribution trailers. Enforced with branch protection once the app is stable.
- Every push to every branch runs all test suites (CI below); required checks on `develop` and `main` once enforcement starts.
- GitHub Actions matrix: macOS 14 arm64, macOS 13 x64, Ubuntu 24.04 (X11 + Wayland headless), Windows 2022. Jobs: fmt, clippy `-D warnings`, test, `cargo deny`, fixture integration tests, nightly perf + AI evals.
- Issues: labels per module `M01…M16`, `good first issue` on parsers/specs/fixtures, `parity` for gaps vs the reference UX.
- RFCs (`docs/rfcs/`) for anything that changes architecture, settings format, or adds a dependency.
- Code of Conduct (Contributor Covenant), SECURITY.md (private disclosure for askpass/IPC/custom-command issues).

## 5. Releases
- SemVer, tag `vX.Y.Z` → release workflow builds: macOS universal `.dmg` (signed + notarized; needs an Apple Developer account),
  Windows `.msi` + portable zip (code-signed; e.g. Azure Trusted Signing), Linux AppImage, `.deb`, `.rpm`, Flatpak (Flathub; git runs on the host via `flatpak-spawn --host`), Homebrew cask, winget, AUR.
- Channels: stable + beta (prereleases). In-app updater reads GitHub Releases; package-manager installs disable it.
- No telemetry. Crash reports are local files the user may attach to an issue.

## 6. Roadmap after 1.0
Plugin API (gpui-shell JavaScript extensions, capability-scoped) for custom panels · in-app PR review comments · partial clone / sparse checkout UI · Jujutsu (jj) colocated-repo support · multi-repo dashboard.
