#!/usr/bin/env python3
"""Where each text the catalogue holds comes from in the reference app's message bundles, and its
translations. The reference app ships English only; the Chinese, Japanese and Korean texts come from
IntelliJ's own language packs (2024.2, the last ones published as plugins), matched by bundle and
key, so a few may be out of date or missing. An app bundling its packs (IntelliJ IDEA's
plugins/localization-{zh,ja,ko}) is given its own, which are left out of its English texts.

usage: strings.py <out dir> <Reference.app> [<language pack jar> ...]
Reads every texts in <out dir>/**/*.json and writes <out dir>/strings.json.
"""

import json
import pathlib
import re
import sys
import zipfile


def parse_properties(text: str) -> dict:
    """Java .properties: `key=value`, `key: value` or `key value`, `\\` continues a line,
    `\\uXXXX` and the usual escapes."""
    entries, pending = {}, ""
    for raw in text.splitlines():
        line = pending + raw.lstrip() if pending else raw.lstrip()
        pending = ""
        if not line or line[0] in "#!":
            continue
        # an odd number of backslashes at the end continues the line
        if (len(line) - len(line.rstrip("\\"))) % 2 == 1:
            pending = line[:-1]
            continue
        match = re.match(r"((?:\\.|[^=:\s\\])+)\s*[=:\s]\s*(.*)", line)
        if not match:
            continue
        key, value = (unescape(part) for part in match.groups())
        entries[key] = value
    return entries


def unescape(text: str) -> str:
    def replace(match):
        code = match.group(1)
        if code.startswith("u"):
            return chr(int(code[1:], 16))
        return {"t": "\t", "n": "\n", "r": "\r", "f": "\f"}.get(code, code)

    return re.sub(r"\\(u[0-9a-fA-F]{4}|.)", replace, text)


def bundles(jar: pathlib.Path) -> dict:
    """path inside the jar → its entries, for every bundle under messages/ (no locale suffix)."""
    found = {}
    try:
        archive = zipfile.ZipFile(jar)
    except zipfile.BadZipFile:
        return found
    for name in archive.namelist():
        if name.endswith(".properties") and "/messages/" in "/" + name and not re.search(r"_[a-z]{2}(_[A-Z]{2})?\.properties$", name):
            found[name] = parse_properties(archive.read(name).decode("utf-8", "replace"))
    return found


def normal(text: str) -> str:
    """A text as the UI shows it: no mnemonic marks, "…" for "...", no HTML tags."""
    text = re.sub(r"<[^>]+>", "", text)
    text = text.replace("...", "…").replace("_", "").replace("&", "")
    return re.sub(r"\s+", " ", text).strip()


def texts(value, out: set) -> None:
    if isinstance(value, dict):
        for key, inner in value.items():
            if key in ("text", "description", "name", "title", "selected") and isinstance(inner, str):
                out.add(inner)
            else:
                texts(inner, out)
    elif isinstance(value, list):
        for inner in value:
            if isinstance(inner, str):
                out.add(inner)
            else:
                texts(inner, out)


def check() -> None:
    entries = parse_properties("a=One\\\n  two\nb: x\\u2026\n# no\nc.d=_Open\\\\\n")
    assert entries == {"a": "Onetwo", "b": "x…", "c.d": "_Open\\"}, entries
    assert normal("_Open <b>Repo</b>...") == "Open Repo…"


if __name__ == "__main__":
    check()
    out, app, packs = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3:]
    english = {}
    for jar in app.rglob("*.jar"):
        # a bundled pack's bundles have the English ones' names
        if not any(jar.samefile(pack) for pack in packs):
            english.update(bundles(jar))
    translated = {}
    for pack in packs:
        # zh.jar, or a bundled pack's localization-zh.jar
        language = pathlib.Path(pack).name.split(".")[0].split("-")[-1]
        translated[language] = bundles(pathlib.Path(pack))
    index = {}
    for path, entries in english.items():
        for key, value in entries.items():
            index.setdefault(normal(value), []).append((path, key))
    shown = set()
    for file in out.rglob("*.json"):
        if file.name != "strings.json":
            texts(json.loads(file.read_text()), shown)
    result = {}
    for text in sorted(shown):
        sources = index.get(normal(text))
        if not sources:
            continue
        entry = {"keys": [f"{path}#{key}" for path, key in sources[:5]]}
        for language, packs_bundles in translated.items():
            for path, key in sources:
                value = packs_bundles.get(path, {}).get(key)
                if value:
                    entry[language] = normal(value)
                    break
        result[text] = entry
    (out / "strings.json").write_text(json.dumps(result, ensure_ascii=False, indent=1))
    print(f"{len(shown)} texts shown, {len(result)} found in bundles,",
          {language: sum(1 for e in result.values() if language in e) for language in translated})
