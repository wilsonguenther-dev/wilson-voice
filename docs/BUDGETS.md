# Yap — measured budgets

Every number in this file was **measured**, on the machine named beside it, by a test that is
checked in and can be re-run. Nothing here is derived from a constant, a vendor claim, or a
previous document. Where a number contradicts a constant in the source, the constant is the thing
that is wrong.

The falsifier for this page is `desktop/src-tauri/tests/polish_envelope.rs`. It skips — with a
reason that **names** the missing path — when the weights or the built sidecar are absent, so CI
and a fresh clone print one line and pass instead of silently asserting nothing.

```
cd desktop/src-tauri
cargo test --features custom-protocol --test polish_envelope -- --nocapture
YAP_ENVELOPE_REPS=9 cargo test --features custom-protocol --test polish_envelope -- --nocapture
```

---

## polish_latency

**Measured on:** Apple M4 Pro, 24 GB unified memory, macOS 26.6.2, `aarch64-apple-darwin`,
release-profile `yap-polish`, on AC power, no other load.
**Measured at:** 2026-09-15. **Sweep:** 9 repetitions per input length (`YAP_ENVELOPE_REPS=9`).
**Model:** `qwen2.5-1.5b-instruct-q4_k_m.gguf` — the catalog's recommended polish model, i.e. the
one that ships.

Timings are **wall clock at the parent**, from writing the request line to parsing the response
line, so they include the pipe. Requests carry a 600 000 ms deadline on purpose: measuring at
`DEFAULT_POLISH_DEADLINE_MS` would measure the parent giving up, not the model.

| input words | ok / 9 | p50 | p95 | errors |
|---|---|---|---|---|
| 30  | 9/9 | 216 ms | 386 ms | — |
| 60  | 9/9 | 387 ms | 426 ms | — |
| 100 | 9/9 | 674 ms | 711 ms | — |
| 150 | 9/9 | 937 ms | 972 ms | — |
| 200 | 9/9 | 949 ms | 980 ms | — |
| 300 | 0/9 | — | — | `max_out` ×9 |
| 400 | 9/9 | 1890 ms | 1960 ms | — |

**Cold readiness (spawn → `ready` line, `model_loaded=true`): 7 765 ms** on a genuinely cold
start — the GGUF not in the page cache. A second spawn while the 1.1 GB file is still cached
reported **183 ms**. Both are real and they are 40× apart, so any claim about "the first
dictation after launch" has to say which one it means. The number a user pays after a reboot is
the 7 765 ms one.

### What this curve says that the constants do not

* `polish.rs:59 DEFAULT_POLISH_DEADLINE_MS = 1200` is cleared at 30–200 words and missed at 400
  (p95 1 960 ms). The stage's working envelope at the shipped deadline is a **paragraph**, not the
  400 words `MAX_POLISH_WORDS` allows.
* `polish.rs:73 MAX_POLISH_WORDS = 400` is **not** a latency bound at all — it is above the
  deadline's real reach by a factor of two.
* `polish.rs:64 MAX_POLISH_DEADLINE_MS = 5000` **does** cover 400 words on this machine
  (1 960 ms p95, well inside 5 000 ms). An earlier estimate that 5 000 ms was below the 400-word
  cost is not what the instrument reports here.
* **`max_out` overruns are content-dependent, not length-monotonic.** 300 words failed all nine
  times while 400 words passed all nine. `yap-polish/src/main.rs` turns a `max_out` overrun into a
  hard error for `KIND_POLISH`, which discards the entire rewrite — so an overrun is a total loss
  of the stage, not a degraded result, and it cannot be predicted from input length alone. Any
  budget must sit **below the shortest length at which an overrun has ever been observed**.

### per_chunk_word_budget

**`per_chunk_word_budget = 160` words.**

Derived by `derive_per_chunk_word_budget()` in the test, not chosen: the largest swept length that
was *both* error-free *and* had a p95 inside `DEFAULT_POLISH_DEADLINE_MS` was **200 words**
(p95 980 ms); `BUDGET_HEADROOM = 0.8` is then applied, giving 160. The headroom buys back a fifth
of the budget for a colder cache, a busier machine, and the p99 a nine-sample sweep cannot see.

