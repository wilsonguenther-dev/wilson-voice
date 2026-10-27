#!/usr/bin/env bash
#
# Y7-C — every test this loop added is proven non-vacuous by a mutation that
# makes it fail. Ledger: docs/loop/MUTATIONS.md (one row per test file in
# desktop/src-tauri/tests/). Engine: scripts/loop/non-vacuous.mjs.
#
#   ./scripts/assert-tests-are-non-vacuous.sh                 fast gate, no cargo
#   ./scripts/assert-tests-are-non-vacuous.sh --execute       full mutation sweep
#   ./scripts/assert-tests-are-non-vacuous.sh --execute --only mic_gate.rs [--only …]
#
# FAST GATE (default — the per-item gate): the ledger must have a row for every
# test file, every row's single-line SOURCE mutation must still apply to exactly
# one line of the current tree, and every row must carry an `executed: red`
# record bound to the sha256 of its test file (written by --execute, never by
# hand). Then the scopeless census: every grep in scripts/loop/items/*.mjs must
# name a path scope, because a scopeless grep in an acceptance gate is the
# single cheapest way to ship a false green.
#
# FULL SWEEP (--execute; YAP_SMOKE_MUTATIONS=1 ./scripts/loop-smoke.sh runs it
# once per part). Needs a clean tree. For each row:
#   1. the NAMED test passes unmutated            (green before)
#   2. apply the one-line mutation to the SOURCE  (never the test)
#   3. cargo test --features custom-protocol --test <file> -- --exact <test>
#      must report "test <name> ... FAILED"       (a compile error is NOT red,
#                                                  and neither is "the suite")
#   4. restore the file, then `git diff --exit-code` must be clean
# and the observed transition is written into the ledger's `executed` column.
# Every exit code here is read bare — never through a pipe.
set -u

REPO="$(cd "$(dirname "$0")/.." && pwd)"
ENGINE="$REPO/scripts/loop/non-vacuous.mjs"
SRC_TAURI="$REPO/desktop/src-tauri"
MODE=verify
ONLY=()
while [ $# -gt 0 ]; do
  case "$1" in
    --execute) MODE=execute ;;
    --only) shift; ONLY+=("${1:?--only needs a test file}") ;;
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) printf 'non-vacuous: unknown argument %s\n' "$1" >&2; exit 64 ;;
  esac
  shift
done

command -v node >/dev/null 2>&1 || { echo "non-vacuous: node is required (it is part of the desktop toolchain)" >&2; exit 2; }

if [ "$MODE" = verify ]; then
  node "$ENGINE" verify; ledger=$?
  node "$ENGINE" scopeless; scopeless=$?
  if [ "$ledger" -eq 0 ] && [ "$scopeless" -eq 0 ]; then
    echo "non-vacuous: PASS (ledger=0 scopeless=0)"; exit 0
  fi
  echo "non-vacuous: FAIL (ledger=$ledger scopeless=$scopeless)"; exit 1
fi

# ── --execute ────────────────────────────────────────────────────────────────
cd "$REPO" || exit 2
if ! git diff --exit-code >/dev/null || ! git diff --cached --exit-code >/dev/null; then
  echo "non-vacuous: --execute needs a clean tree (every mutation is checked with git diff --exit-code)" >&2
  exit 2
fi
export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
TIMEOUT_S="${NV_TIMEOUT:-1200}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/non-vacuous.XXXXXX")"
RESULTS="$WORK/results.tsv"
: > "$RESULTS"
HEAD12="$(git rev-parse --short=12 HEAD)"
LIVE_BACKUP=""
LIVE_TARGET=""
restore_live() {
  if [ -n "$LIVE_BACKUP" ] && [ -f "$LIVE_BACKUP" ]; then
    cp "$LIVE_BACKUP" "$LIVE_TARGET"
    LIVE_BACKUP=""
  fi
}
trap 'restore_live; echo "non-vacuous: interrupted — mutation restored; logs in $WORK" >&2; exit 130' INT TERM
trap 'restore_live' EXIT

run_named() { # <test target> <test fn> <log>  -> cargo's exit code, bare
  (cd "$SRC_TAURI" && perl -e 'alarm shift; exec @ARGV' "$TIMEOUT_S" \
    cargo test --features custom-protocol --test "$1" -- --exact "$2") >"$3" 2>&1 </dev/null
}
record() { printf '%s\t%s\n' "$1" "$2" >> "$RESULTS"; }
stamp() { date -u +%Y-%m-%dT%H:%MZ; }

