#!/usr/bin/env bash
# yap24-OS4 — Entitlement audit: prove, per entitlement, whether the APP binary
# needs cs.allow-jit / cs.allow-unsigned-executable-memory under the HARDENED
# RUNTIME, and that the sidecars run on their own (empty) entitlement files.
#
# WHY IT CANNOT PASS VACUOUSLY. cs.* entitlements only mean anything when the
# hardened runtime is on. So every variant here is signed with
#   codesign --options runtime --timestamp --entitlements <variant> --sign <real identity>
# and, before its run is trusted:
#   (a) codesign -dv must show flags=0x10000(runtime), or the script exits non-zero;
#   (b) the entitlements read back from the signature must match the variant;
#   (c) a DYLD_PRINT_LIBRARIES probe must be SILENT (the runtime strips DYLD_*),
#       and a control copy signed WITHOUT the runtime must be LOUD — proof the
#       probe can tell the two apart, i.e. that this experiment can fail.
# Then the SIGNED binary does its real work: --transcribe-file on the committed
# fixture, in-process transcribe-cpp + ggml-Metal decode, against a scratch
# YAP_DATA_DIR with the cached model linked in (an empty scratch root never
# reaches Metal: it would try to download a model instead). A pass needs exit 0,
# the fixture's words on stdout and "using metal backend" on stderr.
#
# USAGE
#   YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR=<dir holding the ASR gguf> \
#     bash scripts/qa/entitlement-diet.sh
# ENV
#   YAP_DATA_DIR       scratch state root (REQUIRED; never the real one)
#   YAP_MODEL_DIR      directory holding parakeet-unified-en-0.6b-Q8_0.gguf (REQUIRED)
#   YAP_POLISH_MODEL   polish gguf for the yap-polish run (default: YAP_MODEL_DIR,
#                      then the default state root's models dir, read-only)
#   CARGO_TARGET_DIR   where the release binaries are built (warned if unset)
#   YAP_DIET_NO_BUILD=1  use the binaries already in $CARGO_TARGET_DIR/release
#   YAP_DIET_TIMESTAMP=none  sign with --timestamp=none (offline machine)
#   APPLE_SIGNING_IDENTITY  explicit identity (never "-")
# EXIT  0 every variant ran as recorded · 1 a variant failed · 2 usage ·
#       3 the harness itself is broken (runtime flag missing, control not loud)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DESKTOP="$ROOT/desktop"
SRC="$DESKTOP/src-tauri"
ENT="$SRC/Entitlements.plist"
SIDECAR_ENT_DIR="$SRC/entitlements"
CONF="$SRC/tauri.conf.json"
FIXTURE="$SRC/tests/fixtures/quick-brown-fox-16k.wav"
ASR_MODEL_ID="handy-computer/parakeet-unified-en-0.6b-gguf"
ASR_FILE="parakeet-unified-en-0.6b-Q8_0.gguf"
POLISH_FILE="qwen2.5-0.5b-instruct-q4_k_m.gguf"
CANDIDATES=(com.apple.security.cs.allow-jit com.apple.security.cs.allow-unsigned-executable-memory)
PB=/usr/libexec/PlistBuddy

usage() { echo "entitlement-diet: $*" >&2; exit 2; }
broken() { echo "entitlement-diet: HARNESS BROKEN — $*" >&2; exit 3; }
redact() { sed -E -e 's/\(([A-Z0-9]{10})\)/(TEAMID)/g' -e 's/^TeamIdentifier=.*/TeamIdentifier=(TEAMID)/'; }

# ---- inputs -----------------------------------------------------------------
[[ -n "${YAP_DATA_DIR:-}" ]] || usage "YAP_DATA_DIR must name a scratch state root"
[[ -n "${YAP_MODEL_DIR:-}" ]] || usage "YAP_MODEL_DIR must name the directory holding $ASR_FILE"
case "$YAP_DATA_DIR" in
  "$HOME/Library/"*|"$HOME/Library") usage "YAP_DATA_DIR=$YAP_DATA_DIR is under ~/Library — refusing to touch a real state root" ;;
