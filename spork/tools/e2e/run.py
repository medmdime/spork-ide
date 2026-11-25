#!/usr/bin/env python3
"""Spork's end-to-end UI tests: each scenario in scenarios/*.json starts Spork on the fixture
repository (fixture.sh) with the test channel on (crates/spork/src/e2e.rs), plays its steps, checks
what Spork reports showing and compares its screenshots with the recorded ones. The report puts
each screenshot beside the recorded one, their difference and the reference app's (docs/reference-app).

usage: run.py [--record] [--strict] [--no-build] [scenario id ...]
  --record    keep this run's screenshots as the expected ones
  --strict    a screenshot whose window opened at another size than recorded fails
  --no-build  test the spork binary as it is

macOS may open the window at another size than asked (a window manager, the display layout), and
a screenshot of another size can't be compared: it is reported, and fails only with --strict.

A scenario: {"id", "covers": [catalog ids, docs/reference-app/catalog.json], "reference-app": [screenshots
under docs/reference-app], "steps": [...]}, each step one of
  {"do": ...}            a command for the test channel (see e2e.rs)
  {"wait": ms}
  {"expect": {key: value}}  values in {"do": "state"}'s answer ("of": another command's); a
                         dotted key ("log.regex", "balloons.0.text") reaches into a nested
                         answer, a number into a list
  {"shot": name}         a screenshot of Spork's window ("window": "smallest" for Settings)
  {"menu": catalog id}   the menu open last shows the items the reference app's showed (the catalog's
                         "shown"), same texts in the same order, greyed where the reference app's were;
                         "items": [...] instead when the target differs from the reference app's capture;
                         "path": [menu, submenu...] reads that menu of the menu bar; "part": true
                         when the items are a run inside the menu (a group shown inline);
                         "checked": [...] the items that must show a check
  {"toolbar": label, "items": [...]}  the toolbar with that accessible label shows these
                         controls in order, greyed ones marked " (disabled)"; "id": a catalog id
                         instead of "items" for the ones the reference app showed; "part" as for a menu
  {"find": matcher or [matchers]}  each matches an element on screen: a label (or title), or
                         {"label", "text" (label, title or value), "role", "value", "enabled",
                         "in": the label of an element to look under};
                         "absent": true, none does; "missing": [matchers] that mustn't;
                         "ordered": true, in that order, depth first through every window
  {"sh": command}        runs it in the fixture repository; it must succeed, or with "is" (its
                         trimmed output), "contains" or "lacks", waits until its output holds
  {"git": [args]}        the same for git
  {"file": path, "text": ...}  writes a file of the fixture repository
Steps that look at the screen wait up to "within" ms for it to hold.

The submenu mark " ▸" is left out of every comparison: macOS's accessibility doesn't tell a
submenu from an item.

To aim at an element, {"do": "ui"} answers what macOS's accessibility sees of each window, and
{"do": "click", "label": ..., "button": "right"} clicks one by its label (e2e.rs).
"""

import html
import json
import os
import pathlib
import shutil
import socket
import subprocess
import sys
import tempfile
import time

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent.parent
# the monorepo, whose workspace builds Spork
REPO = ROOT.parent
GOLDEN = HERE / "golden"
OUT = HERE / "out"
BIN = HERE / ".bin"
# a share of pixels a screenshot may differ by before it fails: font hinting and the caret
TOLERANCE = 0.002


def walk(node: dict):
    yield node
    for child in node.get("children", []):
        yield from walk(child)


def nodes(spork: "Spork") -> list[dict]:
    """Every element macOS's accessibility sees, window after window, depth first."""
    return [n for w in spork.send({"do": "ui"}).get("windows", []) for n in walk(w["tree"])]


def under(shown: list[dict], label: str) -> list[dict]:
    """The elements under (and including) those labelled or titled `label`."""
    return [n for c in shown if label in (c.get("label"), c.get("title")) for n in walk(c)]