160 also sits below 300, the shortest length at which a `max_out` overrun was observed — which is
the second constraint above, satisfied independently.

**`Y4-E` cites this number, not `MAX_POLISH_WORDS`.** Chunking long-form dictation at 400 words
would put every chunk outside the deadline and inside the overrun band at once.

### The 0.5B "fast tier" is not a tier

Same machine, same sweep, `qwen2.5-0.5b-instruct-q4_k_m.gguf`:

| input words | ok / 9 | p50 | p95 | errors |
|---|---|---|---|---|
| 30  | 9/9 | 130 ms | 195 ms | — |
| 60  | 9/9 | 254 ms | 267 ms | — |
| 100 | 9/9 | 428 ms | 441 ms | — |
| 150 | 9/9 | 641 ms | 655 ms | — |
| 200 | 0/9 | — | — | `max_out` ×9 |
| 300 | 9/9 | 720 ms | 747 ms | — |
| 400 | 9/9 | 575 ms | 603 ms | — |

Cold readiness 377 ms (page cache warm). It is faster at every length it completes — and its
latency is **not monotonic in input length**: 400 words costs less than 300, which costs less than
150. Latency on this path is dominated by tokens *generated*, so a curve that falls as the input
grows means the output has decoupled from the input. Combined with a total `max_out` loss at 200
words, that is a model returning something other than a rewrite of what it was given. It is not a
speed/quality trade; do not ship it as one.

---

## memory_ceiling

**Measured on:** the same M4 Pro, by
`polish_envelope_peak_resident_bytes_with_asr_and_polish_loaded`.

**Peak resident bytes with the ASR engine and the polish child both loaded: 2 241 462 272 bytes
(2 137.6 MiB).**

| component | resident bytes |
|---|---|
| test process before loading anything | 11 665 408 (11.1 MiB) |
| test process with the ASR engine loaded (`parakeet-unified-en-0.6b-gguf`) | 899 088 384 (857.4 MiB) |
| `yap-polish` child with `qwen2.5-1.5b-instruct-q4_k_m.gguf` resident, after one real rewrite | 1 342 373 888 (1 280.2 MiB) |
| **sum — what the machine actually pays** | **2 241 462 272 (2 137.6 MiB)** |

Two processes, so the user pays for both at once; this is the number `Y3-G`'s memory ceiling is to
be asserted against. It is measured *after* a real rewrite in the child and a real engine load in
the parent, so neither model is counted as lazily-mapped-and-untouched pages.

This is also why the polish model is not resident at launch: 1.28 GiB for a stage nobody has
invoked yet.

---

## asr_decode_real_time_factor

**Measured on:** the same M4 Pro, 2026-09-12, by the PANEL pass that scoped `Y3-B`.

A 601-second WAV (241x the `quick-brown-fox-16k.wav` fixture, 16 kHz mono) was pushed through the
shipped headless path — `./target/debug/wilson-voice --transcribe-file <601s.wav>` — and returned
exit 0.

| quantity | value |
|---|---|
| audio decoded | 601.0 s |
| wall clock, **including** model load | 30.5 s |
| words returned | ~1 900 |
| **real-time factor** | **~19.7x** (audio seconds per wall second) |

This is the number `TRANSCRIBE_TIMEOUT` has to be read against, and reading it changes the shape of
the problem. At 19.7x a 120 s per-call ceiling is roughly **forty-five minutes** of audio, not four
— consistent with `transcription.rs`'s own note that "a 60 s take is ~1 s on Metal". So the wall is
real but far out, and the thing that actually broke at ten minutes was not the timeout.

Two consumers of this row:

* **`Y3-B` (shipped).** `DICTATION_CHUNK_THRESHOLD_SECONDS` is *derived* from it —
  `TRANSCRIBE_TIMEOUT * 19.7 / 4` ≈ **591 s** (~9.8 min). The divisor is headroom we refuse to
  spend: a thermally throttled laptop, a cold Metal warm-up and a busy machine are all slower than
  this bench. Under the threshold a take decodes in one call exactly as it always did; over it, the
  take decodes in windows and the 120 s budget is spent **per window**, so a longer take gets more
  budget instead of meeting a fixed wall.
