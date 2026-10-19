#!/usr/bin/env bash
# scripts/qa/release-smoke.sh — the signed-app gate every DMG build runs (yap-p0-release-smoke).
#
# 0.9.0 shipped with every TCC grant dead (Apple Development -> Developer ID changed the
# designated requirement) and nothing noticed. This is the step that notices.
#
#   release-smoke.sh --app APP [--dmg DMG] [--compare OLD] [--allow-dr-change --release-note LINE]
#       The full run, in order: step=signature, step=dr_change, then LAUNCH APP with
#       --release-smoke (open -n, temp data dir), drive a TextEdit document, and parse the
#       app's yap.log with the same parser --parse uses. Release host only.
#   release-smoke.sh --signature-only --app APP [--dmg DMG]     step=signature only (read-only)
#   release-smoke.sh --dr-only --app APP [--compare OLD] [--allow-dr-change --release-note LINE]
#                                                               step=dr_change only (read-only)
#   release-smoke.sh --parse LOG [--doc-text TEXT]              parse a yap.log (no launch)
#   release-smoke.sh --self-test [--force-fail STEP]            fixtures + refusals, launches nothing
#   --force-fail STEP (signature|dr_change|probe|hotkeys|insert|quit) forces that step to fail.
#
# step=signature: codesign --verify --deep --strict on the .app AND on the .app inside the DMG
#   (hdiutil attach -readonly -nobrowse to a temp mountpoint, detached after), each requiring
#   Authority=Developer ID Application: ... and TeamIdentifier=VHYV8C2JNU.
# step=dr_change: scripts/qa/perm-probe.sh --app OLD --compare APP. dr_change=yes FAILS unless
#   both --allow-dr-change and --release-note are given; the note prints as release_note=<line>.
#
# Prints step=<name> ok|fail per step. Exit 0 all ok; 1 any FAIL (named); 2 usage, or another
# Yap with the same bundle id is running (never signalled).
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXTURES="$SCRIPT_DIR/fixtures"
MATRIX="$FIXTURES/release-smoke-matrix"
PERM_PROBE="$SCRIPT_DIR/perm-probe.sh"
TEAM_ID="VHYV8C2JNU"
SMOKE_TEXT="yap release smoke ok"
SERVICES="accessibility listenevent postevent microphone"
STEPS="signature dr_change probe hotkeys insert quit"
QUIT_WAIT_S=10
LAUNCH_WAIT_S=30

FORCE_FAIL=""
FAILED=""

usage() {
  printf 'release-smoke: %s\n' "$1" >&2
  printf 'usage: release-smoke.sh --app APP [--dmg DMG] [--compare OLD] [--allow-dr-change --release-note LINE]\n' >&2
  printf '       release-smoke.sh --signature-only --app APP [--dmg DMG]\n' >&2
  printf '       release-smoke.sh --dr-only --app APP [--compare OLD] [--allow-dr-change --release-note LINE]\n' >&2
  printf '       release-smoke.sh --parse LOG [--doc-text TEXT] | --self-test [--force-fail STEP]\n' >&2
  exit 2
}

step_ok() { printf 'step=%s ok%s\n' "$1" "${2:+ ($2)}"; }
step_fail() {
  printf 'step=%s fail (%s)\n' "$1" "$2"
  case " $FAILED " in *" $1 "*) ;; *) FAILED="${FAILED:+$FAILED }$1" ;; esac
}
forced() { [ -n "$FORCE_FAIL" ] && [ "$FORCE_FAIL" = "$1" ]; }

finish() {
  if [ -n "$FAILED" ]; then
    printf 'release-smoke: FAIL step=%s\n' "${FAILED%% *}"
    return 1
  fi
  printf 'release-smoke: OK\n'
  return 0
}

