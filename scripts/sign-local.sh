#!/usr/bin/env bash
# SEC-A — the one place Yap is code-signed on a developer machine.
#
# WHY THIS EXISTS. macOS keys TCC grants (Microphone, Accessibility, Input
# Monitoring) to the code signature's designated requirement. An ad-hoc
# signature ("-") has no stable designated requirement: its cdhash changes on
# every build, so every rebuild is a brand-new stranger and every grant has to
# be given again. That is what "permissions reset on my own machine" actually
# is. An UNSIGNED bundle is worse still — it has no designated requirement at
# all — so this script never produces either one. If no real identity is on the
# machine it FAILS and prints the command that lists them.
#
# TWO PROFILES (docs/RELEASE.md is the long version):
#   RELEASE — "Developer ID Application", notarized + stapled by CI.
#   LOCAL   — "Apple Development", NOT notarized, but a stable designated
#             requirement, which is the only property TCC persistence needs.
#
# Nothing here writes a certificate common name, team id or serial into a file
# this repo commits: identities are read from the keychain at sign time and the
# printed verification output has the team-id parenthetical masked.
#
# USAGE
#   scripts/sign-local.sh [<path to .app>]   sign + verify a built bundle
#                                            (default: the release bundle under
#                                            desktop/src-tauri/target)
#   scripts/sign-local.sh --identity         resolve + print the identity class,
#                                            sign nothing (build preflight)
#   scripts/sign-local.sh --tauri-build ...  resolve the identity, hand it to
#                                            `tauri build` as APPLE_SIGNING_IDENTITY,
#                                            then verify what came out
#   scripts/sign-local.sh --which-app        print the bundle the default/--tauri-build
#                                            modes would sign/verify, sign nothing
#
# BUNDLE SELECTION IS BY EXACT NAME. The release bundle is "<productName>.app"
# from tauri.conf.json, and its CFBundleIdentifier must be the production
# identifier. Never "the first *.app in the directory": scripts/test-build.sh
# builds "Yap Test.app" into the same CARGO_TARGET_DIR, and "Yap Test.app"
# sorts before "Yap.app", so a glob + head -1 verified the TEST bundle and
# never looked at the one that goes into the DMG (PR #238 review, BLOCKING 1).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
SRC="$DESKTOP/src-tauri"
ENT="$SRC/Entitlements.plist"
CONF="$SRC/tauri.conf.json"

die() { echo "FATAL: $*" >&2; exit 1; }

# The first top-level "productName"/"identifier" in tauri.conf.json (both are
# top-level keys there and appear exactly once).
conf_value() { sed -n "s/^  \"$1\": *\"\([^\"]*\)\".*/\1/p" "$CONF" | head -1; }
PRODUCT_NAME="$(conf_value productName)"
PROD_IDENTIFIER="$(conf_value identifier)"
[[ -n "$PRODUCT_NAME" && -n "$PROD_IDENTIFIER" ]] || die "could not read productName/identifier from $CONF"

# release_app <target dir>... — the first "<dir>/release/bundle/macos/<productName>.app"
# that exists, checked to carry the production bundle id. Prints nothing if none.
release_app() {
  local dir app id
  for dir in "$@"; do
    [[ -n "$dir" ]] || continue
    app="$dir/release/bundle/macos/$PRODUCT_NAME.app"
    [[ -d "$app" ]] || continue
    id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null || true)"
    [[ "$id" == "$PROD_IDENTIFIER" ]] || die "$app carries bundle id '${id:-<none>}', expected $PROD_IDENTIFIER — refusing to sign/verify it as the release bundle"
    printf '%s\n' "$app"
    return 0
  done
}

# Mask the 10-character Apple team id so no log this repo produces carries it.
redact() {
  sed -E -e 's/\(([A-Z0-9]{10})\)/(TEAMID)/g' \
         -e 's/^TeamIdentifier=.*/TeamIdentifier=(TEAMID)/'
}

# Resolve the signing identity. Order is deliberate: Developer ID first (the
# release identity, also usable locally), then Apple Development. An explicit
# APPLE_SIGNING_IDENTITY wins, and is still verified to exist on the machine so
# a typo fails here instead of producing an unsigned bundle.
# Sets: IDENTITY (full common name, never printed raw), IDENTITY_CLASS, PROFILE.
resolve_identity() {
  local available
  available="$(security find-identity -v -p codesigning 2>/dev/null || true)"

  if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    if [[ "${APPLE_SIGNING_IDENTITY}" == "-" ]]; then
      die "APPLE_SIGNING_IDENTITY is the ad-hoc pseudo-identity. Ad-hoc has no stable designated requirement, so every rebuild drops Microphone/Accessibility/Input-Monitoring. Refusing."
    fi
    grep -qF "${APPLE_SIGNING_IDENTITY}" <<<"$available" || die \
      "APPLE_SIGNING_IDENTITY is set but that identity is not in this keychain. List what is: security find-identity -v -p codesigning"
    IDENTITY="${APPLE_SIGNING_IDENTITY}"
  else
    IDENTITY="$(sed -n 's/.*"\(Developer ID Application: [^"]*\)".*/\1/p' <<<"$available" | head -1)"
    if [[ -z "$IDENTITY" ]]; then
      IDENTITY="$(sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' <<<"$available" | head -1)"
    fi
  fi

  if [[ -z "$IDENTITY" ]]; then
    echo "FATAL: no code-signing identity found, and this script never falls back to ad-hoc or unsigned." >&2
    echo "       An ad-hoc signature invalidates every TCC grant on every rebuild; unsigned has no" >&2
    echo "       designated requirement at all. Both look exactly like 'the permission broke again'." >&2
    echo "       List the identities on this machine with:" >&2
    echo "           security find-identity -v -p codesigning" >&2
    echo "       Then install a 'Developer ID Application' (release) or 'Apple Development' (local)" >&2
    echo "       certificate, or export APPLE_SIGNING_IDENTITY to one that is already there." >&2
    exit 1
  fi

  case "$IDENTITY" in
    "Developer ID Application:"*) IDENTITY_CLASS="Developer ID Application"; PROFILE="RELEASE" ;;
    "Apple Development:"*)        IDENTITY_CLASS="Apple Development";        PROFILE="LOCAL"   ;;
    *)                            IDENTITY_CLASS="${IDENTITY%%:*}";          PROFILE="LOCAL"   ;;
  esac
}

