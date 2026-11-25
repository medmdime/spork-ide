#!/bin/sh
# usage: build.sh <folder of jars> — compiles the test runner (src/, GPL-3.0-or-later) into the
# .class files of classes/, which testing.rs writes into the data folder for a test run. It
# compiles against JUnit 4.13.2, the JUnit Platform 1.11 launcher and TestNG 7.10 (Maven
# Central's jars in that folder: junit, hamcrest-core, junit-platform-launcher, -engine, -commons,
# opentest4j, apiguardian-api, testng, jcommander, slf4j-api), and links to any later version of
# each at run time: it calls only their long-stable API. Needs a JDK 11 or newer's javac.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
jars=$(cd "$1" && pwd)
classpath=$(ls "$jars"/*.jar | tr '\n' ':')
rm -rf "$here/classes"
javac --release 11 -g -nowarn -cp "$classpath" -d "$here/classes" "$here"/src/spork/tests/*.java
ls "$here"/classes/spork/tests
