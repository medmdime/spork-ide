#!/usr/bin/env python3
"""Screenshots and control dumps of everything the reference app shows, taken inside the sandboxed
reference app (sandbox.sh) after inventory.js: the main window, each tool window, each menu and
context menu, each Settings page, and each dialog its menus open. For every one: a PNG and a
JSON of the texts and controls on it.

usage: capture.py <out dir> [use|frame|toolwindows|menus|settings|dialogs|welcome ...]

EDITOR_FILE (the project's README.md by default) is the file the editor's context menus are opened
on; when set, the main window is captured with it open, and dialogs are run in it with the caret at
the first CARET_AT text, so the editor's actions (Go to Line, Rename, Extract…) have their context. "use", before inventory.js, opens the tool
windows that appear once used (Run, Build, Debug, Find), for an IDE.
"""

import json
import os
import pathlib
import re
import sys
import time

import robot

HERE = pathlib.Path(__file__).parent
EDITOR_FILE = os.environ.get("EDITOR_FILE", "README.md")


def js(body: str, edt: bool = True) -> None:
    robot.run((HERE / "lib.js").read_text() + "\n" + body, edt)


def later(body: str) -> None:
    """Runs `body` on the event thread once the robot's call returns, holding the write-intent lock
    an action or a project change takes (Swing's invokeLater doesn't, which 252 refuses)."""
    js("IJ.openapi.application.ApplicationManager.getApplication().invokeLater(function () {" + body + "});")


def slug(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "-", text).strip("-").lower() or "unnamed"


def keep(out: pathlib.Path, kind: str, name: str, component_js: str) -> None:
    """Paints the component the JS expression gives, and writes what it shows."""
    base = out / kind / slug(name)
    js(f"""
var notices = IJ.notification.NotificationsManager.getNotificationsManager()
    .getNotificationsOfType(IJ.notification.Notification, project());
for (var n = 0; n < notices.length; n++) notices[n].expire();
var target = {component_js};
if (target !== null) {{
  shot(target, {json.dumps(str(base) + ".png")});
  writeJson({json.dumps(str(base) + ".json")}, {{name: {json.dumps(name)}, ui: dump(target)}});
}}""")


def open_file(path: str) -> str:
    """JS opening `path`, relative to the project, in the editor."""
    return (f'var file = IJ.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(project().getBasePath() + "/" + {json.dumps(path)});'
            ' IJ.openapi.fileEditor.FileEditorManager.getInstance(project()).openFile(file, true);')


def frame(out: pathlib.Path) -> None:
    if "EDITOR_FILE" in os.environ:
        js(open_file(EDITOR_FILE))
        time.sleep(2)
    keep(out, "frame", "main window", "frame().getRootPane()")


def use() -> None:
    """Runs the selected run configuration (the Run and Build tool windows), debugs it to a
    breakpoint on EDITOR_FILE's line BREAKPOINT_LINE (Debug; the session stays suspended until
    "welcome"), and finds the usages of FIND_AT, "file:text", at the text's first place (Find)."""
    run = """
var settings = IJ.execution.RunManager.getInstance(project()).getSelectedConfiguration();
IJ.execution.runners.ExecutionUtil.runConfiguration(settings, IJ.execution.executors.%s.%s());"""
    later(run % ("DefaultRunExecutor", "getRunExecutorInstance"))
    time.sleep(20)
    later(open_file(EDITOR_FILE) + f"""
IJ.xdebugger.XDebuggerUtil.getInstance().toggleLineBreakpoint(project(), file, {int(os.environ["BREAKPOINT_LINE"]) - 1});"""
          + run % ("DefaultDebugExecutor", "getDebugExecutorInstance"))
    time.sleep(20)
    path, text = os.environ["FIND_AT"].split(":", 1)
    later(open_file(path) + f"""
var editor = IJ.openapi.fileEditor.FileEditorManager.getInstance(project()).getSelectedTextEditor();
editor.getCaretModel().moveToOffset(editor.getDocument().getText().indexOf({json.dumps(text)}));
var am = IJ.openapi.actionSystem.ActionManager.getInstance();
am.tryToExecute(am.getAction("FindUsages"), null, editor.getContentComponent(), "EditorPopup", true);""")
    time.sleep(10)


def tool_windows(out: pathlib.Path, inventory: dict) -> None:
    for tw in inventory["toolWindows"]:
        if not tw["available"]:
            continue
        name = json.dumps(tw["id"])
        js(f"IJ.openapi.wm.ToolWindowManager.getInstance(project()).getToolWindow({name}).activate(null);")
        time.sleep(2)
        keep(out, "toolwindows", tw["id"], "frame().getRootPane()")
        js(f"IJ.openapi.wm.ToolWindowManager.getInstance(project()).getToolWindow({name}).hide(null);")
        time.sleep(1)