esac
[[ -f "$YAP_MODEL_DIR/$ASR_FILE" ]] || usage "no $ASR_FILE in YAP_MODEL_DIR=$YAP_MODEL_DIR"
[[ -f "$FIXTURE" ]] || usage "fixture missing: $FIXTURE"
PROD_ID="$(sed -n 's/^  "identifier": *"\([^"]*\)".*/\1/p' "$CONF" | head -1)"
[[ -n "$PROD_ID" ]] || usage "could not read identifier from $CONF"
mkdir -p "$YAP_DATA_DIR/models"
ln -sf "$YAP_MODEL_DIR/$ASR_FILE" "$YAP_DATA_DIR/models/$ASR_FILE"
printf '{"native_model":"%s"}\n' "$ASR_MODEL_ID" > "$YAP_DATA_DIR/settings.json"

# ---- identity (same order as scripts/sign-local.sh: Developer ID first) ------
AVAILABLE="$(security find-identity -v -p codesigning 2>/dev/null || true)"
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  [[ "$APPLE_SIGNING_IDENTITY" != "-" ]] || usage "ad-hoc identity refused"
  grep -qF "$APPLE_SIGNING_IDENTITY" <<<"$AVAILABLE" || usage "APPLE_SIGNING_IDENTITY not in this keychain"
  IDENTITY="$APPLE_SIGNING_IDENTITY"
else
  IDENTITY="$(sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' <<<"$AVAILABLE" | head -1)"
  [[ -n "$IDENTITY" ]] || IDENTITY="$(sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' <<<"$AVAILABLE" | head -1)"
fi
[[ -n "$IDENTITY" ]] || usage "no code-signing identity (security find-identity -v -p codesigning); never ad-hoc"
IDENTITY_CLASS="${IDENTITY%%:*}"
TS=(--timestamp)
[[ "${YAP_DIET_TIMESTAMP:-}" == "none" ]] && TS=(--timestamp=none)

# ---- binaries -----------------------------------------------------------------
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  echo "entitlement-diet: WARNING CARGO_TARGET_DIR is unset — building into $DESKTOP/target (cold build)" >&2
fi
REL="${CARGO_TARGET_DIR:-$DESKTOP/target}/release"
if [[ "${YAP_DIET_NO_BUILD:-}" != "1" ]]; then
  ( cd "$DESKTOP" && cargo build --release -p yap-polish -p yap-diarize ) >&2
  TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
  mkdir -p "$SRC/binaries"
  cp "$REL/yap-polish" "$SRC/binaries/yap-polish-$TRIPLE"
  cp "$REL/yap-diarize" "$SRC/binaries/yap-diarize-$TRIPLE"
  ( cd "$SRC" && cargo build --release --features custom-protocol ) >&2
fi
for b in wilson-voice yap-polish yap-diarize; do
  [[ -x "$REL/$b" ]] || usage "no built binary at $REL/$b"
done

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT

# ---- helpers ----------------------------------------------------------------
# sign_copy <src bin> <dest bin> <entitlements|""> <runtime yes|no> <identifier>
sign_copy() {
  cp "$1" "$2"
  local opts=(--force "${TS[@]}" --identifier "$5" --sign "$IDENTITY")
  [[ "$4" == "yes" ]] && opts+=(--options runtime)
  [[ -n "$3" ]] && opts+=(--entitlements "$3")
  codesign "${opts[@]}" "$2" 2>"$W/codesign.err" || { cat "$W/codesign.err" >&2; broken "codesign failed on $2"; }
}
cd_flags() { codesign -dv --verbose=4 "$1" 2>&1 | sed -n 's/^CodeDirectory .*\(flags=[^ ]*\).*/\1/p'; }
# The runtime must be ON, read back from the signature — the gate this item exists for.
assert_runtime() {
  local info; info="$(codesign -dv --verbose=4 "$1" 2>&1)"
  grep -q 'Signature=adhoc' <<<"$info" && broken "$1 is ad-hoc signed"
  grep -q '^Authority=' <<<"$info" || broken "$1 carries no Authority chain"
  grep -q 'flags=0x10000(runtime)' <<<"$info" || broken "$1 does not show flags=0x10000(runtime): $(cd_flags "$1")"
}
signed_ents() { codesign -d --entitlements - --xml "$1" 2>/dev/null | tr -d '\n\t ' ; }
# DYLD_* env vars are stripped for a hardened-runtime process without
# cs.allow-dyld-environment-variables: count dyld's own lines on stderr.
dyld_lines() { DYLD_PRINT_LIBRARIES=1 "$1" --transcribe-file "$W/does-not-exist.wav" 2>&1 >/dev/null | grep -c '^dyld\[' || true; }

# ---- control: the probe must see the difference ------------------------------
mkdir -p "$W/control"
sign_copy "$REL/wilson-voice" "$W/control/wilson-voice" "$ENT" no "$PROD_ID.entitlement-diet.control"
CONTROL_FLAGS="$(cd_flags "$W/control/wilson-voice")"
[[ "$CONTROL_FLAGS" != *runtime* ]] || broken "control copy signed without the runtime still shows $CONTROL_FLAGS"
CONTROL_DYLD="$(dyld_lines "$W/control/wilson-voice")"
[[ "$CONTROL_DYLD" -gt 0 ]] || broken "control (runtime OFF) printed no dyld lines — the DYLD probe cannot tell runtime on from off"
echo "identity class: $IDENTITY_CLASS · timestamp: ${TS[*]}"
echo "control (same identity, runtime OFF): $CONTROL_FLAGS · DYLD_PRINT_LIBRARIES lines=$CONTROL_DYLD (expected > 0)"

# ---- app-binary variants ----------------------------------------------------
# full    = the committed plist + every candidate (what main shipped before OS4)
# -<c>    = full minus one candidate (one variant per removed entitlement)
# shipped = Entitlements.plist exactly as committed
make_plist() { # make_plist <out> <candidate to leave out | "">  (adds every other candidate)
  cp "$ENT" "$1"
  local c
  for c in "${CANDIDATES[@]}"; do
    $PB -c "Delete :$c" "$1" >/dev/null 2>&1 || true
    [[ "$c" == "$2" ]] && continue
    $PB -c "Add :$c bool true" "$1"
  done
}
VARIANTS=(full)
for c in "${CANDIDATES[@]}"; do VARIANTS+=("-${c#com.apple.security.}"); done
VARIANTS+=(shipped)

FAILS=0
ROWS=()
for v in "${VARIANTS[@]}"; do
  d="$W/$v"; mkdir -p "$d"
  case "$v" in
    full)    make_plist "$d/ents.plist" "" ;;
    shipped) cp "$ENT" "$d/ents.plist" ;;
    -*)      make_plist "$d/ents.plist" "com.apple.security.${v#-}" ;;
  esac
  sign_copy "$REL/wilson-voice" "$d/wilson-voice" "$d/ents.plist" yes "$PROD_ID.entitlement-diet"
  assert_runtime "$d/wilson-voice"
  flags="$(cd_flags "$d/wilson-voice")"
  got="$(signed_ents "$d/wilson-voice")"
  granted=""
  for c in "${CANDIDATES[@]}"; do
    want=no
    $PB -c "Print :$c" "$d/ents.plist" >/dev/null 2>&1 && want=yes
    have=no; [[ "$got" == *"<key>$c</key><true/>"* ]] && have=yes
    [[ "$want" == "$have" ]] || broken "variant $v: $c expected=$want but the signature says $have"
    [[ "$have" == yes ]] && granted+="${c#com.apple.security.cs.} "
  done
  [[ "$got" == *"<key>com.apple.security.app-sandbox</key><false/>"* ]] || broken "variant $v: app-sandbox is not present-and-false in the signature"
  dl="$(dyld_lines "$d/wilson-voice")"
  [[ "$dl" -eq 0 ]] || broken "variant $v: runtime flag shown but DYLD_PRINT_LIBRARIES still printed $dl lines"

  set +e
  YAP_DATA_DIR="$YAP_DATA_DIR" RUST_LOG=info "$d/wilson-voice" --transcribe-file "$FIXTURE" >"$d/out.txt" 2>"$d/err.txt"
  rc=$?
  set -e
  metal=no; grep -q 'using metal backend' "$d/err.txt" && metal=yes
  words=no; grep -qi 'quick brown fox' "$d/out.txt" && words=yes
  verdict=PASS
  if [[ $rc -ne 0 || $metal != yes || $words != yes ]]; then
    verdict=FAIL; FAILS=$((FAILS + 1))
    echo "--- $v stderr (tail) ---" >&2; tail -20 "$d/err.txt" >&2
  fi
  ROWS+=("| $v | ${granted:-none} | $flags | dyld=$dl | exit=$rc | metal=$metal | \"$(tr -d '\n' <"$d/out.txt" | cut -c1-60)\" | $verdict |")
