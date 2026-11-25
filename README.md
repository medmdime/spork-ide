# Spork IDE

Spork IDE is a desktop IDE written in Rust, built on [Zed](https://github.com/zed-industries/zed)'s engines (the GPUI
UI framework, the editor, languages and language servers, projects, the terminal, tasks and the debugger) with an
interface of its own: tool windows around the editor, a project view, run and debug configurations, Docker in
Services, a Settings dialog, plugins, and version control.

This repository also holds **Spork**, a free, cross-platform Git client: a commit graph, diffs, staging of files or
single lines, branches, merge and interactive rebase, conflicts, stashes and shelves, pull requests and more. Spork
runs on its own, and it is Spork IDE's version control.

> Both apps are in early development (alpha). Expect rough edges.

## Download

Each release has both apps for macOS (Apple silicon), Windows (x64) and Linux (x64) on the
[Releases page](https://github.com/medmdime/spork-ide/releases). The downloads are not signed yet: the release notes
say how to open them on each system.

## Build from source

Requirements: the Rust toolchain of `rust-toolchain.toml` (rustup installs it) and git 2.38 or newer.

- **macOS:** Xcode, for Metal.
- **Linux:** the system libraries GPUI needs: `script/linux` installs them.
- **Windows:** Visual Studio's C++ build tools, and a checkout with symlinks on (`git config --global core.symlinks
  true` before cloning).

```bash
cargo run -p zed      # Spork IDE
cargo run -p spork    # Spork, the git client (cargo run -p spork -- /path/to/a/repo)
```

`cargo build --release -p zed -p cli -p spork` builds the release binaries. On macOS, `spork/scripts/package-macos.sh`
builds `target/Spork.app`; `.github/workflows/release.yml` shows how each OS's downloads are packaged.

## Licences

- Zed's code is under the GPL-3.0-or-later ([`LICENSE-GPL`](LICENSE-GPL)), with some crates, GPUI among them, under
  the Apache-2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)); each crate's licence is in its `Cargo.toml` and licence file.
- Spork and Spork IDE's own crates (`crates/spork*`) are under the GPL-3.0-or-later ([`spork/LICENSE`](spork/LICENSE)).
- Together the apps are distributed under the GPL-3.0-or-later.
- Bundled third-party assets keep their own licences, each beside them: icons and theme colours under the Apache-2.0
  (with their NOTICE), the Inter and JetBrains Mono fonts under the SIL Open Font License 1.1, Lucide's icons
  (`assets/icons/LICENSES`).

Spork IDE is an independent project, not affiliated with or endorsed by Zed Industries or any IDE maker.
