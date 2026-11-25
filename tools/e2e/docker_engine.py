#!/usr/bin/env python3
"""A Docker Engine API on a Unix socket for the Services tool window's Docker scenarios, so they
need no Docker: it answers the endpoints Spork IDE calls (the versioned REST API, `/v1.45/...`)
from fixture data it changes as the calls ask (start, stop, delete, pull, create, prune), streams
logs and events, and hijacks the connection for attach and exec (a tiny shell answering `echo`,
`pwd`, `whoami` and `exit` for Create Terminal, `ls -1Ap` for the Files tab). Each request goes to
<calls log> as "METHOD /path?query" (with " <body>" when it has one). Stops after <seconds>.

usage: docker_engine.py <socket path> <calls log> [seconds]
"""

import http.server
import json
import os
import re
import socketserver
import sys
import threading
import time
import urllib.parse

SOCKET = sys.argv[1]
CALLS = sys.argv[2]
LIFETIME = float(sys.argv[3]) if len(sys.argv) > 3 else 300
LOCK = threading.Lock()


def container(cid, name, image, image_id, state, status, labels=None, env=None, ports=None, mounts=None,
              tty=False):
    return {"Id": cid, "Names": ["/" + name], "Image": image, "ImageID": image_id, "State": state,
            "Status": status, "Labels": labels or {}, "Created": 1732300000,
            "Mounts": mounts or [], "_env": env or [], "_ports": ports or {}, "_tty": tty}


def compose(project, service):
    return {"com.docker.compose.project": project, "com.docker.compose.service": service,
            "com.docker.compose.project.config_files": "/work/shop/compose.yaml",
            "com.docker.compose.project.working_dir": "/work/shop"}


CONTAINERS = [
    container("c0ffee0001aa0000000000000000000000000000000000000000000000000001", "nginx-demo",
              "nginx:latest", "sha256:1111aaaa0000000000000000000000000000000000000000000000000000000a",
              "running", "Up 2 hours", env=["PATH=/usr/local/sbin:/usr/bin", "NGINX_VERSION=1.27.0"],
              ports={"80/tcp": [{"HostIp": "0.0.0.0", "HostPort": "8080"}]},
              mounts=[{"Type": "volume", "Name": "web-data", "Source": "/var/lib/docker/volumes/web-data",
                       "Destination": "/usr/share/nginx/html", "RW": True}]),
    container("c0ffee0002bb0000000000000000000000000000000000000000000000000002", "redis-cache",
              "redis:7", "sha256:2222bbbb0000000000000000000000000000000000000000000000000000000b",
              "exited", "Exited (0) 3 hours ago"),
    container("c0ffee0003cc0000000000000000000000000000000000000000000000000003", "shop-api-1",
              "shop-api:latest", "sha256:4444dddd0000000000000000000000000000000000000000000000000000000d",
              "running", "Up 5 minutes", labels=compose("shop", "api")),
    container("c0ffee0004dd0000000000000000000000000000000000000000000000000004", "shop-db-1",
              "postgres:16", "sha256:3333cccc0000000000000000000000000000000000000000000000000000000c",
              "running", "Up 5 minutes", labels=compose("shop", "db"),
              mounts=[{"Type": "volume", "Name": "shop_db-data", "Source": "", "Destination": "/var/lib/postgresql/data",
                       "RW": True}]),
]

IMAGES = [
    {"Id": "sha256:1111aaaa0000000000000000000000000000000000000000000000000000000a", "RepoTags": ["nginx:latest"],
     "Size": 192_340_000, "Created": 1731000000, "Labels": {"maintainer": "NGINX Docker Maintainers"}},
    {"Id": "sha256:2222bbbb0000000000000000000000000000000000000000000000000000000b", "RepoTags": ["redis:7"],
     "Size": 117_000_000, "Created": 1730000000, "Labels": {}},
    {"Id": "sha256:3333cccc0000000000000000000000000000000000000000000000000000000c", "RepoTags": ["postgres:16"],
     "Size": 432_000_000, "Created": 1729000000, "Labels": {}},
    {"Id": "sha256:4444dddd0000000000000000000000000000000000000000000000000000000d",
     "RepoTags": ["shop-api:latest"], "Size": 88_000_000, "Created": 1732000000, "Labels": {"app": "shop"}},
    {"Id": "sha256:5555eeee0000000000000000000000000000000000000000000000000000000e",
     "RepoTags": ["<none>:<none>"], "Size": 5_000_000, "Created": 1728000000, "Labels": {}},
]

