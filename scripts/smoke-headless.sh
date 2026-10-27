#!/usr/bin/env bash
#
# Y7-A — a headless smoke of the REAL BUILT BINARY, end to end.
#
#     ./scripts/smoke-headless.sh          # or, from desktop/:  npm run smoke:headless
#
# Why this exists. The build is green, the tests are green, and the binary can
# still be unlaunchable for a whole population of users (a hard-linked CoreAudio
# symbol), or transcribe nothing, or format nothing. Unit tests exercise the
# parts; this exercises the artifact. It drives the shipped release binary with
# no window, no TCC prompt and no microphone, through the headless CLI the app
# already has (`--transcribe-file`, YV32) plus `--cleanup` (Y7-A), against the
# committed fixture tests/fixtures/quick-brown-fox-16k.wav.
#
# Steps (the first failure stops the run and the summary says which):
#   1. build   stage both sidecars, then `cargo build --release --features custom-protocol`
#   2. link    scripts/assert-weak-linked-14_4-symbols.sh on the built binary
#   3. asr     `--transcribe-file` against a TEMPORARY state root; the transcript
#              must contain the expected words
#   4. isolate the run wrote NOTHING into the real state root
#   5. format  the same file through the cleanup pipeline at the SHIPPED default
#              level; before and after are printed
#   6. summary one PASS/FAIL line plus the version from tauri.conf.json
#
# Rules this script keeps, on purpose:
#   * It never touches the user's state root or /Applications/Yap.app. Every run
#     gets YAP_DATA_DIR=<fresh mktemp dir>. The real root's name is read from
#     src-tauri/src/channel.rs (the one place it is defined), so this file never
#     restates it, and its contents are snapshotted before and after.
#   * It never skips when the ASR model is missing. A smoke that skips is
#     decoration. It FAILS with the command that installs the model.
#   * It never downloads a model. The model the headless CLI would pick (the
#     catalog's smallest) is symlinked READ-ONLY from the real models folder
#     into the temporary one; if it is not there, step 3 fails. Left alone, the
#     CLI would fetch it into the temporary root, which this script must not do.
#   * Exit codes are read bare: no command's status passes through a pipe.
#
# Y7-B is the windowed counterpart (scripts/smoke-windowed.sh).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP="$ROOT/desktop"
SRC_TAURI="$DESKTOP/src-tauri"
TARGET_DIR="${CARGO_TARGET_DIR:-$DESKTOP/target}"
BIN="$TARGET_DIR/release/wilson-voice"
WAV="$SRC_TAURI/tests/fixtures/quick-brown-fox-16k.wav"
CATALOG="$SRC_TAURI/src/catalog.json"
CHANNEL_RS="$SRC_TAURI/src/channel.rs"
EXPECT_WORDS=(quick brown fox lazy dog)
RUN_TIMEOUT_SECS="${YAP_SMOKE_RUN_TIMEOUT:-300}"

# This run must never inherit a state-root override from the caller's shell.
unset YAP_DATA_DIR

VERSION="$(node -p "require('$SRC_TAURI/tauri.conf.json').version" 2>/dev/null || echo unknown)"
WORK=""
STEP="start"
RESULT="FAIL"
REASON="did not finish"

finish() {
  local code=$?
  if [ -n "$WORK" ] && [ -d "$WORK" ]; then rm -rf "$WORK"; fi
  printf '\n'
  if [ "$RESULT" = "PASS" ] && [ "$code" -eq 0 ]; then
    printf 'smoke-headless: PASS — Yap %s (all 5 steps: build, link, asr, isolate, format)\n' "$VERSION"
  else
    printf 'smoke-headless: FAIL — Yap %s — step "%s": %s\n' "$VERSION" "$STEP" "$REASON" >&2
    [ "$code" -ne 0 ] || code=1
  fi
  exit "$code"
}
trap finish EXIT

die() { REASON="$1"; exit 1; }
step() { STEP="$1"; printf '\n──────── %s ────────\n' "$1"; }

# --- the real state root, located without restating its name -----------------
REAL_NAME="$(sed -nE '/data_dir_name: "/{s/.*data_dir_name: "([^"]+)".*/\1/p;q;}' "$CHANNEL_RS")"
case "$REAL_NAME" in
  ''|*-Test) die "could not read the production state-root name from $CHANNEL_RS (got '$REAL_NAME')" ;;
