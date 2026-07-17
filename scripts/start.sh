#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/Library/Python/3.13/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export PYTHONPATH="$ROOT${PYTHONPATH:+:$PYTHONPATH}"
PY="/Library/Frameworks/Python.framework/Versions/3.13/bin/python3"
APP="$HOME/Applications/Wilson Voice.app"
if [[ -d "$APP" ]]; then
  open "$APP"
  echo "Opened $APP — look for 🎙 in the menu bar (top-right)."
  echo "Logs: ~/Library/Logs/WilsonVoice/"
  exit 0
fi
cd "$ROOT"
exec /usr/bin/arch -arm64 "$PY" -m wilson_voice
