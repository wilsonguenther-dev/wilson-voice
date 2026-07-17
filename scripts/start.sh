#!/usr/bin/env bash
# Start Wilson Voice menubar app with the correct Python + PATH.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/Library/Python/3.13/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export PYTHONPATH="$ROOT${PYTHONPATH:+:$PYTHONPATH}"
PY="${WILSON_VOICE_PYTHON:-/Library/Frameworks/Python.framework/Versions/3.13/bin/python3}"
cd "$ROOT"
exec "$PY" -m wilson_voice "$@"
