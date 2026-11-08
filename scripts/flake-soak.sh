#!/usr/bin/env bash
# flake-soak.sh <test-target> <filter> <runs>
#
# Kept for its old call sites and docs (docs/loop/FLAKES-2026-10-16.md). It is
# now a thin call into scripts/stress-tests.sh, which owns the whole soak:
# compile once with --no-run, start the busy loops, kill exactly their PIDs in
# an EXIT trap, read every exit code bare, report "<passes>/<runs>", exit
# non-zero if any run failed, and refuse a run where the filter matched nothing.
#
#   bash scripts/flake-soak.sh dictation_chunked per_chunk_deadline_not_per_take 30
#   bash scripts/flake-soak.sh meeting_manual_start_stop '' 30     # '' = every test in the file
#
# Its one surviving difference is the load: one `yes > /dev/null` per CPU
# (sysctl -n hw.ncpu), as it always ran, instead of the stress standard of 8.
# Runs now also use --test-threads=8 like every stress run. New callers should
# use scripts/stress-tests.sh directly.
set -u

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <test-target> <filter> <runs>" >&2
  exit 64
fi
TARGET="$1"
FILTER="$2"
RUNS="$3"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NCPU="$(sysctl -n hw.ncpu 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 1)"

if [ -n "$FILTER" ]; then
  exec bash "$HERE/stress-tests.sh" --runs "$RUNS" --loaders "$NCPU" -- "$TARGET" "$FILTER"
fi
exec bash "$HERE/stress-tests.sh" --runs "$RUNS" --loaders "$NCPU" -- "$TARGET"
