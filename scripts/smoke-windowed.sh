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
# Y7-B extends it to the whole surface. ONE invocation is ONE run directory,
# docs/smoke-shots/<run>/, holding:
#   seeded-*.png   a deterministic lived-in state (scripts/smoke-fixtures-daily.json:
#                  history, a dictionary entry, a scratchpad note, meetings)
#   empty-*.png    an EMPTY state (scripts/smoke-fixtures.json) — the run that
#                  catches a missing empty state, and the more important one
#   pill/          the float pill for every LivePhase x all three pill_position
#                  docks (bottom/left/right) x both characters, via
#                  scripts/pill-matrix.mjs and its own three pill detectors
# Each state walks all seven Nav views AND all eight SettingsTabs at 980x700
# (the configured default) and 720x520 (tauri.conf.json minWidth/minHeight).
#
# HOW IT IS DRIVEN — AND WHY NOT THE APP'S OWN WEBVIEW. The Tauri window is a
# WKWebView, and a custom-protocol build exposes no remote-debugging port (there
# is no CDP endpoint on WKWebView at all; Safari's Web Inspector cannot be
# scripted). So this drives the SAME built desktop/dist bundle the app loads
# (frontendDist "../dist") in headless Chrome (--headless=new: never a visible
# window) over a bare CDP WebSocket, with the Tauri bridge shimmed from the
# fixture sets. No browser-automation package is added to desktop/package.json.
# The pill's phase is forced through float-main's dev-only query handle
# (src/pill/forced.ts), compiled into the matrix's scratch bundle only. For a
# REAL app run, lib.rs's smoke_forced_float_query() does the same for the
# native float window — compiled out of a shipped build (cfg on the
# custom-protocol feature + release profile) and env-guarded behind --smoke.
#
# Every capture is judged, and the run FAILS on five STRUCTURAL conditions:
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
#   (none)                capture a run (both states + pill), judge it, exit
#                         non-zero on any finding
#   --check-only          re-assert the LAST captured run (every result*.json in
#                         it plus pill/result.json, and that every PNG a record
#                         names is still on disk). A convenience for a
#                         pre-flight, never a substitute for a capture, so it
#                         exits NON-ZERO when no run exists.
#   --check-only --self-test-must-fail
#                         re-assert the last run PLUS one synthetic broken
#                         record, and require the re-assertion to report it.
#                         Satisfied exits 1 (non-zero by design); blind exits 9.
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
#   YAP_SMOKE_SKIP_PILL   1 to skip the pill matrix (a partial run; it says so)
#   YAP_DATA_DIR          honoured when set (Y0-D's per-instance state root);
#                         otherwise a fresh mktemp dir. Never the real one.
#   YAP_SMOKE_SHOTS       capture root (default docs/smoke-shots)
#   YAP_SMOKE_FIXTURES    a SINGLE fixture set; when set, only that state is
#                         captured (as "custom"). Unset = seeded + empty. Sets:
#                         scripts/smoke-fixtures-first-run.json (empty first run,
#                         shows the onboarding overlay) and
#                         scripts/smoke-fixtures-daily.json (a lived-in day).
#   YAP_SMOKE_VIEWS       comma list of Nav labels to walk, case-insensitive,
#                         plus the pseudo-view Onboarding (default: all)

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
export YAP_DATA_DIR="${YAP_DATA_DIR:-${YAP_DATA_DIR_OVERRIDE:-$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-data.XXXXXX")}}"

for a in "$@"; do
  case "$a" in
    --check-only)
      # Needs no browser and no server: it only re-reads the last capture.
      # Passes every flag through, so --check-only --self-test-must-fail
      # self-tests the re-assertion itself.
      exec node "$DRIVER" "$@"
      ;;
  esac
done
case "${1:-}" in
  --self-test-must-fail)
    # Needs a browser but no server: the synthetic view is built in the page.
    exec node "$DRIVER" --self-test-must-fail
    ;;
  "") ;;
  *)
    echo "smoke-windowed: unknown argument '$1' (modes: none, --check-only, --self-test-must-fail)" >&2
    exit 2
    ;;
esac

SHOTS_ROOT="${YAP_SMOKE_SHOTS:-docs/smoke-shots}"
RUN="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
export YAP_SMOKE_RUN="$RUN" YAP_SMOKE_SHOTS="$SHOTS_ROOT"
mkdir -p "$SHOTS_ROOT/$RUN"

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
# A listener already on $PORT is a server this run did not start: the curl
# probe below would succeed against it and the walk would judge whatever dist
# it cached, not the one just built (yap-uiux-2026-10-16-Z found a 96-minute-old
# orphan doing exactly that). Refuse loudly instead of judging the wrong tree.
if curl -fsS "http://127.0.0.1:$PORT/index.html" -o /dev/null 2>/dev/null; then
  echo "smoke: something is already serving 127.0.0.1:$PORT — refusing to judge a server this run did not start" >&2
  lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >&2 2>/dev/null || true
  exit 2
fi
PREVIEW_LOG="$(mktemp "${TMPDIR:-/tmp}/yap-smoke-preview.XXXXXX")"
# exec vite itself, not 'npm run preview': npm does not forward SIGTERM to its
# node child, so killing npm's PID left vite listening on $PORT after every run.
# With exec, $! IS the vite process and cleanup() really stops it.
( cd "$ROOT/desktop" && exec ./node_modules/.bin/vite preview --port "$PORT" --strictPort >"$PREVIEW_LOG" 2>&1 ) &
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
# Read every exit code BARE. Piping it anywhere would report the pipe's status
# instead, which is how a loop records a false green. The run's verdict is the
# WORST code: an environment failure (2/3) outranks a finding (1).
worst=0
note() { [ "$1" -gt "$worst" ] && worst="$1"; return 0; }

if [ -n "${YAP_SMOKE_FIXTURES:-}" ]; then
  STATES="custom=$YAP_SMOKE_FIXTURES"
else
  STATES="seeded=scripts/smoke-fixtures-daily.json empty=scripts/smoke-fixtures.json"
fi
for pair in $STATES; do
  state="${pair%%=*}"; fixtures="${pair#*=}"
  echo ""
  echo "── state: $state ($fixtures) ──────────────────────────────"
  set +e
  YAP_SMOKE_STATE="$state" YAP_SMOKE_FIXTURES="$fixtures" node "$DRIVER"
  rc=$?
  set -e
  echo "smoke: state $state exit=$rc"
  note "$rc"
done

# The preview server is done; the pill matrix builds its own scratch bundle
# (YAP_PILL_MATRIX=1, never desktop/dist) and serves it on the SAME port.
cleanup
trap - EXIT

if [ "${YAP_SMOKE_SKIP_PILL:-0}" = "1" ]; then
  echo "smoke: pill matrix SKIPPED (YAP_SMOKE_SKIP_PILL=1) — this run is PARTIAL"
  note 1
else
  echo ""
  echo "── pill: every LivePhase x bottom/left/right x classic/yappy ──"
  set +e
  YAP_PILL_PORT="$PORT" node "$ROOT/scripts/pill-matrix.mjs" --out "$ROOT/$SHOTS_ROOT/$RUN/pill"
  rc=$?
  set -e
  echo "smoke: pill matrix exit=$rc"
  note "$rc"
fi

printf '%s\n' "$RUN" > "$SHOTS_ROOT/LATEST"
shots="$(find "$SHOTS_ROOT/$RUN" -name '*.png' | wc -l | tr -d ' ')"
echo ""
echo "smoke-windowed: run $RUN — $shots PNG(s) under $SHOTS_ROOT/$RUN — exit $worst"
exit "$worst"
