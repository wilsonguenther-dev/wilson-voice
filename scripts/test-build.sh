#!/usr/bin/env bash
# TC-1 — the TEST CHANNEL: build any head as "Yap Test.app" and install it
# next to (never over) the real Yap. docs/TEST-CHANNEL.md is the long version.
#
#   scripts/test-build.sh [--dry-run] [--no-fetch] [--keep] [--pr N | --branch X | --sha S | main]
#
# What makes it a different app (so the real Yap's TCC grants, history,
# settings, license and updater are never touched):
#   YAP_CHANNEL=test  (build time) → src-tauri/src/channel.rs: state root
#                     ~/Library/Application Support/WilsonVoice-Test, its own
#                     log, updater DISABLED, chip "TEST BUILD <sha> · <branch>".
#   --config overlay  → identifier com.wilsonguenther.wilson-voice.test,
#                     productName "Yap Test", CFBundleName "Yap Test", no
#                     updater artifacts, updater endpoints [].
# A head that predates the channel support gets it MERGED into a throwaway
# build worktree (never pushed). Nothing is notarized: local only.
set -euo pipefail
export GH_PROMPT_DISABLED=1 GH_PAGER=cat GIT_PAGER=cat

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="${YAP_TEST_CACHE:-$HOME/.cache/yap-test-channel}"
export CARGO_TARGET_DIR="${YAP_TEST_TARGET_DIR:-${CARGO_TARGET_DIR:-$CACHE/target}}"
BUILD_WT="${YAP_TEST_WORKTREE:-$CACHE/head}"
INSTALL_DIR="${YAP_TEST_INSTALL_DIR:-/Applications}"
LOG_MD="${YAP_TEST_LOG_MD:-$HOME/Obsidian/Wilson-Brain/Projects/Loop-Logs/yap-test-builds.md}"
IDENTIFIER="com.wilsonguenther.wilson-voice.test"
PRODUCT="Yap Test"
PROD_IDENTIFIER="com.wilsonguenther.wilson-voice"
CHANNEL_MARKER="desktop/src-tauri/src/channel.rs"

die() { echo "FATAL: $*" >&2; exit 1; }
usage() { sed -n '4,5p' "$0" | sed 's/^# *//'; exit "${1:-0}"; }

DRY=0 FETCH=1 KEEP=0 MODE="main" ARG=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)  DRY=1 ;;
    --no-fetch) FETCH=0 ;;
    --keep)     KEEP=1 ;;
    --pr)       MODE="pr";     ARG="${2:?--pr needs a number}"; shift ;;
    --branch)   MODE="branch"; ARG="${2:?--branch needs a name}"; shift ;;
    --sha)      MODE="sha";    ARG="${2:?--sha needs a commit}"; shift ;;
    main)       MODE="main" ;;
    -h|--help)  usage 0 ;;
    *)          echo "unknown argument: $1" >&2; usage 2 ;;
  esac
  shift
done

git_() { git -C "$ROOT" "$@"; }

# ---- 1. resolve the head --------------------------------------------------
case "$MODE" in
  main)
    [[ $FETCH == 1 ]] && git_ fetch -q origin main
    SHA="$(git_ rev-parse --verify -q origin/main^{commit})" || die "origin/main not found"
    LABEL="main" ;;
  branch)
    [[ $FETCH == 1 ]] && git_ fetch -q origin "$ARG"
    SHA="$(git_ rev-parse --verify -q "origin/$ARG^{commit}" || git_ rev-parse --verify -q "$ARG^{commit}")" \
      || die "branch $ARG not found (origin/$ARG or local)"
    LABEL="$ARG" ;;
  pr)
    [[ "$ARG" =~ ^[0-9]+$ ]] || die "--pr takes a number, got $ARG"
    if [[ $FETCH == 1 ]]; then
      git_ fetch -q origin "refs/pull/$ARG/head" || die "could not fetch PR #$ARG"
      SHA="$(git_ rev-parse FETCH_HEAD)"
    else
      SHA="$(git_ rev-parse --verify -q "refs/remotes/origin/pr/$ARG^{commit}")" || die "--no-fetch: no local ref for PR #$ARG"
    fi
    BR="$(cd "$ROOT" && gh pr view "$ARG" --json headRefName -q .headRefName 2>/dev/null || true)"
    LABEL="#$ARG${BR:+ $BR}" ;;
  sha)
    SHA="$(git_ rev-parse --verify -q "$ARG^{commit}")" || die "commit $ARG not found"
    LABEL="$(git_ name-rev --name-only --refs='refs/remotes/origin/*' "$SHA" 2>/dev/null | sed 's#^origin/##; s/[~^].*//')"
    [[ -z "$LABEL" || "$LABEL" == "undefined" ]] && LABEL="sha" ;;
