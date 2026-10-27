#!/usr/bin/env bash
# scripts/qa/hotkey-walk.sh — a REAL fn take on the INSTALLED Yap (yap-qa-harness).
#
# Presses the CONFIGURED push-to-talk chord — pttBinding from Yap's own
# settings.json (fn -> fn; fn_control, the default -> ⌃ down then fn down,
# released in reverse; both -> fn) — as synthetic CGEvent flagsChanged edges
# from scripts/qa/fn-post.swift (built ad hoc into desktop/target/qa), and
# asserts Yap's own log answers, in order and within 5 s of the previous step.
# An unknown pttBinding is refused by name (exit 2) and nothing is posted.
# "the chord" below is that binding's keys:
#
#   pass 1 (hold):        chord down, hold 1.5 s, chord up
#       "PTT hold armed" -> "recording started" -> "PTT hold release"
#       -> "transcript ok" | "no-speech gate" (the take ended)
#   pass 2 (hands-free):  double-tap the chord, wait 3 s, tap it once
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
#   hotkey-walk.sh --dev-build[=BIN]  walk an UNBUNDLED dev binary, matched by its
#                                target path (default: $CARGO_TARGET_DIR or
#                                desktop/target, + /debug/wilson-voice)
#   hotkey-walk.sh --settings P  read pttBinding from P instead of Yap's settings.json
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
#   YAP_QA_APP (default /Applications/Yap.app), YAP_QA_LOG (default Yap's log),
#   YAP_QA_BUNDLE_ID / YAP_QA_TCC_DB (--table; the database is opened with sqlite3 -readonly).
#   YAP_DATA_DIR (the app's own data-dir override; settings.json is read there).
#   YAP_QA_FAKE_PIDS / YAP_QA_FAKE_POST_ACCESS / YAP_QA_NO_POST are --self-test
#   fakes only; a live walk of a dev binary uses --dev-build, never fake pids.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
QA_BIN_DIR="$REPO_ROOT/desktop/target/qa"
FIXTURES="$SCRIPT_DIR/fixtures"
APP="${YAP_QA_APP:-/Applications/Yap.app}"
LOG="${YAP_QA_LOG:-$HOME/Library/Application Support/WilsonVoice/logs/yap.log}"
# The same data dir the app resolves (app_paths.rs: $YAP_DATA_DIR, else
# <Application Support>/WilsonVoice); --settings overrides it.
DATA_ROOT="${YAP_DATA_DIR:-$HOME/Library/Application Support/WilsonVoice}"
SETTINGS="$DATA_ROOT/settings.json"
DEV_BIN=""
# Filled by resolve_binding: the fn-post ops that press / release the chord.
BINDING=""
CHORD=""
PRESS=()
RELEASE=()
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

# Read pttBinding from $1 (a settings.json) and set BINDING, CHORD, PRESS and
# RELEASE. Prints "binding=<value> chord=<keys>". A missing file or a missing
# key is the app's own default (fn_control, lib.rs default_ptt_binding); an
# unreadable file or an unknown value is refused by name — exit 2, nothing posted.
resolve_binding() { # $1 settings path
  local file="$1"
  if [ ! -f "$file" ]; then
    printf 'hotkey-walk: no settings at %s — Yap runs on its default binding
' "$file"
    BINDING="fn_control"
  elif ! plutil -convert json -o /dev/null -- "$file" >/dev/null 2>&1; then
    die "settings at $file are not readable JSON — refusing to guess the binding; posting nothing"
  elif ! BINDING="$(plutil -extract pttBinding raw -o - -- "$file" 2>/dev/null)"; then
    BINDING="fn_control"
  fi
  case "$BINDING" in
    fn | globe | both | fn_or_fn_control)
      CHORD="fn"; PRESS=(down); RELEASE=(up) ;;
    fn_control)
      CHORD="ctrl+fn"; PRESS=(ctrl-down down); RELEASE=(up ctrl-up) ;;
    *)
      printf 'binding=%s chord=none
' "$BINDING"
      die "unknown binding \"$BINDING\" in $file — expected fn, fn_control or both; posting nothing"
      ;;
  esac
  printf 'binding=%s chord=%s
