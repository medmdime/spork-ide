# Rust coding guidelines

Spork follows Zed's conventions (zed-industries/zed `AGENTS.md`), since it will grow into a Zed fork, and the
[rust-skills](https://github.com/leonardomso/rust-skills) rules for everything they don't cover.

* Prioritize correctness and clarity over speed unless a path is measured to be hot.
* Comments explain *why* when it isn't obvious; don't write comments that summarize the code.
* Prefer extending an existing file unless the code is a new logical component; avoid many small files.
* Avoid functions that panic (`unwrap()`, `expect()`, indexing that can go out of bounds) outside tests;
  propagate with `?`.
* Never silently discard an error with `let _ =` or `.ok();` on a fallible operation:
  - propagate it with `?` when the caller should handle it;
  - use `.log_err()` (`crate::util::ResultExt`) when it can't change what happens but must stay visible;
  - match on it when it needs its own handling.
  Dropping a send to a closed channel or an update of an entity that is gone is fine.
* Errors from git and network work reach the UI: `run_op` / `run_outcome` show them as notifications.
* Never create `mod.rs` files: `src/some_module.rs`, not `src/some_module/mod.rs`.
* Crates live in `crates/<snake_case>`, named the same, with the library root named after the crate
  (`[lib] path = "src/spork_git.rs"`), dependencies in `[workspace.dependencies]` and `[lints] workspace = true`.
* Use full words for names (`prefs`, not `p`); `cx`, `window` and `ix` are the GPUI conventions.
* Scope clones for closures and tasks by shadowing:
  ```rust
  cx.spawn({
      let view = view.clone();
      async move |cx| { /* … */ }
  })
  ```
* A view's subscriptions are kept in `_subscriptions: Vec<Subscription>` (or `_subscription`).
* A shortcut with a known limit gets a comment: `// shortcut: <the limit>, <when to upgrade>`.
* No creative additions beyond what was asked.

# Design system

* UI is built from `spork_workbench::tokens` (IntelliJ's Islands colors, type scale and metrics) and
  `spork_workbench::controls` (Button, ActionButton, ToolbarDropdown, ComboBox, TextField, check boxes,
  links). Don't hardcode a color or a control size in the app: add the token or the control to the
  workbench first, with its IntelliJ source (theme key or `docs/reference-spec.md` section).

# GPUI

* `cx` is the context argument; `window` comes before it; callbacks come after it.
* Inside `entity.update(cx, |this, cx| …)` use the closure's `cx`, never the outer one.
* Updating an entity while it is already being updated panics: an action handler running inside a view's update
  must not `read` or `update` that same view (pass what it needs in instead).
* A dropped `Task` cancels its work: await it, `detach()` it, or keep it in a field.
* Call `cx.notify()` after any state change that affects rendering.
* `#[test]` in a module that glob-imports `gpui_kit::*` resolves to gpui's test attribute and recurses: import
  the items under test by name (`use super::{a, b};`).

# Checks

Before committing: `scripts/check.sh` (`cargo fmt`, `cargo clippy --all-targets` with no warnings and
`cargo test`, on Spork's crates). Commits follow CONTRIBUTING.md.
