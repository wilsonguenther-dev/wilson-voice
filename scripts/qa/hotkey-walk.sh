#!/usr/bin/env bash
# scripts/qa/hotkey-walk.sh — a REAL fn take on the INSTALLED Yap (yap-qa-harness).
#
# Posts synthetic fn (Globe) edges — CGEvent flagsChanged with maskSecondaryFn,
# from scripts/qa/fn-post.swift built ad hoc into desktop/target/qa — and
# asserts Yap's own log answers, in order and within 5 s of the previous step:
#
#   pass 1 (hold):        fn down, hold 1.5 s, fn up
#       "PTT hold armed" -> "recording started" -> "PTT hold release"
#       -> "transcript ok" | "no-speech gate" (the take ended)
#   pass 2 (hands-free):  double-tap fn, wait 3 s, tap fn once
#       "hands-free ON" -> "recording started" -> "tap ends hands-free"
#       -> "transcript ok" | "no-speech gate" (the take ended)
#
#   --chord (zero-TCC):   tap the DICTATE_TOGGLE chord ⌃⌥⌘Space, wait 2 s, tap it
#       "hotkey edge Press from Carbon -> Start" -> "recording started"
#       -> "hotkey edge Press from Carbon -> Stop" -> "transcript ok" | "no-speech gate"
#
# It never launches, quits or signals Yap. Yap must already be running.
#
#   hotkey-walk.sh               walk the installed app (needs YAP_QA_DRIVE_RUNNING=1)
#   hotkey-walk --chord          (hotkey-walk.sh --chord) walk the Carbon dictate chord
#   hotkey-walk.sh --table       PASSIVE: this bundle's TCC grants + the hotkey
#                                table Yap logged at launch; posts NO events
#   hotkey-walk.sh --self-test   refusal paths + the log parser on fixtures; posts NO events
#
# Exit: 0 pass, 1 an assertion failed (prints which line never came),
#       2 usage / app or log missing, 3 the CALLER lacks Accessibility,
#       4 Yap is running and YAP_QA_DRIVE_RUNNING is not 1, 5 Yap is not running.
#
# Environment: YAP_QA_ALLOW_KEYS=1 (REQUIRED before any key is posted —
#   never set it while someone is typing), YAP_QA_DRIVE_RUNNING=1 (consent to post fn into a running Yap),
#   YAP_QA_APP (default /Applications/Yap.app), YAP_QA_LOG (default Yap's log).
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
QA_BIN_DIR="$REPO_ROOT/desktop/target/qa"
FIXTURES="$SCRIPT_DIR/fixtures"
APP="${YAP_QA_APP:-/Applications/Yap.app}"
LOG="${YAP_QA_LOG:-$HOME/Library/Application Support/WilsonVoice/logs/yap.log}"
STEP_LIMIT_S=5
END_WAIT_S=15

# One step per line; "a|b" means either fixed string satisfies the step.
HOLD_STEPS='PTT hold armed
recording started
PTT hold release
transcript ok|no-speech gate'
CHORD_STEPS='hotkey edge Press from Carbon -> Start
recording started
hotkey edge Press from Carbon -> Stop
transcript ok|no-speech gate'
# DICTATE_TOGGLE: kVK_Space (49) with ⌃ 0x40000 | ⌥ 0x80000 | ⌘ 0x100000.
CHORD_OP='chord:49:1835008'
HANDSFREE_STEPS='hands-free ON
recording started
tap ends hands-free
transcript ok|no-speech gate'

die() {
  printf 'hotkey-walk: %s\n' "$1" >&2
  exit 2
}

# Build a QA Swift helper into desktop/target/qa (gitignored), signed AD HOC.
build_helper() { # $1 name (scripts/qa/<name>.swift) -> prints binary path
  local src="$SCRIPT_DIR/$1.swift" bin="$QA_BIN_DIR/$1"
  [ -f "$src" ] || return 1
  if [ ! -x "$bin" ] || [ "$src" -nt "$bin" ]; then
    mkdir -p "$QA_BIN_DIR" || return 1
    swiftc -O -o "$bin" "$src" >"$QA_BIN_DIR/$1.build.log" 2>&1 || return 1
    codesign -s - -f "$bin" >>"$QA_BIN_DIR/$1.build.log" 2>&1 || return 1
  fi
  printf '%s\n' "$bin"
}