' "$BINDING" "$CHORD"
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

# Pids of the installed Yap's executable (or, with --dev-build, of the dev
# binary at its target path) — read only, never signalled.
# YAP_QA_FAKE_PIDS (self-test only) stands in for pgrep.
yap_pids() {
  local exe out
  if [ -n "${YAP_QA_FAKE_PIDS+set}" ]; then
    printf '%s\n' "$YAP_QA_FAKE_PIDS"
    return 0
  fi
  if [ -n "$DEV_BIN" ]; then
    out="$(pgrep -f "^${DEV_BIN}( |\$)" 2>/dev/null)" || return 0
    printf '%s\n' "${out//$'\n'/,}"
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
  local mode="${1:-}" pids fnpost="" rc seg offset start target
  if [ -n "${YAP_QA_FAKE_PIDS+set}" ] && [ "${YAP_QA_SELF_TEST:-}" != "1" ]; then
    die "YAP_QA_FAKE_PIDS is a --self-test fake — walk a dev binary with --dev-build instead"
  fi
  if [ -n "$DEV_BIN" ]; then
    [ -x "$DEV_BIN" ] || die "no dev binary at $DEV_BIN — build it (npm run desktop:dev) or pass --dev-build=<path>"
    target="$DEV_BIN"
  else
    [ -f "$APP/Contents/Info.plist" ] || die "no app bundle at $APP"
    target="$APP"
  fi

  pids="$(yap_pids)"
  if [ -z "$pids" ]; then
    printf 'Yap is not running (%s); this walk never launches it — open Yap, then rerun\n' "$target"
    exit 5
  fi
  # focus-2026-10-19: posting keys needs a SECOND, explicit consent —
  # YAP_QA_ALLOW_KEYS=1 — so a walk can never type into someone's session.
  if [ "${YAP_QA_DRIVE_RUNNING:-}" != "1" ] || [ "${YAP_QA_ALLOW_KEYS:-}" != "1" ]; then
    printf 'Yap is running (pid %s); this walk posts fn into it — rerun with YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 when you are not dictating\n' "$pids"
    exit 4
  fi

  # The chord comes from the binding the app is actually running with. Resolved
  # BEFORE anything is built or posted: an unknown binding posts nothing.
  if [ "$mode" != "chord" ]; then
    resolve_binding "$SETTINGS"
    printf 'press=%s release=%s\n' "${PRESS[*]}" "${RELEASE[*]}"
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

  printf -- '-- pass 1: hold %s 1.5 s\n' "$CHORD"
  offset="$(log_size)"
  start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  "$fnpost" "${PRESS[@]}" || { rm -f "$seg"; die "fn-post press failed"; }
  sleep 1.5
  "$fnpost" "${RELEASE[@]}" || { rm -f "$seg"; die "fn-post release failed"; }
  settle_and_check "$offset" "$start" "$HOLD_STEPS" "$seg"
  rc=$?
  if [ "$rc" -ne 0 ]; then
    rm -f "$seg"
    exit 1
  fi

  sleep 1
  printf -- '-- pass 2: double-tap %s (hands-free), wait 3 s, tap %s\n' "$CHORD" "$CHORD"
  offset="$(log_size)"
  start="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  "$fnpost" "${PRESS[@]}" sleep:70 "${RELEASE[@]}" sleep:150 "${PRESS[@]}" sleep:70 "${RELEASE[@]}" \
    || { rm -f "$seg"; die "fn-post double-tap failed"; }
  sleep 3
  "$fnpost" "${PRESS[@]}" sleep:70 "${RELEASE[@]}" || { rm -f "$seg"; die "fn-post tap failed"; }
  settle_and_check "$offset" "$start" "$HANDSFREE_STEPS" "$seg"
  rc=$?
  rm -f "$seg"
  [ "$rc" -eq 0 ] || exit 1
  printf 'hotkey-walk: pass\n'
  exit 0
}

