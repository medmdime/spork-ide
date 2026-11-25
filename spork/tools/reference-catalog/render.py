#!/usr/bin/env python3
"""Writes the catalogue of the reference app's functionality from what inventory.js, capture.py and
strings.py gathered: a page per area with each functionality's texts (and their translations),
its controls and its screenshot, and catalog.json, one entry per functionality with a stable id,
its place in the reference app and its translations, which tools/e2e/tasks.py turns into Spork's work list.
Another app's (IntelliJ IDEA's) is written the same, each entry with the plugin it comes from, as
the reference for Spork IDE.

usage: render.py <out dir> <docs dir>
"""

import json
import pathlib
import re
import shutil
import subprocess
import sys

# Full-window screenshots are shrunk to this width and kept as JPEG to keep the repository light;
# menus and small dialogs stay PNG at the screen's scale.
MAX_WIDTH = 1600
JPEG_FROM_WIDTH = 1000
LANGUAGES = [("zh", "中文"), ("ja", "日本語"), ("ko", "한국어")]
PLATFORM = "com.intellij"


def slug(text: str) -> str:
    return re.sub(r"[^A-Za-z0-9]+", "-", text).strip("-").lower() or "unnamed"


def plain(text: str | None) -> str:
    return re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", text or "")).strip()


README = """# {app}, catalogued

Everything {app} {version} ({build}) shows and does, read from {app} itself, as the reference
{product} is rebuilt against: each functionality with its texts, its translations, its controls and a
screenshot. [tasks/](tasks/README.md) turns it into {product}'s work list, one task per functionality.

| Kind | How many |
| --- | --- |
{counts}

- [Main menu](menus.md): the menu bar, every item, shortcut ({keymap} keymap) and action id.
- [Context menus and popups](context-menus.md): each as shown on its usual target, then all its items.
- [Settings](settings.md): every page, every control with its default, a screenshot each.
- [Tool windows](tool-windows.md), [dialogs](dialogs.md) (what each "…" item of the main menu opens),
  [windows](windows.md) (main, Welcome).
- [Toolbars](toolbars.md): the main toolbar's widgets and every toolbar, item by item.
- [Actions](actions.md): all actions, with text, shortcut, description and the groups they are in.
- [catalog.json](catalog.json): one entry per functionality with a stable id (`kind:id`), for tests.
- [tasks/](tasks/README.md): one task per functionality for {product}, ticked when a test proves it.
{plugins}
## Texts and translations

{texts} distinct texts are traced back to their key in {app}'s message bundles. {packs}

## How it was made, and again

The catalogue is read from a running {app}, not from the screen: `{tools}/sandbox.sh`
starts a second {app} with its own settings, plugins and logs in a scratch folder (yours are never
touched), carrying JetBrains' Remote Robot server, the plugin IntelliJ's UI tests use. Scripts then
run inside it: `inventory.js` lists actions, groups, shortcuts, tool windows and the Settings tree;
`capture.py` opens each menu, tool window, Settings page and dialog and paints it into a PNG from the
inside (no screen recording) with a dump of its controls; `strings.py` traces the texts to their
bundle keys and translations; `render.py` writes these pages. See {tools}/README.md.

Screenshots wider than 1600 px are shrunk; menus and dialogs keep the screen's scale. Menus are
drawn as IntelliJ draws a context menu (the menu bar itself is macOS's). Context menus are opened
on their usual target, but the sandbox window doesn't have the focus, so some items show disabled
that a user would see enabled; their texts and order are right.
"""


# what a catalogued app is the reference for, where the tools are, where its translations come from
REFERENCE_PROFILE = {"product": "Spork", "tools": "tools/reference-catalog", "packs": """The reference app ships
English only; {translated} of those texts have Chinese, Japanese or Korean translations, taken from
IntelliJ's language packs key by key. The packs are from 2024.2, the last JetBrains published as
plugins (later IDEs build them in), so a few may be dated or missing. They show as small text after
each English one."""}
IDE_PROFILE = {"product": "Spork IDE", "tools": "spork/tools/reference-catalog", "packs": """It bundles its
Chinese, Japanese and Korean language packs (plugins/localization-zh, -ja and -ko); {translated} of those
texts have a translation there, taken key by key. They show as small text after each English one."""}

PLUGINS = """
## Plugins

Each functionality names the plugin it comes from: `com.intellij` is the platform, the others
are the plugins the app bundles (a language, a build tool, a version control system…).

| Plugin | Id | Functionalities |
| --- | --- | --- |
{rows}
"""


