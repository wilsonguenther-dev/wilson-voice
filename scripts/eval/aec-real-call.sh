#!/usr/bin/env bash
# yap24-NT11 — the AEC3 pre-pass on the one real two-track call we kept
# (c6187806), scored against its cloud reference. NUMBERS ONLY: no transcript
# text, no speaker names, no full paths.
#
#   YAP_AEC_REAL_DIR        dir holding <x>.t0.wav and <x>.t1.wav
#                           (default ~/yap-eval-corpus/private/call-c6187806)
#   YAP_AEC_REAL_REFERENCE  the reference transcript ("M:SS - Speaker" blocks)
#                           (default <dir>/reference.txt)
#   YAP_EVAL_REQUIRE=1      a missing WAV or reference FAILS instead of skipping
#   YAP_AEC_WINDOW_SECONDS  decode window (default 30)
#
# Everything runs in a mktemp dir from copies; the app's data dir is never
# opened. The ASR model is READ from the installed models dir through a
# symlink inside the temp YAP_DATA_DIR so nothing is downloaded.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
src_dir="${YAP_AEC_REAL_DIR:-$HOME/yap-eval-corpus/private/call-c6187806}"
ref="${YAP_AEC_REAL_REFERENCE:-$src_dir/reference.txt}"
win="${YAP_AEC_WINDOW_SECONDS:-30}"
need() {
  if [[ "${YAP_EVAL_REQUIRE:-0}" == "1" ]]; then echo "FAIL missing=$1"; exit 1; fi
  echo "SKIP missing=$1"; exit 0
}
t0="$(ls "$src_dir"/*.t0.wav 2>/dev/null | grep -v '\.aec\.wav$' | head -1 || true)"
t1="$(ls "$src_dir"/*.t1.wav 2>/dev/null | head -1 || true)"
[[ -n "$t0" && -f "$t0" ]] || need mic_wav
[[ -n "$t1" && -f "$t1" ]] || need system_wav
[[ -f "$ref" ]] || need reference

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cp "$t0" "$work/call.t0.wav"; cp "$t1" "$work/call.t1.wav"; cp "$ref" "$work/reference.txt"

export CARGO_INCREMENTAL=0
target="${CARGO_TARGET_DIR:-$repo/desktop/target}"
( cd "$repo/desktop/src-tauri" && cargo build --release --features custom-protocol --bin wilson-voice >"$work/build.log" 2>&1 ) \
  || { echo "FAIL build (log tail follows)"; tail -20 "$work/build.log"; exit 1; }
bin="$target/release/wilson-voice"

# A private data dir for the binary whose models dir points at the installed one.
data="$work/yap-state"; mkdir -p "$data"
models_src="${YAP_AEC_MODELS_DIR:-$HOME/Library/Application Support/WilsonVoice/models}"
[[ -d "$models_src" ]] && ln -s "$models_src" "$data/models"
export YAP_DATA_DIR="$data"

# (e) acoustic guard, no ASR.
/usr/bin/time -l "$bin" --aec-prepass "$work/call.t0.wav" "$work/call.t1.wav" "$work/call.t0.aec.wav" \
  >"$work/aec.json" 2>"$work/aec.err" || { echo "FAIL aec_prepass"; grep -v '^\s*$' "$work/aec.err" | tail -3; exit 1; }
rss=$(awk '/maximum resident set size/ {print $1}' "$work/aec.err")
python3 -I - "$work/aec.json" "$rss" <<'PY'
import json, sys
d = json.load(open(sys.argv[1])); r = d["report"]
print(f"aec_lag_ms={d['coupling_lag_ms']}")
for k in ("seconds","wall_ms","call_windows","local_windows","call_atten_p25_db","call_atten_p50_db","call_atten_p90_db","local_loss_p50_db","local_loss_p90_db"):
    print(f"aec_{k}={r[k]:.2f}" if isinstance(r[k], float) else f"aec_{k}={r[k]}")
print(f"aec_peak_rss_mb={int(sys.argv[2])/1048576:.1f}")
PY

# Windowed decode: each track cut into ${win}s windows, decoded with --timed,
# shifted onto the meeting clock and merged into one JSON per track.
python3 -I - "$bin" "$work" "$win" <<'PY'
import json, subprocess, sys, wave, os
binp, work, win = sys.argv[1], sys.argv[2], float(sys.argv[3])
def decode(src, out):
    w = wave.open(src, "rb"); rate = w.getframerate(); n = w.getnframes(); per = int(win * rate)
    merged = {"text": "", "segments": [], "words": []}; i = 0; fails = 0
    while i * per < n:
        w.setpos(i * per); frames = w.readframes(min(per, n - i * per))
        chunk = os.path.join(work, "chunk.wav")
        with wave.open(chunk, "wb") as c:
            c.setnchannels(1); c.setsampwidth(w.getsampwidth()); c.setframerate(rate); c.writeframes(frames)
        p = subprocess.run([binp, "--transcribe-file", chunk, "--timed"], capture_output=True, text=True)
        off = i * win
        if p.returncode == 0 and p.stdout.strip():
            t = json.loads(p.stdout)
            for key in ("segments", "words"):
                for s in t.get(key, []):
                    for k in list(s):
                        # --timed prints camelCase (startSeconds/endSeconds,
                        # asr_engine::TimedSpan); every spelling is shifted, or
                        # every window lands on 0-30 s and the merge interleaves.
                        if k in ("start", "end", "start_seconds", "end_seconds", "startSeconds", "endSeconds", "t0", "t1"):
                            s[k] = s[k] + off
                        elif k in ("start_ms", "end_ms"):
                            s[k] = s[k] + off * 1000
                    merged[key].append(s)
            merged["text"] += " " + t.get("text", "")
        else:
            fails += 1
        i += 1
    json.dump(merged, open(out, "w"))
    return i, fails
