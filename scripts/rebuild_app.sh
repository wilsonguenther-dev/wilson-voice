#!/usr/bin/env bash
# Phase 0 hygiene: build ONE real Tauri app → /Applications only. Never DMG, never Desktop.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
APP_SRC="$DESKTOP/src-tauri/target/release/bundle/macos/Wilson Voice.app"
APP_DST="/Applications/Wilson Voice.app"

killall wilson-voice 2>/dev/null || true
ps -axo pid=,command= | awk '/[P]ython -m wilson_voice/{print $1}' | while read -r pid; do
  kill "$pid" 2>/dev/null || true
done
sleep 1

# Remove every fake / duplicate install path
rm -rf "$HOME/Desktop/Wilson Voice.app"
rm -rf "$HOME/Applications/Wilson Voice.app"
rm -rf "$APP_DST"
# Never leave mounted product DMGs around
rm -f "$DESKTOP/src-tauri/target/release/bundle/dmg/"*.dmg 2>/dev/null || true

cd "$DESKTOP"
npm run tauri build

if [[ ! -d "$APP_SRC" ]]; then
  echo "FATAL: missing $APP_SRC"
  exit 1
fi

cp -R "$APP_SRC" "$APP_DST"
xattr -cr "$APP_DST" 2>/dev/null || true
codesign --force --deep -s - "$APP_DST" 2>/dev/null || true

echo "Installed: $APP_DST"
file "$APP_DST/Contents/MacOS/wilson-voice"
/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP_DST/Contents/Info.plist"
/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$APP_DST/Contents/Info.plist"
open -a "Wilson Voice"
echo "DONE — only /Applications/Wilson Voice.app"