# THE LOG PARSER — the one code path live walks and --self-test share.
# check_steps LOGFILE START_ISO STEPS
# Walks LOGFILE lines stamped at or after START_ISO ("YYYY-MM-DDTHH:MM:SSZ"),
# finds each step in order, and requires each within STEP_LIMIT_S of the
# previous match (the first within STEP_LIMIT_S of START_ISO). Log stamps are
# whole seconds. Exit 0 all steps found in time, 1 otherwise.
check_steps() {
  WALK_STEPS="$3" awk -v start_iso="$2" -v limit="$STEP_LIMIT_S" '
    function days(y, m, d,   era, yoe, mp, doy, doe) {
      if (m <= 2) y--
      era = int(y / 400); yoe = y - era * 400
      mp = (m + 9) % 12; doy = int((153 * mp + 2) / 5) + d - 1
      doe = yoe * 365 + int(yoe / 4) - int(yoe / 100) + doy
      return era * 146097 + doe - 719468
    }
    function epoch(s) { # s = YYYY-MM-DDTHH:MM:SSZ
      if (substr(s, 5, 1) != "-" || substr(s, 11, 1) != "T" || substr(s, 20, 1) != "Z") return -1
      return days(substr(s, 1, 4) + 0, substr(s, 6, 2) + 0, substr(s, 9, 2) + 0) * 86400 \
        + substr(s, 12, 2) * 3600 + substr(s, 15, 2) * 60 + substr(s, 18, 2)
    }
    function hit(line, step,   alts, n, i) {
      n = split(step, alts, "|")
      for (i = 1; i <= n; i++) if (index(line, alts[i]) > 0) return 1
      return 0
    }
    BEGIN {
      nsteps = split(ENVIRON["WALK_STEPS"], steps, "\n")
      start = epoch(start_iso); prev = start; prev_label = "the walk start"; idx = 1; failed = 0
      if (start < 0) { print "FAIL bad start stamp: " start_iso; failed = 1; exit 1 }
    }
    idx <= nsteps && substr($0, 1, 1) == "[" {
      t = epoch(substr($0, 2, 20))
      if (t < 0 || t < start) next
      if (!hit($0, steps[idx])) next
      if (t - prev > limit) {
        printf "FAIL \"%s\" came %d s after %s (limit %d s)\n", steps[idx], t - prev, prev_label, limit
        failed = 1; exit 1
      }
      printf "ok   \"%s\" +%ds  %s\n", steps[idx], t - prev, $0
      prev = t; prev_label = "\"" steps[idx] "\""; idx++
    }
    END {
      if (failed) exit 1
      if (idx <= nsteps) {
        printf "FAIL \"%s\" never came within %d s of %s\n", steps[idx], limit, prev_label
        exit 1
      }
      exit 0
    }
  ' "$1"
}

caller_name() {
  if [ -n "${TERM_PROGRAM:-}" ]; then
    printf '%s\n' "$TERM_PROGRAM"
  else
    ps -o comm= -p "$PPID" 2>/dev/null || printf 'this terminal\n'
  fi
}

# Pids of the installed Yap's executable — read only, never signalled.
# YAP_QA_FAKE_PIDS (self-test only) stands in for pgrep.
yap_pids() {
  local exe out
  if [ -n "${YAP_QA_FAKE_PIDS+set}" ]; then
    printf '%s\n' "$YAP_QA_FAKE_PIDS"
    return 0
  fi
  exe="$(plutil -extract CFBundleExecutable raw -o - "$APP/Contents/Info.plist" 2>/dev/null)" || return 0
  out="$(pgrep -f "$APP/Contents/MacOS/$exe" 2>/dev/null)" || return 0
  printf '%s\n' "${out//$'\n'/,}"
}

# 0 = the caller may post events, 3 = it may not. YAP_QA_FAKE_POST_ACCESS
# (self-test only) stands in for the helper's CGPreflightPostEventAccess().
post_access() { # $1 fn-post binary (may be empty under the fake)
  if [ -n "${YAP_QA_FAKE_POST_ACCESS:-}" ]; then
    [ "$YAP_QA_FAKE_POST_ACCESS" = "1" ] && return 0
    return 3
  fi
  "$1" check >/dev/null 2>&1
}

log_size() {
  stat -f %z "$LOG" 2>/dev/null || printf '0\n'
}

# Copy what the log gained since byte OFFSET into FILE (whole log if it rotated).
log_since() { # $1 offset, $2 out file
  local size
  size="$(log_size)"
  if [ "$size" -lt "$1" ]; then
    cat "$LOG" >"$2"
  else
    tail -c "+$(($1 + 1))" "$LOG" >"$2"
  fi
}

# Wait (wall clock, bounded) until the take-end line lands, then check the steps.
settle_and_check() { # $1 offset, $2 start iso, $3 steps, $4 segment file
  local waited=0
  while [ "$waited" -lt $((END_WAIT_S * 4)) ]; do
    log_since "$1" "$4"
    if grep -q -e 'transcript ok' -e 'no-speech gate' "$4"; then
      break
    fi
    sleep 0.25
    waited=$((waited + 1))
  done
  log_since "$1" "$4"
  check_steps "$4" "$2" "$3"
}

