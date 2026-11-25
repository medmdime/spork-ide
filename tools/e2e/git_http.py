#!/usr/bin/env python3
"""A git server over HTTP on 127.0.0.1 that asks for a password, for the end-to-end tests of git's
prompts: it serves the repositories under <root> with `git http-backend` to user `ada`, password
`s3cret`, writes the port it took to <port file> and stops after <seconds>.

usage: git_http.py <root> <port file> [seconds]
"""

import base64
import http.server
import os
import subprocess
import sys
import threading

ROOT, PORT_FILE = sys.argv[1], sys.argv[2]
LIFETIME = float(sys.argv[3]) if len(sys.argv) > 3 else 180
CREDENTIALS = "Basic " + base64.b64encode(b"ada:s3cret").decode()


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.serve()

    def do_POST(self):
        self.serve()

    def serve(self):
        if self.headers.get("Authorization") != CREDENTIALS:
            self.send_response(401)
            self.send_header("WWW-Authenticate", 'Basic realm="fixture"')
            self.end_headers()
            return
        path, _, query = self.path.partition("?")
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        env = dict(os.environ, GIT_PROJECT_ROOT=ROOT, GIT_HTTP_EXPORT_ALL="1", PATH_INFO=path,
                   QUERY_STRING=query, REQUEST_METHOD=self.command, REMOTE_USER="ada",
                   CONTENT_TYPE=self.headers.get("Content-Type", ""), CONTENT_LENGTH=str(len(body)))
        out = subprocess.run(["git", "http-backend"], input=body, env=env, capture_output=True).stdout
        head, _, payload = out.partition(b"\r\n\r\n")
        status, headers = 200, []
        for line in head.decode().split("\r\n"):
            name, _, value = line.partition(": ")
            if name.lower() == "status":
                status = int(value.split()[0])
            elif name:
                headers.append((name, value))
        self.send_response(status)
        for name, value in headers:
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open(PORT_FILE, "w") as port:
    port.write(str(server.server_address[1]))
threading.Timer(LIFETIME, server.shutdown).start()
server.serve_forever()
