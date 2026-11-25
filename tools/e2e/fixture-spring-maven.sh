#!/bin/sh
# usage: fixture-spring-maven.sh <dir> — the Spring Boot scenarios' Maven fixture:
# fixtures/spring-maven (a web application with Actuator, its mvnw Apache Maven Wrapper 3.3.2's,
# Apache-2.0) in a git repository whose commit and date are fixed, as fixture.sh's.
set -eu
dir=$1
rm -rf "$dir"
mkdir -p "$dir"
cp -R "$(dirname "$0")/fixtures/spring-maven/." "$dir"
cd "$dir"
git init -q -b main
git config user.name "Ada Lovelace"
git config user.email ada@example.com
git config commit.gpgsign false
git add .
GIT_AUTHOR_DATE=2026-01-01T10:00:00Z GIT_COMMITTER_DATE=2026-01-01T10:00:00Z git commit -q -m "feat: greet the world"