NETWORKS = [
    {"Name": "bridge", "Id": "n1bridge000000000000000000000000000000000000000000000000000000001", "Driver": "bridge",
     "Scope": "local", "Internal": False, "Attachable": False, "EnableIPv6": False,
     "IPAM": {"Config": [{"Subnet": "172.17.0.0/16", "Gateway": "172.17.0.1"}]},
     "Containers": {"c0ffee0001aa": {"Name": "nginx-demo", "IPv4Address": "172.17.0.2/16"}}},
    {"Name": "host", "Id": "n2host00000000000000000000000000000000000000000000000000000000002", "Driver": "host",
     "Scope": "local", "Internal": False, "Attachable": False, "EnableIPv6": False, "IPAM": {"Config": []},
     "Containers": {}},
    {"Name": "shop_default", "Id": "n3shop00000000000000000000000000000000000000000000000000000000003",
     "Driver": "bridge", "Scope": "local", "Internal": False, "Attachable": True, "EnableIPv6": False,
     "IPAM": {"Config": [{"Subnet": "172.18.0.0/16", "Gateway": "172.18.0.1"}]}, "Containers": {}},
]

VOLUMES = [
    {"Name": "shop_db-data", "Driver": "local", "Mountpoint": "/var/lib/docker/volumes/shop_db-data/_data"},
    {"Name": "unused-data", "Driver": "local", "Mountpoint": "/var/lib/docker/volumes/unused-data/_data"},
    {"Name": "web-data", "Driver": "local", "Mountpoint": "/var/lib/docker/volumes/web-data/_data"},
]

LOGS = {
    "nginx-demo": [(1, "/docker-entrypoint.sh: Configuration complete; ready for start up\n"),
                   (1, "172.17.0.1 - - \"GET / HTTP/1.1\" 200 615\n"),
                   (2, "nginx: [warn] the \"user\" directive makes sense only as root\n")],
}

FOLDERS = {"/": ["bin/", "etc/", "usr/", "docker-entrypoint.sh"], "/etc/": ["hostname", "nginx/", "passwd"]}

EXECS = {}


def public(item):
    return {key: value for key, value in item.items() if not key.startswith("_")}


def find_container(ref):
    for c in CONTAINERS:
        if c["Id"] == ref or c["Id"].startswith(ref) or c["Names"][0] == "/" + ref:
            return c
    return None


def find_image(ref):
    for image in IMAGES:
        if image["Id"] == ref or ref in image["RepoTags"]:
            return image
    return None


