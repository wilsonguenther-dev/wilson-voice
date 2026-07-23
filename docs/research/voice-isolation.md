# Voice Isolation / Noise Suppression for Yap

Research doc — how to make Yap hear **only the user's voice** and reject typing,
music, other people, and fans, so MLX-Whisper transcription stays clean even
when the user speaks quietly.

Scope owner: capture → clean → transcribe. Written to drop straight into the
`scripts/cicd-loop.mjs` flow (each recommendation ends with a **testable
acceptance** and a **loopable vs prototype-first** tag).

---

## 0. What the pipeline actually is (and why it matters)

From `desktop/src-tauri/src/record.rs`:

- Capture is **in-process cpal 0.18** (`record_loop`), device-native rate +
  channels (Mac built-in mic is almost always **48 kHz**), any of F32/I16/U16.
- On stop: samples are **downmixed to mono**, then **`resample_linear(...)` to
  16 kHz**, then `write_wav_i16` → WAV → warm MLX-Whisper daemon.
- VAD today is **energy-only** (`voiced_seconds`): a 20 ms-frame RMS gate with a
  noise-floor percentile + gap-bridge. It is used **only** as the WPM
  denominator (`speech_seconds`) — it does **not** gate capture or clean audio.

Three consequences that shape every recommendation below:

1. **It's batch, not streaming.** We hold the whole utterance in a `Vec<f32>`
   and process on release. So **added algorithmic latency (10–40 ms) is
   irrelevant** — we can run even the heavier denoisers offline over the whole
   clip. The only budget that matters is total post-release wall time
   (`pipeline_ms`, north star p50 < 800 ms), and an RTF of ~0.2 on a 3 s clip is
   ~0.6 s — acceptable, and it overlaps with model warmup.
2. **The natural insertion point is BEFORE the 16 kHz downsample.** RNNoise and
   DeepFilterNet are **48 kHz** models, and our capture is already ~48 kHz.
   Denoise the native/48 k mono buffer, *then* downsample to 16 k for Whisper.
   Cheap DSP (high-pass, normalize) can run either at native rate or at 16 k.
3. **The CI loop rewards pure-Rust `#[test]`-able functions.** `cicd-loop.mjs`
   gates every item on `cargo test` + `tsc --noEmit` + greps. Anything that is
   **pure Rust with a deterministic unit test loops autonomously**. Anything
   that needs a **model asset or a native framework** (ONNX Runtime, Core Audio
   AudioUnit, a bundled C++ build) needs a **human-in-the-loop prototype first**,
   then can be locked with a fixture-based test.

**Reality check on Whisper:** large-v3 / turbo are already noise-robust and were
trained on messy audio. The highest-ROI wins for "quiet speaker + background
noise" are, in order: (a) don't let gain/gating kill the quiet voice
(normalize), (b) knock down steady background (high-pass + a denoiser), (c) trim
non-speech so Whisper doesn't hallucinate on silence/noise. Full target-speaker
separation is overkill for v1.

---

## 1. Apple's built-in Voice Isolation / Voice-Processing I/O

### What exists

