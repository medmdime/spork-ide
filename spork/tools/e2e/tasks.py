#!/usr/bin/env python3
"""Spork's work list for matching the reference app: one task per functionality of docs/reference-app/catalog.json,
an area a file in the order to build them, each with what the reference app shows (place, text and its
translations, shortcut, items, screenshot), what Spork already has under that text, and the
end-to-end scenarios covering it. A task is ticked only when a scenario covering it passed in the
last run (out/results.json): matching the reference app takes a test, so it can't regress unseen.

Writes docs/reference-app/tasks/; exits 1 when a scenario names a functionality the catalogue doesn't
have, so a typo can't count as done.

Spork IDE's list is the same over two catalogues, IntelliJ IDEA's and the reference app's: one task per
functionality either has, the first catalogue given deciding where both have it, each saying the
plugin it comes from; ticked by the IDE's harness (tools/e2e at the monorepo's root), its areas in
the roadmap's phase order. `--ide` gives all of these.

usage: tasks.py [--ide] [--catalog <catalog.json> ...] [--scenarios <dir>] [--results <results.json>]
                [--out <tasks dir>] [--root <product's source tree>] [--product <name>] [--order <areas>]
"""

import argparse
import json
import os
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
# the monorepo: Spork IDE and Spork's crates
REPO = ROOT.parent
DOCS = ROOT / "docs" / "reference-app"
TASKS = DOCS / "tasks"
IDE = {"catalog": [DOCS / "catalog.json", REPO / "reference" / "idea" / "catalog.json"],
       "scenarios": REPO / "tools" / "e2e" / "scenarios",
       "results": REPO / "tools" / "e2e" / "out" / "results.json",
       "out": REPO / "reference" / "idea" / "tasks", "root": REPO, "sources": "crates/*/src/**/*.rs",
       "product": "Spork IDE",
       # the roadmap's phases: the frame, navigation, the editor, files, code, run, settings, git
       "order": "window,window-menu,view,help,navigate,editor,edit,file,code,refactor,run,build,tools,"
                "settings,git,other"}
LANGUAGES = [("zh", "中文"), ("ja", "日本語"), ("ko", "한국어")]

# (key, file, title, what it holds), in the order to build: the frame first, then what a git
# client is for, then the rest of the menu bar
AREAS = [
    ("window", "01-main-window.md", "Main window", "The frame, the main toolbar, the tool windows and their stripes"),
    ("git", "02-git.md", "Git", "The Git menu, the log, commit, branches, changes, shelf and stash, diff, the hosting "
     "services, Settings › Version Control and the dialogs they open"),
    ("settings", "03-settings.md", "Settings", "Every Settings page but Version Control's"),
    ("file", "04-file.md", "File menu", ""),
    ("edit", "05-edit.md", "Edit menu", ""),
    ("view", "06-view.md", "View menu", ""),
    ("navigate", "07-navigate.md", "Navigate menu", "The Navigate menu, with bookmarks, hierarchies and usages"),
    ("window-menu", "08-window-menu.md", "Window menu", ""),
    ("help", "09-help.md", "Help menu", ""),
    ("code", "10-code.md", "Code menu", ""),
    ("refactor", "11-refactor.md", "Refactor menu", ""),
    ("build", "12-build.md", "Build menu", ""),
    ("run", "13-run.md", "Run menu", "The Run menu, with the debugger, run configurations and consoles"),
    ("tools", "14-tools.md", "Tools menu", ""),
    ("editor", "15-editor.md", "Editor", "The editor's context menus, gutter and floating toolbars"),
    ("other", "16-other.md", "Everything else", "Popups no menu leads to, actions run by a shortcut only, and "
     "actions {apps} registers but shows nowhere (Find Action still lists them)"),
]
MENUS = {"File": "file", "Edit": "edit", "View": "view", "Navigate": "navigate", "Code": "code",
         "Refactor": "refactor", "Build": "build", "Run": "run", "Tools": "tools", "Git": "git",
         "Window": "window-menu", "Help": "help"}
