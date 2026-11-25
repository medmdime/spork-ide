#!/usr/bin/env bash
# Launch Fork with git's own tracing on, so every git command it runs is logged.
# This records the app's behaviour at its public interface (the git CLI), which is
# the main input for the behaviour spec. Usage: scripts/trace-git.sh [logfile]
set -euo pipefail

APP="${APP:-$HOME/Applications/Fork.app}"
LOG="${1:-traces/git-trace.log}"
mkdir -p "$(dirname "$LOG")"
LOG="$(cd "$(dirname "$LOG")" && pwd)/$(basename "$LOG")"

osascript -e 'quit app "Fork"' 2>/dev/null || true
echo "Tracing to $LOG — do ONE action in Fork (e.g. 'stage hunk'), then Ctrl-C."
# GIT_TRACE must be an absolute path to write to a file instead of stderr.
GIT_TRACE="$LOG" GIT_TRACE_SETUP=0 "$APP/Contents/MacOS/Fork"
