#!/usr/bin/env bash
# SP-03 — the speed bench. Runs `wilson-voice --bench` (the shipped release-side
# pipeline: DSP + VAD, warm decode, cleanup, no paste) over the three standard
# fixtures, writes ONE dated row to docs/BUDGETS.md (`speed_bench`), and FAILS
# when release→text on the 240 s fixture is more than 20 % slower than the last
# accepted row from the same machine and model.
#
#   scripts/qa/speed-bench.sh [--bin <wilson-voice>] [--note "<why>"] [--dry-run] [--keep-fixtures]
#   scripts/qa/speed-bench.sh --from-json <bench.json>   # score a saved run
#   scripts/qa/speed-bench.sh --self-test                # the gate on fake runs
#
# It never builds. The binary is --bin, $YAP_BENCH_BIN, or the newest of
# desktop/target/{release,debug}/wilson-voice. Fixtures come from
# scripts/eval/make-bench-fixtures.sh (made on first use, and deleted again at
# exit unless --keep-fixtures or they were already there).
#
# State: the bench runs against a SCRATCH data dir (a fresh temp YAP_DATA_DIR,
# deleted at exit) unless the caller sets YAP_DATA_DIR. On a scratch dir with no
# settings the binary benches the SHIPPED default model, which it looks up in
# YAP_MODEL_DIR (e.g. ~/code/wilson-voice-loop/cache/models) and never
# downloads — no installed model is exit 2 with one sentence.
#
# Exit: 0 = row written (or --dry-run), within budget; 1 = 240 s release→text
# regressed past the limit (the row is still written, marked REGRESSED, and is
# never used as the next comparison base); 2 = setup failure (no binary, no
# fixtures, bench crashed, malformed JSON).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUDGETS="${YAP_BENCH_BUDGETS:-$ROOT/docs/BUDGETS.md}"
MAX_PCT="${YAP_BENCH_MAX_REGRESSION_PCT:-20}"
BIN="${YAP_BENCH_BIN:-}"
NOTE=""
DRY=0
FROM_JSON=""
SELF_TEST=0
KEEP_FIXTURES=0

while [ $# -gt 0 ]; do
  case "$1" in
    --bin) BIN="${2:-}"; shift 2 ;;
    --note) NOTE="${2:-}"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --from-json) FROM_JSON="${2:-}"; shift 2 ;;
    --self-test) SELF_TEST=1; shift ;;
    --keep-fixtures) KEEP_FIXTURES=1; shift ;;
    *) echo "speed-bench: unknown argument $1" >&2; exit 2 ;;
  esac
done

