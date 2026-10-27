#!/usr/bin/env bash
#
# Y0-B — "does this tree build from nothing?"
#
# Every item in Yap's build loop is built in a FRESH CLONE. This script is the
# one command that answers that question and fails LOUDLY when the answer is no.
# Run it from the repo root:
#
#     ./scripts/loop-smoke.sh          # or, from desktop/:  npm run smoke
#
# It runs exactly the shared preamble's standard gate and nothing more, prints
# one PASS/FAIL line per stage, and exits non-zero on the FIRST failure. Every
# exit code is read directly from the command that produced it — never through a
# pipe, because a piped exit status is the LAST stage's status (the pager's, not
# cargo's) and that has already shipped a false green on this fleet.
#
# ---------------------------------------------------------------------------
# RUNTIME-DEPENDENCY DISCIPLINE (the Yap rule)
# ---------------------------------------------------------------------------
# Nothing may be "found on the machine". This script DETECTS and REPORTS; it
# never installs. A green smoke run that quietly `brew install`ed its way there
# stops describing the user's machine, which is the only machine that matters.
#
#   * TOOLCHAIN — rustc, cargo, node, npm must already be present. Missing one
#     is a NAMED failure at stage 1, not a confusing error 300 lines later.
#
#   * SIDECARS — yap-polish and yap-diarize are BUILT HERE, from this tree, and
#     staged into desktop/src-tauri/binaries/<name>-<host triple>. They are not
#     downloaded and they are not assumed: `bundle.externalBin` in
#     tauri.conf.json names BOTH, which makes both a precondition of every
#     `cargo build` of the app, not only of `tauri build`.
#
#   * SHERPA-ONNX — `sherpa-onnx-sys`'s build script FETCHES a 19.5 MB archive
#     (sherpa-onnx-v*-osx-arm64-static-lib.tar.bz2) from GitHub Releases when
#     SHERPA_ONNX_LIB_DIR is unset. Two escape hatches exist:
#     SHERPA_ONNX_LIB_DIR (an already-extracted lib tree) and the cheaper
#     SHERPA_ONNX_ARCHIVE_DIR (a directory holding the vendored .tar.bz2, which
#     the build script extracts itself). Stage 2 prints WHICH of the three modes
#     this run used, so an offline-clone failure is diagnosable in one line
#     instead of an hour. See .github/workflows/ci.yml, which documents the same
#     fetch at length.
#
#   * MODEL WEIGHTS — ASR, diarization and polish weights are NOT in this repo,
#     are NOT downloaded here, and MUST NOT BE. The app fetches them at runtime
#     from desktop/src-tauri/src/catalog.json into <state root>/models. The rule
#     this script exists to protect is: NO TEST MAY REQUIRE A MODEL ON DISK. A
#     fresh clone on a fresh machine has no models, so a test that needs one is
#     a test that only passes on the author's laptop.
#
#     The DEFAULT run proves it mechanically, with FRESH-CLONE semantics: every
#     cargo test stage runs with YAP_DATA_DIR (the documented, resolved-once
#     state-root override — see ARCHITECTURE.md's Runtime Dependencies table)
#     pointed at a freshly created EMPTY directory, and with YAP_EVAL_CORPUS and
#     YAP_DIARIZE_MODELS pointed at paths that do not exist. So `cargo test`
#     must pass with an EMPTY application-support dir and nothing found on the
#     machine — the state a fresh clone on a fresh machine is in, and the state
#     CI is in. A test that resolves its models through YAP_DATA_DIR, the
#     corpus env or the diarize-models env finds nothing here and must take a
#     named skip or fail. KNOWN GAP (not enforced): tests/meeting_eval.rs's
#     silero_model() reads dirs::data_dir()/WilsonVoice/models directly,
#     bypassing YAP_DATA_DIR; it is only unreachable here because its test
#     returns early on the absent corpus first. That test belongs to Y7.
#
#     Why the corpus and diarize models are hidden too, not only the state
#     root (Y0-B fix round, 2026-10-19): on a dev box that HAS
#     ~/yap-eval-corpus and ~/yap-diarize-models, stage [11] ran
#     tests/meeting_eval.rs's real-weights sweeps through a DEBUG yap-diarize
#     and the whole run overran the acceptance seat's 600 s window. The
#     trade is stated, not hidden: on such a box a green DEFAULT run says
#     nothing about those eval sweeps, which report as named skips. And with
#     ONLY the state root emptied (the 2026-09-14 probe) six meeting_eval
#     tests go red, because the corpus is found but the models are not:
#         meeting_eval_anti_alias_eer_regression                            <- needs sherpa-onnx-pyannote-segmentation-3-0
#         meeting_eval_lecture_wer_is_under_the_gate                        <- needs an ASR model
#         meeting_eval_seam_dedupe_and_ordering_hold                        <- needs an ASR model
#         meeting_eval_two_track_ordering_survives_the_clock_mismatch       <- needs an ASR model
#         meeting_eval_the_fixed_clock_tie_break_only_pops_words_the_cut_truncated
#         meeting_eval_vad_cut_boundaries_put_no_speech_in_the_overlap
#     Those six mix "host has a corpus" with "host has no model", which a
#     fresh clone never does; fixing them is Y7's item, not this script's.
#     Hiding every host asset puts each eval test on its named-skip path, the
#     same path CI takes. Do NOT make this script download a model to go green.
#
#     Opt-outs (neither is a fresh clone, and the summary line says so):
#       YAP_SMOKE_HOST_ASSETS=1                         use this machine's models,
#                                                       corpus and diarize models
#       YAP_SMOKE_HOST_ASSETS=1 YAP_SMOKE_EMPTY_STATE=1 the 2026-09-14 Y7 probe:
#                                                       empty state root, host
#                                                       corpus visible (known red)
#     YAP_SMOKE_EMPTY_STATE=1 WITHOUT YAP_SMOKE_HOST_ASSETS=1 is refused at
#     stage [2]: the default run already empties the state root, so that
#     combination would silently turn a known-red probe green.
#
# LINT STRICTNESS — this script runs the same lint gate ci.yml runs after Y0-A
# (#234), so a green smoke and a green CI mean the same thing:
#
#   * clippy is BLOCKING, with -D warnings, on the app
#     (`cargo clippy --all-targets --features custom-protocol -- -D warnings`)
#     AND on both sidecars, which the app's clippy never compiles
#     (`cargo clippy -p yap-polish -p yap-diarize --all-targets --release -- -D warnings`).
#     Never silence a lint to make this green.
#
#   * rustfmt is SCOPED. ci.yml keeps `cargo fmt --all -- --check` advisory by
#     ledger decision (PANEL 2026-09-12): main drifts between tree-wide sweeps,
#     and re-sweeping mid-loop conflicts every open branch. Making the whole-tree
#     check blocking here would make this script red on an UNMODIFIED main —
#     a smoke test that answers "no" for a reason no item can fix. Making it
#     advisory outright would let a branch add NEW drift unseen. So stage [9]
#     runs the whole-tree check once and scopes it by LINE, not by file: a
#     rustfmt hunk whose rewritten lines fall on lines THIS checkout changed
#     (`git diff -U0` against the merge base with origin/main — committed,
#     staged or unstaged — or anywhere in an untracked .rs file) is a FAILURE;
#     every other hunk is main's pre-existing drift, printed as ADVISORY, and
#     passes — even when it sits in a file this checkout edited (lib.rs carries
#     19 such hunks on main, and most items edit it). Never reformat lines you
#     did not touch to make it green. Outside a git checkout, or with no merge base with origin/main
#     (shallow, single-branch or differently-named-remote clones), the scope
#     cannot be computed and the stage FAILS CLOSED with a named message; set
#     YAP_SMOKE_FMT_BASE=<fork commit> (must be an ancestor of HEAD) to name the
#     base explicitly.
#
# TOOLCHAIN PIN — desktop/rust-toolchain.toml pins the channel CI uses. This
# script exports RUSTUP_AUTO_INSTALL=0 so rustup cannot quietly download that
# channel: if it is missing, stage [1] fails with a named message instead.

