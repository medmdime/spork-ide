#!/bin/sh
# usage: build.sh <folder of jars> — compiles Spork IDE's coverage helper (src/, GPL-3.0-or-later)
# into the .class files of classes/, which coverage.rs writes into the data folder: `Data` reads a
# run's coverage, `HtmlReport` writes the HTML report. It compiles against IntelliJ's coverage
# agent and reporter (Maven Central's intellij-coverage-agent and intellij-coverage-reporter
# 1.0.777 in that folder) and runs on the same versions, which coverage.rs downloads. Needs a JDK
# 11 or newer's javac.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
jars=$(cd "$1" && pwd)
classpath=$(ls "$jars"/*.jar | tr '\n' ':')
rm -rf "$here/classes"
javac --release 11 -g -nowarn -cp "$classpath" -d "$here/classes" "$here"/src/spork/coverage/*.java
ls "$here"/classes/spork/coverage
