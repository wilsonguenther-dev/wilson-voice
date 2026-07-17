#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$HOME/Applications/Wilson Voice.app"
if [[ -d "$APP" ]]; then
  open -a "$APP"
  echo "Launched Wilson Voice.app"
  echo "Look for 'Voice' in the menu bar and Dock."
  echo "Logs: ~/Library/Logs/WilsonVoice/"
  exit 0
fi
exec "$ROOT/.venv/bin/python" -m wilson_voice