def frame(kind, text):
    data = text.encode()
    return bytes([kind, 0, 0, 0]) + len(data).to_bytes(4, "big") + data


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def body(self):
        length = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(length) if length else b""

    def record(self, body):
        with LOCK, open(CALLS, "a") as log:
            log.write(f"{self.command} {self.path}" + (f" {body.decode()}" if body else "") + "\n")

    def send(self, status, payload=None, content_type="application/json"):
        data = b"" if payload is None else (payload if isinstance(payload, bytes) else json.dumps(payload).encode())
        self.send_response(status)
        if status != 204:
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
        self.send_header("Api-Version", "1.47")
        self.end_headers()
        self.wfile.write(data)

    def error(self, status, message):
        self.send(status, {"message": message})

    def chunked(self):
        self.send_response(200)
        self.send_header("Content-Type", "application/vnd.docker.multiplexed-stream")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()

    def chunk(self, data):
        self.wfile.write(f"{len(data):x}\r\n".encode() + data + b"\r\n")
        self.wfile.flush()

    def hold(self):
        """Keeps a stream open until the IDE closes it."""
        self.connection.settimeout(0.5)
        while True:
            try:
                if self.connection.recv(1) == b"":
                    return
            except TimeoutError:
                continue
            except OSError:
                return

    def upgrade(self):
        self.send_response(101, "UPGRADED")
        self.send_header("Content-Type", "application/vnd.docker.raw-stream")
        self.send_header("Connection", "Upgrade")
        self.send_header("Upgrade", "tcp")
        self.end_headers()
        self.wfile.flush()

    def do_GET(self):
        self.handle_any()

    def do_POST(self):
        self.handle_any()

    def do_DELETE(self):
        self.handle_any()

    def do_HEAD(self):
        self.handle_any()

    def handle_any(self):
        body = self.body()
        self.record(body)
        url = urllib.parse.urlparse(self.path)
        path = re.sub(r"^/v\d+\.\d+", "", url.path)
        query = urllib.parse.parse_qs(url.query)
        method = self.command
        try:
            self.route(method, path, query, json.loads(body) if body else None)
        except (BrokenPipeError, ConnectionResetError):
            self.close_connection = True

    def route(self, method, path, query, body):
        parts = [urllib.parse.unquote(p) for p in path.strip("/").split("/")]
        if path == "/_ping":
            return self.send(200, b"OK", "text/plain")
        if path == "/version":
            return self.send(200, {"Version": "27.3.1", "ApiVersion": "1.47", "MinAPIVersion": "1.24",
                                   "Os": "linux", "Arch": "arm64"})
        if path == "/events":
            self.chunked()
            self.hold()
            self.close_connection = True
            return None
        if parts[0] == "containers":
            return self.containers(method, parts[1:], query, body)
        if parts[0] == "exec":
            return self.execs(method, parts[1:], query)
        if parts[0] == "images":
            return self.images(method, parts[1:], query)
        if parts[0] == "networks":
            return self.networks(method, parts[1:])
        if parts[0] == "volumes":
            return self.volumes(method, parts[1:])
        if path == "/build/prune":
            return self.send(200, {"CachesDeleted": [], "SpaceReclaimed": 0})
        return self.error(404, f"page not found: {path}")

    def containers(self, method, parts, query, body):
        with LOCK:
            if parts == ["json"]:
                return self.send(200, [public(c) for c in CONTAINERS])
            if parts == ["prune"] and method == "POST":
                deleted = [c["Id"] for c in CONTAINERS if c["State"] in ("exited", "created")]
                CONTAINERS[:] = [c for c in CONTAINERS if c["Id"] not in deleted]
                return self.send(200, {"ContainersDeleted": deleted, "SpaceReclaimed": 0})
            if parts == ["create"] and method == "POST":
                name = query.get("name", ["new-container"])[0]
                image = (body or {}).get("Image", "")
                found = find_image(image) or find_image(image + ":latest")
                if not found:
                    return self.error(404, f"No such image: {image}")
                cid = f"c0ffee{len(CONTAINERS) + 1:04d}ff" + "0" * 52
                ports = {port: bindings for port, bindings in
                         ((body or {}).get("HostConfig", {}).get("PortBindings") or {}).items()}
                CONTAINERS.append(container(cid, name, image, found["Id"], "created", "Created",
                                            env=(body or {}).get("Env") or [], ports=ports))
                return self.send(201, {"Id": cid, "Warnings": []})
            c = find_container(parts[0]) if parts else None
            if not c:
                return self.error(404, f"No such container: {parts[0] if parts else ''}")
            action = parts[1] if len(parts) > 1 else ""
            if method == "DELETE":
                CONTAINERS.remove(c)
                return self.send(204)
            if action == "json":
                inspect = {"Id": c["Id"], "Name": c["Names"][0], "Image": c["ImageID"],
                           "State": {"Status": c["State"], "Running": c["State"] == "running"},
                           "Config": {"Image": c["Image"], "Env": c["_env"], "Tty": c["_tty"], "Labels": c["Labels"]},
                           "NetworkSettings": {"Ports": c["_ports"]}, "Mounts": c["Mounts"]}
                return self.send(200, inspect)
            states = {"start": ("running", "Up Less than a second"), "restart": ("running", "Up Less than a second"),
                      "stop": ("exited", "Exited (0) Less than a second ago"), "pause": ("paused", "Up 2 hours (Paused)"),
                      "unpause": ("running", "Up 2 hours")}
            if action in states and method == "POST":
                c["State"], c["Status"] = states[action]
                return self.send(204)
            if action == "exec" and method == "POST":
                eid = f"exec{len(EXECS) + 1}"
                EXECS[eid] = {"container": c, "body": body or {}}
                return self.send(201, {"Id": eid})
            name = c["Names"][0][1:]
        if action == "logs":
            self.chunked()
            for kind, line in LOGS.get(name, [(1, f"{name} started\n")]):
                self.chunk(frame(kind, line))
            if query.get("follow") == ["1"]:
                self.hold()
            self.chunk(b"")
            self.close_connection = True
            return None
        if action == "attach":
            self.upgrade()
            self.wfile.write(frame(1, f"{name}: attached output\n"))
            self.wfile.flush()
            self.hold()
            self.close_connection = True
            return None
        return self.error(404, f"page not found: {action}")

    def execs(self, method, parts, query):
        exec_ = EXECS.get(parts[0]) if parts else None
        if not exec_:
            return self.error(404, "No such exec instance")
        action = parts[1] if len(parts) > 1 else ""
        if action == "resize":
            exec_["size"] = (query.get("h", ["0"])[0], query.get("w", ["0"])[0])
            return self.send(201)
        if action == "json":
            return self.send(200, {"ID": parts[0], "Running": False, "ExitCode": exec_.get("exit", 0)})
        if action != "start":
            return self.error(404, f"page not found: {action}")
        command = exec_["body"].get("Cmd") or []
        self.upgrade()
        self.close_connection = True
        if command[:2] == ["ls", "-1Ap"]:
            folder = command[-1]
            entries = FOLDERS.get(folder if folder.endswith("/") else folder + "/")
            if entries is None:
                self.wfile.write(frame(2, f"ls: {folder}: No such file or directory\n"))
                exec_["exit"] = 1
            else:
                self.wfile.write(frame(1, "".join(entry + "\n" for entry in entries)))
            self.wfile.flush()
            return None
        self.shell(exec_)
        return None

    def shell(self, exec_):
        """A shell on a TTY: echoes what is typed, runs a line on ⏎."""
        user = exec_["body"].get("User") or "nginx"
        prompt = ("/ # " if user == "root" else "/ $ ").encode()
        self.wfile.write(prompt)
        self.wfile.flush()
        line = b""
        while True:
            data = self.rfile.read1(1024)
            if not data:
                return
            for byte in data:
                char = bytes([byte])
                if char in (b"\r", b"\n"):
                    command = line.decode(errors="replace").strip()
                    line = b""
                    self.wfile.write(b"\r\n")
                    if command == "exit":
                        self.wfile.flush()
                        return
                    if command.startswith("echo "):
                        self.wfile.write(command[5:].encode() + b"\r\n")
                    elif command == "pwd":
                        self.wfile.write(b"/\r\n")
                    elif command == "whoami":
                        self.wfile.write(user.encode() + b"\r\n")
                    elif command:
                        self.wfile.write(f"sh: {command}: not found\r\n".encode())
                    self.wfile.write(prompt)
                elif byte == 0x7f:
                    line = line[:-1]
                    self.wfile.write(b"\b \b")
                else:
                    line += char
                    self.wfile.write(char)
                self.wfile.flush()

    def images(self, method, parts, query):
        with LOCK:
            if parts == ["json"]:
                return self.send(200, [{k: v for k, v in image.items() if k != "Labels"} for image in IMAGES])
            if parts == ["prune"]:
                return self.send(200, {"ImagesDeleted": [], "SpaceReclaimed": 0})
            if parts == ["create"] and method == "POST":
                name, tag = query.get("fromImage", [""])[0], query.get("tag", ["latest"])[0]
                if name == "missing":
                    return self.error(404, "pull access denied for missing, repository does not exist")
                ref = f"{name}:{tag}"
                if not find_image(ref):
                    IMAGES.append({"Id": "sha256:6666ffff" + "0" * 55 + "f", "RepoTags": [ref], "Size": 4_300_000,
                                   "Created": 1732100000, "Labels": {}})
        if parts == ["create"]:
            self.chunked()
            for line in [{"status": f"Pulling from library/{name}", "id": tag},
                         {"status": "Pull complete", "id": "a1b2c3d4e5f6"},
                         {"status": f"Digest: sha256:{'9' * 64}"},
                         {"status": f"Status: Downloaded newer image for {ref}"}]:
                self.chunk((json.dumps(line) + "\r\n").encode())
            self.chunk(b"")
            return None
        with LOCK:
            ref = "/".join(parts[:-1]) if parts[-1] in ("json", "history") else "/".join(parts)
            image = find_image(ref)
            if not image:
                return self.error(404, f"No such image: {ref}")
            if method == "DELETE":
                if any(c["ImageID"] == image["Id"] for c in CONTAINERS):
                    return self.error(409, f"conflict: unable to delete {ref} (image is being used by a container)")
                IMAGES.remove(image)
                return self.send(200, [{"Deleted": image["Id"]}])
            if parts[-1] == "history":
                return self.send(200, [
                    {"Id": image["Id"], "Created": image["Created"], "CreatedBy": "CMD [\"nginx\" \"-g\" \"daemon off;\"]",
                     "Size": 0, "Comment": "", "Tags": image["RepoTags"]},
                    {"Id": "<missing>", "Created": image["Created"], "CreatedBy": "COPY docker-entrypoint.sh / # buildkit",
                     "Size": 1_620, "Comment": "buildkit.dockerfile.v0", "Tags": None},
                    {"Id": "<missing>", "Created": image["Created"] - 100, "CreatedBy": "ADD rootfs.tar.xz / # buildkit",
                     "Size": image["Size"] - 1_620, "Comment": "", "Tags": None}])
            return self.send(200, {"Id": image["Id"], "RepoTags": image["RepoTags"], "Size": image["Size"],
                                   "Created": "2024-11-07T10:00:00Z",
                                   "Config": {"Labels": image["Labels"], "Env": ["PATH=/usr/bin"]}})

    def networks(self, method, parts):
        with LOCK:
            if not parts:
                return self.send(200, [{k: v for k, v in n.items() if k != "Containers"} for n in NETWORKS])
            if parts == ["prune"]:
                return self.send(200, {"NetworksDeleted": []})
            network = next((n for n in NETWORKS if n["Id"] == parts[0] or n["Name"] == parts[0]), None)
            if not network:
                return self.error(404, f"network {parts[0]} not found")
            if method == "DELETE":
                if network["Name"] in ("bridge", "host", "none"):
                    return self.error(403, f"{network['Name']} is a pre-defined network and cannot be removed")
                NETWORKS.remove(network)
                return self.send(204)
            return self.send(200, network)

    def volumes(self, method, parts):
        with LOCK:
            if not parts:
                return self.send(200, {"Volumes": VOLUMES, "Warnings": None})
            if parts == ["prune"]:
                used = {m["Name"] for c in CONTAINERS for m in c["Mounts"]}
                deleted = [v["Name"] for v in VOLUMES if v["Name"] not in used]
                VOLUMES[:] = [v for v in VOLUMES if v["Name"] in used]
                return self.send(200, {"VolumesDeleted": deleted, "SpaceReclaimed": 0})
            volume = next((v for v in VOLUMES if v["Name"] == parts[0]), None)
            if not volume:
                return self.error(404, f"get {parts[0]}: no such volume")
            if method == "DELETE":
                VOLUMES.remove(volume)
                return self.send(204)
            return self.send(200, volume)


class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
    daemon_threads = True


def main():
    os.makedirs(os.path.dirname(os.path.abspath(SOCKET)), exist_ok=True)
    if os.path.exists(SOCKET):
        os.remove(SOCKET)
    server = Server(SOCKET, Handler)
    threading.Timer(LIFETIME, server.shutdown).start()
    server.serve_forever()
    os.remove(SOCKET)


if __name__ == "__main__":
    main()