* **`Y3-F`.** The declared maximum session length must be derived from this row rather than
  asserted, so that the number in the product copy is one somebody took.

Caveat, stated rather than buried: this is a single measurement on one machine, on a debug build,
with the model load folded in. It bounds the order of magnitude — it is not a distribution.

---

## long_take_budget (Y3-G)

**Measured on:** Apple M4 Pro, 24 GB unified memory, macOS 26.6.2, `aarch64-apple-darwin`,
`cargo test --features custom-protocol` (dev profile for the app crate, release for the sidecars),
on AC power, no other load. **Measured at:** 2026-09-15.
**Falsifier:** `desktop/src-tauri/tests/long_take_budget.rs`. Every number below is a named
constant in that file; the two that need model weights SKIP with a line that names the missing
path rather than passing quietly.

```
cd desktop/src-tauri
cargo test --features custom-protocol --test long_take_budget -- --nocapture
```

| budget | measured | ceiling | shape |
|---|---|---|---|
| `press_to_capture_start`, spill-writer overhead | **0 ms** (arm p50 48 µs with spill, 0 µs without, n=21) | 25 ms | absolute |
| `chunk_decode_wall_per_audio_second` | **10.3 ms/s chunked vs 11.4 ms/s single-window — ratio 0.90** (0.83 and 0.68 on earlier runs of the same command; the two arms move together, which is why this is a ratio) | 1.60× | ratio, same fixture, same run |
| `progress_events_per_minute_of_audio` | **2.40/min** — Y3-C's `TRANSCRIBE_PROGRESS_EVENT`, driven for real at its worst legal window (25 s) | 60/min | ceiling, measured by running the emitter |
| `resident_bytes_after_a_long_take_return_to_baseline` | **0 bytes** growth after a 20-minute take (peak 76 800 000 B live during it) | 1 MiB | exact, thread-local meter |
| `peak_resident_bytes_with_asr_and_polish_loaded` | **1 882 420 768 B (1 795.2 MiB)** — ASR delta 765 100 032 B + polish weights floor 1 117 320 736 B | 2 621 440 000 B (2.44 GiB) | ceiling, floor machine |
| `no_new_polling_timer_was_introduced` | census below, exact match required | allowlist | census |

### The floor machine is named, because a budget without one is a wish

`Y6-E` is the release-engineering item that ratifies it; until it lands, `FLOOR_MACHINE_BYTES` in
the test is the single place the number is written down: a **fanless 8 GB M1 Air**, which is
already what `transcription.rs:582`, `tests/meeting_capture_memory.rs` and
`tests/meeting_no_model_resident.rs` name as the target machine. The footprint ceiling is set at
2.44 GiB — under a third of that machine's RAM, and ~45% headroom over the 1 795.2 MiB measured
here. `docs/BUDGETS.md`'s `memory_ceiling` section records the *live-child* composition
(2 241 462 272 B) measured by `polish_envelope.rs`; Y3-G's test uses the polish weights **file
size** as the child's floor so it does not have to spawn a sidecar, which is why its number is
lower. Both are under the ceiling.

### The progress ceiling is measured by running the emitter, not by grepping for a throttle

This budget was wrong once, in the way that matters most, and the correction is the point of
keeping it written down. The first version scanned the three Y3 take-path modules for
`.emit("…progress…")` and treated the absence of a `const …PROGRESS…_MS` as "unthrottled". It
reported `sites=[]` and passed — on a tree where `Y3-C` had **already landed a live progress
stream**. Two independent blind spots produced that green:

* **Scope.** `Y3-C`'s emitter lives in `src/transcribe_progress.rs`, a module that did not exist
  when those three files were listed. The sweep now walks every `.rs` in `src/` (52 files) and
  asserts it swept more than ten, so a sweep that found the wrong directory cannot report clean.
* **Pattern.** The shipped emitter names its event with a `const` — `TRANSCRIBE_PROGRESS_EVENT` —
  and spans three lines. A scanner looking for a quoted string on one line cannot see it. The
  scanner now reads the first argument of any `.emit`/`.emit_to` call, literal or ident, and
  proves on synthetic bait that it catches **both** shapes and still refuses the two download
  streams (`MODEL_DOWNLOAD_PROGRESS_EVENT`, `POLISH_DOWNLOAD_PROGRESS_EVENT`).

