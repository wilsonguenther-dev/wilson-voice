#!/usr/bin/env bash
# meeting-walk.sh — walk a Meeting on the INSTALLED Yap, end to end.
#
#   meeting-walk.sh              walk the installed app (needs YAP_QA_DRIVE_RUNNING=1)
#   meeting-walk.sh --self-test  refusal paths + DB/log parsers on fixtures; posts
#                                no events and plays no audio
#
# What the walk does: makes its own two-voice sample at run time
# (say -o "$TMP/sample.aiff" -f scripts/qa/fixtures/meeting-script.txt — no audio
# in git), starts a meeting with Control-Command-M posted through an ad hoc QA
# Swift helper (chord-post.swift, the documented keyboard path, no socket),
# loops the sample through afplay for 30 s, stops the meeting the same way, and
# then reads the DB READ-ONLY (sqlite3 -readonly) and Yap's log:
#   - the newest meeting reaches `complete` within 90 s of Stop;
#   - its track 1 (call audio — what the walk played) totals > 40 words;
#   - summary IS NOT NULL, unless the log says "summary skipped: no model", in
#     which case it prints that and still requires the transcript.
# If the meeting's own verdict is the denial sentence while afplay was playing,
# that is a TRUE denial: exit 5 with the System Settings path. Never a pass.
#
# Exit codes (every one is this script's own; no pipeline masks a status):
#   0 pass   1 assertion failed   2 usage / app / DB / log missing
#   3 the caller lacks Accessibility (cannot post the shortcut)
#   4 Yap is running and YAP_QA_DRIVE_RUNNING is not 1
#   5 System Audio Recording is not granted   6 Yap is not running
#
# Environment: YAP_APP (default /Applications/Yap.app), YAP_WALK_DATA_DIR
# (default ~/Library/Application Support/WilsonVoice), YAP_QA_DRIVE_RUNNING=1.
# YAP_QA_FAKE_PIDS (self-test only) stands in for pgrep.

HERE="$(cd "$(dirname "$0")" && pwd)"
APP="${YAP_APP:-/Applications/Yap.app}"
DATA="${YAP_WALK_DATA_DIR:-$HOME/Library/Application Support/WilsonVoice}"
DB="$DATA/wilson_voice.db"
LOG="$DATA/logs/yap.log"
WAIT_COMPLETE_S=90
PLAY_S=30
MIN_T1_WORDS=40
DENIAL_NEEDLE="has not granted System Audio Recording"

usage() {
  printf 'usage: meeting-walk.sh [--self-test]\n' >&2
}

# ── parsers (pure over a DB file + a log file; the self-test feeds fixtures) ──

q() { sqlite3 -readonly "$1" "$2"; }

newest_meeting() { q "$1" "SELECT id FROM meetings ORDER BY started_at DESC LIMIT 1;"; }

# assess DB LOG ID -> prints a verdict line; returns 0 pass, 1 fail, 5 denied
assess() {
  local db="$1" log="$2" id="$3" state error words summary
  state="$(q "$db" "SELECT state FROM meetings WHERE id='$id';")"
  error="$(q "$db" "SELECT ifnull(error,'') FROM meetings WHERE id='$id';")"
  case "$error" in
    *"$DENIAL_NEEDLE"*)
      printf 'System Audio Recording is not granted for %s (designated requirement: %s) — allow it in System Settings › Privacy & Security › Screen & System Audio Recording, then rerun\n' \
        "$APP" "$(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p' | head -1)"
      return 5
      ;;
  esac
  if [ "$state" != "complete" ]; then
    printf 'FAIL meeting %s is %s, not complete (error: %s)\n' "$id" "${state:-missing}" "$error"
    return 1
  fi
  words="$(q "$db" "SELECT ifnull(group_concat(text,' '),'') FROM meeting_segments WHERE meeting_id='$id' AND track=1;" | wc -w | tr -d ' ')"
  if [ "${words:-0}" -le "$MIN_T1_WORDS" ]; then
    printf 'FAIL track 1 (call audio) has %s words, need > %s\n' "$words" "$MIN_T1_WORDS"
    return 1
  fi
  summary="$(q "$db" "SELECT CASE WHEN summary IS NULL THEN 'null' ELSE 'set' END FROM meetings WHERE id='$id';")"
  if [ "$summary" = "null" ]; then
    if grep -q "summary skipped: no model (meeting $id)" "$log"; then
      printf 'summary skipped: no model\n'
    else
      printf 'FAIL summary is NULL and the log does not say no model is installed\n'
      return 1
    fi
  fi
  printf 'PASS meeting %s complete, track 1 = %s words, summary %s\n' "$id" "$words" "$summary"
  return 0
}