# codesign -dv --verbose=4 + the effective entitlements, turned into assertions.
# A signature that reports "Signature=adhoc", or carries no Authority chain, is
# the exact failure this item exists to delete, so it is fatal here.
verify_bundle() {
  local app="$1" info ents
  info="$(codesign -dv --verbose=4 "$app" 2>&1)"
  ents="$(codesign -d --entitlements :- --xml "$app" 2>/dev/null || codesign -d --entitlements :- "$app" 2>&1)"

  echo "--- codesign -dv --verbose=4 ---"
  printf '%s\n' "$info" | grep -E '^(Identifier|Format|CodeDirectory|Signature|Authority|TeamIdentifier|Timestamp|Runtime)' | redact
  echo "--- codesign -d --entitlements :- ---"
  printf '%s\n' "$ents" | grep -A1 -E 'app-sandbox|audio-input' | redact || true

  if grep -q 'Signature=adhoc' <<<"$info"; then
    die "bundle is AD-HOC signed — TCC grants will not survive a rebuild."
  fi
  grep -q '^Authority=' <<<"$info" || die "signature carries no Authority chain (unsigned or broken)."
  grep -q 'com.apple.security.app-sandbox' <<<"$ents" || die "app-sandbox entitlement missing from the signed bundle."
  # The sandbox silently kills the CGEvent tap, the synthesized Cmd-V and
  # AXIsProcessTrusted. It must be present AND false — the VALUE, not the key.
  local flat
  flat="$(printf '%s' "$ents" | tr -d ' \t\n\r')"
  if [[ "$flat" == *"<key>com.apple.security.app-sandbox</key><true/>"* ]]; then
    die "app-sandbox is TRUE in the signed bundle — that breaks the hotkey tap and paste."
  fi
  echo "verify: OK — ${PROFILE} profile, ${IDENTITY_CLASS}, not adhoc, sandbox off"
}

MODE="sign"
case "${1:-}" in
  --identity)    MODE="identity"; shift ;;
  --tauri-build) MODE="build";    shift ;;
  --which-app)   MODE="which";    shift ;;
esac

if [[ "$MODE" == "which" ]]; then
  APP="$(release_app "${CARGO_TARGET_DIR:-}" "$SRC/target")"
  [[ -n "$APP" ]] || die "no $PRODUCT_NAME.app under ${CARGO_TARGET_DIR:-$SRC/target}/release/bundle/macos"
  printf '%s\n' "$APP"
  exit 0
fi

resolve_identity

if [[ "$MODE" == "identity" ]]; then
  echo "${PROFILE} profile — signing with: ${IDENTITY_CLASS}"
  exit 0
fi

if [[ "$MODE" == "build" ]]; then
  export APPLE_SIGNING_IDENTITY="$IDENTITY"
  echo "${PROFILE} profile — tauri build will sign with: ${IDENTITY_CLASS}"
  cd "$DESKTOP"
  npx tauri build "$@"
  # CARGO_TARGET_DIR first: that is where the workspace build writes, and a
  # stale bundle under src-tauri/target must not stand in for the fresh one.
  APP="$(release_app "${CARGO_TARGET_DIR:-}" "$SRC/target")"
  [[ -n "$APP" ]] || die "tauri build produced no $PRODUCT_NAME.app to verify."
  verify_bundle "$APP"
  exit 0
fi

APP="${1:-}"
if [[ -z "$APP" ]]; then
  APP="$(release_app "${CARGO_TARGET_DIR:-}" "$SRC/target")"
fi
[[ -n "$APP" && -d "$APP" ]] || die "no .app bundle to sign (pass one explicitly: scripts/sign-local.sh '/path/to/Yap.app')"
[[ -f "$ENT" ]] || die "entitlements not found at $ENT"

echo "${PROFILE} profile — signing $(basename "$APP") with: ${IDENTITY_CLASS}"
# Resource-fork / quarantine detritus makes `codesign --deep` flake with
# "resource fork, Finder information, or similar detritus not allowed".
xattr -cr "$APP"
codesign --force --deep --entitlements "$ENT" --sign "$IDENTITY" "$APP"
verify_bundle "$APP"