# by id, for what no menu leads to
BY_ID = [
    ("git", re.compile(r"(?i)^(git|github|gitlab|vcs|dvcs|log\.|changes|activityview|shelv|patch|diff|dirdiff|"
                       r"unversioned|repositorychanges|committedchanges|incomingchanges|code\.review|blankdiff|"
                       r"editorguttervcs|frontend\.shelved|start\.use\.vcs|localhistory|checkinproject|"
                       r"compare\.selected|maintoolbarvcs|maintoolbarquickactions\.vcs|segmentedvcs)")),
    ("window", re.compile(r"(?i)^(activate\w*toolwindow|main ?tool ?bar|navbar|toolbar|toolwindow|projectview|lefttoolbarside|"
                          r"righttoolbarside|newtoolbaractions|customizemaintoolbar|viewtoolbaractions|"
                          r"ideupdatetoolbarwidget|terminal\.|editortab)")),
    ("run", re.compile(r"(?i)^(xdebugger|debug|run|executiontargets|chooserun|choosedebug|editrunconfigurations|"
                       r"viewbreakpoints|evaluateexpression|buildview|externalsystem|console|logdebugconfigure|"
                       r"analyzestacktrace|unscramble)")),
    ("navigate", re.compile(r"(?i)^(goto|callhierarchy|methodhierarchy|typehierarchy|bookmarks|popup@|showbookmarks|"
                            r"editbookmark|togglebookmark|showtypebookmarks|usageview|findinfiles|selectin)")),
    ("editor", re.compile(r"(?i)^(editor|basiceditor|textviewer|floating\.|inlay|doccomment|images\.|structureview|"
                          r"lighteditmode|arrangement)")),
]
GIT_TOOL_WINDOWS = {"Commit", "Version Control", "Pull Requests", "Merge Requests"}
# an IDE's tool windows the reference app doesn't have, by the menu they belong with; the rest are the frame's
TOOL_WINDOWS = {"Run": "run", "Debug": "run", "Build": "run", "Services": "run", "Coverage": "run",
                "Find": "navigate", "Structure": "navigate", "Problems View": "code"}
KINDS = [("window", "Windows"), ("tool-window", "Tool windows"), ("menu", "Menus"), ("toolbar", "Toolbars"),
         ("context-menu", "Context menus and popups"), ("dialog", "Dialogs"), ("settings", "Settings pages"),
         ("action", "Actions")]
MAX_PLACES = 8
# the spec page with every detail of a kind
SPEC = {"window": "windows.md", "tool-window": "tool-windows.md", "menu": "menus.md", "toolbar": "toolbars.md",
        "context-menu": "context-menus.md", "dialog": "dialogs.md", "settings": "settings.md", "action": "actions.md"}


def area(entry: dict) -> str:
    kind, key = entry["kind"], entry["id"].split(":", 1)[1]
    if kind == "window":
        return "window"
    if kind == "tool-window":
        return "git" if key in GIT_TOOL_WINDOWS else TOOL_WINDOWS.get(key, "window")
    if kind == "settings":
        return "git" if key.startswith("Version Control") else "settings"
    if kind == "menu":
        return MENUS.get(entry["name"], "other")
    places = entry.get("places", [])
    roots = [p.split(" ▸ ")[0] for p in places]
    # the menu bar decides first (Copy is Edit's though the diff's popup has it too), Git's menu above all
    if "Git" in roots:
        return "git"
    for root in roots:
        if root in MENUS:
            return MENUS[root]
    for name, pattern in BY_ID:
        if pattern.match(key) or any(pattern.match(r.strip("`")) for r in roots):
            return name
    return "other"


def normal(text: str) -> str:
    return re.sub(r"\s+", " ", text.replace("...", "…")).strip().rstrip("…").strip().lower()


