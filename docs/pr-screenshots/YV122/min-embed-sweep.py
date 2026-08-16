#!/usr/bin/env python3
"""YV122 — the min_embed_seconds truncation-stability sweep, reproducible.

    python3 docs/pr-screenshots/YV122/min-embed-sweep.py desktop/target/release/yap-diarize

Needs the two catalog models at ~/yap-diarize-models (YV123 installs them) and
macOS `say`/`afconvert`. Drives the SHIPPED sidecar over its real protocol --
nothing here reimplements the embedding path.

Every request passes min_embed_seconds=0.0 on purpose: the point is to measure
the UNGATED behaviour, which is what made the old empty-vector gate meaningless.
"""
import json, subprocess, os, sys, struct, math, wave


BIN = sys.argv[1]
SEG = os.path.expanduser("~/yap-diarize-models/sherpa-onnx-pyannote-segmentation-3-0/model.onnx")
EMB = os.path.expanduser("~/yap-diarize-models/wespeaker_en_voxceleb_CAM++.onnx")

class Side:
    def __init__(self):
        self.p = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)
        ready = json.loads(self.p.stdout.readline())
        assert ready.get("type") == "ready", ready
        self.id = 0
    def ask(self, **kw):
        self.id += 1
        req = dict(id=self.id, **kw)
        self.p.stdin.write(json.dumps(req) + "\n"); self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())
    def close(self):
        self.p.stdin.close(); self.p.wait()

def say(dirp, voice, tag, text, rate=16000):
    os.makedirs(dirp, exist_ok=True)
    aiff = os.path.join(dirp, f"{voice}-{tag}.aiff"); wav = os.path.join(dirp, f"{voice}-{tag}.wav")
    for f in (aiff, wav):
        if os.path.exists(f): os.remove(f)
    subprocess.run(["say","-v",voice,"-r","170","-o",aiff,text], check=True)
    subprocess.run(["afconvert","-f","WAVE","-d",f"LEI16@{rate}","-c","1",aiff,wav], check=True)
    return wav

def read_pcm(path):
    w = wave.open(path,'rb'); n=w.getnframes(); rate=w.getframerate()
    data=w.readframes(n); w.close()
    return list(struct.unpack("<%dh"%(len(data)//2), data)), rate

def write_pcm(path, samples, rate=16000):
    w = wave.open(path,'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
    w.writeframes(struct.pack("<%dh"%len(samples), *samples)); w.close()
    return path

def cos(a,b):
    d=sum(x*y for x,y in zip(a,b)); na=math.sqrt(sum(x*x for x in a)); nb=math.sqrt(sum(x*x for x in b))
    return d/(na*nb) if na and nb else 0.0


D = "/tmp/yv122meas/audio"
VOICES = ["Samantha","Daniel","Karen","Rishi","Fred","Ralph"]
UTTS = [
 "The quarterly numbers came in this morning and the growth line finally crossed the mark we set in January.",
 "I would still like to see the churn breakdown before we commit to any hiring plan for the next two quarters.",
 "Let me pull up the dashboard and walk through the retention curve with everyone who is on this call today.",
]
GRID = [0.10,0.20,0.30,0.40,0.50,0.75,1.00,1.25,1.50,1.75,2.00,2.50,3.00]
RATE=16000

s = Side()
r = s.ask(kind="load_models", segmentation_path=SEG, embedding_path=EMB)
assert r["ok"], r
print("embedding_dim =", r["embedding_dim"])
print("min_embed_seconds = 0.0 on every request below: the sweep MEASURES the ungated behaviour that made the old empty-vector gate meaningless.")

full = {}
pcms = {}
for v in VOICES:
    for i,t in enumerate(UTTS):
        w = say(D, v, str(i), t)
        pcm, rate = read_pcm(w); assert rate==RATE
        pcms[(v,i)] = pcm
        resp = s.ask(kind="embed", wav_path=w, min_embed_seconds=0.0)
        assert resp["ok"], resp
        full[(v,i)] = resp["embedding"]
        print(f"  full {v}-{i}: {len(pcm)/RATE:.2f}s dim={len(resp['embedding'])}")

# impostor mean over full-utterance pairs (the PR's published population)
imp=[]; gen=[]
keys=list(full)
for a in range(len(keys)):
    for b in range(a+1,len(keys)):
        ka,kb=keys[a],keys[b]
        c=cos(full[ka],full[kb])
        (gen if ka[0]==kb[0] else imp).append(c)
imp_mean=sum(imp)/len(imp); gen_mean=sum(gen)/len(gen)
print(f"\nfull-utterance population: genuine n={len(gen)} mean {gen_mean:.4f} min {min(gen):.4f} | impostor n={len(imp)} mean {imp_mean:.4f} max {max(imp):.4f}")

print("\n=== truncation-stability sweep: centered window of T seconds vs the SAME utterance's full-length vector ===")
print(f"{'T(s)':>6} {'n':>4} {'min':>8} {'mean':>8} {'max':>8}  {'#below impostor mean':>22}")
rows={}
sid=0
for T in GRID:
    want=int(T*RATE); scores=[]
    for (v,i),pcm in pcms.items():
        if len(pcm) < want: continue
        mid=len(pcm)//2; lo=max(0,mid-want//2); hi=lo+want
        p=f"{D}/win-{v}-{i}-{int(T*1000)}.wav"
        write_pcm(p, pcm[lo:hi], RATE)
        resp=s.ask(kind="embed", wav_path=p, min_embed_seconds=0.0)
        if not resp.get("ok"):
            scores.append(("REFUSED",resp.get("err"))); continue
        e=resp.get("embedding") or []
        if not e:
            scores.append(("EMPTY",None)); continue
        scores.append(cos(e, full[(v,i)]))
    numeric=[x for x in scores if isinstance(x,float)]
    nonnum=[x for x in scores if not isinstance(x,float)]
    below=sum(1 for x in numeric if x < imp_mean)
    rows[T]=(numeric,nonnum,below)
    if numeric:
        print(f"{T:>6.2f} {len(numeric):>4} {min(numeric):>8.4f} {sum(numeric)/len(numeric):>8.4f} {max(numeric):>8.4f}  {below:>22}  nonnumeric={len(nonnum)}")
    else:
        print(f"{T:>6.2f} {0:>4} {'-':>8} {'-':>8} {'-':>8}  nonnumeric={len(nonnum)} {nonnum[:2]}")

print(f"\nimpostor mean (bar) = {imp_mean:.4f}")
for T in GRID:
    numeric,nonnum,below=rows[T]
    ok = numeric and min(numeric) > imp_mean and not nonnum
    print(f"  T={T:.2f}  min_self={min(numeric) if numeric else float('nan'):.4f}  clears={'YES' if ok else 'no'}")
json.dump({str(T):[rows[T][0]] for T in GRID}, open("/tmp/yv122meas/sweep.json","w"))
s.close()
