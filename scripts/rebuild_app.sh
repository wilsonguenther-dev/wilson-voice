#!/usr/bin/env bash
# Build the real Tauri Wilson Voice.app and install ONLY to /Applications.
# Does NOT drop shell wrappers on Desktop or ~/Applications.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
APP_SRC="$DESKTOP/src-tauri/target/release/bundle/macos/Wilson Voice.app"
APP_DST="/Applications/Wilson Voice.app"

# Stop any running instance (binary name only — no self-matching -f)
killall wilson-voice 2>/dev/null || true
# Stop legacy Python menubar if still around
ps -axo pid=,command= | awk '/[P]ython -m wilson_voice/{print $1}' | while read -r pid; do
  kill "$pid" 2>/dev/null || true
done
sleep 1

# Remove ALL known fake / duplicate installs before rebuild
rm -rf "$HOME/Desktop/Wilson Voice.app"
rm -rf "$HOME/Applications/Wilson Voice.app"
rm -rf "$APP_DST"

cd "$DESKTOP"
npm run tauri build

if [[ ! -d "$APP_SRC" ]]; then
  echo "FATAL: build did not produce $APP_SRC"
  exit 1
fi

cp -R "$APP_SRC" "$APP_DST"
xattr -cr "$APP_DST" 2>/dev/null || true
codesign --force --deep -s - "$APP_DST" 2>/dev/null || true

echo "Installed: $APP_DST"
file "$APP_DST/Contents/MacOS/wilson-voice"
/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$APP_DST/Contents/Info.plist"
open -a "Wilson Voice"
echo "DONE — only /Applications/Wilson Voice.app (real Tauri binary)"
