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
  # Ad-hoc signing has no stable designated requirement, so the cdhash changes
  # every build and macOS DROPS Mic/Accessibility/Input-Monitoring grants each
  # rebuild — the exact "permissions don't stick / paste keeps failing" trap.
  # Fail hard unless explicitly overridden, instead of silently shipping it.
  if [[ "${WILSON_VOICE_ALLOW_ADHOC:-0}" == "1" ]]; then
    echo "WARN: no 'Wilson Guenther' codesign identity — ad-hoc signing (TCC grants will NOT persist across rebuilds)."
    IDENTITY="-"
  else
    echo "FATAL: no 'Wilson Guenther' codesign identity found." >&2
    echo "       Ad-hoc signing resets TCC (mic/accessibility) every rebuild." >&2
    echo "       Install the Apple Development cert, or set WILSON_VOICE_ALLOW_ADHOC=1 to force it." >&2
    exit 1
  fi
fi
echo "codesign identity: $IDENTITY"

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
