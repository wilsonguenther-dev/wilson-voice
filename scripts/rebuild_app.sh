#!/usr/bin/env bash
# Build ONE real Tauri app → /Applications. Prefer Apple Development so Mic TCC sticks.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
SRC="$DESKTOP/src-tauri"
APP_SRC="$SRC/target/release/bundle/macos/Wilson Voice.app"
APP_DST="/Applications/Wilson Voice.app"
ENT="$SRC/Entitlements.plist"
IDENTITY="${WILSON_VOICE_SIGN_IDENTITY:-Apple Development: Wilson Guenther (U8BP8Z86T2)}"
if ! security find-identity -v -p codesigning 2>/dev/null | grep -q "$IDENTITY"; then
  IDENTITY="-"
fi
echo "codesign identity: $IDENTITY"

killall wilson-voice 2>/dev/null || true
pkill -f "asr_worker.py --serve" 2>/dev/null || true
sleep 1
rm -rf "$HOME/Desktop/Wilson Voice.app" "$HOME/Applications/Wilson Voice.app" "$APP_DST"

cd "$DESKTOP"
npm run build
cd "$SRC"
# Serial build avoids flaky objc2 rlib race under parallel cargo
export CARGO_BUILD_JOBS=1
cargo build --release --lib
cargo build --release --bin wilson-voice

# Ensure bundle shell exists (from prior tauri build) or scaffold
if [[ ! -d "$APP_SRC/Contents/MacOS" ]]; then
  mkdir -p "$APP_SRC/Contents/MacOS" "$APP_SRC/Contents/Resources"
  cp -f "$SRC/Info.plist" "$APP_SRC/Contents/Info.plist" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleIdentifier string com.wilsonguenther.wilson-voice" "$APP_SRC/Contents/Info.plist" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleName string Wilson Voice" "$APP_SRC/Contents/Info.plist" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :CFBundleExecutable string wilson-voice" "$APP_SRC/Contents/Info.plist" 2>/dev/null || true
  /usr/libexec/PlistBuddy -c "Add :CFBundlePackageType string APPL" "$APP_SRC/Contents/Info.plist" 2>/dev/null || true
fi
cp -f "$SRC/target/release/wilson-voice" "$APP_SRC/Contents/MacOS/wilson-voice"
cp -f "$SRC/icons/icon.icns" "$APP_SRC/Contents/Resources/icon.icns" 2>/dev/null || true

cp -R "$APP_SRC" "$APP_DST"
xattr -cr "$APP_DST" 2>/dev/null || true
if [[ "$IDENTITY" == "-" ]]; then
  echo "WARN: ad-hoc — Mic will re-prompt after rebuilds"
  codesign --force --deep -s - "$APP_DST" || true
else
  codesign --force --deep --options runtime --entitlements "$ENT" -s "$IDENTITY" "$APP_DST"
fi
codesign -dv "$APP_DST" 2>&1 | head -12
open -a "Wilson Voice"
echo "DONE → $APP_DST ($IDENTITY)"
