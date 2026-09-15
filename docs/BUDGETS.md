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