def open_menu(spork: "Spork") -> list[str] | None:
    """The items of the menu opened last, as the catalog writes them: separators left out, a
    greyed item marked " (disabled)"."""
    # a submenu's item holds its closed menu, an AXMenu without items
    menus = [n for n in nodes(spork) if n.get("role") == "AXMenu" and n.get("children")]
    if not menus:
        return None
    return [item["title"] + ("" if item.get("enabled", True) else " (disabled)")
            for item in menus[-1].get("children", []) if item.get("role") == "AXMenuItem" and item.get("title")]


def unmarked(items: list[str] | None) -> list[str] | None:
    return None if items is None else [i.replace(" ▸", "") for i in items]


def holds(shown: list[str] | None, wanted: list[str], part: bool) -> bool:
    """`shown` is `wanted`, or with `part` holds it as a run."""
    if shown is None:
        return False
    return shown == wanted or (part and any(shown[i:i + len(wanted)] == wanted for i in range(len(shown))))


def checked_items(spork: "Spork") -> list[str]:
    """The titles of the open menu's items that show a check."""
    menus = [n for n in nodes(spork) if n.get("role") == "AXMenu" and n.get("children")]
    return [] if not menus else [i["title"] for i in menus[-1].get("children", [])
                                 if i.get("title") and i.get("value") == "1"]


def compare_menu(spork: "Spork", step: dict, catalog: dict) -> dict:
    wanted = unmarked(step.get("items") or catalog.get(step["menu"], {}).get("shown"))
    if not wanted:
        return {"error": f"{step['menu']}: the catalog has no shown items for it; give \"items\""}
    deadline, shown = time.time() + step.get("within", 3000) / 1000, None
    while time.time() < deadline:
        # the menu bar's menu is read anew each time: it follows the project (its hosting)
        if "path" in step:
            answer = spork.send({"do": "menubar", "path": step["path"]})
            if "error" in answer:
                return {"error": f"{step['menu']}: {answer['error']}"}
        shown = unmarked(open_menu(spork))
        if holds(shown, wanted, step.get("part", False)):
            if "checked" in step and checked_items(spork) != step["checked"]:
                return {"error": f"{step['menu']}: checked {checked_items(spork)}, wanted {step['checked']}"}
            return {"menu": shown}
        time.sleep(0.2)
    if shown is None:
        return {"error": f"{step['menu']}: no menu open"}
    missing = [i for i in wanted if i not in shown]
    extra = [i for i in shown if i not in wanted]
    return {"error": f"{step['menu']}: Spork shows {shown}; the reference app {wanted}; "
                     f"missing {missing}, extra {extra}" + ("" if missing or extra else ", in another order")}


CONTROLS = ("AXButton", "AXCheckBox", "AXMenuButton", "AXPopUpButton", "AXLink", "AXRadioButton")


def controls(node: dict) -> list[str]:
    """The controls under `node` in order, by label (or title), a greyed one marked " (disabled)"."""
    items = []
    for child in node.get("children", []):
        text = child.get("label") or child.get("title")
        if child.get("role") in CONTROLS and text:
            items.append(text + ("" if child.get("enabled", True) else " (disabled)"))
        else:
            items.extend(controls(child))
    return items


def compare_toolbar(spork: "Spork", step: dict, catalog: dict) -> dict:
    name = step.get("id", "").split(":", 1)[-1]
    wanted = unmarked(step.get("items") or catalog.get(step.get("id"), {}).get("shown")
                      or catalog.get(f"context-menu:{name}", {}).get("shown"))
    if not wanted:
        return {"error": f"{step['toolbar']}: no items; give \"items\" or a catalog \"id\" with shown items"}
    deadline, shown = time.time() + step.get("within", 3000) / 1000, None
    while time.time() < deadline:
        bars = [n for n in nodes(spork) if step["toolbar"] in (n.get("label"), n.get("title"))]
        bars = [n for n in bars if n.get("role") == "AXToolbar"] or bars
        shown = unmarked(controls(bars[-1])) if bars else None
        if holds(shown, wanted, step.get("part", False)):
            return {"toolbar": shown}
        time.sleep(0.2)
    if shown is None:
        return {"error": f"nothing labelled {step['toolbar']!r} on screen"}
    return {"error": f"{step['toolbar']}: Spork shows {shown}; the reference app {wanted}"}


