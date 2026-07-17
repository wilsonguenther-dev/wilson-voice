#!/usr/bin/env bash
# Build ONE production Wilson Voice.app → /Applications.
# Must embed frontend (custom-protocol). Never leave cfg(dev)/localhost:1420.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
SRC="$DESKTOP/src-tauri"
APP_SRC="$SRC/target/release/bundle/macos/Wilson Voice.app"
APP_DST="/Applications/Wilson Voice.app"
ENT="$SRC/Entitlements.plist"
IDENTITY="${WILSON_VOICE_SIGN_IDENTITY:-Apple Development: Wilson Guenther (U8BP8Z86T2)}"
if ! security find-identity -v -p codesigning 2>/dev/null | grep -qF "Wilson Guenther"; then
  IDENTITY="-"
fi
echo "codesign identity: $IDENTITY"

killall wilson-voice 2>/dev/null || true
pkill -f "asr_worker.py --serve" 2>/dev/null || true
sleep 1
rm -rf "$HOME/Desktop/Wilson Voice.app" "$HOME/Applications/Wilson Voice.app" "$APP_DST"

cd "$DESKTOP"
rm -rf dist
npm run build

cd "$SRC"
export CARGO_BUILD_JOBS=1
# Force re-embed
touch build.rs
cargo build --release --features custom-protocol

BIN="$SRC/target/release/wilson-voice"
test -x "$BIN"

# Sanity: production embed, not dev server
if strings "$BIN" | grep -q "main-.*\.js" || strings "$BIN" | grep -q "from hold time" || strings "$BIN" | grep -q "Welcome"; then
  echo "OK: frontend strings present in binary"
else
  # still allow if compressed — but fail hard if cfg(dev) build output
  if grep -q 'cargo:rustc-cfg=dev' target/release/build/wilson-voice-*/output 2>/dev/null; then
    echo "FATAL: binary still built with cfg(dev) — custom-protocol missing"
    exit 1
  fi
fi
if ! strings "$BIN" | grep -q "index.html"; then
  echo "WARN: index.html string missing — UI may be blank"
fi

# Bundle shell
mkdir -p "$APP_SRC/Contents/MacOS" "$APP_SRC/Contents/Resources"
cp -f "$BIN" "$APP_SRC/Contents/MacOS/wilson-voice"
if [[ ! -f "$APP_SRC/Contents/Info.plist" ]]; then
  cat > "$APP_SRC/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>English</string>
  <key>CFBundleDisplayName</key><string>Wilson Voice</string>
  <key>CFBundleExecutable</key><string>wilson-voice</string>
  <key>CFBundleIdentifier</key><string>com.wilsonguenther.wilson-voice</string>
  <key>CFBundleName</key><string>Wilson Voice</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.4.1</string>
  <key>CFBundleVersion</key><string>0.4.1</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>Wilson Voice records your voice locally to transcribe dictation with Whisper. Audio never leaves your Mac.</string>
  <key>NSAppleEventsUsageDescription</key>
  <string>Wilson Voice pastes transcribed text into the app you're typing in.</string>
</dict>
</plist>
PLIST
fi
cp -f "$SRC/icons/icon.icns" "$APP_SRC/Contents/Resources/icon.icns" 2>/dev/null || true

cp -R "$APP_SRC" "$APP_DST"
xattr -cr "$APP_DST" 2>/dev/null || true

if [[ "$IDENTITY" == "-" ]]; then
  echo "WARN: ad-hoc sign — Mic TCC may re-prompt after rebuilds"
  codesign --force --deep -s - "$APP_DST" || true
else
  # No --options runtime: Apple Development + WebView is more reliable locally
  codesign --force --deep --entitlements "$ENT" -s "$IDENTITY" "$APP_DST"
fi

codesign -dv "$APP_DST" 2>&1 | head -14
open -a "Wilson Voice"
echo "DONE → $APP_DST"
echo "If UI is blank, this script failed the custom-protocol check — do not ship."
