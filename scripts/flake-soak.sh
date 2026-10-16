#!/usr/bin/env bash
# flake-soak.sh <test-target> <filter> <runs>
#
# Runs one integration-test target <runs> times under CPU saturation and
# reports "<passes>/<runs>". Exits non-zero if ANY run failed.
#
#   bash scripts/flake-soak.sh dictation_chunked per_chunk_deadline_not_per_take 30
#   bash scripts/flake-soak.sh meeting_manual_start_stop '' 30     # '' = every test in the file
#
# Load: one `yes > /dev/null` busy loop per CPU (sysctl -n hw.ncpu). Their PIDs
# are recorded and an EXIT trap kills exactly those PIDs, never anything matched
# by name: another lane's processes on this box are not ours to touch.
#
# The test binary is compiled ONCE before the load starts (`--no-run`), so the
# soak measures the test under load, not rustc under load. Every exit code is
# read bare, never through a pipe.
#
# Respects CARGO_TARGET_DIR from the environment (export your lane's target dir
# first). The app crate's bundle.externalBin makes the staged sidecars
# (src-tauri/binaries/yap-{polish,diarize}-<triple>) a precondition of the build.
set -u

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <test-target> <filter> <runs>" >&2
  exit 64
fi
TARGET="$1"
FILTER="$2"
RUNS="$3"
case "$RUNS" in
  '' | *[!0-9]*) echo "runs must be a positive integer, got '$RUNS'" >&2; exit 64 ;;
esac
if [ "$RUNS" -lt 1 ]; then
  echo "runs must be >= 1" >&2
  exit 64
fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$HERE/../desktop/src-tauri"
if [ ! -f "$APP/Cargo.toml" ]; then
  echo "cannot find desktop/src-tauri/Cargo.toml from $HERE" >&2
  exit 66
fi
cd "$APP" || exit 66

ARGS=(test --features custom-protocol --test "$TARGET")
if [ -n "$FILTER" ]; then
  ARGS+=("$FILTER")
fi

echo "flake-soak: target=$TARGET filter='${FILTER}' runs=$RUNS"
echo "flake-soak: CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-<unset: cargo default>}"

# Compile before the load, so a cold build is not what gets soaked.
cargo test --features custom-protocol --test "$TARGET" --no-run
build=$?
if [ "$build" -ne 0 ]; then
  echo "flake-soak: build of test target '$TARGET' failed (exit $build)" >&2
  exit "$build"
fi

NCPU="$(sysctl -n hw.ncpu 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)"
LOAD_PIDS=()
cleanup() {
  if [ "${#LOAD_PIDS[@]}" -gt 0 ]; then
    kill "${LOAD_PIDS[@]}" 2>/dev/null
    wait "${LOAD_PIDS[@]}" 2>/dev/null
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

i=0
while [ "$i" -lt "$NCPU" ]; do
  yes > /dev/null &
  LOAD_PIDS+=("$!")
  i=$((i + 1))
done
echo "flake-soak: started $NCPU busy loops: ${LOAD_PIDS[*]}"

passes=0
fails=0
run=1
while [ "$run" -le "$RUNS" ]; do
  log="$(mktemp -t flake-soak)"
  cargo "${ARGS[@]}" > "$log" 2>&1
  code=$?
  # A filter that matches nothing exits 0 having proved nothing: a run only
  # counts as a pass if it actually ran at least one test.
  if [ "$code" -eq 0 ] && ! grep -Eq 'test result: ok\. [1-9][0-9]* passed' "$log"; then
    echo "run $run/$RUNS: exit 0 but ZERO tests ran -- the filter matched nothing"
    code=65
  fi
  if [ "$code" -eq 0 ]; then
    ran="$(grep -Eo 'test result: ok\. [0-9]+ passed' "$log" | head -1 | grep -Eo '[0-9]+ passed')"
    passes=$((passes + 1))
    echo "run $run/$RUNS: exit 0 ($ran)"
  else
    fails=$((fails + 1))
    echo "run $run/$RUNS: exit $code -- failing output:"
    grep -E 'panicked at|^---- |assert|FAILED|failures:|test result' "$log" | head -40
  fi
  rm -f "$log"
  run=$((run + 1))
done

echo "$passes/$RUNS"
if [ "$fails" -ne 0 ]; then
  exit 1
fi
exit 0
