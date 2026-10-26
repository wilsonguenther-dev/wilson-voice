#!/usr/bin/env bash
# TC-1 — `test-build.sh --dry-run` resolves the head and prints the config
# overlay without building or installing anything. Offline (--no-fetch).
set -euo pipefail
export GH_PROMPT_DISABLED=1 GH_PAGER=cat GIT_PAGER=cat
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
S="$ROOT/scripts/test-build.sh"
fail() { echo "FAIL: $*" >&2; exit 1; }
has() { grep -qF -- "$2" <<<"$1" || fail "expected '$2' in output of $3"; }

# main → origin/main
out="$(bash "$S" --dry-run --no-fetch main)" || fail "--dry-run main exited non-zero"
want="$(git -C "$ROOT" rev-parse origin/main)"
has "$out" "test-build: head      $want (main)" "main"
has "$out" "TEST BUILD ${want:0:7} · main" "main"
has "$out" '"identifier": "com.wilsonguenther.wilson-voice.test"' "main"
has "$out" '"productName": "Yap Test"' "main"
has "$out" '"createUpdaterArtifacts": false' "main"
has "$out" '"endpoints": []' "main"
has "$out" "YAP_CHANNEL=test" "main"
has "$out" "/Yap Test.app" "main"
has "$out" "WilsonVoice-Test" "main"
has "$out" "dry run — nothing built or installed" "main"

# --sha HEAD → this checkout's head; it carries channel support, so no graft
out="$(bash "$S" --dry-run --no-fetch --sha HEAD)" || fail "--dry-run --sha HEAD exited non-zero"
has "$out" "test-build: head      $(git -C "$ROOT" rev-parse HEAD) " "--sha HEAD"
has "$out" "graft     no" "--sha HEAD"

# the production id must never appear as the overlay identifier
grep -q '"identifier": "com.wilsonguenther.wilson-voice"' <<<"$out" && fail "overlay carries the production id"

# bad input fails
bash "$S" --dry-run --no-fetch --sha deadbeefdeadbeef >/dev/null 2>&1 && fail "an unknown sha must fail"
bash "$S" --dry-run --no-fetch --bogus >/dev/null 2>&1 && fail "an unknown flag must fail"
echo "test-build.test.sh: OK"
