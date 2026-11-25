#!/bin/sh
# usage: fixture-java.sh <dir> — a plain Java folder: fixture-maven.sh's project without its build
# files, for what Spork IDE does with Java and no build tool.
set -eu
"$(dirname "$0")/fixture-maven.sh" "$1"
cd "$1"
git rm -q -r pom.xml mvnw .mvn
GIT_AUTHOR_DATE=2026-01-01T10:00:00Z GIT_COMMITTER_DATE=2026-01-01T10:00:00Z git commit -q -m "chore: no build tool"
