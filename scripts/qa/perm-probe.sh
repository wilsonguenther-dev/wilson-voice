#!/usr/bin/env bash
# scripts/qa/perm-probe.sh — what macOS believes about an INSTALLED Yap (yap-qa-harness).
#
# READ-ONLY. Prints key=value lines, one fact per line, about the signed,
# installed bundle: its identity, its signature, every other bundle that claims
# the same bundle id, its LaunchServices registrations, and its running pids.
# It never reads or writes TCC.db, never signals a process, never launches one.
#
#   perm-probe.sh [--app PATH]                 default /Applications/Yap.app
#   perm-probe.sh [--app PATH] --compare NEW   dr_change=yes|no between PATH and NEW
#   perm-probe.sh --self-test                  every code path, against Calculator
#
# Why it exists: 0.9.0 changed signing identity (Apple Development -> Developer
# ID). Every TCC grant is pinned to the designated requirement, so every
# Accessibility / Input Monitoring grant went dead while System Settings kept
# showing ON. `--compare` is the release check that catches that before a ship.
#
# Exit: 0 ok, 1 self-test failure, 2 usage / app not found.
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
QA_BIN_DIR="$REPO_ROOT/desktop/target/qa"
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
SELF_TEST_APP=/System/Applications/Calculator.app
SELF_TEST_OTHER_APP=/System/Applications/TextEdit.app
DR_WARNING="warning: this build changes the designated requirement; every Accessibility, Input Monitoring and Full Disk Access grant for the old build stops matching"

die() {
  printf 'perm-probe: %s\n' "$1" >&2
  exit 2
}

# Resolve a bundle path to its physical absolute form (no trailing slash).
resolve_dir() {
  (cd "$1" 2>/dev/null && pwd -P)
}

plist_value() { # $1 app, $2 key
  plutil -extract "$2" raw -o - "$1/Contents/Info.plist" 2>/dev/null
}

# Build a QA Swift helper into desktop/target/qa (gitignored), signed AD HOC.
# Never Yap's identity: the helper is not the app.
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

# designated_requirement of a bundle, or "unsigned".
designated_requirement() { # $1 app
  local out
  if ! out="$(codesign -d -r- "$1" 2>/dev/null)"; then
    printf 'unsigned\n'
    return 0
  fi
  out="${out#*designated => }"
  printf '%s\n' "${out:-unsigned}"
}

# Same-id bundles: Spotlight plus a direct scan of /Applications and
# ~/Applications (Spotlight can be off or stale). The probed app is excluded.
same_id_bundles() { # $1 bundle id, $2 resolved probed path
  local tmp cand rp bid list="" seen="|"
  tmp="$(mktemp -t yapqa)" || return 0
  mdfind "kMDItemCFBundleIdentifier == '$1'" >"$tmp" 2>/dev/null
  find /Applications "$HOME/Applications" -maxdepth 2 -type d -name '*.app' -prune >>"$tmp" 2>/dev/null
  while IFS= read -r cand; do
    [ -n "$cand" ] && [ -d "$cand" ] || continue
    rp="$(resolve_dir "$cand")" || continue
    [ "$rp" = "$2" ] && continue
    case "$seen" in *"|$rp|"*) continue ;; esac
    bid="$(plist_value "$rp" CFBundleIdentifier)" || continue
    [ "$bid" = "$1" ] || continue
    seen="$seen$rp|"
    list="${list:+$list,}$rp"
  done <"$tmp"
  rm -f "$tmp"
  printf '%s\n' "$list"
}