# ── step=signature ──────────────────────────────────────────────────────────
# One bundle: strict deep verify, Developer ID authority, pinned team. Prints why on failure.
check_one_signature() { # $1 label, $2 app -> 0 ok
  local label="$1" app="$2" info
  if [ ! -d "$app" ]; then
    printf 'signature %s: no bundle at %s\n' "$label" "$app"
    return 1
  fi
  if ! codesign --verify --deep --strict "$app" >/dev/null 2>&1; then
    printf 'signature %s: codesign --verify --deep --strict failed\n' "$label"
    return 1
  fi
  info="$(codesign -dv --verbose=4 "$app" 2>&1)"
  if ! grep -q '^Authority=Developer ID Application: ' <<<"$info"; then
    printf 'signature %s: leaf authority is not "Developer ID Application: ..." (%s)\n' \
      "$label" "$(grep -m1 '^Authority=' <<<"$info" || echo 'no Authority')"
    return 1
  fi
  if ! grep -qx "TeamIdentifier=$TEAM_ID" <<<"$info"; then
    printf 'signature %s: TeamIdentifier is not the pinned team (%s)\n' \
      "$label" "$(grep -m1 '^TeamIdentifier=' <<<"$info" || echo 'none')"
    return 1
  fi
  printf 'signature %s: ok (strict verify, Developer ID Application, team pinned)\n' "$label"
  return 0
}

step_signature() { # $1 app, $2 dmg (may be empty)
  local app="$1" dmg="$2" bad=0 mnt inner
  if forced signature; then step_fail signature forced; return; fi
  check_one_signature app "$app" || bad=1
  if [ -n "$dmg" ]; then
    if [ ! -f "$dmg" ]; then
      printf 'signature dmg: no file at %s\n' "$dmg"; bad=1
    elif ! mnt="$(mktemp -d -t yap-smoke-dmg)"; then
      printf 'signature dmg: mktemp failed\n'; bad=1
    elif ! hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$mnt" "$dmg" >/dev/null 2>&1; then
      printf 'signature dmg: hdiutil attach failed for %s\n' "$dmg"; bad=1; rmdir "$mnt" 2>/dev/null
    else
      inner="$(find "$mnt" -maxdepth 1 -name '*.app' -type d | head -1)"
      if [ -z "$inner" ]; then
        printf 'signature dmg: no .app inside %s\n' "$dmg"; bad=1
      else
        check_one_signature dmg-app "$inner" || bad=1
      fi
      hdiutil detach "$mnt" -quiet >/dev/null 2>&1 || hdiutil detach "$mnt" -force -quiet >/dev/null 2>&1
      rmdir "$mnt" 2>/dev/null
    fi
  else
    printf 'signature dmg: not given (app only)\n'
  fi
  if [ "$bad" -eq 0 ]; then step_ok signature; else step_fail signature 'see signature lines above'; fi
}

# ── step=dr_change ──────────────────────────────────────────────────────────
step_dr_change() { # $1 new app, $2 old app (may be empty), $3 allow (0|1), $4 note
  local new="$1" old="$2" allow="$3" note="$4" out dr
  if forced dr_change; then step_fail dr_change forced; return; fi
  if [ -z "$old" ]; then
    [ -d /Applications/Yap.app ] && old=/Applications/Yap.app
  fi
  if [ -z "$old" ]; then
    printf 'dr_change=skipped (no installed or last-release app to compare against)\n'
    step_ok dr_change 'nothing to compare'
    return
  fi
  if ! out="$(bash "$PERM_PROBE" --app "$old" --compare "$new" 2>&1)"; then
    printf '%s\n' "$out" | sed 's/^/perm-probe: /'
    step_fail dr_change 'perm-probe --compare failed'
    return
  fi
  dr="$(sed -n 's/^dr_change=//p' <<<"$out" | head -1)"
  printf 'dr_change=%s (old=%s new=%s)\n' "${dr:-missing}" "$old" "$new"
  case "$dr" in
    no) step_ok dr_change ;;
    yes)
      grep '^warning:' <<<"$out" || true
      if [ "$allow" = 1 ] && [ -n "$note" ]; then
        printf 'release_note=%s\n' "$note"
        step_ok dr_change 'allowed with a release note'
      elif [ "$allow" = 1 ]; then
        step_fail dr_change 'dr_change=yes needs --release-note "<line>" as well as --allow-dr-change'
      else
        step_fail dr_change 'dr_change=yes: every Accessibility/Input Monitoring grant goes dead; pass --allow-dr-change and --release-note'
      fi
      ;;
    *) step_fail dr_change 'perm-probe printed no dr_change line' ;;
  esac
}

