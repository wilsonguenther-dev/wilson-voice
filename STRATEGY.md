# Wilson Voice — Strategy (updated 2026-07-18)

Honest status and the path to Wispr-class speed.
Canonical architecture decisions: **`ARCHITECTURE.md`** (do not re-open settled items).

## Did we finish phases 0–4?

**Scaffold: yes. Product polish: partial (v0.5 latency + insights hygiene shipped).**

| Phase | Claim | Reality |
|-------|--------|---------|
| 0 Hygiene | Single `/Applications` app | Done |
| 1 Permissions | Mic + AX for bundle id | Done (TCC still re-prompts after ad-hoc re-sign) |
| 2 Hotkeys | ⌘⇧V hold | Done |
| 3 Record + paste | Capture + clipboard/paste | Done (not Wispr-smooth) |
| 4 Product UI | Home / Insights / Dict / Scratch / Settings | Done as v1 shell |

**Not done (what you feel as “not properly built”):**

- Floating pill is a Tauri webview, **not** a real macOS `NSPanel` HUD (fullscreen broken; glitchy when chasing cursor — now parked off by default)
- STT is **batch** (record → stop → load model → full file) — not streaming partials
- Model process **died after every utterance** (cold import tax) until warm daemon
- No real fine-tune loop — only dictionary rewrite + vocab harvest from history
- No Developer ID / notarization — every rebuild resets TCC trust

Phases 0–4 got a working local dictation product. They did **not** get Wispr Flow.

---

## Why STT feels slow

Measured on this machine (warm disk, turbo model):

- First run after download: **~60s** (HF fetch + load) — one-time
- Subsequent one-shot process: **~1.6s** even for 3s of silence (Python + MLX import + model map every time)
- Under heavy coding: Metal/RAM pressure from browsers + agents + MLX competes

Cloud Codex / Claude **do not** use your Mac GPU. Local Whisper **does**. So the conflict is real if you also run local LLMs, video, or multiple heavy Metal apps — not because “the frontier models share your GPU” for cloud chats.

---

## AWS GPU: when yes / when no

| Approach | Latency feel | Cost | Privacy | Best for |
|----------|--------------|------|---------|----------|
| **Warm local MLX** (daemon + right model) | Sub-second → ~1s short clips | $0 | Full | Daily dictation |
| **Local small/base model** | Fastest local | $0 | Full | Heavy coding days |
| **Cloud STT** (Deepgram / Assembly / OpenAI) | Often 200–600ms | Per min | Leaves machine | Wispr-like if privacy OK |
| **Self-hosted AWS GPU** (EC2/SageMaker/RunPod) | Network RTT + queue + cold start | Always-on $ or cold 5–30s | Better than 3rd party if you own VPC | Batch / fine-tune jobs, not every keystroke |

**Recommendation: do not make AWS the primary STT path for hold-to-talk.**

For a 2–8 second utterance, network upload + GPU queue often **loses** to a warm local turbo/small model. AWS wins for:

1. Overnight fine-tune / LoRA jobs on big datasets  
2. Batch re-transcription of long audio  
3. Optional “quality burst” when you’re on a weak Mac  

Daily dictate while coding → **warm local + speed profiles** first. Optional cloud/AWS adapter later as a Settings backend.

---

## Best strategy moving forward (ordered)

### Shipped (v0.5.0) — local speed floor + honest metrics

1. **Warm ASR daemon** — load Whisper once, keep process alive, stdin JSON  
2. **Speed profiles** in Settings (Fast / Balanced / Max scales)  
3. **Preload on launch** via ModelHolder + temp=0 decode path  
4. **Dictionary learning** (jargon rewrite)  
5. **pipeline_ms** north-star metric (release → clipboard) + Insights p50/p95  
6. **speech_seconds** from WAV duration only — WPM never uses asr latency  
7. **source_app** + export + wav deleted post-ASR  

### Next — feel like Wispr

8. **Streaming / partials** while holding (chunked decode or VAD segments)  
9. **Real floating pill** via `tauri-nspanel` (non-activating, all Spaces, fullscreen auxiliary)  
10. **Developer ID + notarize** so Mic/AX survive updates  

### Later — “trains in background”

8. **Correction loop**: when user edits a transcript → store pair → grow personal corpus  
9. **MLX LoRA fine-tune offline** (nights / weekends) on that corpus + Drivia glossary  
10. **Optional remote worker**: RunPod/AWS only for fine-tune jobs or “Quality cloud” backend  

### Explicit non-goals right now

- Replacing Kokori (TTS ≠ STT)  
- Always-on expensive GPU box for every ⌘⇧V  
- Azure STT  

---

## Fine-tune MLX on this Mac — yes, but not for raw speed

Fine-tuning fixes **accents, product names, comma habits, jargon** (Drivia, JAX, Supabase). It does **not** make inference 10× faster.

Speed stack:

```
warm process + smaller model + (later) streaming  →  latency
dictionary + LoRA on corrections                 →  accuracy / personalization
optional cloud/AWS                               →  overflow / train / long audio
```

Keep fine-tuning **local and offline**. Run big jobs when you’re not mid-session. Don’t fight Claude for RAM during a shipping sprint.

---

## Pill honesty

Current pill = second webview, optional, off by default. That is not a production HUD.

Proper pill = `NSPanel` + non-activating + join all spaces + fullscreen auxiliary. That’s a dedicated engineering slice, not a CSS tweak.

---

## North star metric

| Metric | Target |
|--------|--------|
| Time from release hotkey → text on clipboard | **&lt; 800ms** median short utterance (local fast) |
| Mic / Desktop system dialogs mid-session | **0** after one Allow |
| Paste into focused field | Clipboard always; paste when AX + text focus |
| Personal jargon | Dictionary hits grow without user retyping |

If we miss the 800ms local target after warm daemon + Fast profile, then evaluate Deepgram (not raw EC2) as backend B — still simpler than “build GPU infra on AWS.”
