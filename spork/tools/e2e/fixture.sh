#!/bin/sh
# usage: fixture.sh <dir> — a repository whose every commit, branch and date is fixed, so the
# end-to-end screenshots are the same from one run to the next: a few commits on main, a merged
# feature branch, two open branches, a tag, a local "origin", a worktree and uncommitted changes.
set -eu
dir=$1
rm -rf "$dir" "$dir.origin" "$dir-worktree"
mkdir -p "$dir"
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
echo "# Fixture" > README.md && git add README.md && commit "docs: start the readme"
echo "fn main() {}" > main.rs && git add main.rs && commit "feat: an empty program"
git checkout -q -b feature/login
echo "login" > login.rs && git add login.rs && commit "feat(login): a login screen"
echo "logout" >> login.rs && git add login.rs && commit "feat(login): log out too"
git checkout -q main
echo "two" >> README.md && git add README.md && commit "docs: a second line"
date="2026-01-$(printf %02d $((n + 1)))T10:00:00Z"
GIT_AUTHOR_DATE=$date GIT_COMMITTER_DATE=$date git merge -q --no-ff feature/login -m "Merge branch 'feature/login'"
n=$((n + 1))
git tag v1.0
git checkout -q -b fix/typo
echo "typo" > typo.txt && git add typo.txt && commit "fix: a typo"
git checkout -q main
git branch release/2026.1
git init -q --bare "$dir.origin"
git remote add origin "$dir.origin"
git push -q origin main fix/typo
git branch -q -u origin/main
git worktree add -q "$dir-worktree" release/2026.1
echo "changed" >> README.md
echo "new" > notes.txt
