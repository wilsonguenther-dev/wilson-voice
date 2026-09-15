#!/usr/bin/env bash
#
# The loop's smoke entry point.
#
# Y0-E ships the structural windowed smoke (scripts/smoke-windowed.sh) ahead of
# the UI items it judges, and this is where the loop calls it. Y0-B owns the
# rest of this file; if Y0-B lands a richer version, keep the smoke-windowed
# call and drop the scaffolding around it.
#
# A DISPLAY IS NOT OPTIONAL ON THIS RUN. The loop runs on a Mac with a display
# attached, so "no display" is a FAILURE here, not a named skip — a smoke that
# skips itself is a gate that has never once said no.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

have_display() {
  case "$(uname -s)" in
    Darwin) command -v system_profiler >/dev/null 2>&1 ;;
    *) [ -n "${DISPLAY:-}" ] || [ -n "${WAYLAND_DISPLAY:-}" ] ;;
  esac
}

if ! have_display; then
  echo "loop-smoke: no display — the windowed smoke cannot run headfully here." >&2
  exit 2
fi

echo "loop-smoke: running the structural windowed smoke"
( cd "$ROOT/desktop" && npm run smoke:windowed )
