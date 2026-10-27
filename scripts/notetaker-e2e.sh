#!/usr/bin/env bash
# yap24-NT9 — the phase-closing notetaker proof. ONE headless command runs a two-track meeting
# through the shipped pipeline (transcript on both tracks), the notes pass, and the Markdown export,
# and asserts on what came out. It EXECUTES; there is no path through this script that passes
# without running the release binary over real audio.
#
#   YAP_DATA_DIR=<scratch root>  YAP_MODEL_DIR=<cached models>  YAP_EVAL_CORPUS=<meetings corpus> \
#   YAP_E2E_REQUIRE=1 bash scripts/notetaker-e2e.sh
#
# Inputs (all three REQUIRED — a missing one is a non-zero exit naming it):
#   YAP_DATA_DIR     a throwaway state root; the default root (the real history) is refused.
#   YAP_MODEL_DIR    holds the pinned parakeet Q8_0 file (and, optionally, the notes model and the
#                    Silero VAD); provisioned by scripts/provision-notetaker-eval.sh (yap24-NT0).
#   YAP_EVAL_CORPUS  the meetings eval corpus; its two-track-ordering case is run too.
# Optional:
#   YAP_BIN          a prebuilt release wilson-voice binary (yap-polish must sit next to it for
#                    notes). Default: build both with cargo (CARGO_TARGET_DIR is honoured).
#
# Models are LINKED into <YAP_DATA_DIR>/models (symlinks) — nothing is ever written into
# YAP_MODEL_DIR. Notes are asserted when the notes model is in YAP_MODEL_DIR (notes_ready, a
# Summary in the export); without it the meeting must report the #250 waiting phase notes_queued,
# never an error.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/desktop/src-tauri"
CATALOG="$APP/src/catalog.json"
FIXTURE="$APP/tests/fixtures/meeting-two-track"
ASR_ID="handy-computer/parakeet-unified-en-0.6b-gguf"
NOTES_ID="qwen2.5-1.5b-instruct-q4_k_m"
PROVISION="bash scripts/provision-notetaker-eval.sh"

fail() { echo "notetaker-e2e: FAIL — $*" >&2; exit 1; }
need() {
  local name="$1" hint="$2"
  if [[ -z "${!name:-}" ]]; then
    echo "notetaker-e2e: FAIL — $name is not set. $hint" >&2
    [[ "${YAP_E2E_REQUIRE:-}" == 1 ]] || echo "notetaker-e2e: (YAP_E2E_REQUIRE is unset; the proof fails the same way — it never passes without its inputs)" >&2
    exit 1
  fi
}

need YAP_DATA_DIR "Point it at a throwaway state root, e.g. YAP_DATA_DIR=\"\$(mktemp -d)/yap-state\"."
need YAP_MODEL_DIR "Provision the pinned parakeet model with: $PROVISION (it prints YAP_MODEL_DIR)."
need YAP_EVAL_CORPUS "Provision the meetings corpus with: $PROVISION (it prints YAP_EVAL_CORPUS)."
command -v jq >/dev/null || fail "jq is required"

DEFAULT_ROOT="$HOME/Library/Application Support/WilsonVoice"
mkdir -p "$YAP_DATA_DIR/models"
DATA_REAL="$(cd "$YAP_DATA_DIR" && pwd -P)"
DEFAULT_REAL="$( (cd "$DEFAULT_ROOT" 2>/dev/null && pwd -P) || echo "$DEFAULT_ROOT")"
case "$DATA_REAL/" in
  "$DEFAULT_REAL/"*) fail "YAP_DATA_DIR is the default state root ($DEFAULT_ROOT); the proof never touches the real history" ;;
esac

ASR_FILE="$(jq -er --arg id "$ASR_ID" '.models[] | select(.id == $id) | .files[] | select(.quant == "Q8_0") | .filename' "$CATALOG")" \
  || fail "catalog has no Q8_0 file for $ASR_ID"
NOTES_FILE="$(jq -er --arg id "$NOTES_ID" '[.. | objects | select(.id? == $id) | (.file.filename? // empty)] | first' "$CATALOG")" \
  || fail "catalog has no notes model $NOTES_ID"
[[ -f "$YAP_MODEL_DIR/$ASR_FILE" ]] \
  || fail "the pinned ASR model $ASR_FILE is not in YAP_MODEL_DIR=$YAP_MODEL_DIR. Provision it with: $PROVISION"
TWO_TRACK="$YAP_EVAL_CORPUS/two-track-ordering"
[[ -f "$TWO_TRACK/track0.wav" && -f "$TWO_TRACK/track1.wav" ]] \
  || fail "YAP_EVAL_CORPUS=$YAP_EVAL_CORPUS has no two-track-ordering/track0.wav + track1.wav. Provision it with: $PROVISION"
