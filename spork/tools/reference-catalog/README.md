# The reference app catalogue tools

Reads what the reference app shows and does from a running reference app and writes docs/reference-app/ (see its README).

    ./sandbox.sh <scratch>                      # a second reference app, apart from yours, with the robot
    ./robot.py inventory.js OUT=<scratch>/out   # actions, groups, shortcuts, tool windows, Settings
    ./capture.py <scratch>/out                  # frame, toolwindows, menus, settings, dialogs
    ./capture.py <scratch>/out welcome          # last: closes the project for the Welcome window
    ./strings.py <scratch>/out /Applications/Reference.app <zh.jar> <ja.jar> <ko.jar>
    ./render.py <scratch>/out ../../docs/reference-app

The language pack jars are IntelliJ's (plugins 13710, 13964, 13711 on plugins.jetbrains.com,
version 242.x): unzip each and pass its lib/*.jar. Everything a script writes goes to <scratch>/out;
only render.py writes into the repository.

`lib.js` holds the helpers every script runs with inside the reference app (Rhino, with the IntelliJ API at
hand): `dump(component)` for what a component shows, `shot(component, path)` to paint it into a PNG,
`findByClass`, `windows()`, `showLog()`. `capture.py` skips actions that would quit, restart or
leave the sandbox (`DENY`).

## IntelliJ IDEA

The same tools catalogue IntelliJ IDEA Community Edition into reference/idea/ at the monorepo's root,
on the project `ide-fixture.sh` writes (a Java program, a Markdown readme, a git history), with the
app's bundled runtime as its JDK. `capture.py use` runs, debugs (to a breakpoint) and searches it so
the Run, Build, Debug and Find tool windows exist when the inventory is taken; the inventory then
records the plugin each action, tool window and Settings page comes from. The main menu's dialogs
are run in the editor on Main.java, the caret on the class's name, so the editor's and the
refactorings' dialogs open.

    export APP="/Applications/IntelliJ IDEA CE.app" EDITOR_FILE=src/com/example/Main.java \
      CARET_AT="Main {" BREAKPOINT_LINE=10 FIND_AT=src/com/example/Greeting.java:hello\(
    LAUNCHER=idea VMOPTIONS=idea.vmoptions ENV_PREFIX=IDEA FIXTURE=./ide-fixture.sh ./sandbox.sh <scratch>
    ./capture.py <scratch>/out frame use        # once indexing is done
    ./robot.py inventory.js OUT=<scratch>/out
    ./capture.py <scratch>/out toolwindows menus settings dialogs
    ./capture.py <scratch>/out welcome
    ./strings.py <scratch>/out "$APP" "$APP"/Contents/plugins/localization-{zh,ja,ko}/lib/*.jar
    ./render.py <scratch>/out ../../../reference/idea
    ../e2e/tasks.py --ide                       # reference/idea/tasks, over both catalogues

Quit the sandbox by the pid sandbox.sh writes to <scratch>/pid.