# The menus a user opens: the main menu's, then the context menus and popups the inventory names
# as such, all shown as IntelliJ shows a context menu.
MENU_GROUP = re.compile(r"(Popup|PopupMenu|ContextMenu|Menu)$|^(Vcs|Git|Log|Changes|Branch)", re.I)

# What a context menu is opened on, so its items are the ones a user sees there: (group ids,
# JS run first to show and select it, JS giving the component). An IDE's tool windows first.
TARGETS = [
    *((re.compile(pattern, re.I), f'toolWindow("{tool_window}", true);',
       f'inToolWindow("{tool_window}", {json.dumps(name)})') for pattern, tool_window, name in [
        (r"^XDebugger\.(Variables|Watches|Inspect|Evaluation)", "Debug", "XDebuggerTree"),
        (r"^ConsoleView|^Runner\.View", "Run", "EditorComponentImpl"),
        (r"^UsageView", "Find", None),
        (r"^StructureView", "Structure", None),
        (r"^ProblemsView", "Problems View", None),
        (r"^ServiceView|^RunDashboard", "Services", None),
        (r"^Bookmarks|BookmarkContextMenu", "Bookmarks", None),
        (r"^CompilerErrorView", "Build", None),
    ]),
    (re.compile(r"^Vcs\.Log\.ChangesBrowser|^Vcs\.RepositoryChangesBrowser|ChangesBrowser", re.I),
     'if (showLog()) toolWindow("Version Control", false); var t = findByClass(frame().getRootPane(), "VcsLogMainGraphTable");'
     ' if (t) t.setRowSelectionInterval(1, 1);',
     '(function () { var log = findByClass(frame().getRootPane(), "VcsLogChangesBrowser");'
     ' var tree = log ? findByClass(log, "Tree") : null; if (tree && tree.getRowCount() > 1)'
     ' tree.setSelectionRow(tree.getRowCount() - 1); return tree; })()'),
    (re.compile(r"^Vcs\.Log|^Git\.Log(?!\.Branches)", re.I),
     'showLog(); var t = findByClass(frame().getRootPane(), "VcsLogMainGraphTable"); if (t) t.setRowSelectionInterval(1, 1);',
     'findByClass(frame().getRootPane(), "VcsLogMainGraphTable")'),
    (re.compile(r"Branches|Branch", re.I),
     'showLog();',
     '(function () { var tree = findByClass(frame().getRootPane(), "BranchesTree"); if (tree && tree.getRowCount() > 3)'
     ' tree.setSelectionRow(3); return tree; })()'),
    (re.compile(r"^ChangesView|^Vcs\.Commit|^LocalChangesView|^Changes\.|Commit", re.I),
     'toolWindow("Commit", true);',
     '(function () { var tree = findByClass(frame().getRootPane(), "ChangesListView"); if (tree && tree.getRowCount() > 1)'
     ' tree.setSelectionRow(1); return tree; })()'),
    (re.compile(r"^ProjectView|^Project", re.I),
     'toolWindow("Project", true);',
     '(function () { var tree = findByClass(frame().getRootPane(), "ProjectViewTree"); if (tree && tree.getRowCount() > 3)'
     ' tree.setSelectionRow(3); return tree; })()'),
    (re.compile(r"Editor|Diff", re.I),
     open_file(EDITOR_FILE),
     'findByClass(frame().getRootPane(), "EditorComponentImpl")'),
    (re.compile(r"Terminal", re.I),
     'toolWindow("Terminal", true);',
     'findByClass(frame().getRootPane(), "Terminal")'),
]


def menus(out: pathlib.Path, inventory: dict, only: str = "") -> None:
    by_id = {a["id"]: a for a in inventory["actions"]}
    main = [c["id"] for c in by_id["MainMenu"].get("children", []) if "id" in c]
    popups = sorted(a["id"] for a in inventory["actions"]
                    if a.get("group") and a.get("children") and MENU_GROUP.search(a["id"]) and a["id"] != "MainMenu")
    for group in main + popups:
        if only and not re.search(only, group):
            continue
        place = "MainMenu" if group in main else "popup"
        setup, target = "", "frame().getRootPane()"
        if group not in main:
            for pattern, prepare, component in TARGETS:
                if pattern.search(group):
                    setup, target = prepare, component
                    break
        if setup:
            js(setup)
            time.sleep(1.5)
        js(f"""
var group = IJ.openapi.actionSystem.ActionManager.getInstance().getAction({json.dumps(group)});
var focusFirst = {target};
if (focusFirst) focusFirst.requestFocusInWindow();""")
        time.sleep(0.5)
        js(f"""
var group = IJ.openapi.actionSystem.ActionManager.getInstance().getAction({json.dumps(group)});
var on = {target} || frame().getRootPane();
on.requestFocusInWindow();
var popup = IJ.openapi.actionSystem.ActionManager.getInstance().createActionPopupMenu({json.dumps(place)}, group);
popup.setTargetComponent(on);
popup.getComponent().show(on, Math.min(200, on.getWidth() / 2), Math.min(40, on.getHeight() / 2));
""")
        time.sleep(2)
        keep(out, "menus", group, """(function () {
  var all = javax.swing.MenuSelectionManager.defaultManager().getSelectedPath();
  return all.length > 0 ? all[0].getComponent() : null;
})()""")
        js("javax.swing.MenuSelectionManager.defaultManager().clearSelectedPath();")
        time.sleep(0.3)