walk() { # $1 = "" (fn passes) | chord
  local mode="${1:-}" pids fnpost="" rc seg offset start
  [ -f "$APP/Contents/Info.plist" ] || die "no app bundle at $APP"

  pids="$(yap_pids)"
  if [ -z "$pids" ]; then
    printf 'Yap is not running (%s); this walk never launches it — open Yap, then rerun\n' "$APP"
    exit 5
  fi
  # focus-2026-10-19: posting keys needs a SECOND, explicit consent —
  # YAP_QA_ALLOW_KEYS=1 — so a walk can never type into someone's session.
  if [ "${YAP_QA_DRIVE_RUNNING:-}" != "1" ] || [ "${YAP_QA_ALLOW_KEYS:-}" != "1" ]; then
    printf 'Yap is running (pid %s); this walk posts fn into it — rerun with YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 when you are not dictating\n' "$pids"
    exit 4
  fi

  if [ -z "${YAP_QA_FAKE_POST_ACCESS:-}" ]; then
    fnpost="$(build_helper fn-post)" || die "could not build scripts/qa/fn-post.swift (see $QA_BIN_DIR/fn-post.build.log)"
  fi
  post_access "$fnpost"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    printf 'hotkey-walk needs Accessibility for %s — grant it in System Settings -> Privacy & Security -> Accessibility, then rerun\n' "$(caller_name)"
    exit 3
  fi

  if [ "${YAP_QA_NO_POST:-}" = "1" ]; then
    printf 'self-test: refusal gates passed; stopping before any event is posted\n'
    exit 99
  fi
  [ -f "$LOG" ] || die "no Yap log at $LOG"

  seg="$(mktemp -t yapqa)" || die "mktemp failed"
  printf 'walking Yap pid %s, log %s\n' "$pids" "$LOG"

  if [ "$mode" = "chord" ]; then
    printf -- '-- chord pass: tap ⌃⌥⌘Space, wait 2 s, tap ⌃⌥⌘Space\n'
    offset="$(log_size)"
    start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    "$fnpost" "$CHORD_OP" || { rm -f "$seg"; die "fn-post chord failed"; }
    sleep 2
    "$fnpost" "$CHORD_OP" || { rm -f "$seg"; die "fn-post chord failed"; }
    settle_and_check "$offset" "$start" "$CHORD_STEPS" "$seg"
    rc=$?
    rm -f "$seg"
    [ "$rc" -eq 0 ] || exit 1
    printf 'hotkey-walk --chord: pass\n'
    exit 0
  fi

  printf -- '-- pass 1: hold fn 1.5 s\n'
  offset="$(log_size)"
  start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  "$fnpost" down || { rm -f "$seg"; die "fn-post down failed"; }
  sleep 1.5
  "$fnpost" up || { rm -f "$seg"; die "fn-post up failed"; }
  settle_and_check "$offset" "$start" "$HOLD_STEPS" "$seg"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    rm -f "$seg"
    exit 1
  fi

  sleep 1
  printf -- '-- pass 2: double-tap fn (hands-free), wait 3 s, tap fn\n'
  offset="$(log_size)"
  start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  "$fnpost" down sleep:70 up sleep:150 down sleep:70 up || { rm -f "$seg"; die "fn-post double-tap failed"; }
  sleep 3
  "$fnpost" down sleep:70 up || { rm -f "$seg"; die "fn-post tap failed"; }
  settle_and_check "$offset" "$start" "$HANDSFREE_STEPS" "$seg"
  rc=$?
  rm -f "$seg"
  [ "$rc" -eq 0 ] || exit 1
  printf 'hotkey-walk: pass\n'
  exit 0
}

