#!/usr/bin/env bash
# Runs exactly what CI runs; stops at the first failure. Use before every push.
set -euo pipefail
# Spork's crates, in the monorepo's workspace
crates=(-p spork -p spork_ai -p spork_git -p spork_graph -p spork_hosting -p spork_patch -p spork_terminal -p spork_workbench)
cargo fmt "${crates[@]}" --check
cargo clippy "${crates[@]}" --all-targets -- -D warnings
cargo test "${crates[@]}"
echo "all checks passed"
