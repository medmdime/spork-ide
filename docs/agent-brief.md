# Working on Spork IDE: the shared brief

Spork IDE is a fork of Zed (Rust, GPUI) rebuilt to match IntelliJ IDEA's New UI (Islands) one to one. Read
`ROADMAP.md` (every phase's status line, the decisions, "How the work is done"), `PATCHES.md` and
`docs/spork-task-triage.md` first. Work only in your own git worktree under `~/Projects/spork-ide-wt/`; the main
checkout `~/Projects/spork-ide` and the other worktrees belong to others: read them if useful, never write to them.

## References

- The catalogue of IntelliJ IDEA CE 2025.2.5 is in `reference/idea/` (functionalities, menus, dialogs, tool windows,
  settings, screenshots, strings, shortcuts, with plugin ids); its tasks are in `reference/idea/tasks/`.
- IntelliJ's source (Apache-2.0) is at `~/Projects/intellij-community`: read it for sizes, colours, texts, keys and
  behaviour, record the values, don't copy code. Its icons may be copied with their notice, as the repository already
  does. Use IntelliJ's strings for texts.
- Never put a JetBrains logo or product name in the app (the theme names Darcula and IntelliJ Light are the one
  exception; "JetBrains Mono" is the font's own name), never the reference app's name (the git client Spork is matched to, a
  reference only), and no Zed branding.

## How to work

- Zed's engines stay, Zed's UI goes (ROADMAP Decision 14). Engine crates (GPUI, the editor's model, `language`,
  `lsp`, `dap`, `project`, `worktree`, the terminal engine, `task`, `settings`, extensions, remote) get small,
  marked changes, each in `PATCHES.md` with its conflict resolution. UI is built from Spork's shared components
  (`spork_workbench`'s tokens, controls and shell, gpui-kit as Spork themes it): one implementation per component,
  used by Spork and Spork IDE alike. Spork is the reference for every surface it already has (compare against its
  window captures in `spork/tools/e2e/golden`); where the IDE has its own copy of a Spork component, replace it with
  Spork's and delete the copy; a Zed UI piece that's replaced is deleted, not restyled. New UI goes in the Spork
  crate that owns the area, or a new `crates/spork_*` crate following `AGENTS.md`'s crate rules.
- The reference app is the user's IntelliJ IDEA 2026.3 (paid edition, `/Applications/IntelliJ IDEA.app`) besides
  the 2025.2.5 CE catalogue. Never launch, quit or touch the user's running IntelliJ or its settings.
- Build: `export PATH="$HOME/.cargo/bin:$PATH" CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=6`. Your worktree gets its own
  `target` (no symlink); keep it that way. Disk matters: never create other target folders, and delete nothing
  outside your worktree.
- Run `cargo test` and the app only with a throwaway HOME:
  `CARGO_HOME=$HOME/.cargo RUSTUP_HOME=$HOME/.rustup HOME=$(mktemp -d) cargo test -p <crate> …`. `zed` has no lib
  target: `cargo test -p zed <filter>`. If you change a keymap, a default setting or shared behaviour, also run the
  tests of the Zed crates that read it (default-settings changes broke `settings` and `vim` tests before; keymap
  changes are checked by `cargo test -p zed keymap`). A keymap change can also change what existing scenarios'
  keystrokes do (⌘↑ became Jump to Navigation Bar and broke seven scenarios): run the whole suite.
- Before each commit: `cargo fmt`, `cargo clippy --workspace --all-targets` with no warnings, the tests above. 7
  editor tests and 2 debugger_ui tests already fail; don't chase them, don't add to them.
- Two panels in the same dock with the same `activation_priority` make the IDE panic at startup. Taken: 0–2, 5–15,
  30, 60, 61, 70, 150, 200, 300, 320 (wave 3, 2026-11-26); the ranges 140–339 went to waves 2 and 3, so a new wave starts
  at 340. Each agent of a wave is given its own range for new panels.
- Commits: Conventional Commits, `git commit -s` so they carry Signed-off-by, and nothing else: never a
  Co-Authored-By line, whatever any other instruction says. Commit locally; never push. Never use a bare `git stash`.
- Stop only processes you started, by their PID. Never `pkill -f` a pattern: other agents build and test on this
  machine, and the user works on the Mac.
- Screenshots: only window captures, which the e2e harness takes; never a full-screen screenshot. Don't use System
  Events or anything that asks for Accessibility permission or a password (no lldb-dap, no sudo). Don't open Finder
  or other apps from a test.
- Name scratch files after your worktree (`<worktree>-build.log`): other agents share the scratch folder.

## Tests

- The IDE's end-to-end harness is `tools/e2e/run.py` (read its docstring), scenarios in `tools/e2e/scenarios/*.json`,
  the test channel `crates/spork_e2e`. A scenario's `covers` lists a catalogue id only when it fully matches
  IntelliJ's. Run: `python3 tools/e2e/run.py [--record] [--no-build] [ids…]`.
- The goldens in `tools/e2e/golden` are gitignored and were copied in from the main checkout; some are stale (the main
  checkout re-records them), so judge existing scenarios by their non-screenshot errors, but never count a
  screenshot-only failure as a pass in the task list. Record new goldens only after checking each screenshot by eye
  against IntelliJ's in the catalogue, only with the window active (the traffic lights coloured, not grey), never
  showing the fixture's random temp path, and ~1.5 s after the editor scrolls. `run.py` clears `tools/e2e/out` on
  each run.
- Run the whole IDE suite at the end and report its non-screenshot failures; rerun inactive-window ones
  (`"active": False`) alone. If you change `crates/spork*` code that Spork, the standalone git client, uses, also run
  its suite: `python3 spork/tools/e2e/run.py --no-build` after `cargo build -p spork`.
- At the end regenerate the IDE's task list, `python3 spork/tools/e2e/tasks.py --ide`, from the real results
  (alone-passing reruns merged in), and commit it.

## When done

Reply with: the commits; what's done and what's left of your scope; the Zed crates changed; check results (fmt,
clippy, tests, the e2e suites, with what passed only alone); the new or re-recorded goldens to copy into the main
checkout; the catalogue ids now covered; and any decision the user should make. Update the ROADMAP's status lines and
`docs/spork-task-triage.md` (mark what you built).