set -euo pipefail

# Detect, never install: rustup must not fetch the pinned channel behind our back.
export RUSTUP_AUTO_INSTALL=0

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP="$REPO_ROOT/desktop"
SRC_TAURI="$DESKTOP/src-tauri"
TARGET_DIR="${CARGO_TARGET_DIR:-$DESKTOP/target}"

STAGE_NO=0
SMOKE_STATE_ROOT=""
FMT_LOG=""
FMT_RANGES=""
cleanup() { # remove ONLY the temp paths this run created
  if [ -n "$SMOKE_STATE_ROOT" ] && [ -d "$SMOKE_STATE_ROOT" ]; then rm -rf "$SMOKE_STATE_ROOT"; fi
  if [ -n "$FMT_LOG" ] && [ -f "$FMT_LOG" ]; then rm -f "$FMT_LOG"; fi
  if [ -n "$FMT_RANGES" ] && [ -f "$FMT_RANGES" ]; then rm -f "$FMT_RANGES"; fi
}
trap cleanup EXIT

stage() { # stage <human name> <function>
  local name="$1" fn="$2" code
  STAGE_NO=$((STAGE_NO + 1))
  printf '\n──────── [%d] %s ────────\n' "$STAGE_NO" "$name"
  set +e
  "$fn"
  code=$?
  set -e
  if [ "$code" -eq 0 ]; then
    printf 'PASS  [%d] %s\n' "$STAGE_NO" "$name"
  else
    printf 'FAIL  [%d] %s (exit %d)\n' "$STAGE_NO" "$name" "$code"
    printf '\nloop-smoke: FAILED — this tree does not build from nothing.\n'
    printf 'loop-smoke: first failing stage: [%d] %s (exit %d)\n' "$STAGE_NO" "$name" "$code"
    exit "$code"
  fi
}