def settings(out: pathlib.Path, inventory: dict) -> None:
    def walk(nodes, path):
        for node in nodes:
            here = path + [node["name"]]
            yield here, node
            yield from walk(node.get("children", []), here)

    for path, node in walk(inventory["settings"], []):
        # by id: names repeat (Appearance, General); a page without an id by its name
        page_id, name = json.dumps(node.get("id")), json.dumps(node["name"])
        js(f"""
javax.swing.SwingUtilities.invokeLater(function () {{
  // an explicit Predicate: a bare function matches several overloads
  var matches = new java.util.function.Predicate({{test: function (c) {{
    var id = c instanceof IJ.openapi.options.SearchableConfigurable ? String(c.getId()) : null;
    return {page_id} !== null ? id === {page_id} : String(c.getDisplayName()) === {name};
  }}}});
  IJ.openapi.options.ShowSettingsUtil.getInstance()["showSettingsDialog(com.intellij.openapi.project.Project,java.util.function.Predicate,java.util.function.Consumer)"](project(), matches, null);
}});""")
        time.sleep(3)
        # The reference app's Settings is a window of its own, not a modal dialog
        keep(out, "settings", " - ".join(path), """(function () {
  var shown = windows();
  for (var i = shown.length - 1; i >= 0; i--)
    if (/^Settings/.test(String(title(shown[i])))) return shown[i].getRootPane();
  return null;
})()""")
        close_dialogs()


def close_dialogs() -> None:
    js("""
var shown = windows();
for (var i = shown.length - 1; i >= 0; i--) {
  // a window of its own (Local History's), closed as its close button would
  if (shown[i] instanceof java.awt.Frame && shown[i] !== frame())
    shown[i].dispatchEvent(new java.awt.event.WindowEvent(shown[i], java.awt.event.WindowEvent.WINDOW_CLOSING));
  if (!(shown[i] instanceof java.awt.Dialog)) continue;
  // macOS's own file chooser: no Swing inside to close it by
  if (shown[i] instanceof java.awt.FileDialog) { shown[i].dispose(); continue; }
  var wrapper = IJ.openapi.ui.DialogWrapper.findInstance(shown[i].getFocusOwner() || shown[i].getRootPane());
  if (wrapper !== null) wrapper.close(IJ.openapi.ui.DialogWrapper.CANCEL_EXIT_CODE);
  else shown[i].dispose();
}
// by the frame, not its root pane: a popup an action shows in the window's middle is the frame's
var popups = IJ.openapi.ui.popup.JBPopupFactory.getInstance().getChildPopups(frame());
for (var p = 0; p < popups.size(); p++) popups.get(p).cancel();
""")
    # and what an action started in the editor without a window (an in-place rename)
    later("""
var editor = IJ.openapi.fileEditor.FileEditorManager.getInstance(project()).getSelectedTextEditor();
var am = IJ.openapi.actionSystem.ActionManager.getInstance();
if (editor !== null) am.tryToExecute(am.getAction("EditorEscape"), null, editor.getContentComponent(), "", true);""")
    time.sleep(1)


# Actions that would leave the sandbox, quit or restart it, or only open a browser; and in an IDE,
# install a plugin, download a JDK, clone or reach a server, log in, or read or write outside the
# sandbox (Delete Leftover IDE Directories, Create Command-line Launcher).
DENY = re.compile(r"Exit|Restart|Invalidate|Help|Feedback|Support|Browse|Web|Tip|Learn|Youtrack|"
                  r"ReportProblem|CloseProject|Close|Quit|ShowLog|RevealIn|OpenFile|NewWindow|"
                  r"RegisterPlugins|InstallPlugin|PluginManager|Marketplace|Jdk|Sdk|Download|Clone|GetFromVcs|"
                  r"Share|Login|LogIn|SignIn|Account|Github|GitLab|Gist|Perforce|Ssh|Remote|CodeWithMe|Cwm|"
                  r"Station|SettingsSync|CheckForUpdate|LauncherScript|DesktopEntry|EditCustom|Memory|"
                  r"DeleteOldAppDirs|RestoreDefault|ImportSettings|Saul|Diagnostic|Profiler|Performance|"
                  r"Vulnerable|DevKit", re.I)


