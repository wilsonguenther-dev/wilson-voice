#!/usr/bin/env bash
# SP-10 (yap-1106-sp-10-ane-engine-spike) — the SP-03 bench on BOTH engines.
#
# Runs `wilson-voice --bench bench-240s.wav bench-240s.wav` (4 min fixture;
# the second take is the warm one) once with the default GGUF/Metal helper
# and twice with the Neural Engine helper (YAP_ASR_ENGINE=ane): the first ANE
# run's load includes Core ML's on-device compile on a Mac that has never
# loaded it, the second is the cached load. For each run it records warm RTF,
# WER, load time, the helper's peak RSS (ps sampling) and mean power from
# powermetrics (needs passwordless sudo; without it power is reported as not
# measured and the rule cannot pass). Prints one JSON report and the SP-10
# verdict; --markdown also prints the table for the PR.
#
#   scripts/qa/ane-bench.sh --ane-model-dir <parakeet-tdt-0.6b-v3-coreml dir> [--bin <wilson-voice>] [--markdown]
#   scripts/qa/ane-bench.sh --self-test       # the decision rule on fixed numbers
#
# Decision rule (same constants as asr_engine.rs ane_verdict): adopt as the
# default only if WER is within 0.5 pt, warm RTF >= 100x, and GPU power
# (powermetrics "GPU Power") is lower by at least 30 %; package power
# ("Combined Power", CPU+GPU+ANE) must also drop by 30 %.
# Otherwise the ANE engine is kept as the governor's "constrained" engine.
# It never builds and never downloads a model.
# Exit: 0 report printed; 2 setup failure.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="${YAP_BENCH_BIN:-}"
ANE_DIR="${YAP_ANE_MODEL_DIR:-}"
MARKDOWN=0
SELF_TEST=0
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) BIN="${2:-}"; shift 2 ;;
    --ane-model-dir) ANE_DIR="${2:-}"; shift 2 ;;
    --markdown) MARKDOWN=1; shift ;;
    --self-test) SELF_TEST=1; shift ;;
    *) echo "ane-bench: unknown argument $1" >&2; exit 2 ;;
  esac
done

verdict() {
  # verdict <report.json> <markdown 0|1>
  python3 -I - "$@" <<'PY'
import json, sys
src, md = sys.argv[1], sys.argv[2] == "1"
r = json.load(open(src))
g, a = r["gguf"], r["ane"]
MAX_WER_PT, MIN_RTFX, MIN_DROP = 0.5, 100.0, 30.0
wer_pt = (a["wer"] - g["wer"]) * 100.0 if a.get("wer") is not None and g.get("wer") is not None else None
def pct_drop(key):
    if g.get(key) and g[key] > 0 and a.get(key) is not None:
        return (g[key] - a[key]) / g[key] * 100.0
    return None
drop = pct_drop("package_mw")
gpu_drop = pct_drop("gpu_mw")
checks = {
    "wer_within_0_5pt": wer_pt is not None and wer_pt <= MAX_WER_PT,
    "warm_rtfx_ge_100": (a.get("warm_rtfx") or 0) >= MIN_RTFX,
    "gpu_power_drop_ge_30pct": gpu_drop is not None and gpu_drop >= MIN_DROP,
    "package_power_drop_ge_30pct": drop is not None and drop >= MIN_DROP,
}
r["wer_delta_pt"] = wer_pt
r["gpu_power_drop_pct"] = gpu_drop
r["package_power_drop_pct"] = drop
r["checks"] = checks
r["verdict"] = "AdoptAsDefault" if all(checks.values()) else "ConstrainedOnly"
print(json.dumps(r, indent=1))
if md:
    def f(v, fmt):
        return "not measured" if v is None else fmt.format(v)
    print("\n| engine | warm RTF | WER | first load | cached load | peak RSS | package power | GPU power |")
    print("|---|---|---|---|---|---|---|---|")
    for name, e in (("GGUF / Metal (default)", g), ("Parakeet / Neural Engine", a)):
        print(f"| {name} | {f(e.get('warm_rtfx'), '{:.0f}x')} | {f(e.get('wer'), '{:.2%}')} | "
              f"{f(e.get('first_load_ms'), '{} ms')} | {f(e.get('cached_load_ms'), '{} ms')} | "
              f"{f(e.get('peak_rss_mb'), '{} MB')} | {f(e.get('package_mw'), '{:.0f} mW')} | {f(e.get('gpu_mw'), '{:.0f} mW')} |")
    print(f"\nWER delta {f(wer_pt, '{:+.2f} pt')}, GPU power drop {f(gpu_drop, '{:.1f} %')}, package power drop {f(drop, '{:.1f} %')}. "
          f"Verdict: {r['verdict']} ({', '.join(k for k, v in checks.items() if not v) or 'all bars met'}).")
PY
}