# ---------------------------------------------------------------------------
# [1] Toolchain. Detect and report. NEVER install.
# ---------------------------------------------------------------------------
MISSING=0
need() { # need <binary> <what it is for>
  if command -v "$1" >/dev/null 2>&1; then
    printf '  ok      %-6s %s\n' "$1" "$(command -v "$1")"
  else
    printf '  MISSING %-6s — %s\n' "$1" "$2" >&2
    MISSING=$((MISSING + 1))
  fi
}
stage_toolchain() {
  need rustc "the Rust compiler: the app, both sidecars and every cargo stage below"
  need cargo "the Rust build tool and test runner"
  need node  "the frontend build (vite) and the vitest suite"
  need npm   "npm ci — the frontend dependency install this smoke test exists to prove"
  if [ "$MISSING" -ne 0 ]; then
    printf '\n  %d required tool(s) are not on this machine.\n' "$MISSING" >&2
    printf '  loop-smoke DETECTS and REPORTS — it does not brew/rustup anything into place,\n' >&2
    printf '  because a green run must describe THIS machine. Install them and re-run.\n' >&2
    return 1
  fi
  # Ask from desktop/, where rust-toolchain.toml applies: that is the rustc every
  # cargo stage below uses. With RUSTUP_AUTO_INSTALL=0 a missing pinned channel
  # fails HERE, by name, instead of being downloaded.
  local pinned
  if ! pinned="$(cd "$DESKTOP" && rustc --version 2>&1)"; then
    printf '  rustc fails inside desktop/ (pinned by desktop/rust-toolchain.toml):\n  %s\n' "$pinned" >&2
    printf '  The pinned channel is not installed. loop-smoke does not rustup it into place.\n' >&2
    return 1
  fi
  printf '  rustc  %s  (desktop/, as pinned)\n' "$pinned"
  printf '  cargo  %s\n' "$(cargo --version)"
  printf '  node   %s\n' "$(node --version)"
  printf '  npm    %s\n' "$(npm --version)"
  HOST_TRIPLE="$(cd "$DESKTOP" && rustc -vV | awk '/^host:/ {print $2}')"
  if [ -z "$HOST_TRIPLE" ]; then
    printf '  could not read the host triple from `rustc -vV` — sidecar staging would be wrong.\n' >&2
    return 1
  fi
  printf '  host triple  %s\n' "$HOST_TRIPLE"
  printf '  CARGO_TARGET_DIR  %s\n' "$TARGET_DIR"
  return 0
}