esac
REAL_ROOT="$HOME/Library/Application Support/$REAL_NAME"

# snapshot <dir> <outfile> — one line per entry under <dir>: files and symlinks
# as path|size|mtime, directories as their path alone. A directory's own mtime
# is left out on purpose: it moves whenever anything (another process's scratch
# file included) is created and removed inside it, which says nothing about
# what is left behind.
snapshot() {
  if [ -d "$1" ]; then
    {
      find "$1" -type d -print
      find "$1" \( -type f -o -type l \) -exec stat -f '%N|%z|%m' {} +
    } | LC_ALL=C sort >"$2"
  else
    : >"$2"
  fi
}

[ -f "$WAV" ] || die "missing fixture $WAV"
command -v node >/dev/null 2>&1 || die "node is required (parses the catalog and tauri.conf.json)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-headless.XXXXXX")"
STATE="$WORK/state"
mkdir -p "$STATE/models"
printf 'smoke-headless: Yap %s\n' "$VERSION"
printf 'smoke-headless: temporary state root %s\n' "$STATE"
printf 'smoke-headless: real state root      %s (read-only here)\n' "$REAL_ROOT"
snapshot "$REAL_ROOT" "$WORK/real-before.txt"

# ---------------------------------------------------------------- 1. build ----
step "1/5 build the release binary the way the gate does"
HOST_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
[ -n "$HOST_TRIPLE" ] || die "could not read the host triple from rustc -vV"
# bundle.externalBin names BOTH sidecars, so both are a precondition of every
# cargo build of the app.
(cd "$DESKTOP" && cargo build -p yap-polish -p yap-diarize --release) || die "sidecar build failed"
mkdir -p "$SRC_TAURI/binaries"
cp "$TARGET_DIR/release/yap-polish" "$SRC_TAURI/binaries/yap-polish-$HOST_TRIPLE"
cp "$TARGET_DIR/release/yap-diarize" "$SRC_TAURI/binaries/yap-diarize-$HOST_TRIPLE"
(cd "$SRC_TAURI" && cargo build --release --features custom-protocol) || die "cargo build --release --features custom-protocol failed"
[ -x "$BIN" ] || die "no release binary at $BIN after the build"
printf 'built %s\n' "$BIN"

# ----------------------------------------------------------------- 2. link ----
step "2/5 weak-link check on the built binary"
"$ROOT/scripts/assert-weak-linked-14_4-symbols.sh" "$BIN" || die "assert-weak-linked-14_4-symbols.sh rejected $BIN (a hard CoreAudio import: the app will not launch on macOS 12/13)"

# ------------------------------------------------------------------ 3. asr ----
step "3/5 --transcribe-file against a temporary state root"
# The model the CLI would use on a cold root: the catalog's smallest default
# file (models::smallest_model; ties go to the first entry).
MODEL_LINE="$(node -e '
const c = require(process.argv[1]);
let best = null;
for (const m of c.models) {
  const files = m.files || [];
  const f = files.find((x) => x.quant === m.default_quant) || files[0];
  if (f && (best === null || f.size_bytes < best.size)) best = { id: m.id, file: f.filename, size: f.size_bytes };
}
if (best === null) process.exit(1);
console.log(best.id + " " + best.file + " " + best.size);
' "$CATALOG")" || die "could not read the smallest model from $CATALOG"
set -- $MODEL_LINE
MODEL_ID="$1"; MODEL_FILE="$2"; MODEL_BYTES="$3"
REAL_MODEL="$REAL_ROOT/models/$MODEL_FILE"
REAL_BYTES="$(stat -f '%z' "$REAL_MODEL" 2>/dev/null || echo 0)"
if [ "$REAL_BYTES" != "$MODEL_BYTES" ]; then
  printf 'no model installed: %s (%s bytes) is not in %s/models\n' "$MODEL_FILE" "$MODEL_BYTES" "$REAL_ROOT" >&2
  printf 'run:  %s --transcribe-file %s\n' "$BIN" "$WAV" >&2
  printf '      (the first run downloads %s once, sha256-verified, into the default state root)\n' "$MODEL_ID" >&2
  printf '  or: open Yap, Settings, Models, download a model\n' >&2
  die "no model installed, run: $BIN --transcribe-file $WAV"
