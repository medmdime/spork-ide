#!/usr/bin/env python3
"""A Model Context Protocol server for the end-to-end tests, which the fixture-mcp extension
writes into its work folder and runs: newline-delimited JSON-RPC on stdin and stdout, one tool,
`fixture_echo`, saying back its text. Each request's method is logged, a line each, to
mcp-requests.log beside it, for a scenario to check."""

import json
import pathlib
import sys

LOG = pathlib.Path(__file__).with_name("mcp-requests.log")
TOOL = {"name": "fixture_echo", "description": "Says the text back",
        "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}},
                        "required": ["text"]}}


def answer(method, params):
    if method == "initialize":
        return {"protocolVersion": params.get("protocolVersion", "2025-03-26"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "fixture-mcp", "version": "1.0.0"}}
    if method == "tools/list":
        return {"tools": [TOOL]}
    if method == "tools/call":
        text = params.get("arguments", {}).get("text", "")
        return {"content": [{"type": "text", "text": f"fixture says: {text}"}]}
    return {}


for line in sys.stdin:
    if not line.strip():
        continue
    message = json.loads(line)
    method = message.get("method")
    with LOG.open("a") as log:
        log.write(f"{method}\n")
    if "id" in message and method is not None:
        reply = {"jsonrpc": "2.0", "id": message["id"], "result": answer(method, message.get("params") or {})}
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()
