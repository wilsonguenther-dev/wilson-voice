#!/usr/bin/env bash
# FB-03 — the pill shows over every app. Headless check (no screenshots, no
# Screen Recording): order each app of a FIXED list frontmost, then ask the
# window server from outside (scripts/pill-onscreen.swift, CGWindowList) whether
# the pill panel is on screen and uncovered.
#
#   scripts/pill-over-apps.sh --pid <pid of a running Yap TEST build>
#
# Integrator-only: it drives real apps on the Mac it runs on. It REFUSES
# Wilson's /Applications/Yap.app (use scripts/test-build.sh -> Yap Test.app) and
# only quits the apps it launched itself; apps that were already running are
# left running, and every full screen / slideshow it starts it ends.
# osascript needs Accessibility for the full-screen keystroke (System Events).
#
# Fixed list (FB-03 acceptance): Chrome, Safari, Terminal, VS Code, Slack, Zoom
# windowed + full screen, Keynote presenting, a full-screen Space. An app that
# is not installed is reported "skipped" (not a pass). Exit code = failures.
set -u

PID=""
while [ $# -gt 0 ]; do
  case "$1" in
    --pid) PID="${2:-}"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done
if [ -z "$PID" ]; then
  echo "usage: $0 --pid <yap-test-pid>" >&2
  exit 2
fi
CMD="$(ps -p "$PID" -o command= 2>/dev/null)"
if [ -z "$CMD" ]; then
  echo "pid $PID is not running" >&2
  exit 2
fi
case "$CMD" in
  */Applications/Yap.app/*)
    echo "refusing: pid $PID is Wilson's /Applications/Yap.app; run a Yap Test build" >&2
    exit 2 ;;
esac

HERE="$(cd "$(dirname "$0")" && pwd)"
CHECK="$HERE/pill-onscreen.swift"
SETTLE="${PILL_OVER_APPS_SETTLE:-3}"   # > one 1.5 s keeper tick
FAILS=0
LAUNCHED=()

installed() { [ -n "$(mdfind "kMDItemCFBundleIdentifier == '$1'" 2>/dev/null | head -1)" ]; }
running() { osascript -e "application id \"$1\" is running" 2>/dev/null | grep -q true; }
activate() {
  if ! running "$1"; then LAUNCHED+=("$1"); fi
  osascript -e "tell application id \"$1\" to activate" >/dev/null 2>&1
  sleep "$SETTLE"
}
fullscreen_toggle() {
  osascript -e 'tell application "System Events" to keystroke "f" using {control down, command down}' >/dev/null 2>&1
  sleep "$SETTLE"
}

check() { # <label> <bundle id>
  local label="$1" id="$2" out rc
  out="$(swift "$CHECK" "$PID")"
  rc=$?
  case "$out" in
    *"\"front\":\"$id\""*) ;;
    *) echo "FAIL  $label: $id did not come frontmost: $out"; FAILS=$((FAILS + 1)); return ;;
  esac
  if [ "$rc" -eq 0 ]; then
    echo "PASS  $label: $out"
  else
    echo "FAIL  $label: $out"
    FAILS=$((FAILS + 1))
  fi
}

windowed() { # <label> <bundle id>
  if ! installed "$2"; then echo "SKIP  $1: not installed"; return; fi
  activate "$2"
  check "$1" "$2"
}

full_screen() { # <label> <bundle id>
  if ! installed "$2"; then echo "SKIP  $1: not installed"; return; fi
  activate "$2"
  fullscreen_toggle
  check "$1" "$2"
  fullscreen_toggle
}

windowed "Chrome" com.google.Chrome
windowed "Safari" com.apple.Safari
windowed "Terminal" com.apple.Terminal
windowed "VS Code" com.microsoft.VSCode
windowed "Slack" com.tinyspeck.slackmacgap
windowed "Zoom (windowed)" us.zoom.xos
full_screen "Zoom (full screen)" us.zoom.xos
full_screen "Full-screen Space (Terminal)" com.apple.Terminal

KEYNOTE=com.apple.iWork.Keynote
if installed "$KEYNOTE"; then
  activate "$KEYNOTE"
  osascript \
    -e "tell application id \"$KEYNOTE\"" \
    -e "set d to make new document" \
    -e "start d from first slide of d" \
    -e "end tell" >/dev/null 2>&1
  sleep "$SETTLE"
  check "Keynote presenting" "$KEYNOTE"
  osascript \
    -e "tell application id \"$KEYNOTE\"" \
    -e "stop front document" \
    -e "close front document saving no" \
    -e "end tell" >/dev/null 2>&1
else
  echo "SKIP  Keynote presenting: not installed"
fi

# Quit only what this run opened, newest first.
for (( i=${#LAUNCHED[@]}-1; i>=0; i-- )); do
  osascript -e "tell application id \"${LAUNCHED[$i]}\" to quit" >/dev/null 2>&1
done

echo "pill-over-apps: $FAILS failure(s)"
exit "$FAILS"