def matches(node: dict, matcher) -> bool:
    if isinstance(matcher, str):
        matcher = {"label": matcher}
    for key, value in matcher.items():
        if key == "in":
            continue
        if key == "label":
            found = value in (node.get("label"), node.get("title"))
        elif key == "text":
            found = value in (node.get("label"), node.get("title"), node.get("value"))
        elif key == "enabled":
            found = node.get("enabled", True) == value
        else:
            found = node.get(key) == value
        if not found:
            return False
    return True


def find(spork: "Spork", step: dict) -> dict:
    wanted = step.get("find", [])
    wanted = wanted if isinstance(wanted, list) else [wanted]
    absent, ordered = step.get("absent", False), step.get("ordered", False)
    deadline = time.time() + step.get("within", 3000) / 1000
    while True:
        shown = nodes(spork)
        # a matcher's "in" looks only under the element of that label
        pool = lambda m: under(shown, m["in"]) if isinstance(m, dict) and "in" in m else shown
        if ordered:
            left = list(wanted)
            for node in shown:
                if left and matches(node, left[0]):
                    left.pop(0)
        else:
            left = [m for m in wanted if any(matches(n, m) for n in pool(m)) == absent]
        there = [m for m in step.get("missing", []) if any(matches(n, m) for n in pool(m))]
        if not left and not there:
            return {}
        if time.time() > deadline:
            problems = []
            if left:
                problems.append(("still on screen: " if absent else "not on screen" + (", or not in this order" if ordered else "") + ": ")
                                + json.dumps(left, ensure_ascii=False))
            if there:
                problems.append("on screen though it shouldn't be: " + json.dumps(there, ensure_ascii=False))
            return {"error": "; ".join(problems)}
        time.sleep(0.2)


CHECKS = {"is": lambda out, text: out.strip() == text, "contains": lambda out, text: text in out,
          "lacks": lambda out, text: text not in out}


def run_in_fixture(spork: "Spork", step: dict) -> dict:
    """A "sh" or "git" step: what an action did to the repository, or a state to set up."""
    what = step["sh"] if "sh" in step else "git " + " ".join(step["git"])
    wanted = [(key, step[key]) for key in CHECKS if key in step]
    deadline = time.time() + step.get("within", 5000) / 1000
    while True:
        if "sh" in step:
            run = subprocess.run(step["sh"], shell=True, cwd=spork.repo, capture_output=True, text=True)
        else:
            run = subprocess.run(["git", *step["git"]], cwd=spork.repo, capture_output=True, text=True)
        if not wanted:
            if run.returncode != 0:
                return {"error": f"{what}: {run.stderr.strip() or run.returncode}"}
            return {"out": run.stdout.strip()}
        # "is" reads what it prints, "contains" and "lacks" its messages too
        failed = [f"{key} {text!r}" for key, text in wanted
                  if not CHECKS[key](run.stdout if key == "is" else run.stdout + run.stderr, text)]
        if not failed:
            return {}
        if time.time() > deadline:
            return {"error": f"{what} printed {(run.stdout + run.stderr).strip()!r}; expected {', '.join(failed)}"}
        time.sleep(0.3)


def lookup(state: dict, key: str):
    """`state[key]`, a dotted key ("log.regex", "balloons.0.text") reaching into a nested answer,
    a number into a list."""
    for part in key.split("."):
        if isinstance(state, list) and part.isdigit():
            state = state[int(part)] if int(part) < len(state) else None
        else:
            state = state.get(part) if isinstance(state, dict) else None
    return state


def tools() -> None:
    BIN.mkdir(exist_ok=True)
    for name in ("winid", "imgdiff"):
        source, binary = HERE / "swift" / f"{name}.swift", BIN / name
        if not binary.exists() or binary.stat().st_mtime < source.stat().st_mtime:
            subprocess.run(["swiftc", "-O", str(source), "-o", str(binary)], check=True)