self_test() {
  local out rc fails=0 f
  out="$(mktemp -t yapqa)" || exit 1
  # Marks every child below as a self-test run: the only place fake pids are allowed.
  export YAP_QA_SELF_TEST=1
  # Never read the real settings.json: a case without --settings sees "no
  # settings file" (the app default), whatever this machine's binding is.
  export YAP_DATA_DIR="/nonexistent/yap-qa-self-test"

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

  # fix-249 advisory (f): consent to drive a running Yap is NOT consent to
  # post keys — YAP_QA_ALLOW_KEYS must be exactly 1, or the walk refuses.
  env -u YAP_QA_ALLOW_KEYS YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 \
    YAP_QA_DRIVE_RUNNING=1 YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" >"$out" 2>&1
  expect "YAP_QA_DRIVE_RUNNING=1 without YAP_QA_ALLOW_KEYS -> refuses to post keys" 4 $? \
    "rerun with YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=yes \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" >"$out" 2>&1
  expect "YAP_QA_ALLOW_KEYS other than 1 -> refuses to post keys" 4 $? \
    "rerun with YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1"

  env -u YAP_QA_ALLOW_KEYS YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 \
    YAP_QA_DRIVE_RUNNING=1 YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --chord >"$out" 2>&1
  expect "--chord: YAP_QA_DRIVE_RUNNING=1 without YAP_QA_ALLOW_KEYS -> refuses to post keys" 4 $? \
    "rerun with YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1"

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

  # yap-qa-hotkey-walk-binding: the chord comes from pttBinding. All gates open,
  # stopped at the post step (99) — the binding line is what WOULD be pressed.
  for f in fn_control fn; do
    YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
      YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --settings "$FIXTURES/settings-$f.json" >"$out" 2>&1
    expect "settings pttBinding=$f -> its chord, stopped before posting" 99 $? "binding=$f chord="
    grep '^binding=' "$out"
  done
  grep -qx 'press=ctrl-down down release=up ctrl-up' <(YAP_QA_APP=/System/Applications/Calculator.app \
    YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 \
    bash "$0" --settings "$FIXTURES/settings-fn_control.json" 2>&1)
  rc=$?
  : >"$out"
  expect "fn_control presses ⌃ then fn, releases fn then ⌃" 0 "$rc" ""

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --settings "$FIXTURES/settings-unknown.json" >"$out" 2>&1
  expect "unknown binding -> refused by name, nothing posted" 2 $? 'unknown binding "hyper_globe"'
  grep -e '^binding=' -e 'unknown binding' "$out"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --settings "$FIXTURES/settings-no-binding.json" >"$out" 2>&1
  expect "no pttBinding key -> the app default fn_control" 99 $? "binding=fn_control chord=ctrl+fn"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --settings "$FIXTURES/settings-corrupt.json" >"$out" 2>&1
  expect "unreadable settings -> refused, nothing posted" 2 $? "not readable JSON"

  YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --settings /nonexistent/settings.json >"$out" 2>&1
  expect "no settings file -> the app default fn_control" 99 $? "binding=fn_control chord=ctrl+fn"

  # --dev-build: matched by target path, no bundle needed.
  YAP_QA_APP=/nonexistent/Yap.app YAP_QA_FAKE_PIDS=4242 YAP_QA_DRIVE_RUNNING=1 YAP_QA_ALLOW_KEYS=1 \
    YAP_QA_FAKE_POST_ACCESS=1 YAP_QA_NO_POST=1 bash "$0" --dev-build=/bin/ls \
    --settings "$FIXTURES/settings-fn_control.json" >"$out" 2>&1
  expect "--dev-build: no bundle needed, reaches the post step" 99 $? "stopping before any event is posted"
  YAP_QA_APP=/System/Applications/Calculator.app bash "$0" --dev-build=/nonexistent/wilson-voice >"$out" 2>&1
  expect "--dev-build: missing binary -> usage error" 2 $? "no dev binary at /nonexistent/wilson-voice"
  YAP_QA_FAKE_PIDS="" YAP_QA_NO_POST=1 bash "$0" --dev-build=/bin/ls >"$out" 2>&1
  expect "--dev-build: dev binary not running -> refuses" 5 $? "Yap is not running (/bin/ls)"
  env -u YAP_QA_SELF_TEST YAP_QA_APP=/System/Applications/Calculator.app YAP_QA_FAKE_PIDS=4242 \
    YAP_QA_NO_POST=1 bash "$0" >"$out" 2>&1
  expect "YAP_QA_FAKE_PIDS outside --self-test -> refused" 2 $? "YAP_QA_FAKE_PIDS is a --self-test fake"

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

  # --table (advisory 7): read-only, and the bundle id is quoted / refused.
  sql_quote "a'b" >"$out"
  rc=1
  [ "$(cat "$out")" = "'a''b'" ] && rc=0
  expect "sql_quote doubles a single quote" 0 "$rc" ""
  local tcc
  tcc="$(mktemp -t yapqa-tcc)" || exit 1
  sqlite3 "$tcc" "create table access (service text, client text, auth_value integer, last_modified integer);
    insert into access values ('kTCCServiceListenEvent','com.wilsonguenther.wilson-voice',2,0);" || exit 1
  YAP_QA_TCC_DB="$tcc" YAP_QA_LOG="$FIXTURES/yap-walk-pass.log" bash "$0" --table >"$out" 2>&1
  expect "--table reads the fixture TCC row" 0 $? "kTCCServiceListenEvent|2|"
  YAP_QA_TCC_DB="$tcc" YAP_QA_BUNDLE_ID="x' or '1'='1" YAP_QA_LOG="$FIXTURES/yap-walk-pass.log" \
    bash "$0" --table >"$out" 2>&1
  expect "--table refuses a bundle id carrying SQL" 2 $? "not a bundle id"
  rm -f "$tcc"

  rm -f "$out"
  if [ "$fails" -ne 0 ]; then
    printf 'self-test: %d failure(s)\n' "$fails"
    exit 1
  fi
  printf 'self-test: pass (no events posted)\n'
  exit 0
}

