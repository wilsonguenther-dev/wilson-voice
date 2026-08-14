#!/usr/bin/env bash
#
# YV101 — "a one-line CI check that prevents shipping a build that will not
# launch for every pre-14.4 user at once" (plan finding OS-11).
#
# Yap's deployment floor is macOS 12.0 and stays there. Its dependency graph
# nevertheless imports CoreAudio symbols that only exist on newer systems:
#
#   _AudioHardwareCreateProcessTap        macOS 14.2+   (the 22-B process tap)
#   _AudioHardwareDestroyProcessTap       macOS 14.2+
#   _AudioHardwareCreateAggregateDevice   macOS 13.0+
#   _AudioHardwareDestroyAggregateDevice  macOS 13.0+
#
# If any of those is a HARD (non-weak) import, dyld resolves it at LOAD time and
# the whole app fails to launch on every macOS 12/13 machine — not a disabled
# Notetaker, a Yap that does not open, discovered after release. `build.rs`
# weak-links CoreAudio so they bind NULL instead; this script is the standing
# proof that it is still true of the binary that actually ships.
#
# Usage:  scripts/assert-weak-linked-14_4-symbols.sh [path/to/binary]
# Default binary: ${CARGO_TARGET_DIR:-desktop/target}/release/wilson-voice
#
# Exit 0 = every listed symbol is either weak-imported or not imported at all.
# Exit 1 = a hard import (ship-blocking), a missing binary, or — just as fatal —
#          a check that could not prove it was looking at anything (see the
#          controls below: a vacuous PASS is treated as a failure).

set -euo pipefail

BIN="${1:-${CARGO_TARGET_DIR:-desktop/target}/release/wilson-voice}"

if [[ ! -f "$BIN" ]]; then
  echo "FAIL: no binary at '$BIN' — build it first (cargo build --release --features custom-protocol)" >&2
  exit 1
fi

echo "Inspecting: $BIN"

# One `nm -m` pass; every question below is asked of this text.
SYMS="$(nm -m "$BIN")"

# ---------------------------------------------------------------------------
# Controls. Without these the check can pass by looking at nothing at all —
# a stripped binary, a wrong path, an `nm` whose output format moved. Each one
# is a fact that must hold for the real assertion to mean anything.
# ---------------------------------------------------------------------------

# Control 1: this binary really does link CoreAudio and import symbols from it.
# (If it did not, "no hard 14.4 import" would be trivially true and useless.)
if ! otool -L "$BIN" | grep -q "CoreAudio.framework"; then
  echo "FAIL (control 1): '$BIN' does not link CoreAudio at all — wrong binary?" >&2
  exit 1
fi
COREAUDIO_IMPORTS="$(printf '%s\n' "$SYMS" | grep -c "(from CoreAudio)" || true)"
if [[ "$COREAUDIO_IMPORTS" -lt 1 ]]; then
  echo "FAIL (control 1): no CoreAudio imports found in '$BIN' — nm output unusable?" >&2
  exit 1
fi
echo "  control 1 OK: links CoreAudio, ${COREAUDIO_IMPORTS} imported symbol(s) from it"

# Control 2: `nm -m` in THIS run can still tell a hard import from a weak one.
# `_objc_msgSend` is hard-imported by every Cocoa binary and is not in a
# weak-linked framework, so it is the discriminator: if it ever prints as
# "weak external", the grep below would call everything weak and pass vacuously.
OBJC_LINE="$(printf '%s\n' "$SYMS" | grep -E '\(undefined\).* _objc_msgSend( |$)' || true)"
if [[ -z "$OBJC_LINE" ]]; then
  echo "FAIL (control 2): _objc_msgSend is not an undefined import — cannot prove nm discriminates" >&2
  exit 1
fi
if [[ "$OBJC_LINE" == *"weak external"* ]]; then
  echo "FAIL (control 2): _objc_msgSend reads as weak — the weak/hard distinction is not being measured" >&2
  echo "  $OBJC_LINE" >&2
  exit 1
fi
echo "  control 2 OK: nm discriminates (hard control:${OBJC_LINE#*)})"

# ---------------------------------------------------------------------------
# The assertion.
# ---------------------------------------------------------------------------

SYMBOLS=(
  _AudioHardwareCreateProcessTap
  _AudioHardwareDestroyProcessTap
  _AudioHardwareCreateAggregateDevice
  _AudioHardwareDestroyAggregateDevice
)

STATUS=0
for sym in "${SYMBOLS[@]}"; do
  line="$(printf '%s\n' "$SYMS" | grep -E "\(undefined\).* ${sym}( |$)" || true)"
  if [[ -z "$line" ]]; then
    # Not imported at all — e.g. resolved through dlsym. Strictly stronger than
    # a weak import: there is nothing for dyld to bind either way.
    echo "  ok   ${sym}: not imported (no dyld binding at all)"
    continue
  fi
  if [[ "$line" == *"weak external"* ]]; then
    echo "  ok   ${sym}: weak import"
  else
    echo "  FAIL ${sym}: HARD import — every macOS 12/13 user gets a launch failure" >&2
    echo "       $line" >&2
    STATUS=1
  fi
done

if [[ "$STATUS" -ne 0 ]]; then
  cat >&2 <<'EOF'

This build would not launch on macOS 12 or 13. Fix, do not suppress:
  * keep `-Wl,-weak_framework,CoreAudio` in desktop/src-tauri/build.rs, and
  * gate every process-tap call site on os_version_gate::system_audio_gate_now().
EOF
  exit 1
fi

echo "PASS: no 14.4-era CoreAudio symbol is a hard load-time requirement"
