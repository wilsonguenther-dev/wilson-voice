#!/usr/bin/env bash
# quit-during-asr.sh — YAP-FX-01 / yap-1106-engine-owner (g): quitting at any
# moment during an engine load or decode never crashes.
#
#   quit-during-asr.sh               20 runs against a TEST build (YAP_APP)
#   quit-during-asr.sh --self-test   refusal paths + parsers on fixtures; launches nothing
#
# Each run: clones the source data dir (APFS clone, `cp -c`, never the original)
# into a fresh temp dir, marks the newest meeting that still has audio as
# `transcribing` IN THE CLONE, launches the test app's executable directly with
# YAP_DATA_DIR pointing at the clone (launch reconcile re-queues the meeting, so
# the engine loads and starts decoding), sends Quit through AppleScript after
# 1…10 s (run i waits ((i-1) % 10) + 1 s), then requires:
#   - the app exits with code 0 within EXIT_WAIT_S;
#   - no new .ips for the executable appeared in ~/Library/Logs/DiagnosticReports;
#   - the DB was checkpointed (wilson_voice.db-wal absent or empty);
#   - the log has the line `exit: engines freed=N waited_ms=… skipped_destructors=…`;
#   - no `yap-asr` helper from this app bundle outlived the app.
#
# Exit codes: 0 all runs pass  1 a run failed  2 usage / app / data missing
#             3 refused (production bundle id, or the test app already running)
#
# Environment: YAP_APP (required; a "Yap Test.app" from scripts/test-build.sh or a
# dev bundle, NEVER /Applications/Yap.app), YAP_QA_SOURCE_DATA (default
# ~/Library/Application Support/WilsonVoice — read only, through the clone),
# YAP_QA_RUNS (default 20).
#
# Safety: the only process this script ever signals is the PID it launched,
# checked with `ps -p PID -o command=` first. Every temp dir is removed on exit.

set -u

PROD_BUNDLE_ID="com.wilsonguenther.wilson-voice"
RUNS="${YAP_QA_RUNS:-20}"
EXIT_WAIT_S=20
SRC="${YAP_QA_SOURCE_DATA:-$HOME/Library/Application Support/WilsonVoice}"
REPORTS="$HOME/Library/Logs/DiagnosticReports"
EXIT_NEEDLE="exit: engines freed="