# LaunchServices registrations whose identifier is the bundle id.
# Prints lsregister_count=N then lsregister_entry_<i>=<path> stale=<0|1>.
lsregister_facts() { # $1 bundle id
  local dump paths n=0 p stale
  dump="$(mktemp -t yapqa)" || { printf 'lsregister_count=unavailable\n'; return 0; }
  paths="$(mktemp -t yapqa)" || { rm -f "$dump"; printf 'lsregister_count=unavailable\n'; return 0; }
  if ! "$LSREGISTER" -dump >"$dump" 2>/dev/null; then
    rm -f "$dump" "$paths"
    printf 'lsregister_count=unavailable\n'
    return 0
  fi
  # A record is the text between '-----' separators; its path: line precedes
  # its identifier: line. The path carries a trailing " (0x....)" unit id.
  awk -v want="$1" '
    /^-----/ { path = ""; next }
    /^path:/ { p = $0; sub(/^path:[ \t]*/, "", p); sub(/ \(0x[0-9a-fA-F]+\)$/, "", p); path = p; next }
    /^identifier:/ { id = $0; sub(/^identifier:[ \t]*/, "", id); sub(/[ \t]+$/, "", id)
                     if (id == want && path != "") print path; next }
  ' "$dump" >"$paths"
  while IFS= read -r p; do
    n=$((n + 1))
    stale=0
    [ -e "$p" ] || stale=1
    printf 'lsregister_entry_%d=%s stale=%d\n' "$n" "$p" "$stale"
  done <"$paths"
  rm -f "$dump" "$paths"
  printf 'lsregister_count=%d\n' "$n"
}

running_pids() { # $1 executable path — read only, never signalled
  local out
  out="$(pgrep -f "$1" 2>/dev/null)" || { printf '\n'; return 0; }
  printf '%s\n' "${out//$'\n'/,}"
}

probe() { # $1 app
  local app sig exe bid helper hout
  app="$(resolve_dir "$1")" || die "no such app bundle: $1"
  [ -f "$app/Contents/Info.plist" ] || die "not an app bundle (no Contents/Info.plist): $1"
  bid="$(plist_value "$app" CFBundleIdentifier)" || bid="unknown"
  exe="$(plist_value "$app" CFBundleExecutable)" || exe=""

  printf 'app_path=%s\n' "$app"
  printf 'bundle_id=%s\n' "$bid"
  printf 'version=%s\n' "$(plist_value "$app" CFBundleShortVersionString || printf 'unknown')"

  sig="$(codesign -dvvv "$app" 2>&1)" || sig=""
  printf 'team_id=%s\n' "$(awk -F= '/^TeamIdentifier=/ { print $2; exit }' <<<"$sig")"
  printf 'authority_leaf=%s\n' "$(awk -F= '/^Authority=/ { print $2; exit }' <<<"$sig")"
  printf 'cdhash=%s\n' "$(awk -F= '/^CDHash=/ { print $2; exit }' <<<"$sig")"
  printf 'designated_requirement=%s\n' "$(designated_requirement "$app")"

  printf 'same_id_bundles=%s\n' "$(same_id_bundles "$bid" "$app")"
  lsregister_facts "$bid"
  if [ -n "$exe" ]; then
    printf 'running_pids=%s\n' "$(running_pids "$app/Contents/MacOS/$exe")"
  else
    printf 'running_pids=\n'
  fi

  printf '# helper values describe the QA helper, NOT Yap (an ad-hoc-signed binary; macOS answers for the terminal that launched it)\n'
  if helper="$(build_helper perm-helper)" && hout="$("$helper" 2>/dev/null)"; then
    printf 'helper_ax_trusted=%s\n' "$(awk -F= '/^ax_trusted=/ { print $2; exit }' <<<"$hout")"
    printf 'helper_listen_event=%s\n' "$(awk -F= '/^listen_event=/ { print $2; exit }' <<<"$hout")"
  else
    printf 'helper_ax_trusted=unavailable\n'
    printf 'helper_listen_event=unavailable\n'
  fi
}