# ── the parser (probe / hotkeys / insert / quit) ────────────────────────────
# Reads release-smoke lines out of a yap.log (any log prefix) and applies THE MATRIX.
parse_log() { # $1 log, $2 doc text (or the literal __unchecked__)
  local log="$1" doc="$2" lines s line state ledger
  local dead_all=1 any_ledger=0 probe_bad="" ax_granted=0 post_granted=0 absent_insert=1
  lines="$(grep -o 'release-smoke step=.*' "$log" 2>/dev/null)"

  # step=probe — per service, then per run.
  for s in $SERVICES; do
    line="$(grep "step=probe service=$s " <<<"$lines" | tail -1)"
    if [ -z "$line" ]; then probe_bad="${probe_bad}$s:missing "; continue; fi
    state="$(sed -n 's/.* state=\([A-Za-z]*\).*/\1/p' <<<"$line")"
    ledger="$(sed -n 's/.* ledger=\([a-z]*\).*/\1/p' <<<"$line")"
    [ "$ledger" = granted ] && any_ledger=1
    [ "$state" = Granted ] && dead_all=0
    case "$s:$state" in
      accessibility:Granted) ax_granted=1 ;;
      postevent:Granted) post_granted=1 ;;
    esac
    case "$s" in
      accessibility | postevent)
        { [ "$state" = Granted ] || [ "$ledger" = granted ]; } && absent_insert=0 ;;
    esac
    case "$state:$ledger" in
      Granted:*) ;;
      StaleIdentity:*) probe_bad="${probe_bad}$s:StaleIdentity(ledger=$ledger) " ;;
      Denied:granted) probe_bad="${probe_bad}$s:Denied-but-ledger-granted " ;;
      Denied:none | NotDetermined:none) ;;
      *) probe_bad="${probe_bad}$s:$state(ledger=$ledger) " ;;
    esac
  done
  if [ "$dead_all" = 1 ] && [ "$any_ledger" = 1 ]; then
    probe_bad="${probe_bad}every-grant-dead-with-a-ledger-grant(0.9.0) "
  fi
  if forced probe; then step_fail probe forced
  elif [ -n "$probe_bad" ]; then step_fail probe "${probe_bad% }"
  else step_ok probe; fi

  # step=hotkeys
  line="$(grep 'step=hotkeys ' <<<"$lines" | tail -1)"
  if forced hotkeys; then step_fail hotkeys forced
  elif [ -z "$line" ]; then step_fail hotkeys 'no step=hotkeys line'
  elif grep -q 'carbon=registered' <<<"$line"; then step_ok hotkeys "${line#*carbon=}"
  else step_fail hotkeys "carbon dictate chord not registered (${line#*carbon=})"; fi

  # step=insert
  line="$(grep 'step=insert ' <<<"$lines" | tail -1)"
  local path ok
  path="$(sed -n 's/.* path=\([A-Za-z]*\).*/\1/p' <<<"$line")"
  ok="$(sed -n 's/.* ok=\([a-z]*\).*/\1/p' <<<"$line")"
  if forced insert; then step_fail insert forced
  elif [ -z "$line" ]; then step_fail insert 'no step=insert line'
  elif [ "$path" = CopyCard ]; then
    if [ "$ax_granted" = 1 ] || [ "$post_granted" = 1 ]; then
      step_fail insert 'path=CopyCard while Accessibility or PostEvent is Granted: a regression, not a fallback'
    elif [ "$absent_insert" = 1 ] && [ "$ok" = true ]; then
      step_ok insert 'CopyCard: both insert grants genuinely absent'
    else
      step_fail insert "path=CopyCard ok=$ok with an insert grant the ledger saw working"
    fi
  elif [ "$path" = Ax ] || [ "$path" = PasteBridge ]; then
    if [ "$ok" != true ]; then step_fail insert "path=$path ok=$ok"
    elif [ "$doc" != __unchecked__ ] && [ "$doc" != "$SMOKE_TEXT" ]; then
      step_fail insert "path=$path but the TextEdit document reads \"$doc\", not \"$SMOKE_TEXT\""
    else step_ok insert "path=$path"; fi
  else step_fail insert "unknown path \"$path\""; fi

  # step=quit
  if forced quit; then step_fail quit forced
  elif grep -q 'step=quit ok=true' <<<"$lines"; then step_ok quit
  else step_fail quit "no step=quit ok=true line within ${QUIT_WAIT_S} s"; fi
}

# ── --app: the launch ───────────────────────────────────────────────────────
osa() { osascript -e "$1" 2>/dev/null; }

