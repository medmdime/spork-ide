#!/bin/sh
# usage: fixture-gradle.sh <dir> — the Gradle scenarios' fixture: fixtures/gradle (the Maven
# fixture's sources as a Gradle project with its wrapper, Gradle 8.10.2's gradlew and
# gradle-wrapper.jar, Apache-2.0) in a git repository whose commit and date are fixed.
set -eu
dir=$1
rm -rf "$dir"
mkdir -p "$dir"
cp -R "$(dirname "$0")/fixtures/gradle/." "$dir"
cd "$dir"
git init -q -b main
git config user.name "Ada Lovelace"
git config user.email ada@example.com
git config commit.gpgsign false
git add .
GIT_AUTHOR_DATE=2026-01-01T10:00:00Z GIT_COMMITTER_DATE=2026-01-01T10:00:00Z git commit -q -m "feat: greet the world"