- **Microphone Modes** (Standard / Voice Isolation / Wide Spectrum) — the OS-level
  feature in Control Center. It is **opt-in by the *app*, per audio session**;
  the user can only *pick* a mode for apps that have requested a mode-capable
  session. General availability: Macs 2018+, macOS Monterey+, works with
  third-party mics.
  ([Apple: microphone modes overview](https://support.apple.com/guide/mac-help/change-your-mic-mode-on-mac-mchlf5495d9d/mac),
  [Hollyland writeup](https://www.hollyland.com/blog/tips/voice-isolation-on-iphone))
- **The developer hook is the Voice-Processing I/O AudioUnit**
  (`kAudioUnitSubType_VoiceProcessingIO`, aka `AUVoiceProcessing`). Enabling a
  VPIO session is what lets the mic-mode panel offer Voice Isolation, and it also
  runs Apple's tuned **AEC + noise suppression + AGC** on the input stream
  regardless of the UI toggle.
  ([Apple dev forum 733733](https://developer.apple.com/forums/thread/733733),
  [AECAudioStream sample](https://github.com/kasimok/AECAudioStream))

### What it gives you (nearly free, native, Apple-tuned)

- Acoustic Echo Cancellation (irrelevant for dictation unless the user plays
  audio while talking).
- **Noise suppression** — Apple's, genuinely good, no model to ship.
- **AGC** — lifts soft speech. Attractive for the "speaks quietly" goal…

### …and the caveats (these are real and they bite)

- **You cannot cherry-pick.** VPIO is a bundle: turning it on gives you AEC + NS +
  AGC together. `kAUVoiceIOProperty_VoiceProcessingEnableAGC = 0` is documented
  but on macOS the forum reports it still applies the same gain change — you
  don't cleanly get "NS only." ([forum 733733](https://developer.apple.com/forums/thread/733733))
- **It's an input+output unit.** VPIO expects **both** an input and output bus;
  it's built for duplex VoIP, not input-only capture. You typically have to stand
  up the output side too. A slight, expected **gain shift on both mic and speaker
  volume** comes with enabling it. ([forum 733733](https://developer.apple.com/forums/thread/733733))
- **AGC can pump.** For push-to-talk dictation, aggressive AGC on a quiet room can
  breathe/pump the noise floor between words — sometimes *worse* for Whisper than
  a clean, quietly-normalized signal.
- **cpal does not expose VPIO.** cpal's macOS backend uses the default HAL input
  AudioUnit; there is no cpal switch for the VPIO subtype. To use it you must
  **replace the cpal capture path** with a Core-Audio AudioUnit you configure
  yourself. Two ways in this repo's toolchain:
  - `coreaudio-rs` (`IOType::VoiceProcessingIO` exists in its enum) —
    ([coreaudio-rs](https://github.com/RustAudio/coreaudio-rs),
    [IOType docs](https://rustaudio.github.io/coreaudio-rs/coreaudio/audio_unit/types/enum.IOType.html)); or
  - a tiny Obj-C / Core-Audio shim called via the **objc2 / objc2-foundation /
    block2 / core-foundation** crates the app **already links** (see
    `Cargo.toml`), modeled on [AECAudioStream](https://github.com/kasimok/AECAudioStream)
    (16 kHz sample rate, `AVAudioPCMBuffer` callbacks).

### Cost / verdict

Cheapest *in model-shipping terms* (nothing to bundle), but **not cheapest in
integration risk**: it means a second, native capture path replacing cpal, plus
living with forced AEC/AGC and a duplex unit. Because software denoise (Section 2)
gets us the same NS benefit **without touching the proven cpal capture path**,
VPIO is **not the v1 pick** — it's a **Tier-3 prototype** worth trying if a user
dictates over playing audio (echo) or if we want Apple's NS with zero shipped
weights. Do it behind a settings flag, A/B against the software path on the same
recordings, and keep cpal as the default.

---

## 2. On-device denoisers we could bundle

All operate frame-by-frame and keep recurrent state, so for our **batch** use we
just feed the whole clip through in order. Insert **after mono-downmix, before
the 16 kHz resample** (resample native→48 k first if the model needs 48 k).

| Option | License | Runtime | Rate / frame | Quality | Cost | Ship weight | Loopable? |
|---|---|---|---|---|---|---|---|
| **nnnoiseless** (RNNoise, safe-Rust port) | BSD-3 | pure Rust | 48 kHz, 480-samp (10 ms) | Good on steady noise (fans/hum), weak on overlap/transients | RTF ≪ 1, ~85 k params | none (baked in) | **Yes** |
| **DeepFilterNet** (`deep_filter` + `DfTract`/tract) | MIT / Apache-2.0 | pure-Rust inference (tract), needs model asset | 48 kHz, 480-samp hop, ~40 ms look-ahead | **Best open quality** — 2-stage deep filtering, keeps voice detail | RTF ~0.19 / thread, ~2 M params | ~a few MB (enc/erb_dec/df_dec ONNX) | **Prototype-first**, then testable |
| **WebRTC APM** (`tonarino/webrtc-audio-processing`) | BSD-3 | C++ (bundled build) | 10 ms frames @ 16/32/48 kHz | Solid NS + built-in AGC + AEC + VAD | low | none (code) | Prototype-first (needs clang/meson/ninja) |
| **sonora** (pure-Rust WebRTC APM port, M145) | BSD-3 | **pure Rust** | 10 ms @ 16/48 kHz | NS + AGC2 + AEC3, matches C++ ref (2400+ tests pass) | low | none | **Yes** (MSRV 1.91) |
| **speexdsp** (preprocess: NS+AGC+VAD) | BSD (Xiph) | C bindings | any, 10–20 ms | Older/lower than RNNoise | very low | tiny | Prototype-first (C dep) |

### 2a. RNNoise / nnnoiseless — the loopable baseline

- `nnnoiseless` is a **safe-Rust port of Xiph RNNoise**, BSD-3, **no model file to
  ship** (weights compiled in), operates on **48 kHz mono, 480-sample (10 ms)
  frames**, RTF far below realtime.
  ([crate](https://crates.io/crates/nnnoiseless),
  [repo](https://github.com/jneem/nnnoiseless))
- Strengths: fan hum, HVAC, steady broadband — exactly the "quiet room with a
  fan" case. Weaknesses: overlapping speech, sudden keyboard clatter, heavy
  reverb (single gain-per-band). ([Krisp/RNNoise/DFN comparison](https://www.forasoft.com/learn/ai-for-video-engineering/articles-ai/real-time-noise-suppression-krisp-rnnoise-deepfilternet))
- **Why it's the pure-Rust stepping stone:** zero shipped assets, deterministic,
  `cargo test`-able today → it loops without a human. Upgrade to DFN only if
  measured quality demands it.

### 2b. DeepFilterNet — the quality ceiling, still Rust-native

- `deep_filter` crate, **dual MIT/Apache-2.0**.
  ([crate](https://crates.io/crates/deep_filter),
  [repo](https://github.com/Rikorose/DeepFilterNet))
- Inference is **`DfTract` in `libDF/src/tract.rs` running on `tract`** — a
  **pure-Rust** ONNX engine, so **no native onnxruntime dependency**; you only
  ship the exported model (`enc.onnx` + `erb_dec.onnx` + `df_dec.onnx` in one
  tar.gz). **48 kHz, 480-sample (10 ms) hops**, ~40 ms look-ahead, **RTF ~0.19**
  on one laptop thread.
  ([DfTract source](https://github.com/Rikorose/DeepFilterNet/blob/main/libDF/src/tract.rs),
  [ONNX export / deployment (DeepWiki)](https://deepwiki.com/Rikorose/DeepFilterNet/7-deployment-options),
  [Interspeech'23 paper](https://www.isca-archive.org/interspeech_2023/schroter23b_interspeech.pdf))
- Best open-source separation of voice from typing / music bleed / babble. The
  40 ms latency that scares real-time VoIP is a **non-issue for our batch path**.
- Integration: because the API is frame-based and stateful, wrap `DfTract` in a
  helper that runs the whole buffer; the `deep-filter` CLI and the LADSPA plugin
  are working references. **This is prototype-first** only because of the shipped
  model asset — once wired, it's testable with a fixture (below).
- Alternate route if you'd rather use ONNX Runtime: `shimondoodkin/deepfilter-rt`
  (ONNX RT) — but that pulls a native ORT lib; prefer the tract path to stay
  closer to pure-Rust. ([deepfilter-rt](https://github.com/shimondoodkin/deepfilter-rt))

### 2c. WebRTC APM & sonora — NS + AGC + AEC in one box

- **`tonarino/webrtc-audio-processing`** wraps PulseAudio's WebRTC APM: NS + AGC +
  AEC + VAD, **10 ms frames at 16/32/48 kHz**, `NoiseSuppression.suppression_level`
  controls aggressiveness. **BSD-3.** Downside: the `bundled` feature **compiles
  C++** (needs `clang`/`gcc`, `pkg-config`, `meson`, `ninja`) — heavier build, not
  pure Rust.
  ([crate](https://crates.io/crates/webrtc-audio-processing),
  [repo](https://github.com/tonarino/webrtc-audio-processing),
  [APM design doc](https://chromium.googlesource.com/external/webrtc/+/master/modules/audio_processing/g3doc/audio_processing_module.md))
- **`dignifiedquire/sonora`** — a **pure-Rust** port of Google WebRTC M145 APM:
  `sonora`, `sonora-ns`, `sonora-agc2`, `sonora-aec3`, all **published on
  crates.io, BSD-3**, passes 2400+ C++ reference tests, examples at 16 kHz & 48 kHz
  10 ms frames, **MSRV 1.91**. This is the interesting one: it gives NS **and**
  AGC in **pure Rust with no shipped model and no C++ toolchain** → potentially
  **loopable**, and it bundles the "lift quiet speech" AGC we want.
  ([sonora](https://github.com/dignifiedquire/sonora))
- Verdict: **sonora is a strong pure-Rust alternative to nnnoiseless** that also
  gives AGC. It's newer/less battle-tested in the wild than RNNoise; treat as a
  fast-follow to prototype against nnnoiseless and keep whichever scores better.

### 2d. speexdsp

Xiph SpeexDSP preprocess (denoise + AGC + VAD), BSD, tiny, C bindings
(`speexdsp-rs`). Mature but **lower quality than RNNoise** on modern noise; only
worth it if you want an ultra-light AGC/denoise with a C dep. Skip unless the
pure-Rust options disappoint. ([speexdsp](https://github.com/xiph/speexdsp))

---

## 3. VAD — gate on speech, not energy

Today's `voiced_seconds` energy gate is fine as a **WPM denominator** but a real
VAD does two more things: (a) reject a clip that is keyboard/noise-only so Whisper
doesn't hallucinate text on it, and (b) trim leading/trailing/pause non-speech
before the model, improving accuracy and the `speech_seconds` metric.

| Option | License | Runtime | Rate / chunk | Notes | Loopable? |
|---|---|---|---|---|---|
| **Silero VAD** (`voice_activity_detector`, nkeenan38) | MIT | `ort` (ONNX Runtime, native) | 16 kHz, **512-sample** chunk (256 @ 8 k) | Best accuracy, trained on 6000+ langs, robust in noise; `predict()`→prob, `LabeledAudio::Speech/NonSpeech` iterators | Prototype-first (ORT native lib) |
| **Silero VAD** (`silero-vad-rs`, sheldonix) | MIT | `ort` | 16 kHz | **Bundles the ONNX weights** (opset 15/16) — no download | Prototype-first (ORT) |
| **webrtcvad** (`webrtc-vad` crate) | BSD | C bindings | 8/16/32/48 kHz, 10/20/30 ms | GMM VAD, very fast, lighter accuracy than Silero | Prototype-first (C dep) |
| **current energy gate** | — | pure Rust | 16 kHz, 20 ms | Already shipped; keep as WPM source + cheap pre-filter | **Yes** |

- Silero: MIT upstream and MIT Rust ports, **512 samples @ 16 kHz** is the only
  allowed window; returns a 0..1 speech probability per chunk with threshold +
  padding helpers.
  ([nkeenan38 crate](https://crates.io/crates/voice_activity_detector),
  [sheldonix port](https://github.com/sheldonix/silero-vad-rust),
  [Silero upstream (MIT)](https://github.com/snakers4/silero-vad))
- The `ort` dependency means a **native ONNX Runtime** — same "prototype-first,
  then lock with a fixture" story as DeepFilterNet. `silero-vad-rs` bundling the
  weights removes the *download*, not the ORT lib.
- **Recommendation:** keep the current energy VAD as-is for WPM, and add Silero as
  a **speech-present gate** ("if max speech prob over the clip < τ → treat as
  no-speech, skip paste"). This is the single best defense against Whisper
  emitting phantom text from a fan or a keystroke burst. If we want to avoid the
  ORT native dep for now, `webrtc-vad` (or even a tightened energy+ZCR gate) is a
  pure-ish interim.

---

## 4. Speaker isolation / target-speaker (only THE user)

The ask: even in a room with other people, transcribe **only the enrolled user**.

- **VoiceFilter-Lite** (Google) — the on-device reference. Enroll the user once →
  a **d-vector** speaker embedding; a streaming model masks Mel features to keep
  only that speaker. **Quantized model is ~2.2 MB**, ~25% relative WER improvement
  on overlapping speech.
  ([VoiceFilter-Lite paper](https://arxiv.org/pdf/2009.04323),
  [Google research blog](https://research.google/blog/improving-on-device-speech-recognition-with-voicefilter-lite/))
- **Feasible on-device?** Technically yes (that's its whole point), but it is a
  **big lift**: an enrollment flow, a speaker-embedding model, a separation model,
  and either operating on Whisper's front-end features or as a waveform
  pre-filter. It also risks **dropping the user's own words** when they speak
  quietly (the exact failure mode we're trying to avoid).
- **Apple Personal Voice is NOT this.** It's a **TTS** feature — it *synthesizes*
  a voice that sounds like you (accessibility/AAC), exposed via
  `requestPersonalVoiceAuthorization` on `AVSpeechSynthesizer`. It does **not**
  extract your voice from a noisy mic. Not applicable.
  ([WWDC23 Personal Voice](https://developer.apple.com/videos/play/wwdc2023/10033/))
- **Verdict: overkill for v1.** A generic denoiser + VAD handles the dominant
  cases (fan, typing, music, occasional background voice). Target-speaker
  extraction only pays off in genuinely multi-speaker rooms and adds real quality
  risk for quiet talkers. **Defer to a "Focus on my voice" opt-in later**, gated
  behind an enrollment step, once the denoise+VAD baseline is measured.

---

## 5. Signal hygiene — cheap wins before Whisper (all pure Rust, all loopable)

These are trivial DSP, deterministic, unit-testable → they **drop straight into
the CI loop today** and directly target "speaks quietly + steady background."

1. **DC-offset / high-pass filter (~80 Hz, 1-pole or biquad).** Removes AC hum,
   HVAC rumble, desk thumps, and mic handling before they smear the low bands.
   Whisper cares nothing below ~80 Hz for speech.
2. **Normalization / soft AGC to a target (e.g. −20 dBFS RMS, peak-limited).**
   The most direct fix for "even when they speak quietly": lift the whole
   utterance to a consistent level so Whisper's features aren't starved. Do it in
   one offline pass over the buffer (we have the whole clip) — no pumping, unlike
   real-time AGC. Guard against amplifying pure-silence clips (skip if peak below
   a floor).
3. **De-click / de-pop / edge fades.** A short attack/release fade at clip
   start/end and a simple median/peak spike guard kills the PTT key-press click
   and release pop that can become a spurious token.
4. **(Optional) light spectral gate** as a poor-man's denoiser if we don't ship a
   model: subtract an estimated noise floor from the first ~150 ms (assumed
   pre-speech) — cheap, pure Rust, but crude vs RNNoise. Only if we want *zero*
   dependencies.

Order in the buffer: **high-pass → (denoise) → normalize/limit → resample 16 k →
Whisper.** (Normalize last so denoiser residue doesn't set the level.)

---

## 6. Ranked, testable recommendation for Yap

Priority = (impact on "clean transcription of a quiet voice") × (1 / integration
risk). Tags: **[LOOP]** = pure Rust, deterministic, autonomously CI-loopable
today. **[PROTO]** = needs a model asset or native framework → human-in-the-loop
prototype first, then lock with a fixture test.

### Tier 0 — Signal hygiene (do first) · **[LOOP]**

> **Spec:** In `record.rs`, before the 16 kHz resample, add pure functions
> `high_pass(&mut [f32], sr, cutoff=80.0)`, `normalize_rms(&mut [f32], target,
> peak_limit)` with a silence guard, and `edge_fade(&mut [f32], ms)`. Chain them
> in `record_loop` after mono-downmix.
>
> **Testable acceptance:** `cargo test` proves: a synthesized 30 Hz tone is
> attenuated ≥ 20 dB while a 300 Hz tone is preserved (≤ 1 dB); a −40 dBFS clip
> is raised to within ±1.5 dB of target with no sample clipping; a pure-silence
> buffer is left unchanged (no divide-by-noise blowup). `tsc --noEmit` = 0. CI green.

*Highest ROI, zero shipping weight, loops unattended. Ship this batch first.*

### Tier 1 — Denoiser on the 48 k buffer · **[LOOP] then [PROTO] upgrade**

> **Spec (1a, loopable now):** Add `nnnoiseless` (BSD-3). In `record_loop`,
> resample native→48 kHz if needed, run RNNoise over 480-sample frames, then
> resample →16 k. Feature-flag it (`denoise = auto|off`) so it's A/B-able.
>
> **Testable acceptance:** a fixture (clean speech tone + injected broadband
> noise) shows **segmental SNR improves ≥ 6 dB** after denoise while a
> clean-speech fixture loses **≤ 1 dB** of in-band energy (no over-suppression).
> `cargo test` + `tsc` green.
>
> **Spec (1b, quality upgrade, prototype-first):** Swap/augment with
> **DeepFilterNet** via `deep_filter`'s `DfTract` (tract, pure-Rust inference;
> ship the ONNX tar.gz as a bundled asset). Same 48 k in/out, offline over the
> whole clip. Keep nnnoiseless as fallback.
>
> **Testable acceptance:** on the same noisy fixture, DFN beats the RNNoise SNR
> gain by ≥ 3 dB (assert a stored metric threshold), model asset loads from the
> app bundle, `pipeline_ms` on a 3 s clip stays < 800 ms p50 on Fast. Prototype
> and eyeball real recordings **before** wiring into the default path.

*RNNoise loops today with no assets; DFN is the quality ceiling and stays
Rust-native via tract — only the model file makes it prototype-first. **Consider
`sonora` (pure-Rust WebRTC NS+AGC2, BSD-3) as the 1a candidate instead of
nnnoiseless** if you want AGC bundled in — same [LOOP] profile, newer code.*

### Tier 2 — Silero VAD speech-gate · **[PROTO]**

> **Spec:** Add Silero VAD (`voice_activity_detector`, MIT) over the 16 kHz clip
> in 512-sample chunks. If the max speech probability over the clip < τ (e.g.
> 0.5), classify as no-speech → skip paste + surface "no speech detected" instead
> of pasting a Whisper hallucination. Optionally trim to the voiced span to
> improve `speech_seconds`.
>
> **Testable acceptance:** a `cargo test` fixture — synthetic speech clip →
> `has_speech == true`; a fan/noise-only clip → `has_speech == false`. Wire behind
> a `never lose text` guard (real speech must never be dropped). Prototype-first
> because `ort` pulls native ONNX Runtime; lock with the fixture once linked.

*Biggest defense against phantom transcripts on quiet/noisy captures. If we want
to stay off native ORT for now, `webrtc-vad` or a tightened energy+ZCR gate is a
pure-ish interim.*

### Tier 3 — Apple Voice-Processing I/O capture path · **[PROTO], optional**

> **Spec:** Behind a settings flag, add an alternate capture path using the VPIO
> AudioUnit (`kAudioUnitSubType_VoiceProcessingIO`) via `coreaudio-rs` or a small
> objc2 Core-Audio shim (objc2 is already a dependency). 16 kHz, mono, Apple
> AEC+NS+AGC. Keep cpal as default; A/B on identical utterances.
>
> **Testable acceptance:** can't be a pure unit test (needs a live device/output
> unit) → gate with a manual smoke: record the same phrase over playing music via
> cpal+software-denoise vs VPIO, compare Whisper WER. Only promote if VPIO wins
> and AGC doesn't pump the quiet-room floor.

*Only worth it for echo (dictating over playing audio) or to get Apple's NS with
zero shipped weights. Software denoise already covers the core need without
replacing the proven cpal capture — hence Tier 3, not v1.*

### Tier 4 — Target-speaker extraction (VoiceFilter-Lite) · **[PROTO], defer**

> **Spec:** Later, opt-in "Focus on my voice": one-time enrollment → d-vector →
> ~2.2 MB VoiceFilter-Lite mask model filtering to the enrolled speaker.
>
> **Testable acceptance:** two-speaker mixed fixture → enrolled speaker's words
> survive, interfering speaker suppressed, and a **solo quiet-speaker fixture is
> NOT attenuated** (the key risk). Defer until denoise+VAD are measured
> insufficient in real multi-speaker rooms.

*Overkill for v1, real risk of dropping a quiet target talker. Apple Personal
Voice is TTS-only — not applicable.*

---

## 7. One-paragraph bottom line

Ship **Tier 0 signal hygiene now** (pure Rust, loops itself, directly fixes the
quiet-voice case). Add a **48 kHz denoiser before the 16 k downsample** —
**nnnoiseless (or sonora) as the loopable baseline, DeepFilterNet-via-tract as the
prototype-first quality upgrade** (batch path means its 40 ms latency is free).
Add a **Silero VAD speech-gate** so noise-only clips never paste a hallucination.
Treat **Apple VPIO** as an optional native A/B, and **target-speaker separation**
as a deferred opt-in. That stack makes Whisper hear a clean, well-leveled voice
without rebuilding the capture path.

---

## Sources

- Apple dev forum — macOS VPIO / AGC caveats: <https://developer.apple.com/forums/thread/733733>
- AECAudioStream (VPIO sample, Swift, 16 kHz): <https://github.com/kasimok/AECAudioStream>
- coreaudio-rs (`IOType::VoiceProcessingIO`): <https://github.com/RustAudio/coreaudio-rs>
- Apple mic modes overview: <https://support.apple.com/guide/mac-help/change-your-mic-mode-on-mac-mchlf5495d9d/mac>
- nnnoiseless (RNNoise, BSD-3, pure Rust): <https://github.com/jneem/nnnoiseless> · <https://crates.io/crates/nnnoiseless>
- DeepFilterNet (`deep_filter`, MIT/Apache-2.0): <https://github.com/Rikorose/DeepFilterNet> · <https://crates.io/crates/deep_filter>
- DeepFilterNet `DfTract` (tract) source: <https://github.com/Rikorose/DeepFilterNet/blob/main/libDF/src/tract.rs>
- DeepFilterNet deployment / ONNX export (DeepWiki): <https://deepwiki.com/Rikorose/DeepFilterNet/7-deployment-options>
- DeepFilterNet Interspeech'23 paper: <https://www.isca-archive.org/interspeech_2023/schroter23b_interspeech.pdf>
- deepfilter-rt (ONNX RT alt): <https://github.com/shimondoodkin/deepfilter-rt>
- WebRTC APM Rust wrapper (BSD-3, C++ bundled): <https://github.com/tonarino/webrtc-audio-processing> · <https://crates.io/crates/webrtc-audio-processing>
- WebRTC APM design doc: <https://chromium.googlesource.com/external/webrtc/+/master/modules/audio_processing/g3doc/audio_processing_module.md>
- sonora (pure-Rust WebRTC APM port, BSD-3): <https://github.com/dignifiedquire/sonora>
- SpeexDSP (BSD): <https://github.com/xiph/speexdsp>
- Silero VAD upstream (MIT): <https://github.com/snakers4/silero-vad>
- `voice_activity_detector` crate (MIT, ort): <https://crates.io/crates/voice_activity_detector> · <https://github.com/nkeenan38/voice_activity_detector>
- `silero-vad-rs` (bundled weights): <https://github.com/sheldonix/silero-vad-rust>
- VoiceFilter-Lite (target-speaker, on-device): <https://arxiv.org/pdf/2009.04323> · <https://research.google/blog/improving-on-device-speech-recognition-with-voicefilter-lite/>
- Apple Personal Voice (TTS, WWDC23): <https://developer.apple.com/videos/play/wwdc2023/10033/>
- Krisp / RNNoise / DeepFilterNet comparison: <https://www.forasoft.com/learn/ai-for-video-engineering/articles-ai/real-time-noise-suppression-krisp-rnnoise-deepfilternet>
