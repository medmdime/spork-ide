#!/usr/bin/env python3
"""Runs a catalogue script inside the sandboxed reference app, through JetBrains' Remote Robot server
(port 8082, see sandbox.sh). lib.js goes first; KEY=VALUE arguments replace KEY in the script;
a script with "// edt" in it runs on Swing's event thread.

usage: robot.py <script.js> [KEY=VALUE ...]
"""

import json
import pathlib
import sys
import urllib.request

HERE = pathlib.Path(__file__).parent


def run(script: str, edt: bool) -> None:
    request = urllib.request.Request(
        "http://127.0.0.1:8082/js/execute",
        data=json.dumps({"script": script, "runInEdt": edt}).encode(),
        headers={"Content-Type": "application/json"},
    )
    answer = json.loads(urllib.request.urlopen(request, timeout=900).read())
    if answer.get("status") != "SUCCESS":
        error = answer.get("exception", {})
        raise RuntimeError(
            f"{answer.get('message')} at line {error.get('lineNumber')}: {error.get('lineSource')}"
        )


def script(path: str, **values: str) -> str:
    text = (HERE / "lib.js").read_text() + "\n" + pathlib.Path(path).read_text()
    for key, value in values.items():
        text = text.replace(key, value)
    return text


if __name__ == "__main__":
    values = dict(arg.split("=", 1) for arg in sys.argv[2:])
    text = script(sys.argv[1], **values)
    try:
        run(text, "// edt" in text)
    except RuntimeError as error:
        sys.exit(f"ERROR {error}")
    print("ok")
