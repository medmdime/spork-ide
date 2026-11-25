#!/usr/bin/env python3
"""An extension registry over HTTP on 127.0.0.1 for the Plugins page's end-to-end tests, so they
download nothing: the IDE's `server_url` pointed at it, it lists the fixture extensions
(fixtures/extensions, built by extension_fixtures.py first: a theme, an icon theme, snippets, a
language with its grammar and language server, a context server and a debug adapter with a debug
locator) on `/extensions` (narrowed by `filter`), answers `/extensions/updates`, and serves each
one's archive at its latest version (`/extensions/<id>/download`) or a given one
(`/extensions/<id>/<version>/download`), the version written into its manifest.

Each is published at 1.0.0; `/e2e/publish/<id>/<version>` publishes another version, for an
update. `/files/<name>` serves this folder's lsp_fixture.py, the language server the language
fixture downloads. Writes the port it took to <port file> and stops after <seconds>.

usage: extensions_http.py <port file> [seconds]
"""

import gzip
import http.server
import io
import json
import pathlib
import re
import sys
import tarfile
import threading
import urllib.parse

import extension_fixtures

HERE = pathlib.Path(__file__).resolve().parent
PORT_FILE = sys.argv[1]
LIFETIME = float(sys.argv[2]) if len(sys.argv) > 2 else 300

# id: name, description, downloads, provides, the extension API it is built against
PLUGINS = {
    "fixture-theme": ("Fixture Theme", "A dark theme for the tests.", 38_512_345, ["themes"], None),
    "fixture-language": ("Fixture Language",
                         "Highlighting and a language server for the fixture language.", 2_345,
                         ["languages", "grammars", "language-servers"], "0.8.0"),
    "fixture-snippets": ("Fixture Snippets", "Snippets for the tests.", 615, ["snippets"], None),
    "fixture-icons": ("Fixture Icons", "File icons for the tests.", 512, ["icon-themes"], None),
    "fixture-mcp": ("Fixture MCP", "A Model Context Protocol server for the tests.", 401,
                    ["context-servers"], "0.8.0"),
    "fixture-debug": ("Fixture Debugger", "A debug adapter and a debug locator for the tests.",
                      302, ["debug-adapters"], "0.8.0"),
}
STAGED = extension_fixtures.build()
LATEST = {id: "1.0.0" for id in PLUGINS}


def metadata(id):
    name, description, downloads, provides, wasm_api_version = PLUGINS[id]
    return {
        "id": id, "name": name, "version": LATEST[id], "description": description,
        "authors": ["Spork IDE Tests"], "repository": f"https://example.com/{id}",
        "schema_version": 1, "wasm_api_version": wasm_api_version, "provides": provides,
        "published_at": "2026-01-01T00:00:00Z", "download_count": downloads,
    }


def archive(id: str, version: str) -> bytes:
    """The staged extension as a .tar.gz, its manifest saying `version`."""
    folder = STAGED[id]
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode="w") as tar:
        for path in sorted(p for p in folder.rglob("*") if p.is_file()):
            content = path.read_bytes()
            if path.name == "extension.toml":
                content = re.sub(rb'(?m)^version = ".*"$', f'version = "{version}"'.encode(),
                                 content)
            info = tarfile.TarInfo(str(path.relative_to(folder)))
            info.size = len(content)
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(content))
    return gzip.compress(data.getvalue())


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        url = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(url.query)
        parts = url.path.strip("/").split("/")
        if url.path == "/extensions":
            words = query.get("filter", [""])[0].lower()
            found = [metadata(id) for id, plugin in PLUGINS.items()
                     if words in id or words in plugin[0].lower()]
            self.json({"data": found})
        elif url.path == "/extensions/updates":
            ids = query.get("ids", [""])[0].split(",")
            self.json({"data": [metadata(id) for id in ids if id in PLUGINS]})
        elif parts[:1] == ["extensions"] and parts[-1:] == ["download"] and parts[1] in PLUGINS:
            version = parts[2] if len(parts) == 4 else LATEST[parts[1]]
            self.answer(200, "application/gzip", archive(parts[1], version))
        elif parts[:2] == ["e2e", "publish"] and len(parts) == 4 and parts[2] in PLUGINS:
            LATEST[parts[2]] = parts[3]
            self.json({"published": parts[2:]})
        elif parts[:1] == ["files"] and parts[1:] == ["lsp_fixture.py"]:
            self.answer(200, "text/x-python", (HERE / "lsp_fixture.py").read_bytes())
        else:
            self.answer(404, "text/plain", b"not found")

    def json(self, value):
        self.answer(200, "application/json", json.dumps(value).encode())

    def answer(self, status, kind, body):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(PORT_FILE, "w") as f:
    f.write(str(server.server_address[1]))
threading.Timer(LIFETIME, server.shutdown).start()
server.serve_forever()