esac
SHORT="${SHA:0:7}"

# Does the head carry the channel switch? If not, graft this checkout's support.
SUPPORT="$(git_ rev-parse HEAD)"
if git_ cat-file -e "$SHA:$CHANNEL_MARKER" 2>/dev/null; then
  GRAFT="no (head has channel support)"
else
  git_ cat-file -e "$SUPPORT:$CHANNEL_MARKER" 2>/dev/null \
    || die "neither the head nor this checkout ($SUPPORT) has $CHANNEL_MARKER — run from a checkout that has the test channel"
  GRAFT="yes (merge ${SUPPORT:0:7} into a throwaway build worktree)"
fi

# ---- 2. signing identity: Developer ID > Apple Development > ad-hoc --------
IDS="$(security find-identity -v -p codesigning 2>/dev/null || true)"
IDENTITY="$(sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' <<<"$IDS" | head -1)"
[[ -z "$IDENTITY" ]] && IDENTITY="$(sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' <<<"$IDS" | head -1)"
if [[ -n "$IDENTITY" ]]; then ID_CLASS="${IDENTITY%%:*}"; else IDENTITY="-"; ID_CLASS="ad-hoc (TCC grants will NOT survive a rebuild)"; fi

# ---- 3. the config overlay --------------------------------------------------
OVERLAY_DIR="$CARGO_TARGET_DIR/yap-test-channel"
OVERLAY="$OVERLAY_DIR/tauri.test.conf.json"
PLIST="$OVERLAY_DIR/Info.test.plist"
overlay_json() {
  cat <<JSON
{
  "productName": "$PRODUCT",
  "identifier": "$IDENTIFIER",
  "bundle": {
    "targets": ["app"],
    "createUpdaterArtifacts": false,
    "macOS": { "infoPlist": "$PLIST" }
  },
  "plugins": { "updater": { "endpoints": [] } }
}
JSON
}

APP_DEST="$INSTALL_DIR/$PRODUCT.app"
cat <<INFO
test-build: head      $SHA ($LABEL)
test-build: chip      TEST BUILD $SHORT · $LABEL
test-build: graft     $GRAFT
test-build: channel   YAP_CHANNEL=test
test-build: identity  $ID_CLASS (not notarized)
test-build: target    $CARGO_TARGET_DIR
test-build: install   $APP_DEST
test-build: data dir  $HOME/Library/Application Support/WilsonVoice-Test
test-build: config overlay:
$(overlay_json)
INFO
[[ "$APP_DEST" == *"/Yap Test.app" ]] || die "refusing to install anywhere but a 'Yap Test.app' path"
if [[ $DRY == 1 ]]; then echo "test-build: dry run — nothing built or installed"; exit 0; fi

# ---- 4. throwaway build worktree at the head -------------------------------
mkdir -p "$CACHE" "$OVERLAY_DIR"
if [[ -d "$BUILD_WT/.git" || -f "$BUILD_WT/.git" ]]; then
  git -C "$BUILD_WT" merge --abort 2>/dev/null || true
  git -C "$BUILD_WT" checkout -q --force --detach "$SHA"
  git -C "$BUILD_WT" clean -q -fd -e node_modules -e desktop/node_modules
else
  git_ worktree add -q --detach "$BUILD_WT" "$SHA"
fi
if [[ "$GRAFT" == yes* ]]; then
  git -C "$BUILD_WT" -c user.name="yap test-build" -c user.email="test-build@localhost" \
    merge -q --no-edit --no-ff "$SUPPORT" >/dev/null \
    || { git -C "$BUILD_WT" merge --abort || true; die "head $SHORT conflicts with the test-channel support — rebase it on main first"; }
fi
D="$BUILD_WT/desktop"

