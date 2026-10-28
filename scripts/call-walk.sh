#!/usr/bin/env bash
# yap-call-0 (folds the cut yap-call-4) — Wilson's call walk.
#
# Prints the walk steps, then runs `wilson-voice --probe-calls` into
# desktop/src-tauri/tests/fixtures/call_snapshots/<app>.json. It READS only:
# no recording, no network. It never asks for the developer flag that prints
# window-title text, and it refuses to write a snapshot that contains a title
# string. A run from Terminal reports TERMINAL's Accessibility grant, not
# Yap's: browser_titles then comes back null ("AX unknown") and such a
# snapshot can never flip a browser row to verified.
#
# Usage: scripts/call-walk.sh <app-slug> [path/to/wilson-voice]
#   e.g. scripts/call-walk.sh zoom-in-call-recorded
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT/desktop/src-tauri/tests/fixtures/call_snapshots"
SLUG="${1:-}"
BIN="${2:-${YAP_BIN:-}}"

cat <<'STEPS'
Wilson's walk checklist (yap-call-0) — about 10 minutes per app:
  1. Quit every call app. Run this script with slug "idle-recorded": the baseline.
  2. Start a REAL call in the app (Zoom, Teams, FaceTime, Phone, Meet in Chrome,
     Meet in Safari, Slack huddle, Discord, WhatsApp, Webex). Talk for 10 s.
  3. While the call runs, run this script with slug "<app>-in-call-recorded".
  4. Note which process shows "running_input": true and fill the
     "Input process" and "Detected?" columns of docs/loop/CALL-COVERAGE.md.
  5. Listener check: with Yap running (log at <data_dir>/logs/yap.log), start a
     call in an app that is ALREADY a HAL client (Zoom open, not in a call) and
     look for "call_signals scored" within ~1 s. Only the 30 s backstop firing
     means the HAL did not notify on kAudioProcessPropertyIsRunningInput.
  6. Record a meeting during the call; read track 1 from the meeting
     diagnostics row. FaceTime / Phone stay "your side only" until it is non-zero.
  7. A row flips to verified:true in call_catalog.json ONLY with the recorded
     fixture committed (set its "expect" block), and a browser row ONLY from a
     snapshot whose ax_source is "yap".
STEPS

if [ -z "$SLUG" ]; then
  echo "usage: scripts/call-walk.sh <app-slug> [path/to/wilson-voice]" >&2
  exit 2
fi
case "$SLUG" in
  *[!a-z0-9-]*) echo "slug must be lowercase letters, digits and dashes" >&2; exit 2 ;;
esac

if [ -z "$BIN" ]; then
  for cand in "/Applications/Yap.app/Contents/MacOS/wilson-voice" \
              "${CARGO_TARGET_DIR:-$ROOT/desktop/src-tauri/target}/debug/wilson-voice"; do
    if [ -x "$cand" ]; then BIN="$cand"; break; fi
  done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "no wilson-voice binary found; pass its path as the second argument" >&2
  exit 2
fi

TMP="$(mktemp -t yap-call-walk)"
trap 'rm -f "$TMP"' EXIT
"$BIN" --probe-calls > "$TMP"

# Refuse any snapshot that carries a title as text, whatever the key.
if grep -Eq '"(raw_)?title"[[:space:]]*:[[:space:]]*"' "$TMP"; then
  echo "REFUSED: the snapshot contains a window-title string; nothing written" >&2
  exit 3
fi

AX_SOURCE="$(sed -n 's/.*"ax_source"[[:space:]]*:[[:space:]]*"\([a-z]*\)".*/\1/p' "$TMP" | head -1)"
if grep -Eq '"browser_titles"[[:space:]]*:[[:space:]]*null' "$TMP"; then
  TITLES="AX unknown (browser_titles null) — this snapshot can never verify a browser row"
else
  TITLES="window titles read (pattern ids and lengths only)"
fi
case "$AX_SOURCE" in
  terminal) echo "Accessibility grant seen: Terminal's, not Yap's. $TITLES" ;;
  yap) echo "Accessibility grant seen: Yap's. $TITLES" ;;
  *) echo "Accessibility grant seen: unknown process. $TITLES" ;;
esac

DEST="$OUT_DIR/$SLUG.json"
mkdir -p "$OUT_DIR"
cp "$TMP" "$DEST"
echo "wrote $DEST — add its \"expect\" block and a TABLE row in tests/call_signal_matrix.rs"