# ---------------------------------------------------------------------------
# [2] Runtime-asset provenance: sherpa-onnx mode + the no-model-on-disk rule.
# ---------------------------------------------------------------------------
stage_provenance() {
  if [ -n "${SHERPA_ONNX_LIB_DIR:-}" ]; then
    SHERPA_MODE="SHERPA_ONNX_LIB_DIR"
    printf '  sherpa-onnx mode: SHERPA_ONNX_LIB_DIR — pre-extracted lib tree, NO network fetch\n'
    printf '                    %s\n' "$SHERPA_ONNX_LIB_DIR"
    if [ ! -d "$SHERPA_ONNX_LIB_DIR" ]; then
      printf '  SHERPA_ONNX_LIB_DIR is set but is not a directory — the build would fail later, opaquely.\n' >&2
      return 1
    fi
  elif [ -n "${SHERPA_ONNX_ARCHIVE_DIR:-}" ]; then
    SHERPA_MODE="SHERPA_ONNX_ARCHIVE_DIR"
    printf '  sherpa-onnx mode: SHERPA_ONNX_ARCHIVE_DIR — vendored .tar.bz2 extracted locally, NO network fetch\n'
    printf '                    %s\n' "$SHERPA_ONNX_ARCHIVE_DIR"
    if [ ! -d "$SHERPA_ONNX_ARCHIVE_DIR" ]; then
      printf '  SHERPA_ONNX_ARCHIVE_DIR is set but is not a directory — the build would fail later, opaquely.\n' >&2
      return 1
    fi
  else
    SHERPA_MODE="network fetch"
    printf '  sherpa-onnx mode: NETWORK FETCH — neither SHERPA_ONNX_LIB_DIR nor SHERPA_ONNX_ARCHIVE_DIR is set,\n'
    printf '                    so sherpa-onnx-sys will download ~19.5 MB from GitHub Releases into\n'
    printf '                    %s/sherpa-onnx-prebuilt/.\n' "$TARGET_DIR"
    printf '                    An OFFLINE clone fails at the yap-diarize stage below, and this is why.\n'
  fi
  printf '  model weights:    none in this repo, none downloaded by this script.\n'
  printf '                    The app fetches them at runtime from src-tauri/src/catalog.json.\n'
  if [ "${YAP_SMOKE_EMPTY_STATE:-0}" = "1" ] && [ "${YAP_SMOKE_HOST_ASSETS:-0}" != "1" ]; then
    printf '  YAP_SMOKE_EMPTY_STATE=1 without YAP_SMOKE_HOST_ASSETS=1 is refused: the default run already\n' >&2
    printf '  empties the state root AND hides the corpus, so the Y7 probe would silently go green.\n' >&2
    printf '  Run the probe as: YAP_SMOKE_HOST_ASSETS=1 YAP_SMOKE_EMPTY_STATE=1 ./scripts/loop-smoke.sh\n' >&2
    return 1
  fi
  if [ "${YAP_SMOKE_HOST_ASSETS:-0}" = "1" ]; then
    STATE_MODE="HOST ASSETS (YAP_SMOKE_HOST_ASSETS=1) — machine state root, ~/yap-eval-corpus, ~/yap-diarize-models"
    printf '  host assets:      YAP_SMOKE_HOST_ASSETS=1 — tests may find THIS machine%ss models, eval corpus and\n' "'"
    printf '                    diarize models. On a machine that has them, stage [11] runs the real-weights\n'
    printf '                    eval sweeps in a debug build, which is slow. This is not a fresh clone.\n'
    if [ "${YAP_SMOKE_EMPTY_STATE:-0}" = "1" ]; then
      SMOKE_STATE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-state.XXXXXX")" || return 1
      [ -d "$SMOKE_STATE_ROOT" ] || return 1
      export YAP_DATA_DIR="$SMOKE_STATE_ROOT"
      STATE_MODE="$STATE_MODE, but EMPTY state root (YAP_SMOKE_EMPTY_STATE=1)"
      printf '  YAP_DATA_DIR:     %s (EMPTY; host corpus still visible — the Y7 probe, known red)\n' "$YAP_DATA_DIR"
    fi
    return 0
  fi
  # DEFAULT: FRESH-CLONE semantics. A fresh clone on a fresh machine has no
  # models, no eval corpus and no diarize models, so every host-asset lookup a
  # test makes is pointed somewhere EMPTY or ABSENT. This is what makes
  # "cargo test passes with an empty application-support dir" ENFORCED rather
  # than claimed, and it is why this script finishes in minutes on a dev box
  # that does have ~/yap-eval-corpus (where stage [11] otherwise spends its time
  # in debug-build real-weights eval sweeps). The temp root is removed on exit.
  if ! SMOKE_STATE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-state.XXXXXX")" || [ ! -d "$SMOKE_STATE_ROOT" ]; then
    printf '  could not create an empty temp state root (mktemp failed) — refusing to claim a model-free run.\n' >&2
    return 1
  fi
  export YAP_DATA_DIR="$SMOKE_STATE_ROOT/state"            # app state root: no models, no history
  export YAP_EVAL_CORPUS="$SMOKE_STATE_ROOT/absent-eval-corpus"      # never created
  export YAP_DIARIZE_MODELS="$SMOKE_STATE_ROOT/absent-diarize-models" # never created
  mkdir -p "$YAP_DATA_DIR" || return 1
  unset YAP_POLISH_MODEL
  STATE_MODE="fresh clone (empty YAP_DATA_DIR, absent eval corpus + diarize models)"
  printf '  YAP_DATA_DIR:       %s  (EMPTY)\n' "$YAP_DATA_DIR"
  printf '  YAP_EVAL_CORPUS:    %s  (absent)\n' "$YAP_EVAL_CORPUS"
  printf '  YAP_DIARIZE_MODELS: %s  (absent)\n' "$YAP_DIARIZE_MODELS"
  printf '                    Every cargo test stage below runs against these, exactly as a fresh clone\n'
  printf '                    would: "no test requires a model on disk" is ENFORCED, not claimed.\n'
  printf '                    YAP_SMOKE_HOST_ASSETS=1 opts back into this machine%ss own assets.\n' "'"
  return 0
}

