#!/usr/bin/env bash
# Rebuild and launch Wilson Voice.app on the pure arm64 venv.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENV="$ROOT/.venv"
PY="$VENV/bin/python"
APP="$HOME/Applications/Wilson Voice.app"

if [[ ! -x "$PY" ]]; then
  echo "FATAL: missing venv at $PY — run scripts/bootstrap_venv.sh first"
  exit 1
fi

# Stop prior instance (match only venv python module runs)
while read -r pid; do
  [[ -n "$pid" ]] || continue
  kill "$pid" 2>/dev/null || true
done < <(pgrep -f "${VENV}/bin/python -m" || true)
sleep 1

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Wilson Voice</string>
  <key>CFBundleDisplayName</key>
  <string>Wilson Voice</string>
  <key>CFBundleIdentifier</key>
  <string>com.wilsonguenther.wilson-voice</string>
  <key>CFBundleVersion</key>
  <string>0.2.0</string>
  <key>CFBundleShortVersionString</key>
  <string>0.2.0</string>
  <key>CFBundleExecutable</key>
  <string>wilson-voice</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>LSMinimumSystemVersion</key>
  <string>14.0</string>
  <key>LSUIElement</key>
  <false/>
  <key>LSArchitecturePriority</key>
  <array>
    <string>arm64</string>
  </array>
  <key>LSRequiresNativeExecution</key>
  <true/>
  <key>NSMicrophoneUsageDescription</key>
  <string>Wilson Voice needs the microphone for on-device dictation. Audio never leaves your Mac.</string>
  <key>NSAppleEventsUsageDescription</key>
  <string>Wilson Voice pastes transcribed text into the frontmost app.</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

cat > "$APP/Contents/MacOS/wilson-voice" <<EOF
#!/bin/bash
export PATH="/opt/homebrew/bin:/usr/bin:/bin:\$PATH"
ROOT="$ROOT"
PY="\$ROOT/.venv/bin/python"
export PYTHONPATH="\$ROOT"
cd "\$ROOT" || exit 1
mkdir -p "\$HOME/Library/Logs/WilsonVoice"
LOG_OUT="\$HOME/Library/Logs/WilsonVoice/app.stdout.log"
LOG_ERR="\$HOME/Library/Logs/WilsonVoice/app.stderr.log"
echo "\$(date -u +%Y-%m-%dT%H:%M:%SZ) uname=\$(uname -m) arch=\$(/usr/bin/arch) py=\$PY" >> "\$LOG_OUT"
if [[ ! -x "\$PY" ]]; then
  echo "FATAL: venv python missing: \$PY" >> "\$LOG_ERR"
  exit 1
fi
# Always force Apple Silicon
exec /usr/bin/arch -arm64 "\$PY" -m wilson_voice >>"\$LOG_OUT" 2>>"\$LOG_ERR"
EOF
chmod +x "$APP/Contents/MacOS/wilson-voice"

rm -rf "$HOME/Desktop/Wilson Voice.app"
cp -R "$APP" "$HOME/Desktop/Wilson Voice.app"

: > "$HOME/Library/Logs/WilsonVoice/app.stderr.log"
: > "$HOME/Library/Logs/WilsonVoice/app.stdout.log"

open "$APP"
sleep 4

echo "=== process ==="
pgrep -lf "${VENV}/bin/python" || pgrep -lf "wilson_voice" || echo "(checking ps)"
ps -ax -o pid=,args= | head -1
ps -ax -o pid=,args= | grep -F "$VENV/bin/python" | grep -v grep || true
echo "=== stderr ==="
cat "$HOME/Library/Logs/WilsonVoice/app.stderr.log" || true
echo "=== stdout ==="
cat "$HOME/Library/Logs/WilsonVoice/app.stdout.log" || true
echo "APP=$APP"