# Info.plist with the test names (Tauri merges the user plist OVER its own keys).
cp "$D/src-tauri/Info.plist" "$PLIST"
plutil -replace CFBundleName -string "$PRODUCT" "$PLIST"
plutil -replace CFBundleDisplayName -string "$PRODUCT" "$PLIST"
overlay_json > "$OVERLAY"

# ---- 5. build (sidecars ride beforeBuildCommand, exactly like a release) ----
( cd "$D" && { [[ -d node_modules && node_modules/.package-lock.json -nt package-lock.json ]] || npm ci --no-audit --no-fund --prefer-offline; } )
unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD
export APPLE_SIGNING_IDENTITY="$IDENTITY" YAP_CHANNEL=test YAP_BUILD_SHA="$SHA" YAP_BUILD_BRANCH="$LABEL"
( cd "$D" && npx tauri build --bundles app --config "$OVERLAY" )
APP="$CARGO_TARGET_DIR/release/bundle/macos/$PRODUCT.app"
[[ -d "$APP" ]] || die "tauri build produced no $APP"

# ---- 6. prove it is the TEST app before it goes anywhere near /Applications -
GOT_ID="$(defaults read "$APP/Contents/Info" CFBundleIdentifier)"
[[ "$GOT_ID" == "$IDENTIFIER" ]] || die "built bundle id is $GOT_ID, expected $IDENTIFIER — not installing"
[[ "$GOT_ID" != "$PROD_IDENTIFIER" ]] || die "built bundle carries the PRODUCTION id — not installing"
[[ "$(defaults read "$APP/Contents/Info" CFBundleName)" == "$PRODUCT" ]] || die "CFBundleName is not $PRODUCT"
BIN="$APP/Contents/MacOS/$(defaults read "$APP/Contents/Info" CFBundleExecutable)"
grep -aq "WilsonVoice-Test" "$BIN" || die "binary was not compiled with YAP_CHANNEL=test (no test data dir) — not installing"
codesign --verify --deep --strict "$APP" || die "codesign verification failed"
codesign -dvv "$APP" 2>&1 | grep -E '^(Identifier|Authority=Developer|Authority=Apple Development|Signature)' | sed -E 's/\(([A-Z0-9]{10})\)/(TEAMID)/g' || true

# ---- 7. install, replacing only the previous test build --------------------
osascript -e "tell application id \"$IDENTIFIER\" to quit" >/dev/null 2>&1 || true
for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -f "$APP_DEST/Contents/MacOS/" >/dev/null || break; sleep 1; done
rm -rf "$APP_DEST"
ditto "$APP" "$APP_DEST"
xattr -dr com.apple.quarantine "$APP_DEST" 2>/dev/null || true

# Models: an APFS clone (zero extra disk) of the real Yap's models, once, so the
# test build does not re-download gigabytes. History/settings/license stay separate.
TROOT="$HOME/Library/Application Support/WilsonVoice-Test"
PMODELS="$HOME/Library/Application Support/WilsonVoice/models"
if [[ ! -e "$TROOT/models" && -d "$PMODELS" ]]; then
  mkdir -p "$TROOT" && cp -cR "$PMODELS" "$TROOT/models" 2>/dev/null || cp -R "$PMODELS" "$TROOT/models" || true
fi

# ---- 8. record it -----------------------------------------------------------
STAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
if [[ -d "$(dirname "$LOG_MD")" ]]; then
  [[ -f "$LOG_MD" ]] || printf '# Yap Test builds\n\nOne line per `scripts/test-build.sh` install of /Applications/Yap Test.app.\n\n' > "$LOG_MD"
  printf -- '- %s · %s · %s · graft %s · %s\n' "$STAMP" "$SHORT" "$LABEL" "${GRAFT%% *}" "$ID_CLASS" >> "$LOG_MD"
fi
[[ $KEEP == 1 ]] || git_ worktree remove --force "$BUILD_WT" 2>/dev/null || true

pgrep -f "/Applications/Yap.app/Contents/MacOS/" >/dev/null && \
  echo "NOTE: the real Yap is running — quit it before testing (both apps listen for the same hotkeys)."
echo "test-build: installed $APP_DEST — $SHA ($LABEL) at $STAMP"
