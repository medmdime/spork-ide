#!/usr/bin/env python3
"""Writes the IDE's themes (assets/themes/islands/islands.json) from IntelliJ's: each theme's UI
colours from its *.theme.json (and its parents'), its editor colours and syntax from the editor
colour scheme it names (and its parents'), all from intellij-community (Apache-2.0), read with
`git show` so the checkout needs no work tree. Values only: this reads IntelliJ's files, it holds
none of IntelliJ's code.

usage: from_intellij.py [path to intellij-community]   (default ~/Projects/intellij-community)

How a value is found, as IntelliJ resolves it: a theme inherits its parent's `ui` and `colors`
(the child's win); a `ui` value naming a `colors` entry is that colour; a `*.suffix` key stands for
every key ending in `.suffix` that its theme or a child doesn't set itself. A scheme inherits its
`parent_scheme`'s colours and attributes, and an attribute with `baseAttributes` is that one's.
"""

import json
import pathlib
import subprocess
import sys
import xml.etree.ElementTree as ElementTree

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
# Spork's themes (spork_workbench), by name: their colours where both apps draw the same thing
SPORK_THEMES = {
    theme["name"]: theme["colors"]
    for theme in json.loads(
        (ROOT / "crates/spork_workbench/assets/themes/islands.json").read_text())["themes"]
}
OUT = ROOT / "assets" / "themes" / "islands" / "islands.json"
THEMES = "platform/platform-resources/src/themes/"
# the theme files by the ids their parentTheme names (themeProvider ids in intellij.platform.ide.impl.xml)
THEME_FILES = {
    "Darcula": "darcula.theme.json",
    "IntelliJ": "intellijlaf.theme.json",
    "JetBrainsLightTheme": "Light.theme.json",
    "ExperimentalDark": "expUI/expUI_dark.theme.json",
    "ExperimentalLight": "expUI/expUI_light.theme.json",
    "ExperimentalLightWithLightHeader": "expUI/expUI_light_with_light_header.theme.json",
    "Islands Dark": "islands/ManyIslandsDark.theme.json",
    "Islands Light": "islands/ManyIslandsLight.theme.json",
    "Islands Darcula": "islands/ManyIslandsDarcula.theme.json",
    "JetBrainsHighContrastTheme": "islands/HighContrast.theme.json",
}
SCHEME_FILES = [
    "platform/platform-resources/src/DefaultColorSchemesManager.xml",  # Default and Darcula
    THEMES + "islands/IslandSchemeDark.xml",
    THEMES + "islands/highContrastScheme.xml",
    THEMES + "expUI/expUI_darkScheme.xml",
    THEMES + "expUI/expUI_lightScheme.xml",
    THEMES + "Light.xml",  # IntelliJ Light
]
# The reference app's list (spork/docs/reference-app/settings.md, Appearance's Theme), and IntelliJ Light
# (ROADMAP Decision 13), by their theme ids
IDE_THEMES = ["Islands Dark", "Islands Light", "Islands Darcula", "JetBrainsHighContrastTheme",
              "ExperimentalDark", "ExperimentalLight", "ExperimentalLightWithLightHeader", "Darcula",
              "JetBrainsLightTheme"]
# the colour of a one-colour icon: the expUI icons' stroke (#6C707E light, #CED0D6 in their dark
# variants), recoloured by the theme's icons.ColorPalette; the classic icons' for the classic themes
ICON_STROKE = {"ExperimentalLight": "#6C707E", "ExperimentalDark": "#CED0D6",
               "Darcula": "#AFB1B3", "IntelliJ": "#6E6E6E"}


class Source:
    def __init__(self, repo: pathlib.Path):
        self.repo = repo

    def read(self, path: str) -> str:
        return subprocess.run(["git", "-C", str(self.repo), "show", f"HEAD:{path}"],
                              check=True, capture_output=True, text=True).stdout


