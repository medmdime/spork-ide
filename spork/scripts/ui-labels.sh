#!/usr/bin/env bash
# Pull the user-visible labels (button/checkbox/menu titles) out of each compiled nib.
# Output is a UX checklist per dialog: which options exist. Use it to write specs; don't copy prose.
set -euo pipefail
APP="${1:-$HOME/Applications/Fork.app}"; OUT="${2:-inventory/ui-labels}"
R="$APP/Contents/Resources"; mkdir -p "$OUT"

labels() {
  # keep human-readable phrases; drop IB placeholders and ICC/pasteboard noise
  strings -a "$@" \
  | grep -E "^[A-Z][a-z]*([ -][A-Za-z0-9(\"'][A-Za-z0-9'’()/.,%&+-]*)*[:…?.!]?\$" \
  | grep -E '^.{4,}$' \
  | grep -vE '^(Encoding|Table View|Copyright|Generic Gray|Text Cell|Table View Cell|Item [0-9]|Box|Label|Multiline Label|Push Button|Check Box|Image View|Scroll View|Custom View|Window|System|Fork|Apple|NeXT|Text Field|Static Text|Table Column|Column|Outline View|Button|Title|Menu|Lorem|Popover|Content View|Clip View|View|Split View|Tab View|Stack View|Radio|Pop Up|Format)\b' \
  | awk '!seen[$0]++'
}

for n in "$R"/*.nib "$R"/Base.lproj/*.nib; do
  name=$(basename "$n" .nib)
  if [ -d "$n" ]; then labels "$n"/* ; else labels "$n"; fi > "$OUT/$name.txt" || true
done
echo "Labels for $(ls "$OUT" | wc -l | tr -d ' ') nibs in $OUT/"