def spork_texts(sources: str) -> dict[str, str]:
    """Every string literal in the product's sources (a glob in the monorepo), normalized, to its first `file:line`."""
    found: dict[str, str] = {}
    for path in sorted(p for p in REPO.glob(sources) if "spork_e2e" not in p.parts):
        for number, line in enumerate(path.read_text().splitlines(), 1):
            for literal in re.findall(r'"((?:[^"\\]|\\.)+)"', line):
                found.setdefault(normal(literal), f"{path.relative_to(REPO)}:{number}")
    return found


def spork_pages() -> tuple[dict[str, str], dict[str, str]]:
    """The Settings pages Spork builds, by the path (or the end of it) its builders match, and the
    names its Settings tree lists; each to its `file:line`."""
    built: dict[str, str] = {}
    listed: dict[str, str] = {}
    for path in sorted(REPO.glob("crates/spork/src/settings*.rs")):
        source = path.read_text()
        for pattern, into in ((r'"([^"]+)"\s*(?:=>|,\s*Page::)|==\s*"([^"]+)"', built),
                              (r'Node::(?:page|group)\(\s*"([^"]+)"', listed)):
            for found in re.finditer(pattern, source):
                line = source.count("\n", 0, found.start()) + 1
                into.setdefault("".join(g for g in found.groups() if g), f"{path.relative_to(REPO)}:{line}")
    return built, listed


def text_of(entry: dict) -> str | None:
    """The text a user reads for `entry`, None when the reference app gives it only an id (`Vcs.Log.Toolbar`)."""
    name = entry["name"].split(" › ")[-1]
    id_like = re.fullmatch(r"\S*(\.|[a-z][A-Z])\S*", name)
    return None if name == entry["id"].split(":", 1)[1] and id_like else name


def link(where: str, out: pathlib.Path) -> str:
    file, line = where.rsplit(":", 1)
    return f"[{where}]({os.path.relpath(REPO / file, out)}#L{line})"


