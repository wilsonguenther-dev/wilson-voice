#!/usr/bin/env bash
# Launch the one real Wilson Voice install.
set -euo pipefail
APP="/Applications/Wilson Voice.app"
if [[ ! -x "$APP/Contents/MacOS/wilson-voice" ]]; then
  echo "Wilson Voice not installed. Run: scripts/rebuild_app.sh"
  exit 1
fi
# Refuse to start if binary is a shell script (legacy wrapper)
if file "$APP/Contents/MacOS/wilson-voice" | grep -qi 'script\|text'; then
  echo "FATAL: $APP is a legacy shell wrapper. Run scripts/rebuild_app.sh"
  exit 1
fi
open -a "Wilson Voice"
echo "Launched /Applications/Wilson Voice.app"