Widening the scan alone would have been the opposite error. `Y3-C` has **no timer at all** — it
emits once per *completed decode window* — so a test demanding a millisecond throttle constant
would have failed a correct implementation for the wrong reason. The rate is therefore no longer
inferred from source at all: the test constructs the real `ChunkProgress` with a counting
`ProgressSink` and **counts the events a twenty-minute take actually puts on the wire**, at the
narrowest window `ChunkConfig` may legally emit (`min_seconds` = 25 s). That is 48 events over
1 200 s of audio = **2.40/min**, against a ceiling of 60.

The instrument falsifies itself before it is believed: the same emitter driven at a quarter of the
ceiling's interval measures **240/min**, so a green result is a measurement and not a silence. Two
negative controls were run against the committed file — dropping the ceiling to 2 fails with
`fires 2.4 times per minute … ceiling is 2`, and adding one unaccounted
`app.emit("dictation_progress", …)` to `lib.rs` fails the allowlist with
`the set of take-path progress emitters changed`.

The allowlist is the standing guard: `ACCOUNTED_PROGRESS_EMITTERS` names every take-path progress
stream in the tree and what bounds its cadence. A second one cannot be added without saying so
here.

### Why the decode budget is a ratio and not a millisecond

A wall-clock decode budget measures the runner's GPU, so it would be a different number on every
machine and a red gate on the slowest one. The test decodes the committed
`tests/fixtures/quick-brown-fox-16k.wav`, tiled to 60 s, twice in the same run on the same engine:
once as one window, once as four 15 s windows. Only the ratio is asserted. The first decode after
a load is thrown away, because charging the graph build to whichever arm ran first would make the
ratio an artefact of test ordering.

Chunking measured **faster** than single-window here (0.83), which is not a paradox: the engine's
cost is superlinear in window length on this model, so four short decodes beat one long one. The
ceiling is still 1.60 — it exists to catch a chunker that halves throughput, not to reward this
particular curve.

### Process RSS cannot be asserted inside a shared test binary

The memory test was first written against process RSS and it **failed red on the first run**,
reporting a 1 824 374 784 B "leak" across one twenty-minute take. There was no leak:
`cargo test` runs a binary's tests concurrently in one process, and
`chunk_decode_wall_per_audio_second` had a ~900 MB ASR engine resident at the same moment. That is
the same defect `tests/meeting_capture_rt_safety.rs` hit with a process-global allocation counter,
and it has the same fix — the meter is now a **thread-local metering `GlobalAlloc`**, armed for
exactly the take. It reports 0 bytes of growth against a 76 800 000 B peak during the take, and it
proves it saw the take before it reports a verdict. RSS is still printed, labelled
INFORMATIONAL, and asserted on nothing. The two engine-loading tests additionally serialize on one
mutex, because two engines resident at once is a state no user's machine is ever in.

### The YV81 polling-timer census

`no_new_polling_timer_was_introduced` counts every sub-1000 ms `Duration` **literal** in the three
modules Y3 touches and compares the counts, exactly, against an allowlist. A second is the line
because YV81's finding was sub-second wakeups; a 5 s or 60 s deadline is supervision, a 5 ms one in
a loop is a busy-wait wearing a `Duration`'s clothes. As measured on
`344a335`:

```
src/record.rs        {1:1, 2:1, 5:3, 10:3, 60:2, 300:2, 500:4}
src/transcription.rs {2:2, 5:2, 10:2, 20:2, 50:3, 80:1, 120:1, 150:1, 200:1, 300:1, 400:3}
src/lib.rs           {5:2, 10:1, 50:1, 100:1, 150:2, 400:1, 500:1}
```

Row change (2026-10-19, yap-ptt-capture-tail D4): record.rs `300:1 -> 300:2` is `STOP_EDGE_BUDGET`,
the contract that a take stops capturing within 300 ms of its stop edge. It is a test budget, never
slept on or polled with.