# ---------------------------------------------------------------------------
# [3..] The gate.
# ---------------------------------------------------------------------------
stage_npm_ci()    { cd "$DESKTOP" && npm ci; }
stage_npm_build() { cd "$DESKTOP" && npm run build; }
stage_npm_test()  { cd "$DESKTOP" && npm test; }

stage_sidecars() {
  cd "$DESKTOP" || return 1
  cargo build -p yap-polish -p yap-diarize --release || return $?
  mkdir -p src-tauri/binaries || return $?
  cp "$TARGET_DIR/release/yap-polish"  "src-tauri/binaries/yap-polish-$HOST_TRIPLE"  || return $?
  cp "$TARGET_DIR/release/yap-diarize" "src-tauri/binaries/yap-diarize-$HOST_TRIPLE" || return $?
  # bundle.externalBin names BOTH; both must exist or every cargo build of the
  # app dies with "resource path ... doesn't exist".
  test -x "src-tauri/binaries/yap-polish-$HOST_TRIPLE"  || return 1
  test -x "src-tauri/binaries/yap-diarize-$HOST_TRIPLE" || return 1
  printf '  staged src-tauri/binaries/yap-polish-%s\n'  "$HOST_TRIPLE"
  printf '  staged src-tauri/binaries/yap-diarize-%s\n' "$HOST_TRIPLE"
  return 0
}

stage_test_polish()  { cd "$DESKTOP" && cargo test -p yap-polish  --release; }
stage_test_diarize() { cd "$DESKTOP" && cargo test -p yap-diarize --release; }

