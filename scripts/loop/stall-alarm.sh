#!/usr/bin/env bash
# scripts/loop/stall-alarm.sh — the EXTERNAL half of the stall detector (item yap-loop-stall-detector).
#
# SOURCE this file; do not execute it. It defines one function, stall_alarm_check, for the machine
# monitor in ~/bin (reach it by the glob ~/bin/loop-w*-yap-learn.sh — that script lives outside the
# repo, so the orchestrator applies a 3-line patch to it after this merges; see the PR body).
#
# THE CONTRACT
#   The loop writes the sentinel ${STALL_FILE:-/Users/wilsonguenther/code/wilson-voice-loop/STALL.txt}
#   when it stalls (gate red on main, no progress). The file holds a date -u stamp, the rule number
#   and the line. A fresh stall OVERWRITES it. The loop itself NEVER deletes it.
#   The ORCHESTRATOR removes STALL.txt after acting on it — that removal is the acknowledgement, and
#   the next monitor tick prints "cleared STALL".
#
# EDGE-TRIGGERED, like the RSS/mem/disk alerts in the monitor: the caller initialises stall_alert=0
# once, then calls stall_alarm_check every tick.
#   file present, stall_alert=0 -> stall_alert=1, print "<stamp> ALERT STALL <contents, newlines -> ' | '>"
#   file present, stall_alert=1 -> silent
#   file gone,    stall_alert=1 -> stall_alert=0, print "<stamp> cleared STALL"
#   file gone,    stall_alert=0 -> silent
# The monitor's filter already surfaces every line containing ALERT.

stall_alarm_check() {
  local file="${STALL_FILE:-/Users/wilsonguenther/code/wilson-voice-loop/STALL.txt}"
  local stamp body line
  stamp="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  if [ -f "$file" ]; then
    if [ "${stall_alert:-0}" = "0" ]; then
      stall_alert=1
      body=""
      while IFS= read -r line || [ -n "$line" ]; do
        if [ -z "$body" ]; then body="$line"; else body="$body | $line"; fi
      done < "$file"
      printf '%s ALERT STALL %s\n' "$stamp" "$body"
    fi
  elif [ "${stall_alert:-0}" = "1" ]; then
    stall_alert=0
    printf '%s cleared STALL\n' "$stamp"
  fi
  return 0
}