done

# ---- sidecars: their own entitlement files, runtime on ------------------------
POLISH_MODEL="${YAP_POLISH_MODEL:-}"
if [[ -z "$POLISH_MODEL" ]]; then
  for cand in "$YAP_MODEL_DIR/$POLISH_FILE" "$HOME/Library/Application Support/WilsonVoice/models/$POLISH_FILE"; do
    [[ -f "$cand" ]] && { POLISH_MODEL="$cand"; break; }
  done
fi
for s in yap-polish yap-diarize; do
  d="$W/$s"; mkdir -p "$d"
  [[ -f "$SIDECAR_ENT_DIR/$s.plist" ]] || broken "no $SIDECAR_ENT_DIR/$s.plist"
  sign_copy "$REL/$s" "$d/$s" "$SIDECAR_ENT_DIR/$s.plist" yes "$PROD_ID.entitlement-diet.$s"
  assert_runtime "$d/$s"
  flags="$(cd_flags "$d/$s")"
  args=()
  if [[ "$s" == yap-polish ]]; then
    if [[ -z "$POLISH_MODEL" ]]; then
      ROWS+=("| $s (own plist) | none | $flags | - | - | - | SKIPPED: no $POLISH_FILE found | SKIPPED |")
      continue
    fi
    args=(--model "$POLISH_MODEL")
  fi
  set +e
  "$d/$s" ${args[@]+"${args[@]}"} </dev/null >"$d/out.txt" 2>"$d/err.txt"
  rc=$?
  set -e
  ready=no; head -1 "$d/out.txt" | grep -q '^{' && ready=yes
  verdict=PASS
  if [[ $rc -ne 0 || $ready != yes ]]; then
    verdict=FAIL; FAILS=$((FAILS + 1))
    echo "--- $s stderr (tail) ---" >&2; tail -20 "$d/err.txt" >&2
  fi
  ROWS+=("| $s (own plist) | none | $flags | - | exit=$rc | handshake=$ready | $(head -1 "$d/out.txt" | cut -c1-60) | $verdict |")
done

echo
echo "| variant | cs.* granted | CodeDirectory flags | DYLD probe | run | engine | output | verdict |"
echo "|---|---|---|---|---|---|---|---|"
printf '%s\n' "${ROWS[@]}"
if grep -q -- '"--transcribe-meeting"' "$SRC/src/cli.rs" 2>/dev/null; then
  echo "note: --transcribe-meeting exists on this tree but is not exercised by this script yet"
else
  echo "note: --transcribe-meeting is not wired on this tree (NT2) — not run"
fi
if [[ $FAILS -gt 0 ]]; then
  echo "entitlement-diet: FAIL ($FAILS run(s) failed)"
  exit 1
fi
echo "entitlement-diet: PASS — every variant signed with flags=0x10000(runtime) ran its real work"
