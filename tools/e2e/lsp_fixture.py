#!/usr/bin/env python3
"""A language server for the end-to-end tests, which the fixture-language extension downloads
from the test registry (extensions_http.py) and runs: it speaks the Language Server Protocol on
stdin and stdout, warns "fixture-ls: hello" on the first line of every document it is given,
completes `fixtureCompletion` and answers hovers with "Fixture hover"."""

import json
import sys


def read():
    length = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, _, value = line.decode().partition(":")
        if name.lower() == "content-length":
            length = int(value)
    return json.loads(sys.stdin.buffer.read(length))


def send(message):
    body = json.dumps({"jsonrpc": "2.0", **message}).encode()
    sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    sys.stdout.buffer.flush()


def warn(uri):
    send({"method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": [{
        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
        "severity": 2, "source": "fixture-ls", "message": "fixture-ls: hello"}]}})


ANSWERS = {
    "initialize": {"capabilities": {"textDocumentSync": 1, "hoverProvider": True,
                                    "completionProvider": {}},
                   "serverInfo": {"name": "fixture-ls"}},
    "textDocument/completion": [{"label": "fixtureCompletion", "kind": 1}],
    "textDocument/hover": {"contents": {"kind": "plaintext", "value": "Fixture hover"}},
    "shutdown": None,
}

while (message := read()) is not None:
    method = message.get("method")
    if method == "exit":
        break
    if method in ("textDocument/didOpen", "textDocument/didChange"):
        warn(message["params"]["textDocument"]["uri"])
    if "id" in message and method is not None:
        send({"id": message["id"], "result": ANSWERS.get(method)})