class Spork:
    """Spork on a fresh copy of the fixture, with a home of its own, reached on its socket."""

    def __init__(self, scratch: pathlib.Path):
        self.repo, home = scratch / "repo", scratch / "home"
        subprocess.run([str(HERE / "fixture.sh"), str(self.repo)], check=True)
        prefs = home / "Library" / "Application Support" / "Spork"
        prefs.mkdir(parents=True, exist_ok=True)
        (prefs / "preferences.conf").write_text("appearance=dark\n")
        self.socket_path = scratch / "e2e.sock"
        env = dict(os.environ, HOME=str(home), SPORK_E2E=str(self.socket_path),
                   SPORK_WINDOW_SIZE="1400x900")
        self.process = subprocess.Popen([str(REPO / "target" / "debug" / "spork"), str(self.repo)],
                                        env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.connection = self.connect()
        self.wait_for(lambda state: state.get("commits", 0) > 0, "the history to load")

    def connect(self) -> socket.socket:
        deadline = time.time() + 40
        while time.time() < deadline:
            try:
                connection = socket.socket(socket.AF_UNIX)
                connection.connect(str(self.socket_path))
                return connection
            except OSError:
                time.sleep(0.3)
        raise RuntimeError("Spork's test channel didn't open")

    def send(self, command: dict) -> dict:
        self.connection.sendall((json.dumps(command) + "\n").encode())
        answer = b""
        while not answer.endswith(b"\n"):
            chunk = self.connection.recv(65536)
            if not chunk:
                raise RuntimeError("Spork closed the test channel")
            answer += chunk
        return json.loads(answer)

    def wait_for(self, check, what: str, seconds: float = 30, command: dict | None = None) -> dict:
        deadline, state = time.time() + seconds, {}
        while time.time() < deadline:
            state = self.send(command or {"do": "state"})
            if check(state):
                return state
            time.sleep(0.3)
        raise RuntimeError(f"timed out waiting for {what}: {state}")

    def screenshot(self, path: pathlib.Path, which: str = "largest") -> None:
        window = subprocess.run([str(BIN / "winid"), str(self.process.pid), which],
                                capture_output=True, text=True).stdout.strip()
        if not window:
            raise RuntimeError("Spork's window isn't on screen")
        path.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["screencapture", "-x", "-o", f"-l{window}", str(path)], check=True)

    def close(self) -> None:
        try:
            self.send({"do": "quit"})
            self.process.wait(timeout=10)
        except (RuntimeError, OSError, subprocess.TimeoutExpired):
            self.process.kill()


def play(scenario: dict, record: bool, strict: bool, catalog: dict) -> dict:
    result = {"id": scenario["id"], "steps": [], "ok": True}
    with tempfile.TemporaryDirectory(prefix="spork-e2e-") as scratch:
        spork = Spork(pathlib.Path(scratch))
        try:
            for step in scenario["steps"]:
                entry = {"step": step}
                if "do" in step:
                    answer = spork.send(step)
                    if "error" in answer:
                        entry["error"] = answer["error"]
                elif "wait" in step:
                    time.sleep(step["wait"] / 1000)
                elif "expect" in step:
                    wanted = step["expect"]
                    try:
                        spork.wait_for(lambda s: all(lookup(s, k) == v for k, v in wanted.items()),
                                       json.dumps(wanted), seconds=step.get("within", 5000) / 1000,
                                       command=step.get("of"))
                    except RuntimeError as error:
                        entry["error"] = str(error)
                elif "menu" in step:
                    entry.update(compare_menu(spork, step, catalog))
                elif "toolbar" in step:
                    entry.update(compare_toolbar(spork, step, catalog))
                elif "find" in step or "missing" in step:
                    entry.update(find(spork, step))
                elif "sh" in step or "git" in step:
                    entry.update(run_in_fixture(spork, step))
                elif "file" in step:
                    (spork.repo / step["file"]).write_text(step["text"])
                elif "shot" in step:
                    entry.update(shoot(spork, scenario["id"], step["shot"], record,
                                       step.get("window", "largest"), strict))
                result["steps"].append(entry)
                if "error" in entry:
                    result["ok"] = False
        except RuntimeError as error:
            result["ok"], result["error"] = False, str(error)
        finally:
            spork.close()
    return result