[[ -f "$FIXTURE/track0.wav" && -f "$FIXTURE/track1.wav" ]] || fail "the committed fixture $FIXTURE is missing"

link() { ln -sf "$1" "$YAP_DATA_DIR/models/$(basename "$1")"; }
link "$YAP_MODEL_DIR/$ASR_FILE"
NOTES=0
if [[ -f "$YAP_MODEL_DIR/$NOTES_FILE" ]]; then link "$YAP_MODEL_DIR/$NOTES_FILE"; NOTES=1; fi
[[ -f "$YAP_MODEL_DIR/silero_vad_v4.onnx" ]] && link "$YAP_MODEL_DIR/silero_vad_v4.onnx"
echo "notetaker-e2e: root=$YAP_DATA_DIR asr=$ASR_FILE notes_model=$([[ $NOTES == 1 ]] && echo linked || echo absent)"

if [[ -n "${YAP_BIN:-}" ]]; then
  BIN="$YAP_BIN"
else
  echo "notetaker-e2e: building the release binary and the notes sidecar"
  (cd "$APP" && cargo build --release --features custom-protocol)
  (cd "$APP" && cargo build -p yap-polish --release)
  TARGET="$(cd "$APP" && cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
  BIN="$TARGET/release/wilson-voice"
fi
[[ -x "$BIN" ]] || fail "no release binary at $BIN"

# run_case <label> <mic.wav> <sys.wav> <require-action-items 0|1>
run_case() {
  local label="$1" mic="$2" sys="$3" actions="$4" out rc report export
  out="$YAP_DATA_DIR/$label.json"
  echo "notetaker-e2e: [$label] $BIN --transcribe-meeting $(basename "$mic") --sys $(basename "$sys")"
  rc=0
  YAP_DATA_DIR="$YAP_DATA_DIR" "$BIN" --transcribe-meeting "$mic" --sys "$sys" --title "NT9 $label" >"$out" || rc=$?
  report="$(tail -n 1 "$out")"
  echo "notetaker-e2e: [$label] exit=$rc report=$report"
  [[ "$rc" == 0 ]] || fail "[$label] --transcribe-meeting exited $rc"
  [[ "$(jq -r .state <<<"$report")" == complete ]] || fail "[$label] row state is not complete"
  (( $(jq -r .segmentsMic <<<"$report") > 0 )) || fail "[$label] no segments on the mic track"
  (( $(jq -r .segmentsSys <<<"$report") > 0 )) || fail "[$label] no segments on the system track"
  if [[ $NOTES == 1 ]]; then
    [[ "$(jq -r .notes <<<"$report")" == notes_ready ]] || fail "[$label] notes model installed but notes are $(jq -r .notes <<<"$report"): $(jq -r .notesError <<<"$report")"
    [[ "$(jq -r .summary <<<"$report")" == true ]] || fail "[$label] notes_ready but no summary on the row"
  else
    [[ "$(jq -r .notes <<<"$report")" == notes_queued ]] || fail "[$label] no notes model, expected notes_queued, got $(jq -r .notes <<<"$report")"
  fi
  export="$(jq -r .export <<<"$report")"
  [[ -f "$export" ]] || fail "[$label] no Markdown export at $export"
  grep -q '^## Transcript' "$export" || fail "[$label] export has no Transcript section"
  grep -q '^\*\*\[[0-9:]*\] Me:\*\*' "$export" || fail "[$label] export transcript has no Me line"
  grep -q '^\*\*\[[0-9:]*\] Them:\*\*' "$export" || fail "[$label] export transcript has no Them line"
  if [[ $NOTES == 1 ]]; then
    grep -q '^## Summary' "$export" || fail "[$label] export has no Summary section"
    if [[ "$actions" == 1 ]]; then
      grep -q '^### Action items' "$export" || fail "[$label] export has no Action items section"
    fi
  fi
  echo "notetaker-e2e: [$label] PASS — export $(basename "$export"): $(grep -c '' "$export") lines, sections: $(grep -E '^##' "$export" | tr '\n' '|')"
}

# The committed fixture states two action items outright, so its export must carry them; the
# corpus case is a word-ordering probe with nothing to do, so it is held to Summary + transcript.
run_case fixture "$FIXTURE/track0.wav" "$FIXTURE/track1.wav" 1
run_case two-track-ordering "$TWO_TRACK/track0.wav" "$TWO_TRACK/track1.wav" 0
echo "notetaker-e2e: PASS (2 meetings: committed fixture + corpus two-track-ordering)"
