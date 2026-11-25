#!/usr/bin/env bash
# Black-box inventory of an installed Fork.app: metadata, linked libraries,
# UI surface (nib names), bundled helper tools. Reads files only; never executes app code
# except `--version` on the bundled git / git-lfs.
set -euo pipefail

APP="${1:-$HOME/Applications/Fork.app}"
OUT="${2:-inventory}"
C="$APP/Contents"; R="$C/Resources"
[ -d "$C" ] || { echo "No app bundle at $APP" >&2; exit 1; }
mkdir -p "$OUT"

plutil -p "$C/Info.plist"                         > "$OUT/info-plist.txt"
otool -L "$C/MacOS/$(defaults read "$C/Info" CFBundleExecutable)" > "$OUT/linked-libs.txt"
ls "$C/Frameworks"                                > "$OUT/frameworks.txt"
ls "$R" | grep '\.nib$' | sed 's/\.nib$//' | sort > "$OUT/ui-surfaces.txt"
ls "$R" | grep -v '\.nib$' | sort                 > "$OUT/resources-other.txt"

{
  echo "git:      $("$R/git-instance/bin/git" --version)"
  echo "git-lfs:  $("$R/git-instance/git-lfs/git-lfs" version)"
  echo "gitflow:  $(grep -m1 -oE '[0-9]+\.[0-9]+\.[0-9]+' "$R/gitflow-avh/git-flow-version" || echo unknown)"
  for b in fork_cli fork_askpasswd fork_ri; do echo "$b: $(lipo -archs "$R/$b") Mach-O"; done
} > "$OUT/bundled-tools.txt"

echo "Wrote $(ls "$OUT" | wc -l | tr -d ' ') files to $OUT/ ($(wc -l < "$OUT/ui-surfaces.txt" | tr -d ' ') UI surfaces)"