class Lists:
    """What the pages are written from: the catalogues, merged, and what the product has."""

    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.catalogs = [(path.parent, json.loads(path.read_text())) for path in args.catalog]
        self.apps = [catalog.get("product", "the reference app") for _, catalog in self.catalogs]
        self.plugin_names = {k: v for _, catalog in self.catalogs for k, v in catalog.get("plugins", {}).items()}
        # where an entry comes from: its catalogue's folder (its spec pages) and app
        self.origin: dict[str, tuple[pathlib.Path, str]] = {}
        first: dict[str, dict] = {}
        for (docs, catalog), app in zip(self.catalogs, self.apps):
            for entry in catalog["functionalities"]:
                if entry["id"] not in first:
                    first[entry["id"]] = entry
                    self.origin[entry["id"]] = (docs, app)
        last = {e["id"]: e for e in self.catalogs[-1][1]["functionalities"]}
        # a catalogue without plugins (the reference app's) takes them from another's entry for the same id,
        # or the same action under another kind (a dialog is its action's)
        plugins: dict[str, str] = {}
        for _, catalog in self.catalogs:
            for entry in catalog["functionalities"]:
                if "plugin" in entry:
                    plugins.setdefault(entry["id"], entry["plugin"])
                    plugins.setdefault(entry["id"].split(":", 1)[1], entry["plugin"])
        self.entries = []
        # in the last catalogue's order (the largest), then the ids only the others have
        for entry_id in dict.fromkeys(e["id"] for _, c in reversed(self.catalogs) for e in c["functionalities"]):
            entry = first[entry_id]
            if self.plugin_names and "plugin" not in entry:
                found = plugins.get(entry_id) or plugins.get(entry_id.split(":", 1)[1])
                entry = dict(entry, **({"plugin": found} if found else {}))
            if "menu_order" in last.get(entry_id, {}):
                entry = dict(entry, menu_order=last[entry_id]["menu_order"])
            self.entries.append(entry)
        self.spork, self.pages = spork_texts(args.sources), spork_pages()

    def plugin(self, entry: dict) -> str:
        plugin = entry.get("plugin")
        if plugin is None:
            return f"not recorded ({self.origin[entry['id']][1]} only)"
        if plugin == "com.intellij":
            return "platform"
        return f"{self.plugin_names.get(plugin, plugin)} (`{plugin}`)"

    def task(self, entry: dict, covered: dict[str, list[str]], results: dict) -> tuple[str, bool, bool]:
        """The task's markdown, whether the product has its text and whether a passing test covers it."""
        out, product = self.args.out, self.args.product
        docs, app = self.origin[entry["id"]]
        by = covered.get(entry["id"], [])
        passed = bool(by) and all(results.get(s) for s in by)
        text = text_of(entry)
        title = f"**{text}**" if text else f"`{entry['name']}`"
        lines = [f"- [{'x' if passed else ' '}] {title} · `{entry['id']}`"]
        places = entry.get("places", [])
        if places:
            more = f" and {len(places) - MAX_PLACES} more" if len(places) > MAX_PLACES else ""
            lines.append(f"  - {app}: {'; '.join(places[:MAX_PLACES])}{more}")
        elif entry["kind"] == "action":
            lines.append(f"  - {app}: " + ("its shortcut only" if entry.get("shortcut") else "no menu or toolbar; Find Action"))
        if self.plugin_names:
            lines.append(f"  - Plugin: {self.plugin(entry)}")
        if entry.get("shortcut"):
            lines.append(f"  - Shortcut: {entry['shortcut']}")
        if entry.get("tr"):
            lines.append("  - Translations: " + " · ".join(f"{name} {entry['tr'][code]}" for code, name in LANGUAGES
                                                          if code in entry["tr"]))
        if entry.get("description") and entry["description"] != text:
            lines.append(f"  - Does: {entry['description']}")
        if entry.get("shown"):
            lines.append(f"  - Shown on its usual target: {' · '.join(entry['shown'])}")
        if entry.get("contents"):
            lines.append(f"  - Items: {' · '.join(entry['contents'])}")
        if entry.get("controls"):
            lines.append(f"  - Controls: {entry['controls']}")
        spec = SPEC[entry["kind"]]
        lines.append(f"  - Spec: [{spec}]({os.path.relpath(docs / spec, out)})")
        if entry.get("screenshot"):
            lines.append(f"  - Screenshot: [{entry['screenshot'].split('/')[-1]}]({os.path.relpath(docs / entry['screenshot'], out)})")
        if entry["kind"] == "settings":
            steps = entry["name"].split(" › ")
            built = next((self.pages[0][s] for s in ("/".join(steps[i:]) for i in range(len(steps)))
                          if s in self.pages[0]), None)
            where = built or self.pages[1].get(steps[-1])
            if built:
                lines.append(f"  - {product}: builds this page at {link(built, out)}: check every control "
                             f"against {app}'s")
            elif where:
                lines.append(f"  - {product}: lists it in its Settings tree at {link(where, out)}: check whether "
                             "it has every control")
        else:
            where = self.spork.get(normal(text)) if text and len(normal(text)) > 1 else None
            if where:
                lines.append(f"  - {product}: has “{text}” at {link(where, out)}: check its place, shortcut, "
                             f"behavior and look against {app}'s")
        if not where:
            lines.append(f"  - {product}: nothing under this text yet")
        lines.append("  - Tests: " + (", ".join(f"{s} ({'pass' if results.get(s) else 'fail' if s in results else 'not run'})"
                                                for s in by) if by else "none yet"))
        return "\n".join(lines), bool(where), passed


