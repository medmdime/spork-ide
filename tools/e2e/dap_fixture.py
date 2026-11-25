#!/usr/bin/env python3
"""A debug adapter for the end-to-end tests: it speaks the Debug Adapter Protocol on stdin and
stdout and plays the fixture's program (fixture.sh) instead of running it, so a scenario debugs
without downloading an adapter or asking macOS for Developer Tools Access (which lldb-dap needs).

The program it plays is the fixture's `main`: main.rs line 4 calls `greeting::hello("world")`
(greeting.rs lines 2 and 3), prints "Hello, world!", reaches line 5 and exits with code 0. It
stops at breakpoints on those lines (a condition "false" never holds, a hit condition "n" stops
on the n-th hit, a log message is printed instead of stopping), steps over, into and out, and
evaluates a frame's variables and integer arithmetic. Other lines' breakpoints aren't verified.

For the debugger's actions: on main.rs line 4 its step-in targets (Smart Step Into) are
`greeting::hello("world")` and `std::io::_print(…)`, the second stepping over the call; a frame
restarts (restartFrame) from its first line; `greeting` evaluates to a struct with children
(`name`, `len`); with CodeLLDB's "Rust: on panic" exception filter on, the program stops at
greeting.rs line 3 as if it panicked there; attach (to a process the test started, by its "pid")
plays the program as launch does, in the process's working directory. Each request it gets is
logged, one JSON line each, to the file named by the configuration's "requestLog" (an attached
process's directory's parent's dap-requests.log without one, out of the project: a file
changing in it makes the IDE look at it again), for a scenario to check.

A scenario runs it in place of CodeLLDB's adapter:
  "settings": {"dap": {"CodeLLDB": {"binary": "/usr/bin/python3", "args": ["$E2E/dap_fixture.py"]}}}
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import time

MAIN, GREETING = "src/main.rs", "src/greeting.rs"
# the program's states, one per line it runs: its frames, innermost first, as (function, file,
# line), and what it prints on reaching the state
TRACE = [
    {"frames": [("fixture::main", MAIN, 4)]},
    {"frames": [("fixture::greeting::hello", GREETING, 2), ("fixture::main", MAIN, 4)]},
    {"frames": [("fixture::greeting::hello", GREETING, 3), ("fixture::main", MAIN, 4)]},
    {"frames": [("fixture::main", MAIN, 5)], "prints": "Hello, world!\n"},
]
LOCALS = {"fixture::greeting::hello": [("name", "&str", '"world"')], "fixture::main": []}
CODE_LINES = {(state["frames"][0][1], state["frames"][0][2]) for state in TRACE}
# the calls a state's line makes, for stepInTargets: (id, label, the state stepping into it reaches)
STEP_IN_TARGETS = {0: [(1, 'greeting::hello("world")', 1), (2, "std::io::_print(…)", 3)]}
# CodeLLDB's exception filters
EXCEPTION_FILTERS = [
    {"filter": "cpp_throw", "label": "C++: on throw", "default": False},
    {"filter": "cpp_catch", "label": "C++: on catch", "default": False},
    {"filter": "rust_panic", "label": "Rust: on panic", "default": False},
]
PANICS_AT = 2
# an evaluated struct's children, by its variablesReference
STRUCTS = {2000: [("name", "&str", '"world"'), ("len", "usize", "5")]}


def process_cwd(pid: int) -> str | None:
    """The working directory of the process `pid` (macOS has no /proc: lsof tells it)."""
    found = subprocess.run(["/usr/sbin/lsof", "-a", "-p", str(pid), "-d", "cwd", "-Fn"],
                           capture_output=True, text=True).stdout
    return next((line[1:] for line in found.splitlines() if line.startswith("n")), None)


class Adapter:
    def __init__(self) -> None:
        self.seq = 0
        self.cwd = os.getcwd()
        self.breakpoints: dict[str, list[dict]] = {}
        self.hits: dict[tuple[str, int], int] = {}
        self.at = -1  # the state the program stopped in, -1 before it starts
        self.stop_on_entry = False
        self.exception_filters: list[str] = []
        self.request_log: str | None = None

    def send(self, message: dict) -> None:
        self.seq += 1
        body = json.dumps(dict(message, seq=self.seq)).encode()
        sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        sys.stdout.buffer.flush()

    def event(self, name: str, body: dict | None = None) -> None:
        self.send({"type": "event", "event": name, "body": body or {}})

    def respond(self, request: dict, body: dict | None = None, error: str | None = None) -> None:
        message = {"type": "response", "request_seq": request["seq"], "command": request["command"],
                   "success": error is None, "body": body or {}}
        if error:
            message["message"] = error
            message["body"] = {"error": {"id": 1, "format": error, "showUser": True}}
        self.send(message)

    def relative(self, path: str) -> str:
        return os.path.relpath(path, self.cwd) if os.path.isabs(path) else path

    def frames(self) -> list[tuple[str, str, int]]:
        return TRACE[self.at]["frames"] if 0 <= self.at < len(TRACE) else []

    def local_values(self, frame_id: int) -> list[tuple[str, str, str]]:
        frames = self.frames()
        return LOCALS.get(frames[frame_id - 1][0], []) if 0 < frame_id <= len(frames) else []

    def evaluate(self, expression: str, frame_id: int) -> tuple[str, str]:
        if expression.strip() == "greeting":
            return "Greeting { name: \"world\", len: 5 }", "fixture::Greeting"
        for name, kind, value in self.local_values(frame_id or 1):
            if expression.strip() == name:
                return value, kind
        if re.fullmatch(r"[\d\s+\-*/()]+", expression):
            return str(eval(expression, {"__builtins__": {}})), "i32"
        raise ValueError(f"use of undeclared variable `{expression.strip()}`")

    def interpolate(self, message: str, frame_id: int) -> str:
        def value(match: re.Match) -> str:
            try:
                return self.evaluate(match.group(1), frame_id)[0]
            except ValueError as error:
                return f"<{error}>"
        return re.sub(r"\{([^}]*)\}", value, message)

    def stops_at(self, state: int) -> bool:
        """Whether a breakpoint stops the program on reaching `state`, printing log messages."""
        _, path, line = TRACE[state]["frames"][0]
        for breakpoint in self.breakpoints.get(path, []):
            if breakpoint["line"] != line:
                continue
            if breakpoint.get("condition", "").strip() == "false":
                continue
            key = (path, line)
            self.hits[key] = self.hits.get(key, 0) + 1
            wanted = breakpoint.get("hitCondition", "").strip()
            if wanted.isdigit() and self.hits[key] != int(wanted):
                continue
            if breakpoint.get("logMessage"):
                self.event("output", {"category": "console",
                                      "output": self.interpolate(breakpoint["logMessage"], 1) + "\n"})
                continue
            return True
        return False

    def run(self, until) -> None:
        """Runs from the state after the current one, stopping where `until(state)` or a
        breakpoint says, or to the end."""
        self.event("continued", {"threadId": 1, "allThreadsContinued": True})
        # a program takes a moment to run: the IDE handles the request's answer before the stop,
        # which it may otherwise take as running again
        time.sleep(0.1)
        state = self.at + 1
        while state < len(TRACE):
            if "prints" in TRACE[state]:
                self.event("output", {"category": "stdout", "output": TRACE[state]["prints"]})
            if until(state):
                self.at = state
                self.event("stopped", {"reason": "step", "threadId": 1, "allThreadsStopped": True})
                return
            if state == PANICS_AT and "rust_panic" in self.exception_filters:
                self.at = state
                self.event("stopped", {"reason": "exception", "description": "panicked",
                                       "text": "Rust: on panic", "threadId": 1, "allThreadsStopped": True})
                return
            if self.stops_at(state):
                self.at = state
                self.event("stopped", {"reason": "breakpoint", "threadId": 1, "allThreadsStopped": True})
                return
            state += 1
        self.at = len(TRACE)
        self.event("exited", {"exitCode": 0})
        self.event("terminated")

    def log(self, request: dict) -> None:
        if self.request_log:
            with open(self.request_log, "a") as log:
                log.write(json.dumps({"command": request["command"],
                                      "arguments": request.get("arguments") or {}}) + "\n")

    def handle(self, request: dict) -> bool:
        command, args = request["command"], request.get("arguments") or {}
        if command in ("launch", "attach"):
            if command == "attach" and "pid" in args and not args.get("cwd"):
                args["cwd"] = process_cwd(args["pid"])
            self.request_log = args.get("requestLog") or (
                os.path.join(os.path.dirname(args["cwd"]), "dap-requests.log")
                if command == "attach" and args.get("cwd") else None)
        self.log(request)
        if command == "initialize":
            self.respond(request, {
                "supportsConfigurationDoneRequest": True,
                "supportsConditionalBreakpoints": True,
                "supportsHitConditionalBreakpoints": True,
                "supportsLogPoints": True,
                "supportsEvaluateForHovers": True,
                "supportsTerminateRequest": True,
                "supportsStepInTargetsRequest": True,
                "supportsRestartFrame": True,
                "exceptionBreakpointFilters": EXCEPTION_FILTERS,
            })
            self.event("initialized")
        elif command in ("launch", "attach"):
            self.cwd = args.get("cwd") or (os.path.dirname(os.path.dirname(os.path.dirname(
                args["program"]))) if "program" in args else self.cwd)
            self.stop_on_entry = bool(args.get("stopOnEntry"))
            self.respond(request)
        elif command == "setBreakpoints":
            path = self.relative(args.get("source", {}).get("path", ""))
            self.breakpoints[path] = args.get("breakpoints", [])
            self.respond(request, {"breakpoints": [
                {"id": hash((path, b["line"])) % 100000, "verified": (path, b["line"]) in CODE_LINES,
                 "line": b["line"]}
                | ({} if (path, b["line"]) in CODE_LINES else {"message": "No code at this line"})
                for b in self.breakpoints[path]]})
        elif command == "setExceptionBreakpoints":
            self.exception_filters = list(args.get("filters", []))
            self.respond(request, {"breakpoints": []})
        elif command == "configurationDone":
            self.respond(request)
            self.event("process", {"name": "fixture", "startMethod": "launch", "isLocalProcess": True})
            self.event("thread", {"reason": "started", "threadId": 1})
            self.run(lambda state: self.stop_on_entry and state == 0)
        elif command == "threads":
            self.respond(request, {"threads": [{"id": 1, "name": "main"}]})
        elif command == "stackTrace":
            frames = [{"id": ix + 1, "name": name, "line": line, "column": 1,
                       "source": {"name": os.path.basename(path), "path": os.path.join(self.cwd, path)}}
                      for ix, (name, path, line) in enumerate(self.frames())]
            self.respond(request, {"stackFrames": frames, "totalFrames": len(frames)})
        elif command == "scopes":
            frame = args.get("frameId", 1)
            self.respond(request, {"scopes": [{"name": "Locals", "presentationHint": "locals",
                                                "variablesReference": 1000 + frame, "expensive": False}]})
        elif command == "variables":
            reference = args.get("variablesReference", 0)
            values = STRUCTS.get(reference) or self.local_values(reference - 1000)
            self.respond(request, {"variables": [{"name": name, "value": value, "type": kind,
                                                  "variablesReference": 0}
                                                 for name, kind, value in values]})
        elif command == "evaluate":
            try:
                value, kind = self.evaluate(args.get("expression", ""), args.get("frameId", 1))
                reference = 2000 if kind == "fixture::Greeting" else 0
                self.respond(request, {"result": value, "type": kind, "variablesReference": reference})
            except ValueError as error:
                self.respond(request, error=str(error))
        elif command == "continue":
            self.respond(request, {"allThreadsContinued": True})
            self.run(lambda state: False)
        elif command == "stepIn" and "targetId" in args:
            self.respond(request)
            reached = {target: state for target, _, state in STEP_IN_TARGETS.get(self.at, [])}
            goal = reached.get(args["targetId"], self.at + 1)
            self.run(lambda state: state == goal)
        elif command in ("next", "stepIn", "stepOut"):
            self.respond(request)
            depth = len(self.frames())
            until = {"next": lambda state: len(TRACE[state]["frames"]) <= depth,
                     "stepIn": lambda state: True,
                     "stepOut": lambda state: len(TRACE[state]["frames"]) < depth}[command]
            self.run(until)
        elif command == "stepInTargets":
            targets = STEP_IN_TARGETS.get(self.at, [])
            self.respond(request, {"targets": [{"id": target, "label": label}
                                               for target, label, _ in targets]})
        elif command == "restartFrame":
            frames = self.frames()
            frame = args.get("frameId", 1)
            if not 0 < frame <= len(frames):
                self.respond(request, error="no such frame")
                return True
            # the first state the frame's function ran in, at the frame's depth
            depth = len(frames) - frame + 1
            name = frames[frame - 1][0]
            self.respond(request)
            self.at = next(ix for ix, state in enumerate(TRACE)
                           if len(state["frames"]) == depth and state["frames"][0][0] == name)
            self.event("stopped", {"reason": "restart", "threadId": 1, "allThreadsStopped": True})
        elif command == "pause":
            self.respond(request)
            self.event("stopped", {"reason": "pause", "threadId": 1, "allThreadsStopped": True})
        elif command in ("disconnect", "terminate"):
            self.respond(request)
            self.event("terminated")
            return command == "terminate"
        else:
            self.respond(request, error=f"{command} isn't supported")
        return True

    def serve(self) -> None:
        stdin = sys.stdin.buffer
        while True:
            length = None
            while True:
                line = stdin.readline()
                if not line:
                    return
                if line.strip() == b"":
                    break
                name, _, value = line.decode().partition(":")
                if name.strip().lower() == "content-length":
                    length = int(value)
            if length is None:
                continue
            request = json.loads(stdin.read(length))
            try:
                if request.get("type") == "request" and not self.handle(request):
                    return
            except Exception:
                # a scenario reads the log: the adapter's own failure shows there
                if self.request_log:
                    import traceback
                    with open(self.request_log, "a") as log:
                        log.write(json.dumps({"command": "error",
                                              "arguments": {"trace": traceback.format_exc()}}) + "\n")
                raise
            if request.get("command") == "disconnect":
                return


if __name__ == "__main__":
    Adapter().serve()