running_pids() {
  if [ -n "${YAP_QA_FAKE_PIDS+x}" ]; then
    printf '%s' "$YAP_QA_FAKE_PIDS"
    return 0
  fi
  local exe
  exe="$(ls "$APP/Contents/MacOS" 2>/dev/null | head -1)"
  [ -n "$exe" ] || return 0
  pgrep -f "$APP/Contents/MacOS/$exe" 2>/dev/null
  return 0
}

# preflight -> returns the exit code the walk must take (0 = go on)
preflight() {
  [ -d "$APP" ] || { printf 'Yap is not installed at %s\n' "$APP"; return 2; }
  [ -f "$DB" ] || { printf 'no database at %s\n' "$DB"; return 2; }
  [ -f "$LOG" ] || { printf 'no log at %s\n' "$LOG"; return 2; }
  local pids
  pids="$(running_pids)"
  [ -n "$pids" ] || { printf 'Yap is not running — open %s, then rerun\n' "$APP"; return 6; }
  if [ "${YAP_QA_DRIVE_RUNNING:-}" != "1" ]; then
    printf 'Yap is running (pid %s); this walk records a meeting in it — rerun with YAP_QA_DRIVE_RUNNING=1\n' "$pids"
    return 4
  fi
  return 0
}

self_test() {
  local tmp rc failed=0
  tmp="$(mktemp -d)"
  check() { # name expected actual
    if [ "$2" = "$3" ]; then printf 'ok   %s (exit %s)\n' "$1" "$3"; else printf 'FAIL %s: expected %s got %s\n' "$1" "$2" "$3"; failed=1; fi
  }
  bash "$0" --bogus >/dev/null 2>&1; rc=$?; check "usage refusal" 2 "$rc"
  YAP_APP="$tmp/none.app" bash "$0" --walk-preflight-only >/dev/null 2>&1; rc=$?; check "missing app" 2 "$rc"
  mkdir -p "$tmp/Yap.app/Contents/MacOS" "$tmp/data/logs"
  : > "$tmp/Yap.app/Contents/MacOS/Yap"
  YAP_APP="$tmp/Yap.app" YAP_WALK_DATA_DIR="$tmp/data" bash "$0" --walk-preflight-only >/dev/null 2>&1; rc=$?; check "missing DB" 2 "$rc"
  sqlite3 "$tmp/data/wilson_voice.db" < "$HERE/fixtures/meeting/schema.sql"
  YAP_APP="$tmp/Yap.app" YAP_WALK_DATA_DIR="$tmp/data" bash "$0" --walk-preflight-only >/dev/null 2>&1; rc=$?; check "missing log" 2 "$rc"
  : > "$tmp/data/logs/yap.log"
  YAP_QA_FAKE_PIDS="" YAP_APP="$tmp/Yap.app" YAP_WALK_DATA_DIR="$tmp/data" bash "$0" --walk-preflight-only >/dev/null 2>&1; rc=$?; check "not running" 6 "$rc"
  YAP_QA_FAKE_PIDS="4242" YAP_QA_DRIVE_RUNNING="" YAP_APP="$tmp/Yap.app" YAP_WALK_DATA_DIR="$tmp/data" bash "$0" --walk-preflight-only >/dev/null 2>&1; rc=$?; check "no consent" 4 "$rc"
  YAP_QA_FAKE_PIDS="4242" YAP_QA_DRIVE_RUNNING=1 YAP_APP="$tmp/Yap.app" YAP_WALK_DATA_DIR="$tmp/data" bash "$0" --walk-preflight-only >/dev/null 2>&1; rc=$?; check "preflight passes" 0 "$rc"
  local f
  for f in pass few-words denied; do
    sqlite3 "$tmp/$f.db" < "$HERE/fixtures/meeting/schema.sql"
    sqlite3 "$tmp/$f.db" < "$HERE/fixtures/meeting/$f.sql"
  done
  check "newest meeting parser" "m-pass" "$(newest_meeting "$tmp/pass.db")"
  assess "$tmp/pass.db" "$HERE/fixtures/meeting/no-summary-line.log" m-pass >/dev/null; rc=$?; check "pass fixture" 0 "$rc"
  assess "$tmp/few-words.db" "$HERE/fixtures/meeting/no-model.log" m-few >/dev/null; rc=$?; check "track 1 under 40 words fails (track 0 words do not count)" 1 "$rc"
  assess "$tmp/denied.db" "$HERE/fixtures/meeting/no-model.log" m-denied >/dev/null; rc=$?; check "true denial" 5 "$rc"
  sqlite3 "$tmp/few-words.db" "INSERT INTO meeting_segments VALUES ('s9','m-few',10,20,'$(printf 'word%.0s ' $(seq 1 45))',1);"
  assess "$tmp/few-words.db" "$HERE/fixtures/meeting/no-model.log" m-few > "$tmp/out.txt"; rc=$?; check "no model: transcript still required, summary skipped" 0 "$rc"
  grep -q "summary skipped: no model" "$tmp/out.txt"; rc=$?; check "prints summary skipped" 0 "$rc"
  assess "$tmp/few-words.db" "$HERE/fixtures/meeting/no-summary-line.log" m-few >/dev/null; rc=$?; check "NULL summary without the no-model line fails" 1 "$rc"
  rm -rf "$tmp"
  if [ "$failed" -ne 0 ]; then exit 1; fi
  printf 'self-test: all checks passed\n'
  exit 0
}

