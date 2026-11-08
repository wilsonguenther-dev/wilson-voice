#!/usr/bin/env bash
# stress-tests.sh [--runs N] [--log-dir DIR] [--loaders N] <target> [<filter>]
#
# Runs one Rust test (or a set of them) N times at --test-threads=8 while 8
# `yes > /dev/null` busy loops saturate the CPU, and reports "<pass>/<N>".
# Exits non-zero if ANY run failed. Built for the thread-timing races that
# turned main red on 2026-11-07/08 (#370, #374, #378): a test that only passes
# on an idle machine fails here.
#
#   target   an integration test file stem under desktop/src-tauri/tests
#            (e.g. cancel_long_take), `lib` for the crate's #[cfg(test)] unit
#            tests (filter = module::tests::name), or `all` for the whole suite
#   filter   optional cargo test-name filter (substring match, as cargo does)
#
#   bash scripts/stress-tests.sh lib transcription::tests::state_transitions_unloaded_to_loaded_to_idle_unloaded
#   bash scripts/stress-tests.sh --runs 30 matrix_new_asr_chunk_timeout a_chunk_that_exceeds_the_transcribe_timeout_fails_alone
#   bash scripts/stress-tests.sh --log-dir docs/loop/evidence/stress cancel_long_take
#
# Options
#   --runs N      how many runs (default 10)
#   --log-dir DIR keep every run's full cargo output there (run-<k>.log) plus
#                 summary.txt; without it the logs live in a temp dir that is
#                 deleted on exit
#   --loaders N   busy loops to start (default 8, the stress standard). Only
#                 scripts/flake-soak.sh overrides it (one per CPU, its old
#                 contract); keep 8 everywhere else so counts stay comparable.
#
# Every run is `cargo test --features custom-protocol ... -- --test-threads=8`,
# the same feature set CI tests with. The test binaries are compiled ONCE
# (`--no-run`) BEFORE the loaders start, so the load measures the tests, not
# rustc. Loader PIDs are recorded and an EXIT trap kills exactly those PIDs,
# never anything matched by name: other lanes' processes on this box are not
# ours to touch. Every exit code is read bare, never through a pipe.
#
# Respects CARGO_TARGET_DIR from the environment. The app crate's
# bundle.externalBin makes the staged sidecars
# (src-tauri/binaries/yap-{polish,diarize,asr}-<triple>) and a built frontend
# (desktop/dist) preconditions of the build, exactly as in ci.yml.
#
# There is no per-run hang guard (macOS has no `timeout`, and killing cargo
# would orphan the test binary); a wedged test shows up as the Rust harness's
# "has been running for over 60 seconds" line, and CI bounds it with
# timeout-minutes.
#
# Exit codes: 0 every run passed; 1 at least one run failed; 64 usage;
# 66 a path is missing; anything else = the --no-run build's own exit code.
set -u

usage() {
  echo "usage: $0 [--runs N] [--log-dir DIR] [--loaders N] <target> [<filter>]" >&2
  echo "  target: an integration test file stem, 'lib' (unit tests) or 'all' (whole suite)" >&2
}

is_count() {
  case "$1" in
    '' | *[!0-9]*) return 1 ;;
  esac
  [ "$1" -ge 1 ]
}

RUNS=10
LOADERS=8
LOG_DIR=""
POSITIONAL=()
while [ "$#" -gt 0 ]; do
  case "$1" in
    --runs)
      [ "$#" -ge 2 ] || { usage; exit 64; }
      RUNS="$2"
      shift 2
      ;;
    --runs=*) RUNS="${1#--runs=}"; shift ;;
    --loaders)
      [ "$#" -ge 2 ] || { usage; exit 64; }
      LOADERS="$2"
      shift 2
      ;;
    --loaders=*) LOADERS="${1#--loaders=}"; shift ;;
    --log-dir)
      [ "$#" -ge 2 ] || { usage; exit 64; }
      LOG_DIR="$2"
      shift 2
      ;;
    --log-dir=*) LOG_DIR="${1#--log-dir=}"; shift ;;
    -h | --help) usage; exit 0 ;;
    --) shift; while [ "$#" -gt 0 ]; do POSITIONAL+=("$1"); shift; done ;;
    -*) echo "unknown option: $1" >&2; usage; exit 64 ;;
    *) POSITIONAL+=("$1"); shift ;;
  esac
done

if [ "${#POSITIONAL[@]}" -lt 1 ] || [ "${#POSITIONAL[@]}" -gt 2 ]; then
  usage
  exit 64
fi
TARGET="${POSITIONAL[0]}"
FILTER="${POSITIONAL[1]:-}"
if ! is_count "$RUNS"; then
  echo "--runs must be a positive integer, got '$RUNS'" >&2
  exit 64
fi
if ! is_count "$LOADERS"; then
  echo "--loaders must be a positive integer, got '$LOADERS'" >&2
  exit 64
fi

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$HERE/../desktop/src-tauri"
if [ ! -f "$APP/Cargo.toml" ]; then
  echo "cannot find desktop/src-tauri/Cargo.toml from $HERE" >&2
  exit 66
fi

