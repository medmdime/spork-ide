#!/bin/sh
# usage: fixture-copy.sh <dir> <name> — fixtures/<name> (the Kotlin scenarios' projects) in a git
# repository whose commit and date are fixed, as fixture.sh's; a Gradle project gets
# fixtures/gradle's wrapper, a Maven one fixtures/maven's.
set -eu
dir=$1
here=$(dirname "$0")
rm -rf "$dir"
mkdir -p "$dir"
cp -R "$here/fixtures/$2/." "$dir"
if [ -f "$dir/settings.gradle" ] || [ -f "$dir/settings.gradle.kts" ]; then
    cp "$here/fixtures/gradle/gradlew" "$dir/"
    mkdir -p "$dir/gradle"
    cp -R "$here/fixtures/gradle/gradle/wrapper" "$dir/gradle/"
fi
if [ -f "$dir/pom.xml" ]; then
    cp "$here/fixtures/maven/mvnw" "$dir/"
    cp -R "$here/fixtures/maven/.mvn" "$dir/"
fi
cd "$dir"
git init -q -b main
git config user.name "Ada Lovelace"
git config user.email ada@example.com
git config commit.gpgsign false
git add .
GIT_AUTHOR_DATE=2026-01-01T10:00:00Z GIT_COMMITTER_DATE=2026-01-01T10:00:00Z git commit -q -m "feat: greet the world"
