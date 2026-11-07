#!/usr/bin/env bash
# SP-10 (yap-1106-sp-10-ane-engine-spike) — build + stage the Neural Engine
# speech helper. `bundle.externalBin` names `binaries/yap-engine-ane`, and
# tauri-build checks for `binaries/yap-engine-ane-<triple>` on EVERY cargo
# build of the app, so this runs wherever the Rust sidecars are staged
# (desktop `npm run sidecar`, ci.yml, release.yml).
#
#   scripts/build-ane-engine.sh            # release build, stage the binary
#   scripts/build-ane-engine.sh --test     # also run the pure-Swift unit tests
#
# Needs Swift 6.0+ (Xcode 16+; FluidAudio's tools version). SwiftPM fetches
# FluidAudio at the exact pinned tag in desktop/yap-engine-ane/Package.swift;
# no model is downloaded here or by the helper. --disable-keychain: SwiftPM
# otherwise looks up github.com credentials in the login keychain, which
# blocks a headless build on an unanswerable keychain prompt.
# Exit: 0 staged; 1 build/test failed; 2 toolchain missing or too old.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/desktop/yap-engine-ane"
DEST="$ROOT/desktop/src-tauri/binaries"
RUN_TESTS=0
[ "${1:-}" = "--test" ] && RUN_TESTS=1

if ! command -v swift >/dev/null 2>&1; then
  echo "build-ane-engine: swift not found (install Xcode 16 or newer)" >&2
  exit 2
fi
ver="$(swift --version 2>/dev/null | sed -n 's/.*Swift version \([0-9][0-9]*\)\..*/\1/p' | head -1)"
if [ -z "$ver" ] || [ "$ver" -lt 6 ]; then
  echo "build-ane-engine: Swift 6.0+ required, found '${ver:-unknown}'" >&2
  exit 2
fi

TRIPLE="$(rustc -vV 2>/dev/null | awk '/^host:/ {print $2}')"
[ -n "$TRIPLE" ] || TRIPLE="aarch64-apple-darwin"

started=$(date +%s)
swift build -c release --disable-keychain --package-path "$PKG" --product yap-engine-ane
rc=$?
if [ "$rc" != 0 ]; then
  echo "build-ane-engine: swift build failed (exit $rc)" >&2
  exit 1
fi
if [ "$RUN_TESTS" = 1 ]; then
  swift test --disable-keychain --package-path "$PKG" --filter YapEngineANECoreTests
  rc=$?
  if [ "$rc" != 0 ]; then
    echo "build-ane-engine: swift test failed (exit $rc)" >&2
    exit 1
  fi
fi
bin_dir="$(swift build -c release --disable-keychain --package-path "$PKG" --show-bin-path)"
mkdir -p "$DEST"
cp "$bin_dir/yap-engine-ane" "$DEST/yap-engine-ane-$TRIPLE" || exit 1
test -x "$DEST/yap-engine-ane-$TRIPLE" || exit 1
echo "build-ane-engine: staged src-tauri/binaries/yap-engine-ane-$TRIPLE in $(( $(date +%s) - started ))s"