node "$ENGINE" rows "${ONLY[@]+"${ONLY[@]}"}" > "$WORK/rows" || exit 2
total=$(wc -l < "$WORK/rows" | tr -d ' ')
echo "non-vacuous: $total row(s) to execute; logs in $WORK"

(cd "$SRC_TAURI" && cargo test --features custom-protocol --no-run) > "$WORK/build.log" 2>&1 </dev/null
build=$?
if [ "$build" -ne 0 ]; then echo "non-vacuous: the unmutated tree does not build (exit $build) — see $WORK/build.log" >&2; exit 1; fi

# Phase 1 — every named test must be GREEN on the unmutated tree, or a red
# under mutation proves nothing. Nothing changes between these runs, so no
# rebuild happens here.
while IFS=$'\x1f' read -r n target test src <&3; do
  run_named "$target" "$test" "$WORK/base-$n.log"; rc=$?
  if [ "$rc" -ne 0 ] || ! grep -qF "test $test ... ok" "$WORK/base-$n.log"; then
    record "$n" "FAILED: not green before the mutation (cargo exit $rc) — $(stamp) @ $HEAD12"
    echo "  [$n] $target::$test  BASELINE NOT GREEN (exit $rc)"
    touch "$WORK/skip-$n"
  fi
done 3< "$WORK/rows"

# Phase 2 — mutate, run, require the NAMED test red, restore, prove clean.
red=0; bad=0
while IFS=$'\x1f' read -r n target test src <&3; do
  [ -f "$WORK/skip-$n" ] && { bad=$((bad + 1)); continue; }
  LIVE_TARGET="$SRC_TAURI/$src"
  if ! node "$ENGINE" apply "$n" "$WORK/backup-$n" > "$WORK/apply-$n.log" 2>&1; then
    record "$n" "FAILED: the mutation no longer applies ($(tr '\n' ' ' < "$WORK/apply-$n.log")) — $(stamp) @ $HEAD12"
    bad=$((bad + 1)); echo "  [$n] $target::$test  MUTATION DOES NOT APPLY"; continue
  fi
  LIVE_BACKUP="$WORK/backup-$n"
  run_named "$target" "$test" "$WORK/mut-$n.log"; rc=$?
  # Plain cp, not cp -p: the restored file must carry a NEW mtime so the next
  # cargo invocation recompiles it instead of trusting the mutated artifact.
  restore_live
  if ! git diff --exit-code >/dev/null; then
    git diff --stat > "$WORK/dirty-$n.log"
    git checkout -- .
    record "$n" "FAILED: the tree was not clean after restoring (a test wrote into the repo; see dirty-$n.log) — $(stamp) @ $HEAD12"
    bad=$((bad + 1)); echo "  [$n] $target::$test  TREE DIRTY AFTER RESTORE"; continue
  fi
  sha="$(node "$ENGINE" sha "$target.rs")"
  if grep -qF "test $test ... FAILED" "$WORK/mut-$n.log"; then
    record "$n" "executed: red · \"test $test ... FAILED\" (cargo exit $rc) · green before · restored, git diff --exit-code clean · test sha256 $sha · $(stamp) @ $HEAD12"
    red=$((red + 1)); echo "  [$n] $target::$test  green -> RED"
  elif grep -qE '^error(\[E[0-9]+\])?:|could not compile' "$WORK/mut-$n.log"; then
    record "$n" "FAILED: the mutation does not compile — a build error is not a red test — $(stamp) @ $HEAD12"
    bad=$((bad + 1)); echo "  [$n] $target::$test  COMPILE ERROR (not a red test)"
  elif grep -qF "test $test ... ok" "$WORK/mut-$n.log"; then
    record "$n" "FAILED: stayed GREEN under the mutation (vacuous for this mutation) — $(stamp) @ $HEAD12"
    bad=$((bad + 1)); echo "  [$n] $target::$test  STAYED GREEN"
  else
    record "$n" "FAILED: the named test was not reported FAILED (cargo exit $rc; timeout ${TIMEOUT_S}s?) — $(stamp) @ $HEAD12"
    bad=$((bad + 1)); echo "  [$n] $target::$test  NO VERDICT (exit $rc)"
  fi
done 3< "$WORK/rows"

node "$ENGINE" record "$RESULTS"
echo "non-vacuous: executed $total row(s): $red green -> red, $bad failed. Ledger updated; logs in $WORK"
[ "$bad" -eq 0 ] || exit 1
exit 0