def shoot(spork: Spork, scenario: str, name: str, record: bool, which: str, strict: bool) -> dict:
    actual = OUT / scenario / f"{name}.png"
    # a window another app's covers isn't drawn: bring Spork's forward and wait for a frame,
    # again while other windows keep coming over it
    framed, deadline = False, time.time() + 10
    while not framed and time.time() < deadline:
        spork.send({"do": "front"})
        tried = time.time() + 2
        while not (framed := spork.send({"do": "framed"}).get("framed")) and time.time() < tried:
            time.sleep(0.05)
    spork.screenshot(actual, which)
    golden = GOLDEN / scenario / f"{name}.png"
    if record or not golden.exists():
        golden.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(actual, golden)
        return {"shot": str(actual), "recorded": True} | (
            {} if framed else {"error": f"screenshot {name}: recorded, but Spork's window drew no frame"})
    diff = OUT / scenario / f"{name}.diff.png"
    run = subprocess.run([str(BIN / "imgdiff"), str(golden), str(actual), str(diff)],
                         capture_output=True, text=True)
    entry = {"shot": str(actual), "golden": str(golden), "diff": str(diff)}
    if not framed:
        entry["warning"] = f"screenshot {name}: Spork's window drew no frame, it may show an earlier one"
    if run.returncode == 2 and not strict:
        entry["warning"] = f"screenshot {name} not compared: {run.stdout.strip()}"
    elif run.returncode != 0:
        entry["error"] = f"screenshot {name}: {run.stdout.strip() or 'unreadable'}"
    elif float(run.stdout) > TOLERANCE:
        entry["error"] = f"screenshot {name}: {float(run.stdout):.2%} of its pixels changed"
    return entry


def report(scenarios: list[dict], results: list[dict]) -> pathlib.Path:
    reference_app = ROOT / "docs" / "reference-app"
    rows = []
    for scenario, result in zip(scenarios, results):
        status = "pass" if result["ok"] else "fail"
        rows.append(f"<h2 class={status}>{html.escape(scenario['id'])}: {status}</h2>")
        if "error" in result:
            rows.append(f"<pre>{html.escape(result['error'])}</pre>")
        for reference in scenario.get("reference-app", []):
            rows.append(f"<figure><img src='{(reference_app / reference).as_uri()}'><figcaption>The reference app</figcaption></figure>")
        for step in result["steps"]:
            for key in ("error", "warning"):
                if key in step:
                    rows.append(f"<pre class={key}>{html.escape(step[key])}</pre>")
            for key in ("shot", "golden", "diff"):
                if key in step and pathlib.Path(step[key]).exists():
                    caption = {"shot": "Spork now", "golden": "Spork as recorded", "diff": "difference"}[key]
                    rows.append(f"<figure><img src='{pathlib.Path(step[key]).as_uri()}'><figcaption>{caption}</figcaption></figure>")
    page = OUT / "report.html"
    page.write_text("<meta charset=utf-8><style>body{font:14px system-ui;background:#1e1f22;color:#ddd}"
                    "img{max-width:420px;border:1px solid #444}figure{display:inline-block;margin:6px}"
                    ".pass{color:#6db083}.fail,.error{color:#f57e84}.warning{color:#d59637}</style>" + "\n".join(rows))
    return page


def main() -> int:
    args = sys.argv[1:]
    record, strict, build = "--record" in args, "--strict" in args, "--no-build" not in args
    only = [a for a in args if not a.startswith("--")]
    tools()
    if build:
        subprocess.run(["cargo", "build", "-p", "spork"], cwd=REPO, check=True)
    scenarios = [json.loads(p.read_text()) for p in sorted((HERE / "scenarios").glob("*.json"))]
    scenarios = [s for s in scenarios if not only or s["id"] in only]
    if OUT.exists():
        shutil.rmtree(OUT)
    catalog = {e["id"]: e for e in json.loads((ROOT / "docs" / "reference-app" / "catalog.json").read_text())["functionalities"]}
    results = []
    for scenario in scenarios:
        result = play(scenario, record, strict, catalog)
        results.append(result)
        print(("ok   " if result["ok"] else "FAIL ") + scenario["id"], flush=True)
        for step in result["steps"]:
            for key in ("error", "warning"):
                if key in step:
                    print(f"     {key}: {step[key]}")
        if "error" in result:
            print("     " + result["error"])
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "results.json").write_text(json.dumps(results, indent=1))
    print("report:", report(scenarios, results))
    return 0 if all(r["ok"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