compare() { # $1 installed app, $2 new app
  local old new dr_old dr_new
  old="$(resolve_dir "$1")" || die "no such app bundle: $1"
  new="$(resolve_dir "$2")" || die "no such app bundle: $2"
  dr_old="$(designated_requirement "$old")"
  dr_new="$(designated_requirement "$new")"
  printf 'app_path=%s\n' "$old"
  printf 'compare_path=%s\n' "$new"
  printf 'designated_requirement=%s\n' "$dr_old"
  printf 'compare_designated_requirement=%s\n' "$dr_new"
  if [ "$dr_old" = "$dr_new" ]; then
    printf 'dr_change=no\n'
  else
    printf 'dr_change=yes\n'
    printf '%s\n' "$DR_WARNING"
  fi
}

self_test() {
  local out rc fails=0 key
  out="$(mktemp -t yapqa)" || exit 1

  bash "$0" --app "$SELF_TEST_APP" >"$out" 2>&1
  rc=$?
  [ "$rc" -eq 0 ] || { printf 'FAIL probe exited %d\n' "$rc"; fails=$((fails + 1)); }
  for key in app_path bundle_id version team_id authority_leaf cdhash designated_requirement \
    same_id_bundles lsregister_count running_pids helper_ax_trusted helper_listen_event; do
    if grep -q "^$key=" "$out"; then printf 'ok   key %s\n' "$key"; else printf 'FAIL key %s missing\n' "$key"; fails=$((fails + 1)); fi
  done
  grep -q '^bundle_id=com.apple.calculator$' "$out" || { printf 'FAIL bundle_id is not com.apple.calculator\n'; fails=$((fails + 1)); }
  grep -q '^cdhash=[0-9a-f]\{40\}$' "$out" || { printf 'FAIL cdhash is not a 40-hex cdhash\n'; fails=$((fails + 1)); }
  grep -q '^lsregister_count=[1-9]' "$out" || { printf 'FAIL Calculator has no LaunchServices registration\n'; fails=$((fails + 1)); }
  grep -q '^# helper values describe the QA helper, NOT Yap' "$out" || { printf 'FAIL helper label line missing\n'; fails=$((fails + 1)); }
  grep -q '^helper_ax_trusted=\(true\|false\)$' "$out" || { printf 'FAIL helper did not answer ax_trusted\n'; fails=$((fails + 1)); }

  bash "$0" --app "$SELF_TEST_APP" --compare "$SELF_TEST_APP" >"$out" 2>&1
  rc=$?
  if [ "$rc" -eq 0 ] && grep -q '^dr_change=no$' "$out" && ! grep -q '^warning:' "$out"; then
    printf 'ok   compare same app -> dr_change=no\n'
  else
    printf 'FAIL compare same app (rc=%d)\n' "$rc"; fails=$((fails + 1))
  fi

  bash "$0" --app "$SELF_TEST_APP" --compare "$SELF_TEST_OTHER_APP" >"$out" 2>&1
  rc=$?
  if [ "$rc" -eq 0 ] && grep -q '^dr_change=yes$' "$out" && grep -qF "$DR_WARNING" "$out"; then
    printf 'ok   compare different app -> dr_change=yes + warning\n'
  else
    printf 'FAIL compare different app (rc=%d)\n' "$rc"; fails=$((fails + 1))
  fi

  bash "$0" --app /nonexistent/Yap.app >"$out" 2>&1
  rc=$?
  if [ "$rc" -eq 2 ]; then printf 'ok   missing app -> exit 2\n'; else printf 'FAIL missing app exited %d\n' "$rc"; fails=$((fails + 1)); fi

  rm -f "$out"
  if [ "$fails" -ne 0 ]; then
    printf 'self-test: %d failure(s)\n' "$fails"
    exit 1
  fi
  printf 'self-test: pass\n'
  exit 0
}

APP=/Applications/Yap.app
COMPARE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --app) [ $# -ge 2 ] || die "--app needs a path"; APP="$2"; shift 2 ;;
    --compare) [ $# -ge 2 ] || die "--compare needs a path"; COMPARE="$2"; shift 2 ;;
    --self-test) self_test ;;
    -h | --help) sed -n '2,20p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

if [ -n "$COMPARE" ]; then
  compare "$APP" "$COMPARE"
else
  probe "$APP"
fi