# Scoped rustfmt (see LINT STRICTNESS in the header). The whole-tree check runs
# once; its exit code is read bare; each of its hunks is then classed as NEW
# drift (it rewrites a line THIS checkout changed: blocking) or pre-existing
# drift (advisory), by line range — never by file, which would make every item
# that edits a drifting file sweep hunks it did not write.
stage_fmt() {
  cd "$SRC_TAURI" || return 1
  if ! git -C "$REPO_ROOT" rev-parse --git-dir >/dev/null 2>&1; then
    printf '  not a git checkout — cannot tell new rustfmt drift from drift already on main. Failing closed.\n' >&2
    return 1
  fi
  # The scope base MUST include every commit this branch added. Falling back to
  # HEAD would drop committed drift out of scope and pass it as "pre-existing"
  # (fail OPEN) — so with no computable merge base the stage FAILS CLOSED,
  # unless the caller names a base explicitly with YAP_SMOKE_FMT_BASE.
  local base base_name
  if [ -n "${YAP_SMOKE_FMT_BASE:-}" ]; then
    if ! base="$(git -C "$REPO_ROOT" rev-parse --verify --quiet "${YAP_SMOKE_FMT_BASE}^{commit}")"; then
      printf '  YAP_SMOKE_FMT_BASE=%s does not name a commit in this checkout. Failing closed.\n' "$YAP_SMOKE_FMT_BASE" >&2
      return 1
    fi
    if ! git -C "$REPO_ROOT" merge-base --is-ancestor "$base" HEAD; then
      printf '  YAP_SMOKE_FMT_BASE=%s is not an ancestor of HEAD, so it cannot bound what this checkout\n' "$YAP_SMOKE_FMT_BASE" >&2
      printf '  changed. Failing closed.\n' >&2
      return 1
    fi
    base_name="YAP_SMOKE_FMT_BASE=$YAP_SMOKE_FMT_BASE"
  elif base="$(git -C "$REPO_ROOT" merge-base HEAD origin/main 2>/dev/null)" && [ -n "$base" ]; then
    base_name="merge-base with origin/main"
  else
    printf '  no merge base with origin/main (no origin/main ref, a shallow or single-branch clone,\n' >&2
    printf '  or a remote not named origin). Without it, rustfmt drift COMMITTED on this branch\n' >&2
    printf '  cannot be told from drift already on main. Failing closed.\n' >&2
    printf '  Fix: git fetch origin main (unshallow if needed: git fetch --unshallow origin), or set\n' >&2
    printf '  YAP_SMOKE_FMT_BASE=<the commit this branch forked from> explicitly.\n' >&2
    return 1
  fi
  printf '  scope: .rs files changed since %s (%s), plus staged, unstaged and untracked\n' "${base:0:9}" "$base_name"

  FMT_LOG="$(mktemp "${TMPDIR:-/tmp}/yap-smoke-fmt.XXXXXX")" || return 1
  local fmt_code=0
  cargo fmt --all -- --check --color never >"$FMT_LOG" 2>&1 || fmt_code=$?
  if [ "$fmt_code" -eq 0 ]; then
    printf '  cargo fmt --all -- --check: exit 0 — the whole tree is formatted.\n'
    return 0
  fi
  if ! grep -q '^Diff in ' "$FMT_LOG"; then
    # A non-zero exit with no diff hunks is rustfmt itself failing (parse error,
    # missing component), not drift. Never report that as advisory.
    printf '  cargo fmt exited %d without any diff hunks — rustfmt itself failed:\n' "$fmt_code" >&2
    cat "$FMT_LOG" >&2
    return "$fmt_code"
  fi

  # Which LINES did this checkout change? Per touched .rs file, the new-side line
  # ranges of `git diff -U0 <base>` (committed since the base, staged and
  # unstaged); an untracked .rs file counts as changed on every line. A pure
  # deletion at new-side line c touches the seam c..c+1.
  local rel
  FMT_RANGES="$(mktemp "${TMPDIR:-/tmp}/yap-smoke-fmt-ranges.XXXXXX")" || return 1
  {
    git -C "$REPO_ROOT" -c core.quotePath=false diff -U0 --no-color --no-ext-diff --src-prefix=a/ --dst-prefix=b/ "$base" -- '*.rs' |
      awk '/^\+\+\+ / { f = $0; sub(/^\+\+\+ /, "", f); sub(/\t$/, "", f); sub(/^b\//, "", f); next }
           /^@@ / { split($3, a, ","); c = substr(a[1], 2) + 0; d = (2 in a) ? a[2] + 0 : 1;
                    if (d == 0) { s = c; e = c + 1 } else { s = c; e = c + d - 1 }
                    if (f != "/dev/null") printf "%s\t%d\t%d\n", f, s, e }'
    git -C "$REPO_ROOT" -c core.quotePath=false ls-files --others --exclude-standard -- '*.rs' |
      while IFS= read -r rel; do [ -n "$rel" ] && printf '%s\t1\t2147483647\n' "$rel"; done
  } >"$FMT_RANGES" || return 1

  # rustfmt's "Diff in <file>:<N>:" (or "... at line N:") header gives N = the
  # CURRENT-file line of the hunk's first row; ' ' rows are context, '-' rows are
  # current lines rustfmt would rewrite, '+' rows are its replacement. A hunk is
  # NEW drift — blocking — when one of its '-' lines (or, for a pure insertion,
  # its insertion point) falls inside a line range this checkout changed. Hunks
  # that only sit in untouched lines are main's pre-existing drift — advisory,
  # even inside a file this checkout edited — so an item that edits lib.rs is
  # never asked to sweep the hunks it did not write (PANEL 2026-09-12). The path
  # is everything after "Diff in " (never an awk field), so whitespace parses whole.
  # Output: "F<TAB>new<TAB>total<TAB>path" per file, "H<TAB>path<TAB>N" per new hunk.
  local kind a b path blocking=0 blocking_hunks=0 advisory_files=0 advisory_hunks=0 unparsed=0
  local new_hunks_list=""
  while IFS=$'\t' read -r kind a b path; do
    if [ "$kind" = "H" ]; then
      new_hunks_list="${new_hunks_list}      ${a}:${b}"$'\n'
      continue
    fi
    if [ ! -f "$path" ]; then
      printf '  UNPARSED  %3d hunk(s)  %s   <- not a file on disk; cannot scope it\n' "$b" "$path" >&2
      unparsed=$((unparsed + 1))
      continue
    fi
    if [ "$a" -gt 0 ]; then
      printf '  FAIL      %3d of %3d hunk(s)  %s   <- on lines this checkout changed\n' "$a" "$b" "$path" >&2
      blocking=$((blocking + 1)); blocking_hunks=$((blocking_hunks + a))
      advisory_hunks=$((advisory_hunks + b - a))
    else
      advisory_files=$((advisory_files + 1))
      advisory_hunks=$((advisory_hunks + b))
      printf '  advisory  %3d hunk(s)  %s\n' "$b" "$path"
    fi
  done < <(awk -F'\t' '
    FILENAME == ARGV[1] { nr++; rf[nr] = $1; rs[nr] = $2 + 0; re[nr] = $3 + 0; next }
    function flush(   i, j, hit) {
      if (p == "") return
      total[p]++
      hit = 0
      for (i = 1; i <= nm && !hit; i++)
        for (j = 1; j <= nr && !hit; j++) {
          if (length(p) < length(rf[j]) + 1) continue
          if (substr(p, length(p) - length(rf[j])) != "/" rf[j]) continue
          if (m[i] >= rs[j] && m[i] <= re[j]) hit = 1
        }
      if (hit) { newc[p]++; hl[++nh] = p "\t" start }
      p = ""
    }
    /^Diff in / {
      flush()
      p = $0; sub(/^Diff in /, "", p)
      if (match(p, / at line [0-9]+:$/)) { start = substr(p, RSTART + 9); sub(/:$/, "", start); p = substr(p, 1, RSTART - 1) }
      else if (match(p, /:[0-9]+:$/)) { start = substr(p, RSTART + 1); sub(/:$/, "", start); p = substr(p, 1, RSTART - 1) }
      else { sub(/:$/, "", p); start = 0 }
      start += 0; cur = start; nm = 0; np = 0; nminus = 0
      next
    }
    p == "" { next }
    /^-/ { m[++nm] = cur; nminus++; cur++; next }
    /^\+/ { if (nminus == 0 && np == 0) { m[++nm] = cur - 1; m[++nm] = cur; np = 1 } next }
    { cur++ }
    END {
      flush()
      for (f in total) printf "F\t%d\t%d\t%s\n", newc[f] + 0, total[f], f
      for (i = 1; i <= nh; i++) { split(hl[i], x, "\t"); printf "H\t%s\t%s\n", x[1], x[2] }
    }' "$FMT_RANGES" "$FMT_LOG")

  if [ "$unparsed" -ne 0 ] || [ $((blocking + advisory_files)) -eq 0 ]; then
    printf '\n  rustfmt reported drift but %d path(s) did not resolve to a file (%d scoped).\n' "$unparsed" "$((blocking + advisory_files))" >&2
    printf '  The scope cannot be trusted, so the stage FAILS CLOSED. rustfmt output:\n' >&2
    cat "$FMT_LOG" >&2
    return 1
  fi

  if [ "$blocking" -ne 0 ]; then
    printf '\n  %d rustfmt hunk(s) in %d file(s) sit on lines this checkout changed — NEW drift:\n' "$blocking_hunks" "$blocking" >&2
    printf '%s' "$new_hunks_list" >&2
    printf '  Format THOSE hunks (the "Diff in <file>:<line>" blocks above, in the rustfmt\n' >&2
    printf '  output below). Do not run cargo fmt over the whole file or tree: that also sweeps\n' >&2
    printf '  main'"'"'s pre-existing drift, which no item may reformat mid-loop. rustfmt output:\n' >&2
    cat "$FMT_LOG" >&2
    return 1
  fi
  printf '  ADVISORY: %d hunk(s) of pre-existing drift on lines this checkout did not touch\n' "$advisory_hunks"
  printf '            (ci.yml keeps whole-tree rustfmt advisory until the next tree-wide sweep). PASS.\n'
  return 0
}

# Blocking, -D warnings, app AND sidecars — the two clippy steps ci.yml gates on.
stage_clippy() {
  cd "$SRC_TAURI" || return 1
  cargo clippy --all-targets --features custom-protocol -- -D warnings || return $?
  cd "$DESKTOP" || return 1
  cargo clippy -p yap-polish -p yap-diarize --all-targets --release -- -D warnings
}

stage_cargo_test()    { cd "$SRC_TAURI" && cargo test --features custom-protocol; }
stage_cargo_release() { cd "$SRC_TAURI" && cargo build --release --features custom-protocol; }

# The release Mach-O that CI inspects. Never a debug build: a hard import of a
# 14.2+ CoreAudio symbol is a dyld LOAD failure for every macOS 12/13 user, and
# nothing else in this gate can see it.
stage_weak_link() {
  cd "$REPO_ROOT" || return 1
  local bin="$TARGET_DIR/release/wilson-voice"
  if [ ! -f "$bin" ]; then
    printf '  no release binary at %s — the release build stage should have produced it.\n' "$bin" >&2
    return 1
  fi
  ./scripts/assert-weak-linked-14_4-symbols.sh "$bin"
}

# The structural windowed smoke (Y0-E, #160) is a UI JUDGE, not a build-from-nothing check, and it
# is DESIGNED to be red on today's tree (docs/loop/SMOKE-BASELINE.md: no view carries
# data-empty-state yet). Running it as a blocking stage ahead of the cargo gate made every later
# stage unreachable, so this script could never answer its own question. It is therefore GATED
# BEHIND A FLAG, as Y7's item text specifies ("Run it as part of Y0-B's loop-smoke script, gated
# behind a flag"), and it runs LAST, after the build-from-nothing verdict is already in:
#     YAP_SMOKE_WINDOWED=1 ./scripts/loop-smoke.sh
# With the flag set it is blocking (exit non-zero on any finding) and "no display" is a FAILURE,
# never a named skip. Without the flag it is reported as NOT RUN — never as a pass.
stage_windowed_smoke() {
  case "$(uname -s)" in
    Darwin) command -v system_profiler >/dev/null 2>&1 || { echo "loop-smoke: no display" >&2; return 2; } ;;
    *) [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ] || { echo "loop-smoke: no display" >&2; return 2; } ;;
  esac
  cd "$DESKTOP" && npm run smoke:windowed
}