for name, src in (("t0", "call.t0.aec.wav"), ("t1", "call.t1.wav")):
    n, f = decode(os.path.join(work, src), os.path.join(work, name + ".json"))
    print(f"{name}_windows={n} {name}_empty_windows={f}")
PY

python3 -I "$here/compare-reference.py" --tracks "$work/t0.json" "$work/t1.json" "$work/reference.txt" | tee "$work/scores.txt"

# INFORMATIONAL, never a gate (fix round 2026-10-27): the numbers a human needs
# to read gates (b)-D and (d) honestly. Printed as info_* lines; the gate block
# below reads scores.txt only and is unchanged.
#  - info_rendered_wer_in_reference_span: rendered WER with the hypothesis cut
#    to the span the cloud reference covers (it starts offset_seconds into the
#    audio, so every word decoded before that is scored as an insertion).
#  - D words split by what the mic physically heard in each reference block:
#    "playback" = the AEC removed >= 10 dB there, i.e. the block's audio reached
#    the mic through the speakers (it is in the system tap). D's Me share is
#    also given over the remaining blocks only, with its ceiling.
python3 -I - "$here/compare-reference.py" "$work" <<'PY'
import importlib.util, math, sys, wave
from array import array
spec = importlib.util.spec_from_file_location("cr", sys.argv[1])
cr = importlib.util.module_from_spec(spec); spec.loader.exec_module(cr)
work = sys.argv[2]
blocks, speakers = cr.parse_reference(open(work + "/reference.txt", encoding="utf-8").read())
segs = {k: cr.timed_json_segments(f"{work}/t{k}.json") for k in (0, 1)}
ref = [w for _, _, ws in blocks for w in ws]
ref_who = [who for _, who, ws in blocks for _ in ws]
ref_blk = [b for b, (_, _, ws) in enumerate(blocks) for _ in ws]
offset = 0.0
for line in open(work + "/scores.txt"):
    if line.startswith("offset_seconds="):
        try:
            offset = float(line.split("=", 1)[1])
        except ValueError:
            pass
if math.isnan(offset):
    offset = 0.0
lo = blocks[0][0] + offset if blocks else 0.0
hi = (blocks[-1][0] + offset + 30.0) if blocks else 0.0
clipped = {k: [s for s in segs[k] if s[1] > lo and s[0] < hi] for k in segs}
merged = sorted((p for k in clipped for p in cr.timed_words(clipped[k])), key=lambda p: p[1])
c = cr.counts(cr.align(ref, [w for w, _ in merged]))
print(f"info_rendered_wer_in_reference_span={cr.wer(c, len(ref)):.4f}")
def energies(path):
    w = wave.open(path, "rb"); rate = w.getframerate(); out = []
    while True:
        raw = w.readframes(rate)
        if not raw:
            break
        a = array("h"); a.frombytes(raw)
        out.append(sum(x * x for x in a) + 1.0)
    return out
e_mic = energies(work + "/call.t0.wav"); e_aec = energies(work + "/call.t0.aec.wav")
playback = set()
for b, (start, _, _) in enumerate(blocks):
    end = blocks[b + 1][0] if b + 1 < len(blocks) else start + 20.0
    s0 = int(start + offset); s1 = max(s0 + 1, int(min(end, start + 20.0) + offset))
    m = sum(e_mic[s0:s1]); a = sum(e_aec[s0:s1])
    if m and a and 10.0 * math.log10(m / a) >= 10.0:
        playback.add(b)
found = {i for op, i, _ in cr.align(ref, [w for w, _ in cr.timed_words(segs[0])]) if op == cr.MATCH}
d_all = [i for i, who in enumerate(ref_who) if who == "D"]
d_loc = [i for i in d_all if ref_blk[i] not in playback]
nan = float("nan")
print(f"info_D_words={len(d_all)}")
print(f"info_D_words_in_playback_blocks={len(d_all) - len(d_loc)}")
print(f"info_share_t0_D_outside_playback_blocks={(sum(1 for i in d_loc if i in found) / len(d_loc)) if d_loc else nan:.4f}")
print(f"info_share_t0_D_ceiling_if_every_local_word_found={(len(d_loc) / len(d_all)) if d_all else nan:.4f}")
PY

# Gates (a)-(e). Speaker letters are reference order; the owner is B, local D, remote A and C.
python3 -I - "$work/scores.txt" "$work/aec.json" <<'PY'
import json, sys
s = {}
for line in open(sys.argv[1]):
    if "=" in line:
        k, v = line.strip().split("=", 1)
        try: s[k] = float(v)
        except ValueError: pass
r = json.load(open(sys.argv[2]))["report"]
g = {
  "a_echo_dup_rate<=0.10": s.get("echo_rate", 1) <= 0.10,
  "b_owner_B_in_me>=0.60": s.get("share_t0_B", 0) >= 0.60,
  "b_local_D_in_me>=0.70": s.get("share_t0_D", 0) >= 0.70,
  "c_remote_A_in_me<=0.10": s.get("share_t0_A", 1) <= 0.10,
  "c_remote_C_in_me<=0.10": s.get("share_t0_C", 1) <= 0.10,
  "d_rendered_wer<=0.25": s.get("rendered_wer", 1) <= 0.25,
  "e_local_loss_p90<=1.0dB": r["local_loss_p90_db"] <= 1.0,
  "e_call_atten_p50>=15dB": r["call_atten_p50_db"] >= 15.0,
}
bad = 0
for k, ok in g.items():
    print(f"gate_{k}={'pass' if ok else 'FAIL'}"); bad += (not ok)
sys.exit(1 if bad else 0)
PY