# Row writer + regression gate. Standard library only, isolated interpreter.
#   score <bench.json> <budgets.md> <date> <commit> <machine> <note> <max_pct> <dry>
score() {
  python3 -I - "$@" <<'PY'
import json, re, sys

src, budgets, date, commit, machine, note, max_pct, dry = sys.argv[1:9]
max_pct, dry = float(max_pct), dry == "1"
START, END = "<!-- speed-bench:rows:start -->", "<!-- speed-bench:rows:end -->"

try:
    report = json.load(open(src))
    takes = {t["file"]: t for t in report["takes"]}
except Exception as e:  # malformed or missing output is a setup failure
    print(f"speed-bench: unreadable bench JSON ({e})", file=sys.stderr)
    sys.exit(2)

need = ["bench-20s.wav", "bench-127s.wav", "bench-240s.wav"]
for name in need:
    t = takes.get(name)
    if t is None or "error" in t or "release_text_ms" not in t:
        why = (t or {}).get("error", "missing")
        print(f"speed-bench: fixture {name} has no timing ({why})", file=sys.stderr)
        sys.exit(2)

def wer(t):
    w = t.get("wer")
    return "-" if w is None else f"{w:.3f}"

model = report.get("model", "?")
vad = report.get("vad", "?")
long = takes["bench-240s.wav"]
new_ms = int(long["release_text_ms"])

text = open(budgets).read()
if START not in text or END not in text:
    print(f"speed-bench: {budgets} has no speed_bench row markers", file=sys.stderr)
    sys.exit(2)
head, rest = text.split(START, 1)
rows_block, tail = rest.split(END, 1)

# Columns: date | commit | machine | model | vad | 20s | 127s | 240s | 240s decode
#          | rtf_p50 | WER 20/127/240 | status | note
base = None
for line in rows_block.splitlines():
    cells = [c.strip() for c in line.strip().strip("|").split("|")]
    if len(cells) < 13 or not re.match(r"^\d{4}-\d{2}-\d{2}T", cells[0]):
        continue
    if cells[2] != machine or cells[3] != model or cells[11].startswith("REGRESSED"):
        continue
    try:
        base = (int(cells[7]), cells[0], cells[1])
    except ValueError:
        continue

if base is None:
    status, failed = "baseline", False
else:
    pct = (new_ms - base[0]) * 100.0 / base[0] if base[0] > 0 else 0.0
    failed = pct > max_pct
    status = (f"REGRESSED {pct:+.1f}%" if failed else f"ok {pct:+.1f}%") + f" vs {base[2]}"

cell = lambda s: str(s).replace("|", "/").replace("\n", " ").strip() or "-"
row = "| " + " | ".join(cell(c) for c in [
    date, commit, machine, model, vad,
    takes["bench-20s.wav"]["release_text_ms"],
    takes["bench-127s.wav"]["release_text_ms"],
    new_ms,
    long["asr_decode_ms"],
    f"{float(report.get('rtf_p50', 0.0)):.3f}",
    "/".join(wer(takes[n]) for n in need),
    status,
    note,
]) + " |"
print(row)

if not dry:
    block = rows_block.rstrip("\n") + "\n" + row + "\n"
    open(budgets, "w").write(head + START + block + END + tail)

if failed:
    print(f"speed-bench: 240 s release→text {new_ms} ms is more than {max_pct:g}% over "
          f"{base[0]} ms ({base[1]}, {base[2]})", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
PY
}

if [ "$SELF_TEST" = 1 ]; then
  # Proves the gate on synthetic runs, against a scratch BUDGETS: a first row
  # is the baseline, +10 % passes, +30 % fails and is marked REGRESSED, and a
  # later +15 % is compared against the last ACCEPTED row, not the regressed one.
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  printf '# x\n\n<!-- speed-bench:rows:start -->\n| date | commit | machine | model | vad | 20 s | 127 s | 240 s | 240 s decode | rtf_p50 | WER | status | note |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|\n<!-- speed-bench:rows:end -->\n' >"$tmp/B.md"
  fake() {
    printf '{"model":"m","vad":"energy","rtf_p50":0.05,"takes":[{"file":"bench-20s.wav","release_text_ms":100,"asr_decode_ms":90,"wer":0.01},{"file":"bench-127s.wav","release_text_ms":600,"asr_decode_ms":550,"wer":0.02},{"file":"bench-240s.wav","release_text_ms":%s,"asr_decode_ms":%s,"wer":0.03}]}' "$1" "$1" >"$tmp/r.json"
  }
  expect() {
    local want="$1" ms="$2" got
    fake "$ms"
    score "$tmp/r.json" "$tmp/B.md" "2026-01-01T00:00Z" "c$ms" "mac" "" "$MAX_PCT" 0 >/dev/null 2>&1
    got=$?
    if [ "$got" != "$want" ]; then
      echo "speed-bench self-test FAILED: 240 s at ${ms} ms exited $got, wanted $want" >&2
      exit 1
    fi
  }
  expect 0 1000   # baseline
  expect 0 1100   # +10 %
  expect 1 1430   # +30 % vs 1100 → REGRESSED
  expect 0 1260   # +14.5 % vs 1100 (the regressed row is skipped)
  printf '{"takes":[{"file":"bench-20s.wav","error":"x"}]}' >"$tmp/r.json"
  score "$tmp/r.json" "$tmp/B.md" "2026-01-01T00:00Z" c "mac" "" "$MAX_PCT" 0 >/dev/null 2>&1
  [ $? = 2 ] || { echo "speed-bench self-test FAILED: a failed fixture must exit 2" >&2; exit 1; }
  rows=$(grep -c '^| 2026-01-01T' "$tmp/B.md")
  regressed=$(grep -c 'REGRESSED' "$tmp/B.md")
  if [ "$rows" != 4 ] || [ "$regressed" != 1 ]; then
    echo "speed-bench self-test FAILED: rows=$rows regressed=$regressed (want 4 / 1)" >&2
    exit 1
  fi
  echo "speed-bench self-test ok (baseline, +10% pass, +30% fail, regressed row skipped, error exits 2)"
  exit 0
fi

DATE="$(date -u +%Y-%m-%dT%H:%MZ)"
COMMIT="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
if [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no 2>/dev/null)" ]; then
  COMMIT="$COMMIT-dirty"
fi
CHIP="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
MEM_GB="$(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 ))"
MACHINE="$CHIP ${MEM_GB}GB"

if [ -n "$FROM_JSON" ]; then
  score "$FROM_JSON" "$BUDGETS" "$DATE" "$COMMIT" "$MACHINE" "$NOTE" "$MAX_PCT" "$DRY"
  exit $?
fi

if [ -z "$BIN" ]; then
  for cand in "$ROOT/desktop/target/release/wilson-voice" "$ROOT/desktop/target/debug/wilson-voice"; do
    if [ -x "$cand" ] && { [ -z "$BIN" ] || [ "$cand" -nt "$BIN" ]; }; then
      BIN="$cand"
    fi
  done
fi
if [ -z "$BIN" ] || [ ! -x "$BIN" ]; then
  echo "speed-bench: no wilson-voice binary (build it first, or pass --bin)" >&2
  exit 2
fi

FIX="$ROOT/scripts/eval/bench"
WAVS=("$FIX/bench-20s.wav" "$FIX/bench-127s.wav" "$FIX/bench-240s.wav")
# Fixtures this run generates are deleted at exit; ones already there are kept.
MADE=()
for w in "${WAVS[@]}"; do
  [ -e "$w" ] || MADE+=("$w")
done
tmp="$(mktemp -d)"
SCRATCH_DATA=""
cleanup() {
  rm -rf "$tmp"
  [ -n "$SCRATCH_DATA" ] && rm -rf "$SCRATCH_DATA"
  if [ "$KEEP_FIXTURES" != 1 ] && [ "${#MADE[@]}" -gt 0 ]; then
    rm -f "${MADE[@]}"
  fi
}
trap cleanup EXIT

"$ROOT/scripts/eval/make-bench-fixtures.sh"
rc=$?
if [ "$rc" != 0 ]; then
  echo "speed-bench: fixtures unavailable (make-bench-fixtures exit $rc)" >&2
  exit 2
fi

# Never the real data dir by default: a scratch root, removed at exit.
if [ -z "${YAP_DATA_DIR:-}" ]; then
  SCRATCH_DATA="$(mktemp -d)"
  export YAP_DATA_DIR="$SCRATCH_DATA/yap-bench-state"
  mkdir -p "$YAP_DATA_DIR"
fi
"$BIN" --bench "${WAVS[@]}" >"$tmp/bench.json" 2>"$tmp/bench.err"
rc=$?
if [ "$rc" != 0 ]; then
  echo "speed-bench: wilson-voice --bench exited $rc" >&2
  tail -5 "$tmp/bench.err" >&2
  if [ "$rc" = 2 ] && [ -z "${YAP_MODEL_DIR:-}" ]; then
    echo "speed-bench: set YAP_MODEL_DIR to a folder holding the shipped default model" >&2
  fi
  exit 2
fi
echo "speed-bench: $(basename "$BIN") on $MACHINE at $COMMIT"
score "$tmp/bench.json" "$BUDGETS" "$DATE" "$COMMIT" "$MACHINE" "$NOTE" "$MAX_PCT" "$DRY"
exit $?
