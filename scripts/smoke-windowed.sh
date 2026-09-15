#!/usr/bin/env bash
#
# Y0-E — the structural windowed smoke.
#
# The merge gate for this repo is eight headless commands: tsc, vitest, vite,
# two cargo builds and two cargo test runs. Not one of them can see a pill that
# never animates, a settings panel that overflows its window at 720 wide, or a
# list that renders nothing and never says so. This script is the instrument
# that can, and it ships BEFORE the UI items it judges rather than ten items
# after them.
#
# It walks the seven Nav views at two window sizes, captures a PNG per view per
# size, and FAILS on five STRUCTURAL conditions:
#   1. a view whose content region renders no text at all
#   2. a spinner still present after 10 s
#   3. a horizontal scrollbar on the document
#   4. any element's box outside the window bounds
#   5. a view rendering zero rows AND carrying no data-empty-state
#
# It does NOT assert pixel equality against a golden image. The conditions are
# structural on purpose, so a legitimate restyle never has to relitigate a PNG.
#
# IT IS EXPECTED TO FAIL ON TODAY'S TREE. That failure is the baseline, and it
# is written down in docs/loop/SMOKE-BASELINE.md. Do not weaken a detector to
# make this green — record the red and fix the views.
#
# MODES
#   (none)                capture a run, judge it, exit non-zero on any finding
#   --check-only          re-assert the LAST captured run. A convenience for a
#                         pre-flight, never a substitute for a capture, so it
#                         exits NON-ZERO when no run exists.
#   --self-test-must-fail inject a synthetic broken view and require the
#                         detector to report it — the detector's own
#                         non-vacuity proof. The run it performs contains a
#                         deliberate break, so a SATISFIED self-test still
#                         exits non-zero (1); a blind detector exits 9.
#
# ENV
#   YAP_SMOKE_PORT        vite preview port (default 5273)
#   YAP_SMOKE_CHROME      path to a Chromium binary that speaks CDP
#   YAP_SMOKE_SKIP_BUILD  1 to reuse desktop/dist instead of rebuilding
#   YAP_SMOKE_SHOTS       capture root (default docs/smoke-shots)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PORT="${YAP_SMOKE_PORT:-5273}"
DRIVER="$ROOT/scripts/smoke-windowed.mjs"

# The two window sizes under test. 980x700 is the comfortable desktop window;
# 720x520 is the smallest size the window is allowed to reach, which is where
# overflow and clipping actually show up.
export YAP_SMOKE_W1=980 YAP_SMOKE_H1=700
export YAP_SMOKE_W2=720 YAP_SMOKE_H2=520

# Condition 5's marker attribute. A view with nothing to show must SAY it has
# nothing to show; data-empty-state is how it says so.
export YAP_SMOKE_EMPTY_ATTR="data-empty-state"

# Never point a smoke run at a real history file. Y0-D's --smoke mode reads this
# for per-instance state isolation; the harness exports it for every mode so no
# path in this script can reach the user's SQLite history or settings.
export YAP_DATA_DIR="${YAP_DATA_DIR_OVERRIDE:-$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-data.XXXXXX")}"

case "${1:-}" in
  --check-only)
    # Needs no browser and no server: it only re-reads the last capture.
    exec node "$DRIVER" --check-only
    ;;
  --self-test-must-fail)
    # Needs a browser but no server: the synthetic view is built in the page.
    exec node "$DRIVER" --self-test-must-fail
    ;;
esac

# ── build the exact bundle the app loads ────────────────────────────────────
# tauri.conf.json frontendDist is "../dist", so this is the same artifact the
# packaged app ships. Judging a stale dist is judging code nobody wrote.
if [ "${YAP_SMOKE_SKIP_BUILD:-0}" != "1" ]; then
  echo "smoke: building desktop/dist (set YAP_SMOKE_SKIP_BUILD=1 to reuse)"
  ( cd "$ROOT/desktop" && npm run build >/dev/null )
elif [ ! -f "$ROOT/desktop/dist/index.html" ]; then
  echo "smoke: YAP_SMOKE_SKIP_BUILD=1 but desktop/dist/index.html does not exist" >&2
  exit 2
fi

# ── serve it and walk it ────────────────────────────────────────────────────
PREVIEW_LOG="$(mktemp "${TMPDIR:-/tmp}/yap-smoke-preview.XXXXXX")"
( cd "$ROOT/desktop" && npm run preview -- --port "$PORT" --strictPort >"$PREVIEW_LOG" 2>&1 ) &
PREVIEW_PID=$!
cleanup() {
  kill "$PREVIEW_PID" 2>/dev/null || true
  wait "$PREVIEW_PID" 2>/dev/null || true
}
trap cleanup EXIT

up=0
for _ in $(seq 1 120); do
  if curl -fsS "http://127.0.0.1:$PORT/index.html" -o /dev/null 2>/dev/null; then up=1; break; fi
  sleep 0.5
done
if [ "$up" -ne 1 ]; then
  echo "smoke: vite preview never came up on 127.0.0.1:$PORT" >&2
  cat "$PREVIEW_LOG" >&2
  exit 2
fi

export YAP_SMOKE_URL="http://127.0.0.1:$PORT"
# Read the driver's exit code BARE. Piping it anywhere would report the pipe's
# status instead, which is how a loop records a false green.
set +e
node "$DRIVER" "$@"
rc=$?
set -e
exit "$rc"