def flatten(node: dict, prefix: str = "") -> dict:
    flat = {}
    for key, value in node.items():
        # a segment named UI joins without a dot (TreeUI)
        name = f"{prefix}{key}" if key == "UI" else (f"{prefix}.{key}" if prefix else key)
        if isinstance(value, dict) and not ({"os.mac", "os.default"} & value.keys()):
            flat.update(flatten(value, name))
        elif isinstance(value, dict):
            flat[name] = value.get("os.mac", value.get("os.default"))
        else:
            flat[name] = value
    return flat


class Theme:
    """One theme with its parents: `ui(key)` answers as IntelliJ's UIManager would."""

    def __init__(self, source: Source, theme_id: str):
        self.levels = []  # the theme first, then its parents
        self.id = theme_id
        current = theme_id
        while current:
            data = json.loads(source.read(THEMES + THEME_FILES[current]))
            self.levels.append({"ui": flatten(data.get("ui", {})), "colors": data.get("colors", {}),
                                "icons": data.get("icons", {}), "data": data, "id": current})
            current = data.get("parentTheme")
        self.name = self.levels[0]["data"]["name"]
        self.dark = self.levels[0]["data"]["dark"]
        # the classic themes name none: Darcula's is Darcula, IntelliJ's Default
        self.scheme = next((level["data"]["editorScheme"] for level in self.levels
                            if level["data"].get("editorScheme")), "Darcula" if self.dark else "Default")
        self.palette = {}
        for level in reversed(self.levels):
            self.palette.update(level["colors"])

    def color(self, value):
        """A colour value: #hex, or a palette name (aliases of aliases too)."""
        for _ in range(5):
            if not isinstance(value, str):
                return None
            if value.startswith("#"):
                return hex_color(value)
            if value == "transparent":
                return "#00000000"
            if value not in self.palette:
                return None
            value = self.palette[value]
        return None

    def ui(self, *keys):
        """The first of `keys` the theme gives a colour, or None."""
        for key in keys:
            for level in self.levels:
                ui = level["ui"]
                if key in ui:
                    value = self.color(ui[key])
                    if value:
                        return value
                    break
                suffix = "*." + key.rsplit(".", 1)[-1] if "." in key else None
                if suffix and suffix in ui and self.color(ui[suffix]):
                    return self.color(ui[suffix])
        return None

    def icon_stroke(self):
        base = next(ICON_STROKE[level["id"]] for level in self.levels if level["id"] in ICON_STROKE)
        for level in self.levels:
            mapped = level["icons"].get("ColorPalette", {}).get(base)
            if mapped:
                return self.color(mapped) or base
        return hex_color(base)


def hex_color(value: str) -> str:
    """IntelliJ's #RGB, #RRGGBB and #RRGGBBAA as Zed's #rrggbbaa."""
    digits = value.lstrip("#").lower()
    if len(digits) == 3:
        digits = "".join(c * 2 for c in digits)
    if len(digits) == 6:
        digits += "ff"
    return "#" + digits


def scheme_color(value: str | None) -> str | None:
    """A scheme's colour: hex digits without '#', 6 or 8 (RRGGBBAA) of them, leading zeros dropped."""
    if value is None or value == "":
        return None
    if len(value) == 8:
        return "#" + value.lower()
    return f"#{int(value, 16):06x}ff"


class Scheme:
    """An editor colour scheme with its parents."""

    def __init__(self, schemes: dict, name: str):
        self.levels = []
        while name:
            element = schemes[name]
            self.levels.append(element)
            name = element.get("parent_scheme")

    def option(self, name: str):
        for scheme in self.levels:
            for option in scheme.findall("./colors/option"):
                if option.get("name") == name:
                    return scheme_color(option.get("value"))
        return None

    def attributes(self, name: str, depth: int = 0) -> dict:
        """FOREGROUND, BACKGROUND, FONT_TYPE, EFFECT_COLOR… of an attribute key."""
        for scheme in self.levels:
            for option in scheme.findall("./attributes/option"):
                if option.get("name") != name:
                    continue
                if option.get("baseAttributes") and depth < 5:
                    return self.attributes(option.get("baseAttributes"), depth + 1)
                value = option.find("value")
                if value is None:
                    return {}
                return {o.get("name"): o.get("value") for o in value.findall("option")}
        return {}

    def fg(self, *names):
        for name in names:
            color = scheme_color(self.attributes(name).get("FOREGROUND"))
            if color:
                return color
        return None

    def bg(self, *names):
        for name in names:
            color = scheme_color(self.attributes(name).get("BACKGROUND"))
            if color:
                return color
        return None

    def effect(self, *names):
        for name in names:
            color = scheme_color(self.attributes(name).get("EFFECT_COLOR"))
            if color:
                return color
        return None