TMP_ROOT=""
LAUNCHED_PID=""
cleanup() {
  if [[ -n "$LAUNCHED_PID" ]] && kill -0 "$LAUNCHED_PID" 2>/dev/null; then
    if [[ "$(ps -p "$LAUNCHED_PID" -o command= 2>/dev/null)" == "$EXE"* ]]; then
      kill -9 "$LAUNCHED_PID" 2>/dev/null
    fi
  fi
  [[ -n "$TMP_ROOT" && -d "$TMP_ROOT" ]] && rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

say_() { printf '%s quit-during-asr %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"; }

# ── pure checks (the self-test feeds them fixtures) ──────────────────────────

# exit_line_ok LOG -> 0 when the documented exit line is present
exit_line_ok() {
  grep -Eq 'exit: engines freed=[0-9]+ waited_ms=[0-9]+ skipped_destructors=(true|false)' "$1" 2>/dev/null
}

# checkpointed DATA -> 0 when the WAL is gone or empty
checkpointed() {
  local wal="$1/wilson_voice.db-wal"
  [[ ! -e "$wal" || ! -s "$wal" ]]
}

# new_ips STAMP EXE_NAME -> prints the crash reports newer than STAMP
new_ips() {
  [[ -d "$REPORTS" ]] || return 0
  find "$REPORTS" -maxdepth 1 -name "$2*.ips" -newer "$1" 2>/dev/null
}

# refuse_bundle ID -> 0 when the id is the production app's
refuse_bundle() {
  [[ "$1" == "$PROD_BUNDLE_ID" ]]
}

# sql_quote TEXT -> TEXT as a single-quoted SQL literal
sql_quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/''/g")"; }

# repoint_paths DB FROM TO -> rewrites every text cell that contains FROM
repoint_paths() {
  local db="$1" from to table col
  from="$(sql_quote "$2")"
  to="$(sql_quote "$3")"
  while IFS='|' read -r table col; do
    [[ -n "$table" && -n "$col" ]] || continue
    sqlite3 "$db" "UPDATE \"$table\" SET \"$col\" = replace(\"$col\", $from, $to) WHERE typeof(\"$col\") = 'text' AND instr(\"$col\", $from) > 0;" || return 1
  done < <(sqlite3 "$db" "SELECT m.name, p.name FROM sqlite_master m, pragma_table_info(m.name) p WHERE m.type = 'table' AND m.sql NOT LIKE 'CREATE VIRTUAL%' AND m.name NOT LIKE 'sqlite_%' AND m.name NOT LIKE '%fts%';")
  # Nothing may still point at the source.
  [[ "$(sqlite3 "$db" "SELECT count(*) FROM meetings WHERE instr(ifnull(mic_wav_path,''), $from) > 0 OR instr(ifnull(sys_wav_path,''), $from) > 0;")" == "0" ]]
}

self_test() {
  local t fails=0
  t="$(mktemp -d)"
  printf 'x exit: engines freed=1 waited_ms=42 skipped_destructors=false\n' > "$t/good.log"
  printf 'x exit: stopped 1 notes sidecar(s)\n' > "$t/bad.log"
  exit_line_ok "$t/good.log" || { echo "self-test: exit line not recognised"; fails=1; }
  exit_line_ok "$t/bad.log" && { echo "self-test: wrong exit line accepted"; fails=1; }
  mkdir -p "$t/d1" "$t/d2"
  : > "$t/d1/wilson_voice.db-wal"
  printf 'pages' > "$t/d2/wilson_voice.db-wal"
  checkpointed "$t/d1" || { echo "self-test: empty WAL not accepted"; fails=1; }
  checkpointed "$t/d2" && { echo "self-test: non-empty WAL accepted"; fails=1; }
  refuse_bundle "$PROD_BUNDLE_ID" || { echo "self-test: production id not refused"; fails=1; }
  refuse_bundle "$PROD_BUNDLE_ID.test" && { echo "self-test: test id refused"; fails=1; }
  sqlite3 "$t/p.db" "CREATE TABLE meetings(id TEXT, mic_wav_path TEXT, sys_wav_path TEXT); INSERT INTO meetings VALUES('m1', '/src dir/a''b/m1.t0.wav', NULL);"
  repoint_paths "$t/p.db" "/src dir/a'b" "/clone" || { echo "self-test: repoint failed"; fails=1; }
  [[ "$(sqlite3 "$t/p.db" "SELECT mic_wav_path FROM meetings;")" == "/clone/m1.t0.wav" ]] \
    || { echo "self-test: path not re-pointed"; fails=1; }
  rm -rf "$t"
  if [[ $fails -eq 0 ]]; then echo "self-test: ok"; return 0; fi
  return 1
}

if [[ "${1:-}" == "--self-test" ]]; then
  self_test
  exit $?
fi
[[ $# -eq 0 ]] || { echo "usage: quit-during-asr.sh [--self-test]" >&2; exit 2; }

APP="${YAP_APP:-}"
[[ -n "$APP" && -d "$APP" ]] || { echo "YAP_APP must name a test build .app" >&2; exit 2; }
[[ "$APP" != "/Applications/Yap.app" ]] || { echo "refusing /Applications/Yap.app" >&2; exit 3; }
PLIST="$APP/Contents/Info.plist"
BUNDLE_ID="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST" 2>/dev/null)"
EXE_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$PLIST" 2>/dev/null)"
[[ -n "$BUNDLE_ID" && -n "$EXE_NAME" ]] || { echo "cannot read $PLIST" >&2; exit 2; }
refuse_bundle "$BUNDLE_ID" && { echo "refusing the production bundle id $BUNDLE_ID" >&2; exit 3; }
EXE="$APP/Contents/MacOS/$EXE_NAME"
[[ -x "$EXE" ]] || { echo "no executable at $EXE" >&2; exit 2; }
[[ -x "$APP/Contents/MacOS/yap-asr" ]] || { echo "the build has no yap-asr helper" >&2; exit 2; }
if pgrep -f "$EXE" >/dev/null 2>&1; then
  echo "the test app is already running; quit it first" >&2
  exit 3
fi
[[ -f "$SRC/wilson_voice.db" ]] || { echo "no DB in $SRC" >&2; exit 2; }

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/yap-quit-asr.XXXXXX")"
passed=0
failed=0
for ((i = 1; i <= RUNS; i++)); do
  wait_s=$(( (i - 1) % 10 + 1 ))
  data="$TMP_ROOT/run-$i"
  mkdir -p "$data"
  # APFS clone of the source: the original is never opened for writing.
  cp -cR "$SRC/." "$data/" 2>/dev/null || cp -R "$SRC/." "$data/"
  rm -f "$data/wilson_voice.db-wal" "$data/wilson_voice.db-shm"
  # Every stored absolute path into the source dir is re-pointed at the clone,
  # so retention, recovery or a retry in the test app can never touch a file of
  # the source (Wilson's real data stays read-only).
  repoint_paths "$data/wilson_voice.db" "$SRC" "$data" \
    || { say_ "run=$i could not re-point stored paths"; failed=$((failed + 1)); continue; }
  sqlite3 "$data/wilson_voice.db" \
    "UPDATE meetings SET state='transcribing' WHERE id=(SELECT id FROM meetings WHERE mic_wav_path IS NOT NULL ORDER BY started_at DESC LIMIT 1);" \
    || { say_ "run=$i could not mark a meeting transcribing"; failed=$((failed + 1)); continue; }
  stamp="$TMP_ROOT/stamp-$i"
  : > "$stamp"
  YAP_DATA_DIR="$data" "$EXE" >"$TMP_ROOT/stdout-$i.log" 2>&1 &
  LAUNCHED_PID=$!
  sleep "$wait_s"
  osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1
  code=""
  for ((w = 0; w < EXIT_WAIT_S * 10; w++)); do
    if ! kill -0 "$LAUNCHED_PID" 2>/dev/null; then
      wait "$LAUNCHED_PID"
      code=$?
      break
    fi
    sleep 0.1
  done
  verdict="pass"
  reasons=""
  if [[ -z "$code" ]]; then
    verdict="fail"; reasons="$reasons did_not_exit_in_${EXIT_WAIT_S}s"
    if [[ "$(ps -p "$LAUNCHED_PID" -o command= 2>/dev/null)" == "$EXE"* ]]; then
      kill -9 "$LAUNCHED_PID" 2>/dev/null
      wait "$LAUNCHED_PID" 2>/dev/null
    fi
  elif [[ "$code" != "0" ]]; then
    verdict="fail"; reasons="$reasons exit_code=$code"
  fi
  LAUNCHED_PID=""
  ips="$(new_ips "$stamp" "$EXE_NAME")"
  [[ -z "$ips" ]] || { verdict="fail"; reasons="$reasons ips=$(echo "$ips" | wc -l | tr -d ' ')"; }
  checkpointed "$data" || { verdict="fail"; reasons="$reasons wal_not_checkpointed"; }
  exit_line_ok "$data/logs/yap.log" || { verdict="fail"; reasons="$reasons no_exit_line"; }
  if pgrep -f "$APP/Contents/MacOS/yap-asr" >/dev/null 2>&1; then
    verdict="fail"; reasons="$reasons helper_outlived_app"
  fi
  say_ "run=$i quit_after_s=$wait_s verdict=$verdict${reasons}"
  if [[ "$verdict" == "pass" ]]; then passed=$((passed + 1)); else failed=$((failed + 1)); fi
  rm -rf "$data"
done
say_ "runs=$RUNS passed=$passed failed=$failed"
[[ $failed -eq 0 ]]
