# SP-10 — ANE engine spike: Parakeet on Core ML as a second speech helper

Item: yap-1106-sp-10-ane-engine-spike. Status: code written, bench numbers
pending the integrator's run on the Mac (coders may not compile or run).

## What was built

`desktop/yap-engine-ane/` is a Swift package that builds one binary,
`yap-engine-ane`. It uses FluidAudio (Apache-2.0, pinned `exact: "0.18.2"`,
released 2026-11-01) to run Parakeet TDT 0.6B v3 on the Neural Engine
(`MLComputeUnits.cpuAndNeuralEngine`, FluidAudio's default for ASR). It speaks
protocol 0.1.0 of `desktop/src-tauri/src/asr_protocol.rs` on stdin/stdout,
the same protocol as `yap-asr`, so `engine_owner::HelperEngine` drives it
with no change: one ready line, `load` answers with caps
(`arch=parakeet-tdt-coreml`, `backend=ane`, word timestamps), `transcribe`
reads the app's 0600 f32 samples file, `cancel` is answered by the in-flight
decode's own error, `shutdown` frees and exits, stdin EOF frees and exits 0.
Per-token times are merged into words and segments in `Words.swift`.

It never downloads. `AsrModels.loadLocal` reads the Core ML folder already on
disk (`model_path`, or `YAP_ANE_MODEL_DIR` when the request carries the app's
GGUF path, which is what the bench does). Weights: FluidInference/
parakeet-tdt-0.6b-v3-coreml at revision 7dd20fe6b1797d35f5e3307e8b1732d9a178edfe,
CC-BY-4.0 (NVIDIA). Both licenses are recorded in `catalog.json` under
`ane_engines`, which the model picker does not read.

Selection lives in `asr_engine.rs`: `EngineKind::DEFAULT` is `Gguf`
(unchanged). `YAP_ASR_ENGINE=ane` makes `engine_owner::helper_binary()` spawn
`yap-engine-ane`; a selected ANE helper that is not staged is logged
(`engine helper_missing engine=ane`), never swapped for the default, so the
bench cannot time the wrong engine. `EngineKind::for_governor` is the hook the
resource governor uses for its "constrained" engine.

Packaging: `tauri.conf.json` `bundle.externalBin` gains
`binaries/yap-engine-ane`; `scripts/build-ane-engine.sh` builds and stages it
(called from `npm run sidecar`, ci.yml and release.yml); the sidecar gets its
own empty entitlements file and joins `sign-local.sh`'s list.

## How the bench runs

`scripts/qa/ane-bench.sh --ane-model-dir <coreml folder> --markdown` runs the
SP-03 bench (`wilson-voice --bench`) on the 4 min fixture `bench-240s.wav`
twice per run (the second take is warm): once on the default GGUF/Metal
helper, twice on the ANE helper. The first ANE run's load is the first-load
number (it includes Core ML's on-device ANE compile only on a Mac whose Core
ML cache has never seen these models); the second is the cached load. It
samples each helper's RSS with `ps` and, with passwordless sudo, records
`powermetrics --samplers cpu_power,gpu_power,ane_power -i 500` during each
run. `scripts/qa/ane-bench.sh --self-test` checks the rule on fixed numbers.

## Ship decision rule (goes into the PR as written)

Adopt the ANE engine as the default only if all three hold on the 4 min
fixture:

1. WER is within 0.5 percentage points of the default engine.
2. Warm RTF is at least 100x (audio seconds per decode second).
3. GPU power (powermetrics "GPU Power") is lower by at least 30 percent
   than the default engine's, AND package power ("Combined Power", CPU + GPU
   + ANE) is lower by the same 30 percent. Moving work to the ANE always
   lowers GPU power by itself, so the GPU bar alone could adopt an engine
   that costs the battery more; the package bar closes that hole. A missing
   baseline (no sudo for powermetrics) never passes.

Otherwise it is kept as the governor's "constrained" engine. The same
constants are in `asr_engine.rs` (`ANE_MAX_WER_DELTA_PT`, `ANE_MIN_WARM_RTFX`,
`ANE_MIN_POWER_DROP_PCT`, `ane_verdict`). This item changes no default either
way: adoption is a later item.

## Results

Pending: the integrator pastes the `--markdown` table here and into the PR.

| engine | warm RTF | WER | first load | cached load | peak RSS | package power | GPU power |
|---|---|---|---|---|---|---|---|
| GGUF / Metal (default) | pending | pending | pending | pending | pending | pending | pending |
| Parakeet / Neural Engine | pending | pending | pending | pending | pending | pending | pending |

Verdict: pending.

## Known risks

The helper's load budget is 120 s (`HELPER_LOAD_BUDGET`); a first ANE compile
longer than that fails the load and must be reported, not hidden. The helper
needs macOS 14 (FluidAudio's floor) while the app supports 12.0; it is only
spawned when selected. The language hint is not forwarded (English model).