Row change (2026-10-19, yap-lifecycle): lib.rs gains `600:1` and `900:1`. They are
`EXIT_TAKE_TIMEOUT` and `EXIT_DRAIN_TIMEOUT`, which were 3 s each and are now cut to fit inside
the quit's step deadlines (`lifecycle::QuitStep::deadline`, summing to `QUIT_BUDGET` = 3 s). Each
is a one-shot bound on a wait that only happens at quit, not a polling interval; the 10 ms
`EXIT_WAIT_POLL` that the take wait loops on was already in the row.

**Limits, stated rather than implied:** literals only. `Duration::from_millis(interval)` with a
variable is invisible to it, and so is a sleep built from arithmetic. That hole is why this is a
census against an allowlist and not a proof of absence — it makes the cheap regression (a
`from_millis(16)` dropped into a decode loop) impossible to land silently, and adding a row is a
deliberate act that has to be written down here.

### Every scanner in this file falsifies itself first

The emitter scan, the throttle scan and the timer census each run over a synthetic source that
contains the shape they are meant to catch, and assert they catch it, before they run over the
real tree. A grep proving absence with the wrong pattern is a green test that guards nothing;
these three prove the pattern can match the shape feared, not just the shape remembered.

---

## meeting_stop_to_notes (yap24-NT2)

**What it is:** how long a user waits between pressing Stop on a meeting and the row reaching
`complete` (transcript ready; no summary model installed, so the summary stage is not in it), for
the 15-minute single-speaker lecture fixture. Measured through the app's own
`meeting_pipeline::run_meeting_job`, not the harness's per-window subprocess decoder: `WavWindows`
over the WAV, the shipped `MeetingAsr` driver, `WarmEngineChunkAsr` over ONE warm
`TranscriptionManager` holding the pinned model (`handy-computer/parakeet-unified-en-0.6b-gguf`,
Q8_0), segments landing in a real SQLite file.

**Falsifier:** `meeting_eval_lecture_stop_to_notes_through_the_shipped_pipeline` in
`desktop/src-tauri/tests/meeting_eval.rs`, run REQUIRED (a missing corpus or model panics):

```
cd desktop/src-tauri
YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 \
  YAP_EVAL_CORPUS="$HOME/yap-eval-corpus/meetings" \
  YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" \
  cargo test --features custom-protocol --test meeting_eval -- --nocapture --test-threads=1
```