run_app() { # $1 app
  local app="$1" bid tmp log doc waited=0 src
  bid="$(plutil -extract CFBundleIdentifier raw -o - "$app/Contents/Info.plist" 2>/dev/null)" ||
    usage "no CFBundleIdentifier in $app"
  if lsappinfo find "bundleid=$bid" 2>/dev/null | grep -q .; then
    printf 'release-smoke: another app with bundle id %s is running — quit it first (it is never signalled)\n' "$bid" >&2
    exit 2
  fi
  tmp="$(mktemp -d -t yap-release-smoke)" || { step_fail probe 'mktemp failed'; return; }
  # Read-only copies of the host's signature ledgers, so a stale identity is visible.
  src="$HOME/Library/Application Support/WilsonVoice"
  for f in permission-signature.json last-run-signature.json; do
    [ -f "$src/$f" ] && cp "$src/$f" "$tmp/$f"
  done
  log="$tmp/logs/yap.log"
  osa 'tell application "TextEdit" to make new document' >/dev/null
  osa 'tell application "TextEdit" to activate' >/dev/null
  open -n -a "$app" --env "YAP_DATA_DIR=$tmp" --args --release-smoke
  while [ "$waited" -lt "$LAUNCH_WAIT_S" ] && ! grep -q 'release-smoke step=' "$log" 2>/dev/null; do
    sleep 1; waited=$((waited + 1))
  done
  waited=0
  while [ "$waited" -lt "$QUIT_WAIT_S" ] && ! grep -q 'release-smoke step=quit ok=true' "$log" 2>/dev/null; do
    sleep 1; waited=$((waited + 1))
  done
  doc="$(osa 'tell application "TextEdit" to get text of document 1')"
  osa 'tell application "TextEdit" to close document 1 saving no' >/dev/null
  [ -f "$log" ] || : >"$log"
  parse_log "$log" "$doc"
  printf 'release-smoke: app log kept at %s\n' "$log"
}

# ── --self-test ─────────────────────────────────────────────────────────────
self_test() {
  local fails=0 f want rc out
  expect() { # $1 want rc, $2 label, rest: argv
    local w="$1" label="$2"; shift 2
    out="$(bash "$0" "$@" 2>&1)"; rc=$?
    if [ "$rc" -eq "$w" ]; then printf 'ok   %s (exit %d)\n' "$label" "$rc"
    else printf 'FAIL %s (exit %d, want %d)\n%s\n' "$label" "$rc" "$w" "$out"; fails=$((fails + 1)); fi
  }
  for f in granted-all-ax:0 absent-grants-copy-card:0 all-dead-ledger-granted:1 \
           stale-ledger-granted-probe-denied:1 granted-but-copy-card:1 carbon-failed:1 no-quit:1; do
    want="${f##*:}"; f="${f%%:*}"
    if [ ! -f "$MATRIX/$f.log" ]; then printf 'FAIL matrix fixture %s.log missing\n' "$f"; fails=$((fails + 1)); continue; fi
    expect "$want" "matrix $f" --parse "$MATRIX/$f.log"
  done
  expect 1 'negative 0.9.0 capture' --parse "$FIXTURES/release-smoke-negative-2026-10-17.log"
  expect 1 'doc text mismatch fails insert' --parse "$MATRIX/granted-all-ax.log" --doc-text 'not it'
  expect 0 'doc text match passes' --parse "$MATRIX/granted-all-ax.log" --doc-text "$SMOKE_TEXT"
  # refusals
  expect 2 'no arguments' 
  expect 2 'unknown flag' --bogus
  expect 2 '--parse without a file' --parse
  expect 2 '--parse missing file' --parse /nonexistent/yap.log
  expect 2 '--app missing bundle' --app /nonexistent/Yap.app
  expect 2 '--force-fail unknown step' --parse "$MATRIX/granted-all-ax.log" --force-fail nope
  expect 2 '--signature-only without --app' --signature-only
  expect 2 '--release-note without --allow-dr-change' --dr-only --app /System/Applications/Calculator.app --release-note x
  # read-only signature + dr paths (no launch)
  expect 1 'signature: Apple-signed Calculator is not Developer ID' --signature-only --app /System/Applications/Calculator.app
  expect 0 'dr_change=no on the same app' --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/Calculator.app
  expect 1 'dr_change=yes refused' --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app
  expect 1 'dr_change=yes allowed but no note' --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app --allow-dr-change
  expect 0 'dr_change=yes allowed with a note' --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/TextEdit.app --allow-dr-change --release-note 'self-test'
  # every force-fail path
  for f in probe hotkeys insert quit; do
    expect 1 "force-fail $f" --parse "$MATRIX/granted-all-ax.log" --force-fail "$f"
  done
  expect 1 'force-fail signature' --signature-only --app /System/Applications/Calculator.app --force-fail signature
  expect 1 'force-fail dr_change' --dr-only --app /System/Applications/Calculator.app --compare /System/Applications/Calculator.app --force-fail dr_change
  if [ "$fails" -ne 0 ]; then printf 'self-test: %d FAIL\n' "$fails"; return 1; fi
  printf 'self-test: all ok\n'
  if [ -n "$FORCE_FAIL" ]; then
    # A clean run with STEP forced: must exit 1.
    case "$FORCE_FAIL" in
      signature) step_fail signature forced ;;
      dr_change) step_fail dr_change forced ;;
      *) parse_log "$MATRIX/granted-all-ax.log" __unchecked__ ;;
    esac
    finish
    return $?
  fi
  return 0
}