if [ "$SELF_TEST" = 1 ]; then
  tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
  printf '{"gguf":{"wer":0.05,"warm_rtfx":40,"package_mw":10000,"gpu_mw":4000},"ane":{"wer":0.054,"warm_rtfx":120,"package_mw":6500,"gpu_mw":400}}' >"$tmp/a.json"
  printf '{"gguf":{"wer":0.05,"warm_rtfx":40,"package_mw":10000,"gpu_mw":4000},"ane":{"wer":0.05,"warm_rtfx":150,"package_mw":7100,"gpu_mw":400}}' >"$tmp/b.json"
  printf '{"gguf":{"wer":0.05,"warm_rtfx":40,"package_mw":null,"gpu_mw":null},"ane":{"wer":0.05,"warm_rtfx":150,"package_mw":1,"gpu_mw":1}}' >"$tmp/c.json"
  a="$(verdict "$tmp/a.json" 0 | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["verdict"])')"
  b="$(verdict "$tmp/b.json" 0 | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["verdict"])')"
  c="$(verdict "$tmp/c.json" 0 | python3 -I -c 'import json,sys;print(json.load(sys.stdin)["verdict"])')"
  if [ "$a" = AdoptAsDefault ] && [ "$b" = ConstrainedOnly ] && [ "$c" = ConstrainedOnly ]; then
    echo "ane-bench self-test: ok"; exit 0
  fi
  echo "ane-bench self-test: FAILED ($a / $b / $c)" >&2; exit 1
fi

if [ -z "$BIN" ]; then
  for cand in "$ROOT/desktop/target/release/wilson-voice" "$ROOT/desktop/target/debug/wilson-voice"; do
    if [ -x "$cand" ] && { [ -z "$BIN" ] || [ "$cand" -nt "$BIN" ]; }; then BIN="$cand"; fi
  done
fi
[ -n "$BIN" ] && [ -x "$BIN" ] || { echo "ane-bench: no wilson-voice binary (--bin)" >&2; exit 2; }
[ -n "$ANE_DIR" ] && [ -d "$ANE_DIR" ] || { echo "ane-bench: --ane-model-dir must name the Core ML model folder" >&2; exit 2; }
[ -x "$(dirname "$BIN")/yap-engine-ane" ] || { echo "ane-bench: yap-engine-ane is not staged next to $BIN (scripts/build-ane-engine.sh, then copy it there)" >&2; exit 2; }
"$ROOT/scripts/eval/make-bench-fixtures.sh" || { echo "ane-bench: fixtures unavailable" >&2; exit 2; }
F240="$ROOT/scripts/eval/bench/bench-240s.wav"

tmp="$(mktemp -d)"
PIDS=()
cleanup() {
  for p in "${PIDS[@]:-}"; do
    [ -n "$p" ] && ps -p "$p" -o command= >/dev/null 2>&1 && kill "$p" 2>/dev/null
  done
  rm -rf "$tmp"
}
trap cleanup EXIT
HAVE_SUDO=0
sudo -n true 2>/dev/null && HAVE_SUDO=1