fi
ln -s "$REAL_MODEL" "$STATE/models/$MODEL_FILE"
printf 'model: %s (symlinked read-only into the temporary root)\n' "$MODEL_ID"

run_bin() { # run_bin <stdout file> <stderr file> <args...> ; sets RC, bounded by a watchdog
  local out="$1" err="$2"; shift 2
  set +e
  YAP_DATA_DIR="$STATE" perl -e 'alarm shift; exec @ARGV or die "exec: $!"' "$RUN_TIMEOUT_SECS" "$BIN" "$@" >"$out" 2>"$err"
  RC=$?
  set -e
}

run_bin "$WORK/transcript.txt" "$WORK/transcript.err" --transcribe-file "$WAV"
if [ "$RC" -ne 0 ]; then
  sed 's/^/  stderr: /' "$WORK/transcript.err" >&2
  die "--transcribe-file exited $RC (142 means the $RUN_TIMEOUT_SECS s watchdog fired)"
fi
TRANSCRIPT="$(cat "$WORK/transcript.txt")"
printf 'transcript: %s\n' "$TRANSCRIPT"
LOWER="$(printf '%s' "$TRANSCRIPT" | tr '[:upper:]' '[:lower:]')"
for w in "${EXPECT_WORDS[@]}"; do
  case "$LOWER" in
    *"$w"*) ;;
    *) die "transcript is missing the expected word '$w': $TRANSCRIPT" ;;
  esac
done
printf 'ok: transcript contains %s\n' "${EXPECT_WORDS[*]}"
# Positive control: the override was honoured (the CLI creates recordings/ under
# whatever root it resolved), so "nothing in the real root" in step 4 is not
# vacuous.
[ -d "$STATE/recordings" ] || die "the binary did not use the temporary state root (no $STATE/recordings), so step 4 would prove nothing"

# -------------------------------------------------------------- 5. format ----
# (run before the isolation check so that check covers BOTH runs of the binary)
run_bin "$WORK/cleanup.json" "$WORK/cleanup.err" --transcribe-file "$WAV" --cleanup
if [ "$RC" -ne 0 ]; then
  sed 's/^/  stderr: /' "$WORK/cleanup.err" >&2
  STEP="5/5 cleanup pipeline at the shipped default level"
  die "--transcribe-file --cleanup exited $RC"
fi

# ------------------------------------------------------------- 4. isolate ----
step "4/5 nothing written into the real state root"
snapshot "$REAL_ROOT" "$WORK/real-after.txt"
if ! cmp -s "$WORK/real-before.txt" "$WORK/real-after.txt"; then
  diff "$WORK/real-before.txt" "$WORK/real-after.txt" | sed 's/^/  /' >&2 || true
  die "the real state root changed during the run (if Yap is running on this machine it writes there too: quit it and re-run)"
fi
printf 'ok: %s unchanged (%s entries, before == after)\n' "$REAL_ROOT" "$(wc -l <"$WORK/real-before.txt" | tr -d ' ')"

# -------------------------------------------------------------- 5. format ----
step "5/5 cleanup pipeline at the shipped default level"
REPORT="$(node -e '
const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
if (!r.cleaned || !r.cleaned.trim()) { console.error("cleanup produced empty text"); process.exit(1); }
if (r.level === "none") { console.error("shipped default cleanup level is none: formatting is off"); process.exit(1); }
if (!r.stages) { console.error("no cleanup stage ran at level " + r.level); process.exit(1); }
console.log("level:   " + r.level + "   mode: " + r.mode + "   stages: " + r.stages);
console.log("before:  " + r.raw);
console.log("after:   " + r.cleaned);
console.log("changed: " + (r.raw === r.cleaned ? "no (already clean)" : "yes"));
' "$WORK/cleanup.json")" || die "cleanup report rejected: $(cat "$WORK/cleanup.json")"
printf '%s\n' "$REPORT"

RESULT="PASS"