def arguments(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Writes the task pages from catalogues and test results.")
    parser.add_argument("--ide", action="store_true", help="Spork IDE's list: every default below is the IDE's")
    parser.add_argument("--catalog", type=pathlib.Path, action="append",
                        help="a catalog.json; several are merged, the first deciding where they share an id")
    parser.add_argument("--scenarios", type=pathlib.Path, help="the scenarios whose covers tick tasks")
    parser.add_argument("--results", type=pathlib.Path, help="the last run's results.json")
    parser.add_argument("--out", type=pathlib.Path, help="the folder the pages are written to")
    parser.add_argument("--root", type=pathlib.Path, help="the folder the pages' paths are written from")
    parser.add_argument("--sources", help="a glob in the monorepo of the product's sources, searched for its texts")
    parser.add_argument("--product", help="the product's name")
    parser.add_argument("--order", help="the areas' keys, comma-separated, in the order to build them")
    args = parser.parse_args(argv)
    defaults = IDE if args.ide else {
        "catalog": [DOCS / "catalog.json"], "scenarios": HERE / "scenarios", "results": HERE / "out" / "results.json",
        "out": TASKS, "root": ROOT, "sources": "crates/spork*/src/**/*.rs", "product": "Spork", "order": ",".join(key for key, *_ in AREAS)}
    for key, value in defaults.items():
        if getattr(args, key) is None:
            setattr(args, key, value)
    for key in ("scenarios", "results", "out", "root"):
        setattr(args, key, getattr(args, key).resolve())
    args.catalog = [path.resolve() for path in args.catalog]
    return args


def main(argv: list[str]) -> int:
    args = arguments(argv)
    lists = Lists(args)
    entries, apps, product, out = lists.entries, lists.apps, args.product, args.out
    known = {entry["id"] for entry in entries}
    scenarios = [json.loads(p.read_text()) for p in sorted(args.scenarios.glob("*.json"))]
    results = {r["id"]: r["ok"] for r in json.loads(args.results.read_text())} if args.results.exists() else {}
    covered: dict[str, list[str]] = {}
    unknown = []
    for scenario in scenarios:
        for entry_id in scenario.get("covers", []):
            if entry_id not in known:
                unknown.append(f"{scenario['id']}: {entry_id}")
            covered.setdefault(entry_id, []).append(scenario["id"])

    # the apps as they read in a sentence: IntelliJ IDEA's first, the reference app last
    apps_or, apps_and = " or ".join(apps[::-1]), " and ".join(apps[::-1])
    areas = {key: (file, title, holds.format(apps=apps_or)) for key, file, title, holds in AREAS}
    order = args.order.split(",")
    assert sorted(order) == sorted(areas), f"--order names each area once: {sorted(areas)}"
    out.mkdir(parents=True, exist_ok=True)
    for stale in out.glob("*.md"):
        stale.unlink()
    summary, by_kind = [], {kind: [0, 0, 0] for kind, _ in KINDS}
    by_plugin: dict[str, list[int]] = {}
    for number, key in enumerate(order, 1):
        file, title, holds = areas[key]
        file = f"{number:02d}-{file.split('-', 1)[1]}"
        mine = [e for e in entries if area(e) == key]
        parts = [f"# {title}\n", (f"{holds}. " if holds else "") +
                 "One task per functionality; see [README](README.md) for how to do one.\n"]
        has = done = 0
        for kind, heading in KINDS:
            of_kind = [e for e in mine if e["kind"] == kind]
            if kind == "action":
                of_kind.sort(key=lambda e: (e.get("menu_order", 1 << 30), e.get("places", ["~"])[0] if e.get("places") else "~", e["name"].lower()))
            if not of_kind:
                continue
            parts.append(f"\n## {heading} ({len(of_kind)})\n")
            for entry in of_kind:
                text, found, passed = lists.task(entry, covered, results)
                parts.append(text)
                has, done = has + found, done + passed
                by_kind[kind][0] += 1
                by_kind[kind][1] += entry["id"] in covered
                by_kind[kind][2] += passed
                counts = by_plugin.setdefault(lists.plugin(entry), [0, 0])
                counts[0] += 1
                counts[1] += passed
        (out / file).write_text("\n".join(parts) + "\n")
        summary.append(f"| [{title}]({file}) | {len(mine)} | {has} | {sum(e['id'] in covered for e in mine)} | {done} |")

    total = len(entries)
    # an area() key missing from AREAS would drop its tasks from every page
    assert sum(by_kind[kind][0] for kind, _ in KINDS) == total, "a functionality fell outside every area"
    script = os.path.relpath(HERE / "tasks.py", args.root) + (" --ide" if args.ide else "")
    run = os.path.relpath(args.scenarios.parent / "run.py", args.root)
    if len(lists.catalogs) == 1:
        catalog = lists.catalogs[0][1]
        source = (f"One task per functionality {apps[0]} {catalog['reference-app']} ({catalog['build']}) has, {total} in all, "
                  f"from [catalog.json]({os.path.relpath(lists.catalogs[0][0] / 'catalog.json', out)}), an area a page in the "
                  "order to build them.")
    else:
        shared = sum(1 for e in entries if sum(e["id"] in {f["id"] for f in c["functionalities"]}
                                                for _, c in lists.catalogs) > 1)
        versions = " or ".join(f"{app} {c.get('version') or c['reference-app']} ({c['build']})"
                               for app, (_, c) in reversed(list(zip(apps, lists.catalogs))))
        links = ", ".join(f"[{app}'s]({os.path.relpath(docs / 'catalog.json', out)})"
                          for app, (docs, _) in reversed(list(zip(apps, lists.catalogs))))
        source = (f"One task per functionality {versions} has, {total} in all, from their catalogues ({links}); "
                  f"{shared} are in both, where {apps[0]}'s entry decides. Each names the plugin it comes from. A page "
                  "an area, in the order to build them.")
    readme = [
        f"# Matching {apps_and}: the tasks\n",
        source + f" Written by `{script}`; don't edit the pages, they are rewritten.\n",
        f"Each task says where {apps_or} shows the functionality, its text and translations, its shortcut, its items "
        f"or controls and its screenshot (the spec pages beside this folder have every detail); what {product} already "
        "has under that text, a place to start from that doesn't mean it matches; and the end-to-end scenarios "
        "testing it.\n",
        "## Doing a task\n",
        f"1. Read it in {apps_or}: the screenshot, the spec page, and "
        f"{apps[0] if len(apps) == 1 else 'the app'} itself for the behavior.",
        f"2. Build it in {product} the same: place, text, shortcut, behavior and look.",
        f"3. Test it: name its id in a scenario's `covers` ({os.path.relpath(args.scenarios, args.root)}) with steps that "
        "check it.",
        f"4. Run `{run}`, then `{script}`: the task is ticked once its scenarios pass, and a "
        "later failure unticks it.\n",
        "## Progress\n",
        f"| Area | Tasks | {product} has the text | Tested | Passing |", "| --- | --- | --- | --- | --- |",
        *summary,
        f"| **All** | {total} | | {sum(v[1] for v in by_kind.values())} | {sum(v[2] for v in by_kind.values())} |",
        "\n| Kind | Tasks | Tested | Passing |", "| --- | --- | --- | --- |",
        *[f"| {heading} | {by_kind[kind][0]} | {by_kind[kind][1]} | {by_kind[kind][2]} |" for kind, heading in KINDS],
    ]
    if lists.plugin_names:
        readme += ["\n| Plugin | Tasks | Passing |", "| --- | --- | --- |",
                   *[f"| {plugin} | {n} | {ok} |" for plugin, (n, ok)
                     in sorted(by_plugin.items(), key=lambda item: (-item[1][0], item[0]))]]
    if unknown:
        readme += ["\n## Unknown ids in scenarios\n", *[f"- {u}" for u in unknown]]
    (out / "README.md").write_text("\n".join(readme) + "\n")
    print(f"{total} tasks, {len(covered)} tested;", f"unknown: {unknown}" if unknown else "no unknown ids")
    return 1 if unknown else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