# yap-hotkeys-advisories-r1 #7: the bundle id goes into SQL, so it must look
# like a bundle id AND it is quoted as an SQL string literal ('' escapes ').
sql_quote() { # $1 -> '$1' with every ' doubled
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/''/g")"
}

table() {
  bundle="${YAP_QA_BUNDLE_ID:-com.wilsonguenther.wilson-voice}"
  tcc_db="${YAP_QA_TCC_DB:-/Library/Application Support/com.apple.TCC/TCC.db}"
  case "$bundle" in
    "" | *[!A-Za-z0-9.-]*) die "not a bundle id: $bundle" ;;
  esac
  printf '== TCC grants for %s\n' "$bundle"
  # -readonly: this walk never writes the TCC database.
  sqlite3 -readonly "$tcc_db" \
    "select service, auth_value, datetime(last_modified,'unixepoch') from access where client = $(sql_quote "$bundle")" 2>/dev/null \
    || echo "(TCC.db not readable from this terminal — needs Full Disk Access)"
  [ -f "$LOG" ] || die "no log at $LOG"
  printf '== hotkey table at the last launch (%s)\n' "$LOG"
  awk '/hotkey table at launch:/{buf=""; grab=7} grab>0{buf=buf $0 "\n"; grab--} END{printf "%s", buf}' "$LOG"
  printf '== last 15 hotkey lines\n'
  grep -E "hotkey edge|FN PTT|dictate-toggle|meeting-toggle|re-arm" "$LOG" | tail -15
  exit 0
}

MODE=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --table | --self-test | --chord) MODE="$1" ;;
    -h | --help) sed -n '2,45p' "$0"; exit 0 ;;
    --settings)
      [ "$#" -ge 2 ] || die "--settings needs a path"
      SETTINGS="$2"; shift ;;
    --settings=*) SETTINGS="${1#--settings=}" ;;
    --dev-build) DEV_BIN="${CARGO_TARGET_DIR:-$REPO_ROOT/desktop/target}/debug/wilson-voice" ;;
    --dev-build=*) DEV_BIN="${1#--dev-build=}" ;;
    *) die "unknown argument: $1" ;;
  esac
  shift
done

case "$MODE" in
  --table) table ;;
  --self-test) self_test ;;
  --chord) walk chord ;;
  *) walk ;;
esac
