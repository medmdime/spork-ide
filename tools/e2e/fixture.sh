#!/bin/sh
# usage: fixture.sh <dir> — a small Rust project in a git repository whose every commit and date
# is fixed, so the end-to-end screenshots are the same from one run to the next: two commits on
# main, a tag, and a branch with a commit of its own (its file isn't in main's working tree).
set -eu
dir=$1
rm -rf "$dir"
mkdir -p "$dir/src"
cd "$dir"
git init -q -b main
git config user.name "Ada Lovelace"
git config user.email ada@example.com
git config commit.gpgsign false
n=0
commit() {
  n=$((n + 1))
  date="2026-01-$(printf %02d "$n")T10:00:00Z"
  GIT_AUTHOR_DATE=$date GIT_COMMITTER_DATE=$date git commit -q -m "$1"
}
echo "# Fixture" > README.md
git add README.md
commit "docs: start the readme"
cat > Cargo.toml <<'TOML'
[package]
name = "fixture"
version = "0.1.0"
edition = "2024"
TOML
cat > src/main.rs <<'RUST'
mod greeting;

fn main() {
    println!("{}", greeting::hello("world"));
}
RUST
cat > src/greeting.rs <<'RUST'
pub fn hello(name: &str) -> String {
    format!("Hello, {name}!")
}
RUST
git add .
commit "feat: greet the world"
git tag v0.1.0
git checkout -q -b farewell
cat > src/farewell.rs <<'RUST'
pub fn bye(name: &str) -> String {
    format!("Goodbye, {name}!")
}
RUST
git add .
commit "feat: say goodbye"
git checkout -q main