# run_one <label> <engine> : writes $tmp/<label>.{json,pm,rss}
run_one() {
  local label="$1" engine="$2" helper pm_pid="" rss_pid
  helper="yap-asr"; [ "$engine" = ane ] && helper="yap-engine-ane"
  : >"$tmp/$label.rss"
  ( while :; do ps -axo rss=,comm= | awk -v h="$helper" '$2 ~ h"$" {print $1}' >>"$tmp/$label.rss"; sleep 0.5; done ) &
  rss_pid=$!; PIDS+=("$rss_pid")
  if [ "$HAVE_SUDO" = 1 ]; then
    sudo -n powermetrics --samplers cpu_power,gpu_power,ane_power -i 500 -o "$tmp/$label.pm" >/dev/null 2>&1 &
    pm_pid=$!; PIDS+=("$pm_pid")
  fi
  YAP_ASR_ENGINE="$engine" YAP_ANE_MODEL_DIR="$ANE_DIR" "$BIN" --bench "$F240" "$F240" \
    >"$tmp/$label.json" 2>"$tmp/$label.err"
  local rc=$?
  [ -n "$pm_pid" ] && sudo -n kill -INT "$pm_pid" 2>/dev/null; [ -n "$pm_pid" ] && wait "$pm_pid" 2>/dev/null
  kill "$rss_pid" 2>/dev/null; wait "$rss_pid" 2>/dev/null
  if [ "$rc" != 0 ]; then
    echo "ane-bench: $label bench exited $rc" >&2; tail -5 "$tmp/$label.err" >&2; return 2
  fi
}

run_one gguf gguf || exit 2
run_one ane_first ane || exit 2
run_one ane ane || exit 2

python3 -I - "$tmp" "$HAVE_SUDO" >"$tmp/report.json" <<'PY'
import json, re, sys, os
tmp, have_sudo = sys.argv[1], sys.argv[2] == "1"
def power(label):
    p = os.path.join(tmp, label + ".pm")
    if not have_sudo or not os.path.exists(p):
        return None, None
    txt = open(p, errors="replace").read()
    def mean(pat):
        v = [float(x) for x in re.findall(pat, txt)]
        return sum(v) / len(v) if v else None
    return mean(r"Combined Power \(CPU \+ GPU \+ ANE\):\s*([\d.]+)\s*mW"), mean(r"GPU Power:\s*([\d.]+)\s*mW")
def rss(label):
    v = [int(x) for x in open(os.path.join(tmp, label + ".rss")).read().split() if x.isdigit()]
    return max(v) // 1024 if v else None
def one(label):
    b = json.load(open(os.path.join(tmp, label + ".json")))
    takes = [t for t in b["takes"] if "error" not in t]
    warm = takes[-1] if takes else {}
    rtf = b.get("rtf_last") or 0.0
    pkg, gpu = power(label)
    return {"engine": b.get("engine"), "model": b.get("model"), "wer": warm.get("wer"),
            "warm_rtfx": (1.0 / rtf) if rtf > 0 else None, "warm_decode_ms": warm.get("asr_decode_ms"),
            "cached_load_ms": b.get("cold_load_ms"), "peak_rss_mb": rss(label),
            "package_mw": pkg, "gpu_mw": gpu}
g, first, a = one("gguf"), one("ane_first"), one("ane")
for e, want in ((g, "gguf"), (first, "ane"), (a, "ane")):
    if e["engine"] != want:
        sys.exit(f"ane-bench: a run timed engine {e['engine']!r}, expected {want!r}")
g["first_load_ms"] = g["cached_load_ms"]
a["first_load_ms"] = first["cached_load_ms"]
a["ane_compile_ms_est"] = (first["cached_load_ms"] or 0) - (a["cached_load_ms"] or 0)
print(json.dumps({"bench": "sp-10", "fixture": "bench-240s.wav", "power_measured": have_sudo, "gguf": g, "ane": a}))
PY
rc=$?
[ "$rc" = 0 ] || exit 2
verdict "$tmp/report.json" "$MARKDOWN"
