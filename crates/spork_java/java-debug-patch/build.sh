#!/bin/sh
# usage: build.sh <com.microsoft.java.debug.plugin-0.53.1.jar> — compiles JavaDebugServer.java
# (java-debug 0.53.1's, its server bound to the loopback interface) against that jar and its
# embedded core into the .class files beside this script, which jdtls.rs puts into the official
# jar when it installs it. Needs a JDK 11 or newer's javac.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
jar=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
unzip -q "$jar" 'lib/com.microsoft.java.debug.core-*.jar'
javac --release 11 -g -nowarn -cp "$jar:$(echo lib/com.microsoft.java.debug.core-*.jar)" \
    -d out "$here/JavaDebugServer.java"
rm -f "$here"/JavaDebugServer*.class
cp out/com/microsoft/java/debug/plugin/internal/JavaDebugServer*.class "$here/"
ls "$here"/JavaDebugServer*.class
