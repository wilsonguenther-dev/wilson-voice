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
#     `YAP_SMOKE_EMPTY_STATE=1` proves it mechanically: it points YAP_DATA_DIR
#     (the documented, resolved-once state-root override — see ARCHITECTURE.md's
#     Runtime Dependencies table) at a freshly created EMPTY directory for every
#     cargo test stage, so a test leaning on the developer's own
#     ~/Library/Application Support/WilsonVoice/models fails HERE.
#
#     It is OPT-IN, and that is a measured decision, not an oversight. Run on
#     2026-09-14 against origin/main it is RED, with six named failures, all in
#     tests/meeting_eval.rs and all only reachable on a machine that also has the
#     ~/yap-eval-corpus fixtures (CI has neither, and returns early):
#         meeting_eval_anti_alias_eer_regression                            <- needs sherpa-onnx-pyannote-segmentation-3-0
#         meeting_eval_lecture_wer_is_under_the_gate                        <- needs an ASR model
#         meeting_eval_seam_dedupe_and_ordering_hold                        <- needs an ASR model
#         meeting_eval_two_track_ordering_survives_the_clock_mismatch       <- needs an ASR model
#         meeting_eval_the_fixed_clock_tie_break_only_pops_words_the_cut_truncated
#         meeting_eval_vad_cut_boundaries_put_no_speech_in_the_overlap
#     Those six tests are wrong by this rule and fixing them is Y7's item, not
#     this script's. Making the probe blocking today would paint the smoke test
#     red for a defect it does not own, and a smoke test that is always red
#     tells you nothing. Do NOT make this script download a model to go green.
#     The default run therefore uses the machine's real state root, which is
#     exactly what CI and the shared preamble's gate measure.
#
# LINT STRICTNESS — clippy runs in the same INFORMATIONAL mode that ci.yml uses
# today: `cargo clippy --all-targets --features custom-protocol`, with no
# `-D warnings`. That is deliberate and it is measured, not assumed: on
# origin/main today `cargo clippy ... -- -D warnings` exits 101 with 16 lints
# that need real refactors (field_reassign_with_default and friends). Making it
# blocking is its own loop item (Y0-A); flipping the default here would make
# this script red on an unmodified tree, which is a smoke test that tells you
# nothing. Set YAP_SMOKE_CLIPPY_STRICT=1 to gate on `-D warnings` once that item
# lands. Never silence a lint to make this green.
# `cargo fmt --all -- --check` IS blocking here and it passes on origin/main
# today (measured, exit 0).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP="$REPO_ROOT/desktop"
SRC_TAURI="$DESKTOP/src-tauri"
TARGET_DIR="${CARGO_TARGET_DIR:-$DESKTOP/target}"

STAGE_NO=0

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
  printf '  rustc  %s\n' "$(rustc --version)"
  printf '  cargo  %s\n' "$(cargo --version)"
  printf '  node   %s\n' "$(node --version)"
  printf '  npm    %s\n' "$(npm --version)"
  HOST_TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
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
  if [ "${YAP_SMOKE_EMPTY_STATE:-0}" = "1" ]; then
    SMOKE_STATE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-state.XXXXXX")"
    export YAP_DATA_DIR="$SMOKE_STATE_ROOT"
    if [ -n "$(ls -A "$YAP_DATA_DIR" 2>/dev/null)" ]; then
      printf '  the smoke state root is not empty — refusing to claim a model-free test run.\n' >&2
      return 1
    fi
    STATE_MODE="empty (YAP_SMOKE_EMPTY_STATE=1)"
    printf '  YAP_DATA_DIR:     %s\n' "$YAP_DATA_DIR"
    printf '                    freshly created and EMPTY. Every cargo test stage below runs against it,\n'
    printf '                    which is how "no test requires a model on disk" is ENFORCED, not claimed.\n'
    printf '                    Known RED on origin/main: six tests/meeting_eval.rs tests need models and\n'
    printf '                    the ~/yap-eval-corpus fixtures. That is Y7, not this script. See the header.\n'
  else
    STATE_MODE="machine default (set YAP_SMOKE_EMPTY_STATE=1 to prove no test needs a model)"
    printf '  YAP_DATA_DIR:     unset — using this machine%ss real state root, which is what CI measures.\n' "'"
    printf '                    Set YAP_SMOKE_EMPTY_STATE=1 to re-run every cargo test against an EMPTY\n'
    printf '                    state root and prove no test requires a model on disk (header explains\n'
    printf '                    why that probe is opt-in and which six tests it currently catches).\n'
  fi
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
stage_fmt()          { cd "$SRC_TAURI" && cargo fmt --all -- --check; }

stage_clippy() {
  cd "$SRC_TAURI" || return 1
  if [ "${YAP_SMOKE_CLIPPY_STRICT:-0}" = "1" ]; then
    printf '  YAP_SMOKE_CLIPPY_STRICT=1 — gating on -D warnings\n'
    cargo clippy --all-targets --features custom-protocol -- -D warnings
    return $?
  fi
  printf '  informational, matching ci.yml (no -D warnings; 16 known lints are Y0-A, not this gate).\n'
  printf '  Set YAP_SMOKE_CLIPPY_STRICT=1 to make it blocking.\n'
  cargo clippy --all-targets --features custom-protocol
  return $?
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

printf 'loop-smoke — proving a fresh clone of Yap builds, tests and stages both sidecars.\n'
printf 'repo root: %s\n' "$REPO_ROOT"

stage "toolchain present (detect, never install)"        stage_toolchain
stage "runtime-asset provenance (sherpa mode, no models)" stage_provenance
stage "npm ci (desktop)"                                  stage_npm_ci
stage "npm run build (tsc + vite)"                        stage_npm_build
stage "npm test (vitest)"                                 stage_npm_test
stage "build + stage BOTH sidecars for $(rustc -vV | awk '/^host:/ {print $2}')" stage_sidecars
stage "cargo test -p yap-polish --release"                stage_test_polish
stage "cargo test -p yap-diarize --release"               stage_test_diarize
stage "cargo fmt --all -- --check"                        stage_fmt
stage "cargo clippy --all-targets --features custom-protocol" stage_clippy
stage "cargo test --features custom-protocol"             stage_cargo_test
stage "cargo build --release --features custom-protocol"  stage_cargo_release
stage "assert weak-linked 14.4 CoreAudio symbols"         stage_weak_link

printf '\n════════════════════════════════════════════════════════════\n'
printf 'loop-smoke: PASS — %d/%d stages green.\n' "$STAGE_NO" "$STAGE_NO"
printf 'sherpa-onnx mode used: %s\n' "$SHERPA_MODE"
printf 'model weights: none downloaded by this script; state root mode: %s\n' "$STATE_MODE"
printf '════════════════════════════════════════════════════════════\n'