def dialogs(out: pathlib.Path, inventory: dict) -> None:
    """Each action under the main menu whose text ends with "…" (IntelliJ's mark for one that
    asks something first), run as from the menu."""
    by_id = {a["id"]: a for a in inventory["actions"]}
    seen, order = set(), []

    def walk(group_id):
        for child in by_id.get(group_id, {}).get("children", []):
            child_id = child.get("id")
            if not child_id or child_id in seen:
                continue
            seen.add(child_id)
            action = by_id.get(child_id, {})
            if action.get("group"):
                walk(child_id)
            elif (action.get("text") or "").endswith(("…", "...")) and not DENY.search(child_id):
                order.append(action)

    walk("MainMenu")
    # those not kept yet; then again those that showed nothing, longer: a heavy dialog is slow the
    # first time it opens
    for wait in (3, 8):
        for action in order:
            if (out / "dialogs" / f"{slug(action['id'])}.json").exists():
                continue
            try:
                dialog(out, action, wait)
                close_dialogs()
            except RuntimeError as error:
                print(f"   {action['id']}: {error}", flush=True)


def dialog(out: pathlib.Path, action: dict, wait: float = 3) -> None:
    """Runs `action` as from the menu, and keeps the window it opens after `wait` seconds."""
    on = "frame().getRootPane()"
    if "EDITOR_FILE" in os.environ:
        later(open_file(EDITOR_FILE) + f"""
  var editor = IJ.openapi.fileEditor.FileEditorManager.getInstance(project()).getSelectedTextEditor();
  editor.getCaretModel().moveToOffset(Math.max(0, editor.getDocument().getText().indexOf({json.dumps(os.environ.get("CARET_AT", ""))})));""")
        time.sleep(0.5)
        on = "IJ.openapi.fileEditor.FileEditorManager.getInstance(project()).getSelectedTextEditor().getContentComponent()"
    later(f"""
  var am = IJ.openapi.actionSystem.ActionManager.getInstance();
  am.tryToExecute(am.getAction({json.dumps(action["id"])}), null, {on}, "MainMenu", true);""")
    time.sleep(wait)
    keep(out, "dialogs", action["id"], """(function () {
  var shown = windows();
  var top = shown.length > 0 ? shown[shown.length - 1] : null;
  // macOS's own file chooser can't be painted from the inside
  if (top === null || top === frame() || top instanceof java.awt.FileDialog) return null;
  return top.getRootPane ? top.getRootPane() : top;
})()""")


def welcome(out: pathlib.Path) -> None:
    # a session still running would make closing the project ask first
    js("""
var sessions = IJ.xdebugger.XDebuggerManager.getInstance(project()).getDebugSessions();
for (var i = 0; i < sessions.length; i++) sessions[i].stop();""")
    time.sleep(3)
    # an IDE then shows its Welcome window as closing the last project from the menu does
    later("""IJ.openapi.project.ProjectManager.getInstance().closeAndDispose(project());
if (project() === null) IJ.openapi.wm.impl.welcomeScreen.WelcomeFrame.showIfNoProjectOpened();""")
    time.sleep(5)
    keep(out, "frame", "welcome", """(function () {
  var shown = windows();
  return shown.length > 0 ? shown[shown.length - 1].getRootPane() : null;
})()""")


if __name__ == "__main__":
    out = pathlib.Path(sys.argv[1])
    # "use" comes before the inventory
    inventory = json.loads((out / "inventory.json").read_text()) if (out / "inventory.json").exists() else {}
    steps = sys.argv[2:] or ["frame", "toolwindows", "menus", "settings", "dialogs"]
    for step in steps:
        print("==", step, flush=True)
        # the robot plugin's own "Cannot update plugin" balloon isn't the reference app's
        js("""
var shown = IJ.notification.NotificationsManager.getNotificationsManager()
    .getNotificationsOfType(IJ.notification.Notification, project());
for (var i = 0; i < shown.length; i++) shown[i].expire();""")
        {
            "use": use,
            "frame": lambda: frame(out),
            "toolwindows": lambda: tool_windows(out, inventory),
            "menus": lambda: menus(out, inventory),
            "context-menus": lambda: menus(out, inventory, "|".join(p.pattern for p, _, _ in TARGETS)),
            "settings": lambda: settings(out, inventory),
            "dialogs": lambda: dialogs(out, inventory),
            "welcome": lambda: welcome(out),
        }[step]()
