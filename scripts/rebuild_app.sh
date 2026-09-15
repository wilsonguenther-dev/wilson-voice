#!/usr/bin/env bash
# Build ONE production Wilson Voice.app → /Applications.
# Must embed frontend (custom-protocol). Never leave cfg(dev)/localhost:1420.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
SRC="$DESKTOP/src-tauri"
APP_SRC="$SRC/target/release/bundle/macos/Wilson Voice.app"
APP_DST="/Applications/Wilson Voice.app"
# SEC-A: signing lives in ONE place now — scripts/sign-local.sh. It resolves a
# real identity (Developer ID, then Apple Development), never falls back to
# ad-hoc, and fails with the `security find-identity` hint when none is present.
# Ad-hoc has no stable designated requirement, so every rebuild used to drop the
# Microphone / Accessibility / Input-Monitoring grants. Run the resolve FIRST so
# a missing certificate fails in a second instead of after a full release build.
SIGN="$ROOT/scripts/sign-local.sh"
"$SIGN" --identity

killall wilson-voice 2>/dev/null || true
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
if strings "$BIN" | grep -qE "main-.*\.js|/index\.html|hold→clipboard|p50 hold"; then
  echo "OK: frontend strings present in binary"
else
  # still allow if compressed — but fail hard if cfg(dev) build output
  if grep -q 'cargo:rustc-cfg=dev' target/release/build/wilson-voice-*/output 2>/dev/null; then
    echo "FATAL: binary still built with cfg(dev) — custom-protocol missing"
    exit 1
  fi
fi
if ! strings "$BIN" | grep -q "index.html"; then
  # Tauri may embed as /index.html path; also accept asset hashes
  if ! strings "$BIN" | grep -qE "/index\.html|main-.*\.js|float\.html"; then
    echo "FATAL: index.html string missing — UI will be blank"
    exit 1
  fi
  echo "OK: frontend assets present (path form)"
fi
if strings "$BIN" | grep -q "http://localhost:1420" && ! strings "$BIN" | grep -q "tauri://localhost"; then
  echo "FATAL: looks like cfg(dev) only — blank white window risk"
  exit 1
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
  <key>CFBundleShortVersionString</key><string>0.5.0</string>
  <key>CFBundleVersion</key><string>0.5.0</string>
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

# xattr -cr, codesign with the resolved identity + Entitlements.plist, then
# verify (Authority chain present, NOT adhoc, app-sandbox false). Fails loudly.
"$SIGN" "$APP_DST"

open -a "Wilson Voice"
echo "DONE → $APP_DST"
echo "If UI is blank, this script failed the custom-protocol check — do not ship."
