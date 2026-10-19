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
#     CI is in. A test that leans on the developer's own
#     ~/Library/Application Support/WilsonVoice/models fails HERE.
#
#     Why the corpus and diarize models are hidden too, not only the state
#     root (measured 2026-10-19, fix round for Y0-B): on a dev box that HAS
#     ~/yap-eval-corpus and ~/yap-diarize-models, stage [11] ran
#     tests/meeting_eval.rs's real-weights sweeps through a DEBUG yap-diarize
#     and was still running after the acceptance seat's 600 s window — the
#     smoke test could not answer its own question in a usable time. And with
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
# `cargo fmt --all -- --check` IS blocking here. It was green on origin/main
# when this script landed (#163), then drifted RED (exit 1, seven files) at
# ca50beb on 2026-10-19 because ci.yml still runs it as `|| true`, so nothing
# stopped #218/#227/#229/#231 from merging unformatted. That drift is exactly
# what this stage exists to catch: a red here is a real finding about the tree,
# not noise. Restored to green mechanically with `cargo fmt --all` (no semantic
# change); making the CI step blocking too is Y0-A's item (#234), not this one.

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
  if [ "${YAP_SMOKE_HOST_ASSETS:-0}" = "1" ]; then
    STATE_MODE="HOST ASSETS (YAP_SMOKE_HOST_ASSETS=1) — machine state root, ~/yap-eval-corpus, ~/yap-diarize-models"
    printf '  host assets:      YAP_SMOKE_HOST_ASSETS=1 — tests may find THIS machine%ss models, eval corpus and\n' "'"
    printf '                    diarize models. On a machine that has them, stage [11] runs the real-weights\n'
    printf '                    eval sweeps in a debug build: HOURS, not minutes. This is not a fresh clone.\n'
    if [ "${YAP_SMOKE_EMPTY_STATE:-0}" = "1" ]; then
      SMOKE_STATE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-state.XXXXXX")"
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
  # that does have ~/yap-eval-corpus (where stage [11] otherwise spends hours
  # in debug-build real-weights eval sweeps — measured 2026-10-19).
  SMOKE_STATE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yap-smoke-state.XXXXXX")"
  if [ -n "$(ls -A "$SMOKE_STATE_ROOT" 2>/dev/null)" ]; then
    printf '  the smoke state root is not empty — refusing to claim a model-free test run.\n' >&2
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
stage "build + stage BOTH sidecars for $(rustc -vV | awk '/^host:/ {print $2}')" stage_sidecars
stage "cargo test -p yap-polish --release"                stage_test_polish
stage "cargo test -p yap-diarize --release"               stage_test_diarize
stage "cargo fmt --all -- --check"                        stage_fmt
stage "cargo clippy --all-targets --features custom-protocol" stage_clippy
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
