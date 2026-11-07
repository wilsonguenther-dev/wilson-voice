# yap-engine-ane

SP-10 spike: Parakeet TDT 0.6B v3 on the Neural Engine (Core ML via
FluidAudio 0.18.2, Apache-2.0; weights CC-BY-4.0) behind the `yap-asr` wire
protocol (`desktop/src-tauri/src/asr_protocol.rs`). Not the default engine.

    scripts/build-ane-engine.sh --test     # build, run unit tests, stage binaries/yap-engine-ane-<triple>
    YAP_ASR_ENGINE=ane YAP_ANE_MODEL_DIR=<coreml dir> wilson-voice --bench <wav>

Design, bench and decision rule: docs/research/ane-engine-spike.md.
