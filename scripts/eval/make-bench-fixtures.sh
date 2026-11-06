#!/usr/bin/env bash
# SP-03 — make the three speed-bench fixtures: ~20 s, ~127 s and ~240 s of
# synthetic speech (macOS `say`, never captured user audio) from the reference
# texts in scripts/eval/bench/<name>.txt, written as 16 kHz mono 16-bit WAVs
# beside them and padded with trailing silence to the exact target length.
#
# The WAVs are generated, not committed (`*.wav` is gitignored): the texts are
# the fixture, and the voice + rate are pinned so every machine hears the same
# words. Re-running is a no-op when a WAV already has its target length.
#
#   scripts/eval/make-bench-fixtures.sh            # make the missing ones
#   scripts/eval/make-bench-fixtures.sh --force    # remake all three
#
# Exit 0 = all three present at their target length; 1 = a fixture could not
# be made (the line on stderr says which and why); 2 = not macOS / no `say`.
set -uo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)/bench"
VOICE="${YAP_BENCH_VOICE:-Samantha}"
FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

if ! command -v say >/dev/null 2>&1; then
  echo "make-bench-fixtures: needs macOS 'say'" >&2
  exit 2
fi
VOICE_ARGS=()
if say -v '?' 2>/dev/null | grep -q "^${VOICE} "; then
  VOICE_ARGS=(-v "$VOICE")
else
  echo "make-bench-fixtures: voice '$VOICE' not installed, using the system voice" >&2
fi

# python3 -I: isolated, standard library only. Prints the WAV's seconds, or
# pads it in place to exactly <target> seconds when given a target.
wav_tool() {
  python3 -I - "$@" <<'PY'
import sys, wave
path = sys.argv[1]
with wave.open(path, "rb") as w:
    params = w.getparams()
    frames = w.readframes(params.nframes)
rate = params.framerate
secs = params.nframes / rate
if len(sys.argv) == 2:
    print(f"{secs:.3f}")
    sys.exit(0)
target = int(round(float(sys.argv[2]) * rate))
if params.nchannels != 1 or params.sampwidth != 2 or rate != 16000:
    sys.exit(f"{path}: not 16 kHz mono 16-bit")
if params.nframes > target:
    sys.exit(f"{path}: {secs:.2f}s is longer than the target")
frames += b"\x00\x00" * (target - params.nframes)
with wave.open(path, "wb") as w:
    w.setparams(params)
    w.writeframes(frames)
print(f"{target / rate:.3f}")
PY
}

make_one() {
  local name="$1" target="$2"
  local txt="$DIR/$name.txt" wav="$DIR/$name.wav"
  if [ ! -f "$txt" ]; then
    echo "make-bench-fixtures: missing reference text $name.txt" >&2
    return 1
  fi
  if [ "$FORCE" = 0 ] && [ -f "$wav" ]; then
    local have
    have=$(wav_tool "$wav") || have=0
    if python3 -I -c "import sys; sys.exit(0 if abs(float('$have') - $target) < 0.01 else 1)"; then
      echo "fixture $name.wav ok (${have}s)"
      return 0
    fi
  fi
  local words rate secs
  words=$(wc -w <"$txt" | tr -d ' ')
  # Words per minute that lands just under the target, so the pad is short.
  rate=$(python3 -I -c "print(max(90, min(300, int($words * 60 / $target * 0.97))))")
  for _ in 1 2 3; do
    rm -f "$wav"
    say ${VOICE_ARGS[@]+"${VOICE_ARGS[@]}"} -r "$rate" --file-format=WAVE --data-format=LEI16@16000 \
      -o "$wav" -f "$txt"
    local rc=$?
    if [ "$rc" != 0 ] || [ ! -s "$wav" ]; then
      echo "make-bench-fixtures: say failed for $name (exit $rc)" >&2
      return 1
    fi
    secs=$(wav_tool "$wav") || return 1
    if python3 -I -c "import sys; sys.exit(0 if float('$secs') <= $target else 1)"; then
      secs=$(wav_tool "$wav" "$target") || return 1
      echo "fixture $name.wav made (${secs}s, ${words} words, rate ${rate} wpm)"
      return 0
    fi
    # Too long at this rate: speak faster by the overshoot plus 2 %.
    rate=$(python3 -I -c "print(min(300, int($rate * float('$secs') / $target * 1.02) + 1))")
  done
  rm -f "$wav"
  echo "make-bench-fixtures: $name stays longer than ${target}s even at ${rate} wpm" >&2
  return 1
}

fail=0
make_one bench-20s 20 || fail=1
make_one bench-127s 127 || fail=1
make_one bench-240s 240 || fail=1
exit "$fail"