self_test() {
  local out rc fails=0
  out="$(mktemp -t yapqa)" || exit 1

  expect() { # $1 label, $2 want rc, $3 got rc, $4 required text ("" = none)
    if [ "$3" -eq "$2" ] && { [ -z "$4" ] || grep -qF -- "$4" "$out"; }; then
      printf 'ok   %s (exit %d)\n' "$1" "$3"
    else
      printf 'FAIL %s: want exit %d%s, got %d:\n' "$1" "$2" "${4:+ and \"$4\"}" "$3"
      sed 's/^/       /' "$out"
      fails=$((fails + 1))
    fi
  }

  # Refusal paths. Fakes stand in for pgrep and the Accessibility preflight;
  # YAP_QA_NO_POST makes reaching the posting step a hard stop (exit 99).
  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS="" YAP_QA_NO_POST=1 \
    bash "$0" >"$out" 2>&1
  expect "Yap not running -> refuses" 5 $? "this walk never launches it"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING="" \
    YAP_QA_NO_POST=1 bash "$0" >"$out" 2>&1
  expect "Yap running without YAP_QA_DRIVE_RUNNING=1 -> refuses" 4 $? \
    "Yap is running (pid 4242); this walk posts fn into it — rerun with YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 when you are not dictating"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=0 YAP_QA_NO_POST=1 TERM_PROGRAM=QA-Terminal bash "$0" >"$out" 2>&1
  expect "caller without Accessibility -> refuses" 3 $? \
    "hotkey-walk needs Accessibility for QA-Terminal — grant it in System Settings -> Privacy & Security -> Accessibility, then rerun"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" >"$out" 2>&1
  expect "all gates open -> reaches the post step (stopped, nothing posted)" 99 $? "stopping before any event is posted"

  YAP_QA_APP=/nonexistent/Yap.app bash "$0" >"$out" 2>&1
  expect "missing app -> usage error" 2 $? "no app bundle"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING="" \
    YAP_QA_NO_POST=1 bash "$0" --chord >"$out" 2>&1
  expect "--chord: Yap running without YAP_QA_DRIVE_RUNNING=1 -> refuses" 4 $? "this walk posts fn into it"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --chord >"$out" 2>&1
  expect "--chord: all gates open -> reaches the post step (stopped, nothing posted)" 99 $? "stopping before any event is posted"

  # The log parser, against fixtures.
  check_steps "$FIXTURES/yap-walk-pass.log" 2026-10-16T18:08:54Z "$HOLD_STEPS" >"$out" 2>&1
  expect "pass fixture: hold take" 0 $? 'ok   "transcript ok|no-speech gate"'
  check_steps "$FIXTURES/yap-walk-pass.log" 2026-10-16T18:08:59Z "$HANDSFREE_STEPS" >"$out" 2>&1
  expect "pass fixture: hands-free take" 0 $? 'ok   "tap ends hands-free"'
  check_steps "$FIXTURES/yap-walk-stuck.log" 2026-10-16T16:13:51Z "$HANDSFREE_STEPS" >"$out" 2>&1
  expect "stuck fixture: tap never ends hands-free" 1 $? 'FAIL "tap ends hands-free" never came within 5 s of "recording started"'
  check_steps "$FIXTURES/yap-walk-stuck.log" 2026-10-16T16:13:51Z "$HOLD_STEPS" >"$out" 2>&1
  expect "stuck fixture: no hold was armed" 1 $? 'FAIL "PTT hold armed" never came within 5 s of the walk start'
  check_steps "$FIXTURES/yap-walk-pass.log" 2026-10-16T18:08:40Z "$HOLD_STEPS" >"$out" 2>&1
  expect "pass fixture from an early start: first step late" 1 $? 'FAIL "PTT hold armed" came 15 s after the walk start (limit 5 s)'
  check_steps "$FIXTURES/yap-walk-pass.log" 2026-10-16T18:09:05Z "$HOLD_STEPS" >"$out" 2>&1
  expect "lines before the start stamp are ignored" 1 $? 'FAIL "PTT hold armed" never came'
  check_steps "$FIXTURES/yap-walk-chord-pass.log" 2026-10-17T09:12:30Z "$CHORD_STEPS" >"$out" 2>&1
  expect "chord fixture: zero-TCC chord take" 0 $? 'ok   "hotkey edge Press from Carbon -> Stop"'
  check_steps "$FIXTURES/yap-walk-pass.log" 2026-10-16T18:08:54Z "$CHORD_STEPS" >"$out" 2>&1
  expect "fn-only fixture: no chord edge" 1 $? 'FAIL "hotkey edge Press from Carbon -> Start" never came'

  rm -f "$out"
  if [ "$fails" -ne 0 ]; then
    printf 'self-test: %d failure(s)\n' "$fails"
    exit 1
  fi
  printf 'self-test: pass (no events posted)\n'
  exit 0
}

table() {
  bundle="${YAP_QA_BUNDLE_ID:-com.wilsonguenther.wilson-voice}"
  printf '== TCC grants for %s\n' "$bundle"
  sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
    "select service, auth_value, datetime(last_modified,'unixepoch') from access where client = '$bundle'" 2>/dev/null \
    || echo "(TCC.db not readable from this terminal — needs Full Disk Access)"
  [ -f "$LOG" ] || die "no log at $LOG"
  printf '== hotkey table at the last launch (%s)\n' "$LOG"
  awk '/hotkey table at launch:/{buf=""; grab=7} grab>0{buf=buf $0 "\n"; grab--} END{printf "%s", buf}' "$LOG"
  printf '== last 15 hotkey lines\n'
  grep -E "hotkey edge|FN PTT|dictate-toggle|meeting-toggle|re-arm" "$LOG" | tail -15
  exit 0
}

case "${1:-}" in
  --table) table ;;
  --self-test) self_test ;;
  -h | --help) sed -n '2,32p' "$0"; exit 0 ;;
  --chord) walk chord ;;
  "") walk ;;
  *) die "unknown argument: $1" ;;
esac