class Catalogue:
    def __init__(self, out: pathlib.Path, docs: pathlib.Path):
        self.out, self.docs = out, docs
        self.inventory = json.loads((out / "inventory.json").read_text())
        self.app = self.inventory.get("product", "the reference app")
        strings = out / "strings.json"
        self.strings = json.loads(strings.read_text()) if strings.exists() else {}
        self.actions = {a["id"]: a for a in self.inventory["actions"]}
        self.parents: dict[str, list[str]] = {}
        for action in self.inventory["actions"]:
            for child in action.get("children", []):
                if "id" in child:
                    self.parents.setdefault(child["id"], []).append(action["id"])
        self.entries: list[dict] = []
        self.main_menus = {c.get("id") for c in self.actions["MainMenu"].get("children", [])}
        self.found_places: dict[str, list[str]] = {}
        # an action's place in the menu bar: the order a walk of it meets the action
        self.menu_order: dict[str, int] = {}
        pending = [c.get("id") for c in self.actions["MainMenu"].get("children", [])][::-1]
        while pending:
            action_id = pending.pop()
            if action_id and action_id not in self.menu_order:
                self.menu_order[action_id] = len(self.menu_order)
                pending.extend(c.get("id") for c in self.actions.get(action_id, {}).get("children", [])[::-1])

    def tr(self, text: str | None) -> str:
        """`text` with its translations, when the language packs have them."""
        text = plain(text)
        found = self.strings.get(text, {})
        others = [f"{name} {found[code]}" for code, name in LANGUAGES if found.get(code)]
        return f"{text} <sub>({' · '.join(others)})</sub>" if others else text

    def translations(self, text: str | None) -> dict:
        found = self.strings.get(plain(text), {})
        return {code: found[code] for code, _ in LANGUAGES if found.get(code)}

    def places(self, action_id: str, seen: tuple = ()) -> list[str]:
        """Where `action_id` shows: its path in the menu bar ("Git ▸ Uncommitted Changes"), or the
        context menu or toolbar holding it (`Vcs.Log.ContextMenu`), through the groups that only
        gather items in their place."""
        if action_id in self.found_places:
            return self.found_places[action_id]
        found = []
        for parent in self.parents.get(action_id, []):
            group = self.actions.get(parent, {})
            if parent in self.main_menus:
                found.append(plain(group.get("text")) or parent)
            elif parent == "MainMenu" or parent in seen or parent.endswith("KeymapGroup"):
                # keymap groups only sort Settings › Keymap; no menu shows them
                continue
            elif not self.parents.get(parent):
                found.append(f"`{parent}`")
            else:
                step = f" ▸ {plain(group['text'])}" if group.get("popup") and group.get("text") else ""
                found.extend(place + step for place in self.places(parent, seen + (action_id,)))
        self.found_places[action_id] = list(dict.fromkeys(found))
        return self.found_places[action_id]

    def contents(self, group_id: str, seen: tuple = ()) -> list[str]:
        """A group's items as a user sees them, in order: "—" a separator, "▸" a submenu, and the
        items of a group that only gathers others in its place."""
        items = []
        for child in self.actions.get(group_id, {}).get("children", []):
            if "separator" in child:
                items.append("—")
                continue
            child_id = child.get("id")
            action = self.actions.get(child_id, child.get("inline", {}))
            if action.get("group") and not action.get("popup") and action.get("children") \
                    and child_id not in seen + (group_id,):
                items.extend(self.contents(child_id, seen + (group_id,)))
                continue
            keys = f" ({', '.join(action['shortcuts'])})" if action.get("shortcuts") else ""
            text = plain(action.get("text")) or f"`{child_id}`"
            items.append(text + (" ▸" if action.get("popup") else "") + keys)
        return items

    def image(self, kind: str, name: str) -> str | None:
        """Copies the screenshot of `name` into the docs (shrunk when wide); its relative path."""
        source = self.out / kind / f"{slug(name)}.png"
        if not source.exists():
            return None
        width = int(subprocess.run(["sips", "-g", "pixelWidth", str(source)], capture_output=True,
                                   text=True).stdout.split()[-1])
        wide = width > JPEG_FROM_WIDTH
        target = self.docs / "screenshots" / kind / (source.stem + (".jpg" if wide else ".png"))
        target.parent.mkdir(parents=True, exist_ok=True)
        if not wide:
            shutil.copyfile(source, target)
            return f"screenshots/{kind}/{target.name}"
        command = ["sips", "-s", "format", "jpeg", "-s", "formatOptions", "82"]
        if width > MAX_WIDTH:
            command += ["-Z", str(MAX_WIDTH)]
        subprocess.run(command + [str(source), "--out", str(target)], capture_output=True, check=True)
        return f"screenshots/{kind}/{target.name}"

    def dump(self, kind: str, name: str) -> dict | None:
        path = self.out / kind / f"{slug(name)}.json"
        return json.loads(path.read_text()).get("ui") if path.exists() else None

    def controls(self, node: dict | None, skip_tree: bool = False) -> list[str]:
        """What a dump shows, in reading order, one line a control."""
        lines: list[str] = []

        def off(n):
            return "" if n.get("enabled", True) else " *(disabled)*"

        def walk(n, first_tree=[skip_tree]):
            kind, text = n.get("type", ""), plain(n.get("text"))
            if "rows" in n:
                if first_tree[0]:
                    first_tree[0] = False
                else:
                    for row in n["rows"]:
                        lines.append("  " * row["depth"] + "- " + self.tr(row["text"]))
            elif "cells" in n:
                lines.append("| " + " | ".join(plain(c) for c in n["columns"]) + " |")
                lines.append("|" + " --- |" * len(n["columns"]))
                for row in n["cells"][:30]:
                    lines.append("| " + " | ".join(plain(c) for c in row) + " |")
            elif "items" in n and "selected" in n:
                items = ", ".join(plain(i) for i in n["items"][:20])
                lines.append(f"- ▾ **{self.tr(n['selected'])}**{off(n)} — choices: {items}")
            elif "items" in n:
                lines.extend(f"- {self.tr(i)}" for i in n["items"][:60])
            elif text and "CheckBox" in kind:
                lines.append(f"- {'☑' if n.get('selected') else '☐'} {self.tr(text)}{off(n)}")
            elif text and "RadioButton" in kind:
                lines.append(f"- {'◉' if n.get('selected') else '○'} {self.tr(text)}{off(n)}")
            elif text and "TitledSeparator" in kind:
                lines.append(f"\n**{self.tr(text)}**\n")
            elif text and ("Link" in kind):
                lines.append(f"- link: {self.tr(text)}{off(n)}")
            elif text and ("Button" in kind or "MenuItem" in kind or "Menu" == kind):
                label = "menu item" if "Menu" in kind else "button"
                lines.append(f"- {label}: {self.tr(text)}{off(n)}")
            elif text and ("TextField" in kind or "TextArea" in kind or "Editor" in kind):
                lines.append(f"- field: `{text[:120]}`{off(n)}")
            elif text:
                lines.append(f"- {self.tr(text)}{off(n)}")
            if n.get("tooltip") and not text:
                lines.append(f"- icon *{self.tr(n['tooltip'])}*{off(n)}")
            for child in n.get("children", []):
                walk(child)

        if node:
            walk(node)
        return lines

    @staticmethod
    def menu_items(node: dict | None) -> list[str]:
        """The items a captured menu showed, in order, " ▸" after a submenu, " (disabled)" after one
        that was greyed: what the end-to-end tests compare Spork's menu with."""
        items = []
        for child in (node or {}).get("children", []):
            kind, text = child.get("type", ""), plain(child.get("text"))
            if text and (kind.endswith("MenuItem") or kind == "ActionMenu"):
                items.append(text + (" ▸" if kind == "ActionMenu" else "")
                             + ("" if child.get("enabled", True) else " (disabled)"))
            elif not text:
                items.extend(Catalogue.menu_items(child))
        return items

    def add(self, kind: str, entry_id: str, name: str, plugin: str | None = None, **fields) -> None:
        entry = {"id": f"{kind}:{entry_id}", "kind": kind, "name": plain(name), **fields}
        if "plugins" in self.inventory:
            # settings pages and tool windows say their own; the rest are actions or groups
            if kind not in ("settings", "tool-window", "window"):
                plugin = self.actions.get(entry_id, {}).get("plugin")
            entry["plugin"] = plugin or PLATFORM
        if self.translations(name):
            entry["tr"] = self.translations(name)
        if entry_id in self.actions and kind not in ("menu", "tool-window"):
            entry["places"] = self.places(entry_id)
        if entry_id in self.menu_order and kind != "tool-window":
            entry["menu_order"] = self.menu_order[entry_id]
        self.entries.append(entry)

    # ---- pages ------------------------------------------------------------------------------

    def menu_tree(self, group_id: str, depth: int, seen: tuple) -> list[str]:
        lines = []
        for child in self.actions.get(group_id, {}).get("children", []):
            if "separator" in child:
                lines.append("  " * depth + ("- ───" + (f" {child['separator']}" if child["separator"] else "")))
                continue
            child_id = child.get("id")
            action = self.actions.get(child_id, child.get("inline", {}))
            text = action.get("text") or child_id or "(unnamed)"
            keys = f" `{', '.join(action['shortcuts'])}`" if action.get("shortcuts") else ""
            dynamic = " *(filled when shown)*" if action.get("dynamic") else ""
            lines.append("  " * depth + f"- {self.tr(text)}{keys} <sub>`{child_id}`</sub>{dynamic}")
            if action.get("group") and child_id not in seen and depth < 6:
                lines.extend(self.menu_tree(child_id, depth + 1, seen + (child_id,)))
        return lines

    def menus_page(self) -> str:
        parts = ["# Main menu\n", f"{self.app}'s menu bar, every item with its macOS shortcut and action id. "
                 "Screenshots show each menu as IntelliJ draws it as a popup (the real one is macOS's).\n"]
        for child in self.actions["MainMenu"].get("children", []):
            group = child.get("id")
            if not group:
                continue
            name = self.actions[group].get("text") or group
            image = self.image("menus", group)
            self.add("menu", group, name, screenshot=image, contents=self.contents(group))
            parts.append(f"\n## {self.tr(name)} <sub>`{group}`</sub>\n")
            if image:
                parts.append(f"<img src=\"{image}\" width=\"360\">\n")
            parts.extend(self.menu_tree(group, 0, (group,)))
        return "\n".join(parts) + "\n"

    def context_menus_page(self) -> str:
        main = {c.get("id") for c in self.actions["MainMenu"].get("children", [])}
        parts = ["# Context menus and popups\n",
                 "Every action group that is a menu or popup, as shown on its usual target (the log with a "
                 "commit selected, a change, a file, the editor…): what a user sees there, then all the "
                 "group's items.\n"]
        for path in sorted((self.out / "menus").glob("*.json")):
            name = json.loads(path.read_text())["name"]
            if name in main or name not in self.actions:
                continue
            image = self.image("menus", name)
            shown = self.controls(self.dump("menus", name))
            self.add("context-menu", name, name, screenshot=image, items=len(shown), contents=self.contents(name),
                     shown=self.menu_items(self.dump("menus", name)))
            parts.append(f"\n## `{name}`\n")
            if image:
                parts.append(f"<img src=\"{image}\" width=\"320\">\n")
            if shown:
                parts.append("Shown:\n")
                parts.extend(shown)
            parts.append("\nAll items:\n")
            parts.extend(self.menu_tree(name, 0, (name,)) or ["- *(filled when shown)*"])
        return "\n".join(parts) + "\n"

    def toolbars_page(self) -> str:
        parts = ["# Toolbars\n", "Every toolbar group: the main toolbar's widgets (project, branch, run, search, "
                 "settings), the tool windows' and the log's toolbars, with their items in order.\n"]
        for action in sorted(self.inventory["actions"], key=lambda a: a["id"].lower()):
            if not action.get("group") or "toolbar" not in action["id"].lower():
                continue
            self.add("toolbar", action["id"], action.get("text") or action["id"], contents=self.contents(action["id"]))
            parts.append(f"\n## `{action['id']}`" + (f" — {self.tr(action['text'])}" if action.get("text") else "") + "\n")
            parts.extend(self.menu_tree(action["id"], 0, (action["id"],)) or ["- *(filled when shown)*"])
        return "\n".join(parts) + "\n"

    def actions_page(self) -> str:
        rows = ["# Actions\n",
                f"All {len(self.actions)} actions {self.app} registers, with their text, macOS shortcut, "
                "description and the groups (menus, toolbars, popups) they are in. Actions in no group "
                "are run from a shortcut, Find Action or code.\n",
                "| Id | Text | Shortcut | Description | In |", "| --- | --- | --- | --- | --- |"]
        for action in sorted(self.inventory["actions"], key=lambda a: a["id"].lower()):
            if action.get("group"):
                continue
            where = ", ".join(f"`{p}`" for p in self.parents.get(action["id"], [])[:4])
            keys = ", ".join(action.get("shortcuts", []))
            self.add("action", action["id"], action.get("text") or action["id"], shortcut=keys,
                     groups=self.parents.get(action["id"], []), description=plain(action.get("description")))
            cell = lambda text: plain(text).replace("|", "\\|")
            rows.append(f"| `{action['id']}` | {cell(self.tr(action.get('text')))} | {keys} | "
                        f"{cell(action.get('description'))} | {where} |")
        return "\n".join(rows) + "\n"

    def settings_page(self) -> str:
        parts = ["# Settings\n", f"Every page of {self.app}'s Settings, in its tree, with every control as {self.app} "
                 "shows it by default and its screenshot.\n"]

        def walk(nodes, path):
            for node in nodes:
                here = path + [node["name"]]
                name = " - ".join(here)
                image = self.image("settings", name)
                shown = self.controls(self.dump("settings", name), skip_tree=True)
                self.add("settings", " › ".join(here), " › ".join(here), node.get("plugin"), configurable=node.get("id"),
                         screenshot=image, controls=len(shown))
                parts.append(f"\n{'#' * min(len(here) + 1, 6)} {' › '.join(self.tr(p) for p in here)}"
                             f" <sub>`{node.get('id') or node['class']}`</sub>\n")
                if image:
                    parts.append(f"<img src=\"{image}\" width=\"720\">\n")
                parts.extend(shown)
                walk(node.get("children", []), here)

        walk(self.inventory["settings"], [])
        return "\n".join(parts) + "\n"

    def gallery(self, kind: str, entry_kind: str, title: str, intro: str, names: list[str],
                tool_windows: dict | None = None) -> str:
        parts = [f"# {title}\n", intro + "\n"]
        for name in names:
            image = self.image(kind, name)
            shown = self.controls(self.dump(kind, name))
            # a tool window by its own title, not an action's of the same id (Coverage's "Run with Coverage")
            window = (tool_windows or {}).get(name, {})
            label = window.get("title") or self.actions.get(name, {}).get("text") or name
            self.add(entry_kind, name, label, window.get("plugin"), screenshot=image, controls=len(shown))
            parts.append(f"\n## {self.tr(label)} <sub>`{name}`</sub>\n")
            if image:
                parts.append(f"<img src=\"{image}\" width=\"720\">\n")
            parts.extend(shown)
        return "\n".join(parts) + "\n"

    def write(self) -> None:
        self.docs.mkdir(parents=True, exist_ok=True)
        names = lambda kind: [json.loads(p.read_text())["name"] for p in sorted((self.out / kind).glob("*.json"))] \
            if (self.out / kind).exists() else []
        tool_windows = [t["id"] for t in self.inventory["toolWindows"]]
        pages = {
            "menus.md": self.menus_page(),
            "context-menus.md": self.context_menus_page(),
            "settings.md": self.settings_page(),
            "tool-windows.md": self.gallery("toolwindows", "tool-window", "Tool windows",
                                            "Each tool window opened, in the main window.", tool_windows,
                                            {t["id"]: t for t in self.inventory["toolWindows"]}),
            "dialogs.md": self.gallery("dialogs", "dialog", "Dialogs", "What each main-menu action ending in “…” opens.",
                                       names("dialogs")),
            "windows.md": self.gallery("frame", "window", "Windows", "The main window and the Welcome window.", names("frame")),
            "toolbars.md": self.toolbars_page(),
            "actions.md": self.actions_page(),
        }
        for file, text in pages.items():
            (self.docs / file).write_text(text)
        head = {"reference-app": self.inventory["version"]}
        if "product" in self.inventory:
            head = {"product": self.app, "version": self.inventory["version"]}
        if "plugins" in self.inventory:
            head["plugins"] = {p["id"]: p["name"] for p in self.inventory["plugins"]}
        (self.docs / "catalog.json").write_text(json.dumps({
            **head, "build": self.inventory["build"],
            "keymap": self.inventory["keymap"], "functionalities": self.entries}, indent=1, ensure_ascii=False))
        counts = {}
        for entry in self.entries:
            counts[entry["kind"]] = counts.get(entry["kind"], 0) + 1
        translated = sum(1 for e in self.strings.values() if any(code in e for code, _ in LANGUAGES))
        profile = REFERENCE_PROFILE if self.app == "the reference app" else IDE_PROFILE
        plugins = ""
        if "plugins" in self.inventory:
            names = {p["id"]: p["name"] for p in self.inventory["plugins"]}
            by_plugin: dict[str, int] = {}
            for entry in self.entries:
                by_plugin[entry["plugin"]] = by_plugin.get(entry["plugin"], 0) + 1
            plugins = PLUGINS.format(rows="\n".join(
                f"| {names.get(plugin, plugin)} | `{plugin}` | {n} |"
                for plugin, n in sorted(by_plugin.items(), key=lambda item: (-item[1], item[0]))))
        (self.docs / "README.md").write_text(README.format(
            app=self.app, product=profile["product"], tools=profile["tools"], plugins=plugins,
            packs=profile["packs"].format(translated=translated),
            version=self.inventory["version"], build=self.inventory["build"], keymap=self.inventory["keymap"],
            counts="\n".join(f"| {kind} | {n} |" for kind, n in sorted(counts.items())),
            texts=len(self.strings)))
        print(counts)


if __name__ == "__main__":
    Catalogue(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])).write()