walk() {
  preflight; local rc=$?
  [ "$rc" -eq 0 ] || exit "$rc"
  local tmp helper id before stop_at now state rc2 apid
  tmp="$(mktemp -d)"
  helper="$tmp/chord-post"
  swiftc -O -o "$helper" "$HERE/chord-post.swift" >/dev/null 2>&1 || { printf 'could not build the QA shortcut helper\n'; exit 2; }
  "$helper" check >/dev/null; rc=$?
  if [ "$rc" -eq 3 ]; then printf 'this terminal lacks Accessibility, so it cannot post ⌃⌘M — grant it in System Settings › Privacy & Security › Accessibility\n'; exit 3; fi
  [ "$rc" -eq 0 ] || exit 2
  say -o "$tmp/sample.aiff" -f "$HERE/fixtures/meeting-script.txt"; rc=$?
  [ "$rc" -eq 0 ] || { printf 'say failed (%s)\n' "$rc"; exit 2; }
  before="$(newest_meeting "$DB")"
  "$helper" post >/dev/null || exit 2
  sleep 2
  ( end=$((SECONDS + PLAY_S)); while [ "$SECONDS" -lt "$end" ]; do afplay "$tmp/sample.aiff"; done ) &
  apid=$!
  sleep "$PLAY_S"
  wait "$apid"
  "$helper" post >/dev/null || exit 2
  stop_at=$SECONDS
  id="$(newest_meeting "$DB")"
  if [ -z "$id" ] || [ "$id" = "$before" ]; then printf 'FAIL no new meeting row after ⌃⌘M\n'; exit 1; fi
  state=""
  while :; do
    state="$(q "$DB" "SELECT state FROM meetings WHERE id='$id';")"
    case "$state" in complete|failed|partial) break ;; esac
    now=$SECONDS
    [ $((now - stop_at)) -lt "$WAIT_COMPLETE_S" ] || break
    sleep 2
  done
  sleep 1
  assess "$DB" "$LOG" "$id"; rc2=$?
  printf 'cleanup: meeting %s was created by this walk — delete it in Yap (Meetings › Delete), the app’s own delete path\n' "$id"
  rm -rf "$tmp"
  exit "$rc2"
}

case "${1:-}" in
  --self-test) self_test ;;
  --walk-preflight-only) preflight; exit $? ;;
  "") walk ;;
  *) usage; exit 2 ;;
esac
