#!/usr/bin/env bash
# SEC-A / TC-1 — sign-local.sh picks the RELEASE bundle by exact name and
# bundle id, never "the first *.app" (PR #238 review, BLOCKING 1). Offline: it
# builds fake bundles in a temp target dir and only uses --which-app, which
# resolves no identity and signs nothing.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
S="$ROOT/scripts/sign-local.sh"
fail() { echo "FAIL: $*" >&2; exit 1; }
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
M="$T/release/bundle/macos"
fake() { # fake <name> <bundle id>
  mkdir -p "$M/$1/Contents"
  plutil -create xml1 "$M/$1/Contents/Info.plist"
  plutil -insert CFBundleIdentifier -string "$2" "$M/$1/Contents/Info.plist"
}

# 1. The test bundle alone is never the release bundle.
fake "Yap Test.app" com.wilsonguenther.wilson-voice.test
CARGO_TARGET_DIR="$T" bash "$S" --which-app >/dev/null 2>&1 && fail "picked a bundle when only 'Yap Test.app' exists"

# 2. Both present: "Yap Test.app" sorts first in every locale, Yap.app must still win.
fake "Yap.app" com.wilsonguenther.wilson-voice
[[ "$(ls -d "$M/"*.app | head -1)" == "$M/Yap Test.app" ]] || fail "precondition: the glob no longer sorts the test bundle first"
got="$(CARGO_TARGET_DIR="$T" bash "$S" --which-app)" || fail "--which-app exited non-zero with Yap.app present"
[[ "$got" == "$M/Yap.app" ]] || fail "selected '$got', expected '$M/Yap.app'"

# 3. A Yap.app that carries the wrong bundle id is refused, not verified.
plutil -replace CFBundleIdentifier -string com.wilsonguenther.wilson-voice.test "$M/Yap.app/Contents/Info.plist"
CARGO_TARGET_DIR="$T" bash "$S" --which-app >/dev/null 2>&1 && fail "accepted a Yap.app carrying the test bundle id"
echo "sign-local.test.sh: OK"