printf 'loop-smoke — proving a fresh clone of Yap builds, tests and stages both sidecars.\n'
printf 'repo root: %s\n' "$REPO_ROOT"

stage "toolchain present (detect, never install)"        stage_toolchain
stage "runtime-asset provenance (sherpa mode, no models)" stage_provenance
stage "npm ci (desktop)"                                  stage_npm_ci
stage "npm run build (tsc + vite)"                        stage_npm_build
stage "npm test (vitest)"                                 stage_npm_test
stage "build + stage BOTH sidecars for the host triple"    stage_sidecars
stage "cargo test -p yap-polish --release"                stage_test_polish
stage "cargo test -p yap-diarize --release"               stage_test_diarize
stage "rustfmt: blocking on lines this checkout changed"  stage_fmt
stage "cargo clippy -D warnings (app + both sidecars)"    stage_clippy
stage "cargo test --features custom-protocol"             stage_cargo_test
stage "cargo build --release --features custom-protocol"  stage_cargo_release
stage "assert weak-linked 14.4 CoreAudio symbols"         stage_weak_link

if [ "${YAP_SMOKE_WINDOWED:-0}" = "1" ]; then
  WINDOWED_MODE="RUN (YAP_SMOKE_WINDOWED=1) — passed"
  stage "structural windowed smoke (Y0-E, YAP_SMOKE_WINDOWED=1)" stage_windowed_smoke
else
  WINDOWED_MODE="NOT RUN — set YAP_SMOKE_WINDOWED=1 (expected red on today's tree; see docs/loop/SMOKE-BASELINE.md)"
fi

printf '\n════════════════════════════════════════════════════════════\n'
printf 'loop-smoke: PASS — %d/%d stages green.\n' "$STAGE_NO" "$STAGE_NO"
printf 'sherpa-onnx mode used: %s\n' "$SHERPA_MODE"
printf 'model weights: none downloaded by this script; state root mode: %s\n' "$STATE_MODE"
printf 'structural windowed smoke (Y0-E): %s\n' "$WINDOWED_MODE"
printf '════════════════════════════════════════════════════════════\n'