**Measured 2026-10-26**, Apple M4 Pro, 24 GB (Wilson's dev machine), `cargo test` debug test
profile of the harness with the app crate's dependency profile, lecture-15min = 904.7 s of audio,
30 windows:

| run | stop_to_notes | real-time factor | peak RSS (test process) | pipeline WER |
|---|---|---|---|---|
| first run after build (cold model pages + Metal pipeline compile) | 49.2 s | 0.0544 | 1128 MB | 0.0728 |
| second run (warm page cache) | 13.6 s | 0.0150 | 1216 MB | 0.0728 |

The model-only WER on the same fixture (harness decoder, its own seam merge) is **0.0048**
(`meeting_eval lecture-15min wer=0.0048`; timed merge 0.0090; VAD-cut timed merge 0.0042), under
the 0.02 `WER_GATE`, which is unchanged.

**Finding, not a target — the shipped pipeline WER is 0.0728.** 15 substitutions, 0 deletions,
**212 insertions** over 3117 reference words across 30 windows: about seven words per seam land
twice. The harness's merge removes the 2 s window overlap; the shipped per-window append
(`meeting_pipeline::chunk_segment`) does not on this model's output. Root cause is not yet
verified (the likeliest reading is that the windows' spans are not joinable, so the whole window
text — overlap included — is written). This needs its own item. Until it lands the pipeline arm
asserts a measured regression ceiling (`PIPELINE_WER_CEILING = 0.08`), to be lowered to
`WER_GATE` by the fix.

---

## summary recall and memory (yap24-NT4)

**What it is:** whether the default meeting notes (the 1.5B notes model, `qwen2.5-1.5b-instruct-q4_k_m`,
through `summary_jobs` exactly as the app runs a notes job) find what a meeting committed to, and
what that job costs in memory on the 8 GB floor machine.

**Fixture:** `tests/support/notes_planted.rs`, a 15-minute two-person call (Me on the mic track,
Them on the system track, 56 lines spread over 900 s) planting 6 explicit commitments, 4 implicit
commitments, 2 decisions, 6 small-talk distractors that use "going to" / "'ll", and 3 commitments
in the last 20% of the call. It is fed as transcript segments (the ASR leg is measured by
`meeting_stop_to_notes` above; this eval measures the notes).

**Falsifier:** `real_notes_model_meets_the_summary_recall_gates` in
`desktop/src-tauri/tests/meeting_notes_recall_eval.rs`, `#[ignore]`, REQUIRED under
`YAP_EVAL_REQUIRE=1` (a missing model or sidecar panics). The model is provisioned next to the
parakeet file by `scripts/provision-notetaker-eval.sh` (pin read from the catalog's `polish_models`,
APFS-cloned from the installed copy). The un-ignored tests in the same file prove every metric
scores known answers and that each audit failure mode trips its gate.

```
cd desktop/src-tauri
YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_REQUIRE=1 \
  YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" \
  cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1
```

**Gates and the measured numbers, 2026-10-27**, Apple M4 Pro 24 GB, `yap-polish` release sidecar,
one MAP chunk, 47.9 s wall:

| metric | gate | measured | passes |
|---|---|---|---|
| action recall (all 10 commitments) | >= 0.8 | **0.20** (explicit 0.17, implicit 0.25, last-20% 0.33) | no |
| action precision | >= 0.8 | **0.50** (6 actions, 1 distractor filed) | no |
| decision recall | >= 0.5 | 1.00 | yes |
| Summary 4-gram verbatim ratio | <= 0.5 | 0.35 | yes |
| cross-section duplicates | = 0 | **16** | no |
| items per cited segment | <= 2 | **3** | no |
| owner accuracy, blank counts as no claim | >= 0.8 | **0.50** (1 right, 1 wrong, 0 blank) | no |

**This is a finding, not a target met.** The default notes fail five of seven gates on a fixture
built to look like the audit's real meetings (docs/audits/meeting-notetaker-audit-2026-10-26.md,
issues #2 and #18): one line is filed as an action, a decision and a question at once, plain
statements become "decisions", and the implicit and end-of-call commitments are mostly missed.
The mechanical fixes belong to yap-audit-notes-guards and the strategy to yap-audit-notes-strategy;
this eval is their falsifier, and its gates are not lowered to match today's model.

**Human half (numbers only), 2026-10-27**, `scripts/eval/compare-reference.py --notes` on a read-only
`.backup` copy of the 31-minute real meeting (1864 s) as #250 summarized it on Wilson's Mac:
items=35, duplicate_lines=8, max_items_per_cited_segment=4, owners Me=27 Them=8 Someone-here=0
blank=0. The script read summary_words=0 from that row (its Summary section did not parse), so its
summary_verbatim_ratio=0.0000 is not a measurement; the audit's 0.91-0.97 stands for that meeting.

### Memory during a summary (R4)

| what | peak RSS |
|---|---|
| notes sidecar (1.5B Q4_K_M, one MAP chunk + REDUCE + HEADLINE) | **1395 MB** |
| test host + sidecar | 1408 MB |
| app with the meeting ASR engine warm (from meeting_stop_to_notes above) | 1128-1216 MB |

ASR warm plus the notes sidecar is about 2.6 GB, over the **2560 MB ceiling** for the 8 GB floor
machine. So the notes job is serialized with ASR (`summary_jobs::wait_for_idle`): it does not start
while a meeting is being transcribed (`meeting_pipeline::transcribing_now`), while a dictation
wants the engine, or while app RSS + `NOTES_SIDECAR_PEAK_MB` (1400) would exceed
`NOTES_RSS_CEILING_MB` (2560). It waits for the next idle tick (`NOTES_IDLE_TICK`, 15 s), which
in practice means until the ASR engine's 15-minute idle unload, and runs anyway after
`NOTES_MAX_DEFERRAL` (30 min) so a machine that never idles still gets notes. Every wait is logged
(`summary deferred`, `summary defer_end`, `summary defer_cap`). The 8 GB machine itself was not
measured; the figures above are per-process RSS from the 24 GB dev machine.