# ── argv ────────────────────────────────────────────────────────────────────
MODE="" APP="" DMG="" COMPARE="" PARSE="" DOC="__unchecked__" ALLOW=0 NOTE="" NOTE_SET=0
[ $# -gt 0 ] || usage 'no mode given'
while [ $# -gt 0 ]; do
  case "$1" in
    --app) [ $# -ge 2 ] || usage '--app needs a path'; APP="$2"; MODE="${MODE:-app}"; shift 2 ;;
    --dmg) [ $# -ge 2 ] || usage '--dmg needs a path'; DMG="$2"; shift 2 ;;
    --compare) [ $# -ge 2 ] || usage '--compare needs a path'; COMPARE="$2"; shift 2 ;;
    --parse) [ $# -ge 2 ] || usage '--parse needs a log file'; PARSE="$2"; MODE=parse; shift 2 ;;
    --doc-text) [ $# -ge 2 ] || usage '--doc-text needs text'; DOC="$2"; shift 2 ;;
    --signature-only) MODE=signature; shift ;;
    --dr-only) MODE=dr; shift ;;
    --self-test) MODE=self; shift ;;
    --allow-dr-change) ALLOW=1; shift ;;
    --release-note) [ $# -ge 2 ] || usage '--release-note needs a line'; NOTE="$2"; NOTE_SET=1; shift 2 ;;
    --force-fail) [ $# -ge 2 ] || usage '--force-fail needs a step'; FORCE_FAIL="$2"; shift 2 ;;
    *) usage "unknown argument: $1" ;;
  esac
done
if [ -n "$FORCE_FAIL" ]; then
  case " $STEPS " in *" $FORCE_FAIL "*) ;; *) usage "--force-fail: unknown step '$FORCE_FAIL' (one of: $STEPS)" ;; esac
fi
if [ "$NOTE_SET" = 1 ] && [ "$ALLOW" = 0 ]; then usage '--release-note is only meaningful with --allow-dr-change'; fi
if [ "$NOTE_SET" = 1 ] && [ -z "$NOTE" ]; then usage '--release-note must not be empty'; fi

case "$MODE" in
  self) self_test; exit $? ;;
  parse)
    [ -f "$PARSE" ] || usage "no log at $PARSE"
    parse_log "$PARSE" "$DOC"; finish; exit $? ;;
  signature)
    [ -n "$APP" ] || usage '--signature-only needs --app'
    [ -d "$APP" ] || usage "no bundle at $APP"
    step_signature "$APP" "$DMG"; finish; exit $? ;;
  dr)
    [ -n "$APP" ] || usage '--dr-only needs --app'
    [ -d "$APP" ] || usage "no bundle at $APP"
    step_dr_change "$APP" "$COMPARE" "$ALLOW" "$NOTE"; finish; exit $? ;;
  app)
    [ -d "$APP" ] || usage "no bundle at $APP"
    step_signature "$APP" "$DMG"
    step_dr_change "$APP" "$COMPARE" "$ALLOW" "$NOTE"
    if [ -n "$FAILED" ]; then
      printf 'release-smoke: not launching — signature/dr_change failed\n'
    else
      run_app "$APP"
    fi
    finish; exit $? ;;
  *) usage 'no mode given' ;;
esac