OWN_LOG_DIR=0
LOAD_PIDS=()
# shellcheck disable=SC2329 # invoked by the EXIT trap
cleanup() {
  if [ "${#LOAD_PIDS[@]}" -gt 0 ]; then
    kill "${LOAD_PIDS[@]}" 2>/dev/null
    wait "${LOAD_PIDS[@]}" 2>/dev/null
    LOAD_PIDS=()
  fi
  if [ "$OWN_LOG_DIR" -eq 1 ] && [ -n "$LOG_DIR" ] && [ -d "$LOG_DIR" ]; then
    rm -rf "$LOG_DIR"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

# Resolve the log dir BEFORE changing directory, so a relative --log-dir means
# relative to where the caller stands.
if [ -n "$LOG_DIR" ]; then
  mkdir -p "$LOG_DIR" || exit 66
  LOG_DIR="$(cd "$LOG_DIR" && pwd)" || exit 66
else
  LOG_DIR="$(mktemp -d -t stress-tests)" || exit 66
  OWN_LOG_DIR=1
fi

cd "$APP" || exit 66

# Which test targets cargo builds and runs.
SELECT=()
case "$TARGET" in
  all) ;;
  lib) SELECT=(--lib) ;;
  *)
    if [ ! -f "tests/$TARGET.rs" ] && [ ! -f "tests/$TARGET/main.rs" ]; then
      echo "no integration test target '$TARGET' (looked for desktop/src-tauri/tests/$TARGET.rs)" >&2
      echo "use 'lib' for unit tests or 'all' for the whole suite" >&2
      exit 66
    fi
    SELECT=(--test "$TARGET")
    ;;
esac

RUN_ARGS=(test --features custom-protocol)
if [ "${#SELECT[@]}" -gt 0 ]; then
  RUN_ARGS+=("${SELECT[@]}")
fi
if [ -n "$FILTER" ]; then
  RUN_ARGS+=("$FILTER")
fi
RUN_ARGS+=(-- --test-threads=8)

SUMMARY="$LOG_DIR/summary.txt"
: > "$SUMMARY"
say() {
  echo "$*"
  echo "$*" >> "$SUMMARY"
}

say "stress-tests: target=$TARGET filter='${FILTER}' runs=$RUNS threads=8 loaders=$LOADERS"
say "stress-tests: cargo ${RUN_ARGS[*]}"
say "stress-tests: CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-<unset: cargo default>}"
if [ "$OWN_LOG_DIR" -eq 0 ]; then
  say "stress-tests: logs in $LOG_DIR"
fi

# Compile before the load, so a cold build is not what gets stressed.
BUILD_ARGS=(test --features custom-protocol)
if [ "${#SELECT[@]}" -gt 0 ]; then
  BUILD_ARGS+=("${SELECT[@]}")
fi
BUILD_ARGS+=(--no-run)
cargo "${BUILD_ARGS[@]}" > "$LOG_DIR/build.log" 2>&1
build=$?
if [ "$build" -ne 0 ]; then
  say "stress-tests: build of '$TARGET' failed (exit $build); last lines of its log:"
  tail -40 "$LOG_DIR/build.log" >&2
  exit "$build"
fi

i=0
while [ "$i" -lt "$LOADERS" ]; do
  yes > /dev/null &
  LOAD_PIDS+=("$!")
  i=$((i + 1))
done
say "stress-tests: started $LOADERS busy loops: ${LOAD_PIDS[*]}"

# Sum of "N passed" over every `test result:` line (the `all` target prints
# one per test binary, most of them 0 under a filter).
passed_count() {
  awk '/^test result: /{for(i=1;i<=NF;i++) if($(i)=="passed;") n+=$(i-1)} END{print n+0}' "$1"
}

# The first panic of a log: the "panicked at" line and its message (Rust prints
# the message on the lines after it, up to a blank line or the backtrace note).
first_assertion() {
  awk '
    found==0 && /panicked at/ {found=1; print; next}
    found==1 {
      if ($0 ~ /^[[:space:]]*$/ || $0 ~ /^note: / || $0 ~ /^---- / || $0 ~ /^stack backtrace:/) exit
      print; if (++lines >= 20) exit
    }
  ' "$1"
}

passes=0
fails=0
first_fail_log=""
run=1
while [ "$run" -le "$RUNS" ]; do
  log="$LOG_DIR/run-$run.log"
  cargo "${RUN_ARGS[@]}" > "$log" 2>&1
  code=$?
  ran="$(passed_count "$log")"
  # A filter that matches nothing exits 0 having proved nothing: a run only
  # counts as a pass if it actually ran at least one test.
  if [ "$code" -eq 0 ] && [ "$ran" -lt 1 ]; then
    say "run $run/$RUNS: FAIL exit 0 but ZERO tests ran -- the filter matched nothing"
    code=65
  elif [ "$code" -eq 0 ]; then
    passes=$((passes + 1))
    say "run $run/$RUNS: pass ($ran passed)"
  else
    failed="$(grep -E '^test .* \.\.\. FAILED$' "$log" | sed -e 's/^test //' -e 's/ \.\.\. FAILED$//' | tr "\n" " " | sed -e "s/ *\$//")"
    say "run $run/$RUNS: FAIL exit $code (${ran} passed; failed: ${failed:-<no test marked FAILED: see $log>})"
  fi
  if [ "$code" -ne 0 ]; then
    fails=$((fails + 1))
    if [ -z "$first_fail_log" ]; then
      first_fail_log="$log"
    fi
  fi
  run=$((run + 1))
done

say "$passes/$RUNS"
if [ "$fails" -eq 0 ]; then
  exit 0
fi

say "stress-tests: first failure (run log $first_fail_log):"
assertion="$(first_assertion "$first_fail_log")"
if [ -n "$assertion" ]; then
  say "$assertion"
else
  # No panic line: a crash, an abort or a zero-test run. Show what there is.
  say "$(grep -E 'FAILED|error|signal|SIGSEGV|SIGABRT|test result|ZERO' "$first_fail_log" | head -20)"
fi
if [ "$OWN_LOG_DIR" -eq 1 ]; then
  echo "stress-tests: pass --log-dir DIR to keep the full logs" >&2
fi
exit 1