def load_schemes(source: Source) -> dict:
    schemes = {}
    for path in SCHEME_FILES:
        root = ElementTree.fromstring(source.read(path))
        for scheme in [root] if root.tag == "scheme" else root.findall("scheme"):
            schemes[scheme.get("name")] = scheme
    return schemes


def alpha(color: str, value: float) -> str:
    return color[:7] + f"{round(value * 255):02x}"


def first(*values):
    return next((v for v in values if v), None)


# Zed's syntax captures → the scheme's language-default attributes (DefaultLanguageHighlighterColors)
SYNTAX = {
    "attribute": ["DEFAULT_METADATA"],
    "boolean": ["DEFAULT_KEYWORD"],
    "comment": ["DEFAULT_LINE_COMMENT"],
    "comment.doc": ["DEFAULT_DOC_COMMENT"],
    "constant": ["DEFAULT_CONSTANT"],
    "constructor": ["DEFAULT_CLASS_NAME"],
    "enum": ["DEFAULT_CLASS_NAME"],
    "function": ["DEFAULT_FUNCTION_DECLARATION"],
    "function.call": ["DEFAULT_FUNCTION_CALL"],
    "function.method": ["DEFAULT_INSTANCE_METHOD", "DEFAULT_FUNCTION_DECLARATION"],
    "function.method.call": ["DEFAULT_FUNCTION_CALL"],
    "function.builtin": ["DEFAULT_PREDEFINED_SYMBOL"],
    "hint": ["INLAY_DEFAULT"],
    "debugger.inline_value": ["DEBUGGER_INLINED_VALUES"],
    "debugger.inline_value.execution_line": ["DEBUGGER_INLINED_VALUES_EXECUTION_LINE"],
    "debugger.inline_value.modified": ["DEBUGGER_INLINED_VALUES_MODIFIED"],
    "keyword": ["DEFAULT_KEYWORD"],
    "label": ["DEFAULT_LABEL"],
    "link_text": ["HYPERLINK_ATTRIBUTES"],
    "link_uri": ["HYPERLINK_ATTRIBUTES"],
    "number": ["DEFAULT_NUMBER"],
    "operator": ["DEFAULT_OPERATION_SIGN"],
    "preproc": ["DEFAULT_METADATA"],
    "property": ["DEFAULT_INSTANCE_FIELD"],
    "punctuation": ["DEFAULT_DOT"],
    "punctuation.bracket": ["DEFAULT_BRACKETS", "DEFAULT_PARENTHS"],
    "punctuation.delimiter": ["DEFAULT_COMMA"],
    "punctuation.special": ["DEFAULT_BRACES"],
    "selector": ["DEFAULT_TAG"],
    "string": ["DEFAULT_STRING"],
    "string.escape": ["DEFAULT_VALID_STRING_ESCAPE"],
    "string.regex": ["DEFAULT_STRING"],
    "string.special": ["DEFAULT_VALID_STRING_ESCAPE"],
    "string.special.symbol": ["DEFAULT_CONSTANT"],
    "tag": ["DEFAULT_TAG"],
    "text.literal": ["DEFAULT_STRING"],
    "type": ["DEFAULT_CLASS_NAME"],
    "type.builtin": ["DEFAULT_KEYWORD"],
    "variable": ["DEFAULT_IDENTIFIER"],
    "variable.parameter": ["DEFAULT_PARAMETER"],
    "variable.special": ["DEFAULT_KEYWORD"],
    "variable.builtin": ["DEFAULT_KEYWORD"],
    "variant": ["DEFAULT_CONSTANT"],
    "lifetime": ["DEFAULT_LABEL"],
}


