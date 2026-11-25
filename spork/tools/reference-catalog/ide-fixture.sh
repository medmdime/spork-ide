#!/bin/sh
# usage: ide-fixture.sh <dir> [<sandbox config dir>] — the project the IDE catalogue opens in
# IntelliJ IDEA: a small Java program (a `main`, so the editor shows its run icons, and a run
# configuration for it) with a Markdown readme, as an IntelliJ project, in a git repository whose
# every commit and date is fixed. Given the sandbox's config folder, it registers the app's bundled
# runtime ($APP) there as the project's JDK, so nothing is detected or downloaded.
#
# Not tools/e2e/fixture.sh: that is a Rust project the IDE's own screenshots are of, and IntelliJ
# needs a Java module, its project files and a JDK it knows to show its Java surfaces.
set -eu
dir=$1
config=${2:-}
jdk=${APP:-/Applications/IntelliJ IDEA CE.app}/Contents/jbr/Contents/Home
rm -rf "$dir"
mkdir -p "$dir/src/com/example" "$dir/.idea/runConfigurations"
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

cat > README.md <<'MD'
# Fixture

A small Java program that greets whoever is named on the command line.

## Running it

1. Open `src/com/example/Main.java`.
2. Click the run icon beside `main`, or choose the **Main** run configuration.

```sh
java -cp out/production/fixture com.example.Main Ada
```

| Argument | Greeting |
| --- | --- |
| *(none)* | Hello, world! |
| `Ada` | Hello, Ada! |
MD
printf 'out/\n.idea/workspace.xml\n' > .gitignore
git add README.md .gitignore && commit "docs: start the readme"

cat > src/com/example/Greeting.java <<'JAVA'
package com.example;

/** The greeting {@link Main} prints. */
public final class Greeting {
    private Greeting() {
    }

    public static String hello(String name) {
        return "Hello, " + name + "!";
    }
}
JAVA
cat > src/com/example/Main.java <<'JAVA'
package com.example;

import java.util.List;

/** Greets whoever is named on the command line, or the world. */
public class Main {
    public static void main(String[] args) {
        List<String> names = args.length > 0 ? List.of(args) : List.of("world");
        for (String name : names) {
            System.out.println(Greeting.hello(name));
        }
    }
}
JAVA
cat > fixture.iml <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<module type="JAVA_MODULE" version="4">
  <component name="NewModuleRootManager" inherit-compiler-output="true">
    <exclude-output />
    <content url="file://$MODULE_DIR$">
      <sourceFolder url="file://$MODULE_DIR$/src" isTestSource="false" />
    </content>
    <orderEntry type="inheritedJdk" />
    <orderEntry type="sourceFolder" forTests="false" />
  </component>
</module>
XML
cat > .idea/misc.xml <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectRootManager" version="2" languageLevel="JDK_21" default="true" project-jdk-name="21" project-jdk-type="JavaSDK">
    <output url="file://$PROJECT_DIR$/out" />
  </component>
</project>
XML
cat > .idea/modules.xml <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="ProjectModuleManager">
    <modules>
      <module fileurl="file://$PROJECT_DIR$/fixture.iml" filepath="$PROJECT_DIR$/fixture.iml" />
    </modules>
  </component>
</project>
XML
cat > .idea/vcs.xml <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="VcsDirectoryMappings">
    <mapping directory="" vcs="Git" />
  </component>
</project>
XML
cat > .idea/runConfigurations/Main.xml <<'XML'
<component name="ProjectRunConfigurationManager">
  <configuration default="false" name="Main" type="Application" factoryName="Application">
    <option name="MAIN_CLASS_NAME" value="com.example.Main" />
    <module name="fixture" />
    <method v="2">
      <option name="Make" enabled="true" />
    </method>
  </configuration>
</component>
XML
git add . && commit "feat: greet the world"

git checkout -q -b feature/farewell
sed -i '' 's/"Hello, "/"Goodbye, "/' src/com/example/Greeting.java
git add . && commit "feat: say goodbye instead"
git checkout -q main
echo "Run it with no argument to greet the world." >> README.md
git add README.md && commit "docs: the default greeting"
git tag v1.0

# opened with Main.java in the editor and the Main run configuration chosen
cat > .idea/workspace.xml <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<project version="4">
  <component name="FileEditorManager">
    <leaf>
      <file current="true" pinned="false">
        <entry file="file://$PROJECT_DIR$/src/com/example/Main.java">
          <provider selected="true" editor-type-id="text-editor" />
        </entry>
      </file>
    </leaf>
  </component>
  <component name="RunManager" selected="Application.Main" />
</project>
XML
# an uncommitted change, for the Commit tool window and the editor's change markers
sed -i '' 's/or the world/or the whole world/' src/com/example/Main.java

if [ -n "$config" ]; then
  mkdir -p "$config/options"
  {
    printf '<application>\n  <component name="ProjectJdkTable">\n    <jdk version="2">\n'
    printf '      <name value="21" />\n      <type value="JavaSDK" />\n      <homePath value="%s" />\n' "$jdk"
    printf '      <roots>\n        <annotationsPath><root type="composite" /></annotationsPath>\n'
    printf '        <classPath>\n          <root type="composite">\n'
    "$jdk/bin/java" --list-modules | sed 's/@.*//' | while read -r module; do
      printf '            <root url="jrt://%s!/%s" type="simple" />\n' "$jdk" "$module"
    done
    printf '          </root>\n        </classPath>\n'
    printf '        <javadocPath><root type="composite" /></javadocPath>\n'
    printf '        <sourcePath><root type="composite" /></sourcePath>\n'
    printf '      </roots>\n      <additional />\n    </jdk>\n  </component>\n</application>\n'
  } > "$config/options/jdk.table.xml"
fi
