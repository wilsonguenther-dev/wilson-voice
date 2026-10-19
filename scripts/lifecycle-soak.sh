#!/usr/bin/env bash
# yap-lifecycle DO 6 — RSS flat under load.
#
#   bash scripts/lifecycle-soak.sh <runs>
#
# Runs the BUILT app binary headless (`--transcribe-file <fixture> --soak <runs>`:
# one process, <runs> load -> transcribe -> unload cycles of the same
# TranscriptionManager the app holds for days). After every cycle the binary
# prints "soak iter=N" and blocks on stdin; this script samples that pid's RSS
# with `ps` at that quiet point, then releases it. Prints the series and exits
# non-zero when RSS after the warm-up iterations grows more than 10 percent
# (last sample vs. the sample that ends warm-up).
#
# Never launches the GUI (cli::handle_args exits before the Tauri builder) and
# never touches a running Yap: state goes to YAP_DATA_DIR="$(mktemp -d)/yap-state".
# The ASR model is symlinked READ-ONLY from the default models dir when present
# (YAP_SOAK_SEED_MODELS overrides), so a soak does not re-download it.
set -euo pipefail

RUNS="${1:-40}"
case "$RUNS" in ''|*[!0-9]*) echo "usage: $0 <runs>" >&2; exit 2 ;; esac
[ "$RUNS" -ge 2 ] || { echo "need at least 2 runs" >&2; exit 2; }
MAX_GROWTH_PCT="${YAP_SOAK_MAX_GROWTH_PCT:-10}"
# Warm-up 10: the first 40-run soak (2026-10-19) showed a one-time allocator /
# Metal high-water step at iteration 8 (+9.3 MB) and a flat line after it.
WARMUP="${YAP_SOAK_WARMUP:-10}"
[ "$WARMUP" -lt "$RUNS" ] || WARMUP=$(( RUNS / 2 ))

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="${CARGO_TARGET_DIR:-$ROOT/desktop/target}"
BIN="$T/release/wilson-voice"
WAV="$ROOT/desktop/src-tauri/tests/fixtures/quick-brown-fox-16k.wav"
[ -x "$BIN" ] || { echo "no built binary at $BIN — run: (cd desktop/src-tauri && cargo build --release --features custom-protocol)" >&2; exit 2; }
[ -f "$WAV" ] || { echo "missing fixture $WAV" >&2; exit 2; }

WORK="$(mktemp -d)"
export YAP_DATA_DIR="$WORK/yap-state"
mkdir -p "$YAP_DATA_DIR/models"
SEED="${YAP_SOAK_SEED_MODELS:-$HOME/Library/Application Support/WilsonVoice/models}"
if [ -d "$SEED" ]; then
  for f in "$SEED"/whisper-tiny*.gguf; do
    [ -f "$f" ] && ln -s "$f" "$YAP_DATA_DIR/models/$(basename "$f")"
  done
fi

PID=""
cleanup() {
  exec 3>&- 2>/dev/null || true
  exec 4<&- 2>/dev/null || true
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then kill "$PID" 2>/dev/null || true; fi
  rm -rf "$WORK"
}
trap cleanup EXIT

mkfifo "$WORK/in" "$WORK/out"
"$BIN" --transcribe-file "$WAV" --soak "$RUNS" <"$WORK/in" >"$WORK/out" 2>"$WORK/err" &
PID=$!
exec 3>"$WORK/in"
exec 4<"$WORK/out"

echo "lifecycle soak: runs=$RUNS warmup=$WARMUP max_growth=${MAX_GROWTH_PCT}% pid=$PID bin=$BIN"
SAMPLES=()
DONE=0
while IFS= read -r line <&4; do
  case "$line" in
    "soak iter="*)
      rss="$(ps -o rss= -p "$PID" | tr -d ' ')"
      SAMPLES+=("$rss")
      printf '%s rss_kb=%s\n' "$line" "$rss"
      echo >&3
      ;;
    "soak done"*) DONE=1; echo "$line" ;;
    *) echo "binary: $line" ;;
  esac
done
set +e
wait "$PID"; CODE=$?
set -e
PID=""
if [ "$CODE" -ne 0 ] || [ "$DONE" -ne 1 ]; then
  echo "soak binary failed (exit $CODE):" >&2
  cat "$WORK/err" >&2
  exit 1
fi
[ "${#SAMPLES[@]}" -eq "$RUNS" ] || { echo "expected $RUNS samples, got ${#SAMPLES[@]}" >&2; exit 1; }

echo "rss_kb series: ${SAMPLES[*]}"
BASE="${SAMPLES[$((WARMUP - 1))]}"
LAST="${SAMPLES[$((RUNS - 1))]}"
MAX=0; for s in "${SAMPLES[@]:$WARMUP}"; do [ "$s" -gt "$MAX" ] && MAX="$s"; done
GROWTH="$(awk -v b="$BASE" -v l="$LAST" 'BEGIN { printf "%.2f", (l - b) / b * 100 }')"
echo "warm-up end rss_kb=$BASE  last rss_kb=$LAST  max-after-warmup rss_kb=$MAX  growth=${GROWTH}%"
if awk -v g="$GROWTH" -v m="$MAX_GROWTH_PCT" 'BEGIN { exit !(g > m) }'; then
  echo "FAIL: RSS grew ${GROWTH}% after warm-up (limit ${MAX_GROWTH_PCT}%)" >&2
  exit 1
fi
echo "PASS: RSS flat under load (${GROWTH}% <= ${MAX_GROWTH_PCT}%)"