# highlights whose background the editor paints under the text (`DebuggerColors`)
BACKGROUNDS = {
    "debugger.smart_step_into.target": "DEBUGGER_SMART_STEP_INTO_TARGET",
    "debugger.smart_step_into.selection": "DEBUGGER_SMART_STEP_INTO_SELECTION",
}


def syntax_style(scheme: Scheme, names: list[str], text: str) -> dict:
    for name in names:
        attributes = scheme.attributes(name)
        if attributes:
            break
    font_type = int(attributes.get("FONT_TYPE", "0") or 0)
    return {
        "color": scheme_color(attributes.get("FOREGROUND")) or text,
        "font_style": "italic" if font_type & 2 else None,
        "font_weight": 700 if font_type & 1 else None,
    }


def zed_theme(theme: Theme, scheme: Scheme) -> dict:
    ui, dark = theme.ui, theme.dark
    transparent = "#00000000"
    editor_bg = scheme.bg("TEXT")
    editor_fg = scheme.fg("TEXT")
    text = ui("Label.foreground", "Panel.foreground")
    muted = first(ui("Label.infoForeground", "Component.infoForeground"), alpha(text, 0.6))
    disabled = ui("Label.disabledForeground", "Component.disabledForeground", "Button.disabledText",
                  "MenuItem.disabledForeground")
    frame = ui("MainWindow.background", "Panel.background")
    island = ui("ToolWindow.background", "Panel.background")
    popup = ui("Popup.background", "PopupMenu.background", "Panel.background")
    border = ui("Borders.color", "Separator.separatorColor")
    hover = ui("ActionButton.hoverBackground")
    pressed = ui("ActionButton.pressedBackground", "ActionButton.hoverBackground")
    selection = ui("List.selectionBackground", "Tree.selectionBackground")
    selection_inactive = ui("List.selectionInactiveBackground", "Tree.selectionInactiveBackground")
    # Zed keeps a selected row's text colour: a selection IntelliJ draws white text on (the classic
    # light theme's) would be unreadable, so the unfocused one stands in
    if not dark and ui("List.selectionForeground") == "#ffffffff":
        selection = selection_inactive
    focus = ui("Component.focusColor", "Component.focusedBorderColor")
    link = ui("Link.activeForeground", "Hyperlink.linkColor")
    error = first(scheme.effect("ERRORS_ATTRIBUTES"), ui("Label.errorForeground"))
    warning = first(scheme.effect("WARNING_ATTRIBUTES"), ui("Label.warningForeground"),
                    scheme_color(scheme.attributes("WARNING_ATTRIBUTES").get("ERROR_STRIPE_COLOR")))
    info = first(scheme.effect("INFO_ATTRIBUTES", "WEAK_WARNING_ATTRIBUTES"), link)
    success = first(ui("Label.successForeground"), scheme.option("ADDED_LINES_COLOR"))
    added = scheme.option("ADDED_LINES_COLOR")
    modified = scheme.option("MODIFIED_LINES_COLOR")
    deleted = scheme.option("DELETED_LINES_COLOR")
    file_added = scheme.option("FILESTATUS_ADDED")
    file_modified = scheme.option("FILESTATUS_MODIFIED")
    file_deleted = scheme.option("FILESTATUS_DELETED")
    file_ignored = scheme.option("FILESTATUS_IDEA_FILESTATUS_IGNORED")
    file_conflict = scheme.option("FILESTATUS_IDEA_FILESTATUS_MERGED_WITH_CONFLICTS")
    file_renamed = first(scheme.option("FILESTATUS_RENAMED"), file_modified)
    file_unversioned = first(scheme.option("FILESTATUS_UNKNOWN"), file_added)
    unused = first(scheme.fg("NOT_USED_ELEMENT_ATTRIBUTES"), disabled)
    console = {name: first(scheme.fg(f"CONSOLE_{key}_OUTPUT"), editor_fg) for name, key in [
        ("black", "BLACK"), ("red", "RED"), ("green", "GREEN"), ("yellow", "YELLOW"), ("blue", "BLUE"),
        ("magenta", "MAGENTA"), ("cyan", "CYAN"), ("white", "GRAY"), ("bright_black", "DARKGRAY"),
        ("bright_red", "RED_BRIGHT"), ("bright_green", "GREEN_BRIGHT"), ("bright_yellow", "YELLOW_BRIGHT"),
        ("bright_blue", "BLUE_BRIGHT"), ("bright_magenta", "MAGENTA_BRIGHT"), ("bright_cyan", "CYAN_BRIGHT"),
        ("bright_white", "WHITE")]}
    # the editor banners' colours; the themes without banners have the notification tool window's
    notice = {"error": "error", "warning": "warning", "info": "informative", "success": "informative"}
    banner = lambda kind: (
        ui(f"Banner.{kind}Background", f"Notification.ToolWindow.{notice[kind]}Background"),
        ui(f"Banner.{kind}BorderColor", f"Notification.ToolWindow.{notice[kind]}BorderColor"))
    # shortcut: Zed's title bar draws in the text colour, so a header IntelliJ draws in a foreground
    # of its own (Light's dark one) takes the panels' colour until the main toolbar is rebuilt
    header = ui("MainToolbar.background", "Panel.background")
    header_inactive = ui("MainToolbar.inactiveBackground", "MainToolbar.background", "Panel.background")
    if ui("MainToolbar.foreground") not in (None, text):
        header = header_inactive = ui("Panel.background")
    # macOS draws the Mac scroll bar (ScrollBarPainter's defaults where a theme sets none)
    thumb = first(ui("ScrollBar.Mac.thumbColor"), "#80808059" if dark else "#00000033")
    thumb_hover = first(ui("ScrollBar.Mac.hoverThumbColor"), "#8080808c" if dark else "#00000080")
    # the themes Spork has take its scrollbar's colours, so the two apps' scrollbars are one
    spork = SPORK_THEMES.get(theme.name)
    if spork:
        thumb = spork["scrollbar.thumb.background"].lower()
        thumb_hover = spork["scrollbar.thumb.hover.background"].lower()
    thumb_border = first(ui("ScrollBar.Mac.thumbBorderColor"), "#26262659" if dark else "#00000033")
    style = {
        "background.appearance": "opaque",
        "border": border,
        "border.variant": first(ui("Separator.separatorColor"), border),
        "border.focused": focus,
        "border.selected": selection,
        "border.transparent": transparent,
        "border.disabled": ui("Component.disabledBorderColor", "Button.disabledBorderColor"),
        "elevated_surface.background": popup,
        "surface.background": island,
        "background": frame,
        "element.background": ui("TextField.background", "ComboBox.background"),
        "element.hover": hover,
        "element.active": pressed,
        "element.selected": selection_inactive,
        "element.selection_background": alpha(selection, 0.6),
        "element.disabled": ui("TextField.disabledBackground", "ComboBox.disabledBackground"),
        "drop_target.background": ui("DragAndDrop.areaBackground"),
        "drop_target.border": ui("DragAndDrop.borderColor"),
        "ghost_element.background": transparent,
        "ghost_element.hover": hover,
        "ghost_element.active": pressed,
        "ghost_element.selected": selection,
        "ghost_element.disabled": transparent,
        "text": text,
        "text.muted": muted,
        "text.placeholder": first(ui("TextField.placeholderForeground", "Component.infoForeground"), disabled),
        "text.disabled": disabled,
        "text.accent": link,
        "icon": theme.icon_stroke(),
        "icon.muted": muted,
        "icon.disabled": disabled,
        "icon.placeholder": disabled,
        "icon.accent": link,
        "status_bar.background": ui("StatusBar.background", "Panel.background"),
        "title_bar.background": header,
        "title_bar.inactive_background": header_inactive,
        "toolbar.background": editor_bg,
        "tab_bar.background": first(ui("EditorTabs.background"), editor_bg),
        "tab.inactive_background": first(ui("EditorTabs.background"), editor_bg),
        "tab.active_background": first(ui("EditorTabs.underlinedTabBackground"), editor_bg),
        "search.match_background": scheme.bg("SEARCH_RESULT_ATTRIBUTES"),
        "search.active_match_background": first(scheme.bg("TEXT_SEARCH_RESULT_ATTRIBUTES"),
                                                scheme.bg("SEARCH_RESULT_ATTRIBUTES")),
        "panel.background": island,
        "panel.focused_border": focus,
        "panel.indent_guide": first(ui("Tree.hash"), scheme.option("INDENT_GUIDE")),
        "panel.indent_guide_hover": first(scheme.option("SELECTED_INDENT_GUIDE"), ui("Tree.hash")),
        "panel.indent_guide_active": first(scheme.option("SELECTED_INDENT_GUIDE"), ui("Tree.hash")),
        "pane.focused_border": focus,
        "pane_group.border": border,
        "scrollbar.thumb.background": thumb,
        "scrollbar.thumb.hover_background": thumb_hover,
        "scrollbar.thumb.active_background": thumb_hover,
        "scrollbar.thumb.border": thumb_border,
        "scrollbar.track.background": transparent,
        "scrollbar.track.border": transparent,
        "minimap.thumb.background": thumb,
        "minimap.thumb.hover_background": thumb_hover,
        "minimap.thumb.active_background": thumb_hover,
        "minimap.thumb.border": transparent,
        "editor.foreground": editor_fg,
        "editor.background": editor_bg,
        # the New UI reads EDITOR_GUTTER_BACKGROUND, and the editor's background without it (EditorGutterColor)
        "editor.gutter.background": first(scheme.option("EDITOR_GUTTER_BACKGROUND"), editor_bg),
        "editor.subheader.background": first(ui("EditorTabs.background"), editor_bg),
        "editor.active_line.background": scheme.option("CARET_ROW_COLOR"),
        "editor.highlighted_line.background": scheme.option("CARET_ROW_COLOR"),
        "editor.debugger_active_line.background": scheme.bg("EXECUTIONPOINT_ATTRIBUTES"),
        "editor.line_number": scheme.option("LINE_NUMBERS_COLOR"),
        "editor.active_line_number": first(scheme.option("LINE_NUMBER_ON_CARET_ROW_COLOR"),
                                           scheme.option("LINE_NUMBERS_COLOR")),
        "editor.hover_line_number": first(scheme.option("LINE_NUMBER_ON_CARET_ROW_COLOR"),
                                          scheme.option("LINE_NUMBERS_COLOR")),
        "editor.invisible": scheme.option("WHITESPACES"),
        "editor.wrap_guide": scheme.option("RIGHT_MARGIN_COLOR"),
        "editor.active_wrap_guide": scheme.option("RIGHT_MARGIN_COLOR"),
        "editor.indent_guide": scheme.option("INDENT_GUIDE"),
        "editor.indent_guide_active": scheme.option("SELECTED_INDENT_GUIDE"),
        "editor.document_highlight.read_background": scheme.bg("IDENTIFIER_UNDER_CARET_ATTRIBUTES"),
        "editor.document_highlight.write_background": first(
            scheme.bg("WRITE_IDENTIFIER_UNDER_CARET_ATTRIBUTES"), scheme.bg("IDENTIFIER_UNDER_CARET_ATTRIBUTES")),
        "editor.document_highlight.bracket_background": scheme.bg("MATCHED_BRACE_ATTRIBUTES"),
        "editor.diff_hunk.added.background": scheme.bg("DIFF_INSERTED"),
        "editor.diff_hunk.deleted.background": scheme.bg("DIFF_DELETED"),
        "terminal.background": first(scheme.option("CONSOLE_BACKGROUND_KEY"), editor_bg),
        "terminal.foreground": first(scheme.fg("CONSOLE_NORMAL_OUTPUT"), editor_fg),
        "terminal.ansi.background": first(scheme.option("CONSOLE_BACKGROUND_KEY"), editor_bg),
        "terminal.bright_foreground": first(scheme.fg("CONSOLE_NORMAL_OUTPUT"), editor_fg),
        "terminal.dim_foreground": first(scheme.fg("CONSOLE_NORMAL_OUTPUT"), editor_fg),
        "link_text.hover": first(ui("Link.hoverForeground"), link),
        "version_control.added": added,
        "version_control.modified": modified,
        "version_control.deleted": deleted,
        "version_control.renamed": file_renamed,
        "version_control.conflict": file_conflict,
        "version_control.ignored": file_ignored,
        "version_control.untracked": file_unversioned,
        "version_control.word_added": scheme.bg("DIFF_INSERTED"),
        "version_control.word_deleted": scheme.bg("DIFF_DELETED"),
        "conflict": file_conflict,
        "created": file_added,
        "deleted": file_deleted,
        "modified": file_modified,
        "renamed": file_renamed,
        "ignored": file_ignored,
        "hidden": file_ignored,
        "error": error,
        "warning": warning,
        "info": info,
        "success": success,
        "hint": first(scheme.fg("INLAY_DEFAULT"), muted),
        "hint.background": scheme.bg("INLAY_DEFAULT"),
        "predictive": disabled,
        "unreachable": unused,
    }
    for name, color in console.items():
        style[f"terminal.ansi.{name}"] = color
        if not name.startswith("bright_"):
            style[f"terminal.ansi.dim_{name}"] = color
    for kind, zed in [("error", "error"), ("warning", "warning"), ("info", "info"), ("success", "success")]:
        background, edge = banner(kind)
        style[f"{zed}.background"] = background
        style[f"{zed}.border"] = edge
    for status in ["conflict", "created", "deleted", "modified", "renamed", "ignored", "hidden", "hint",
                   "predictive", "unreachable"]:
        style.setdefault(f"{status}.background", transparent)
        style[f"{status}.border"] = style.get(status)
    caret = scheme.option("CARET_COLOR")
    style["players"] = [{"cursor": caret, "background": caret,
                         "selection": scheme.option("SELECTION_BACKGROUND")}]
    style["syntax"] = {name: syntax_style(scheme, keys, editor_fg) for name, keys in SYNTAX.items()}
    # Smart Step Into's calls in place: a background over the text's own colours
    for name, key in BACKGROUNDS.items():
        style["syntax"][name] = {"color": scheme.fg(key), "background_color": scheme.bg(key),
                                 "font_style": None, "font_weight": None}
    style["syntax"]["emphasis"] = {"color": editor_fg, "font_style": "italic", "font_weight": None}
    style["syntax"]["emphasis.strong"] = {"color": editor_fg, "font_style": None, "font_weight": 700}
    style["syntax"]["title"] = {"color": editor_fg, "font_style": None, "font_weight": 700}
    style["syntax"]["diff.plus"] = {"color": added, "font_style": None, "font_weight": None}
    style["syntax"]["diff.minus"] = {"color": deleted, "font_style": None, "font_weight": None}
    missing = [key for key, value in style.items() if value is None]
    if missing:
        print(f"{theme.name}: no value for {', '.join(missing)}", file=sys.stderr)
    return {"name": theme.name, "appearance": "dark" if dark else "light",
            "style": {key: value for key, value in style.items() if value is not None}}


def main() -> None:
    source = Source(pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "~/Projects/intellij-community").expanduser())
    schemes = load_schemes(source)
    themes = []
    for theme_id in IDE_THEMES:
        theme = Theme(source, theme_id)
        themes.append(zed_theme(theme, Scheme(schemes, theme.scheme)))
    family = {
        "$schema": "https://zed.dev/schema/themes/v0.2.0.json",
        "name": "Islands",
        "author": "intellij-community (Apache-2.0), by tools/themes/from_intellij.py",
        "themes": themes,
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(family, indent=2, ensure_ascii=False) + "\n")
    print(f"wrote {len(themes)} themes to {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
