# YV92 — manual repro: swap to AirPods mid-recording

**Status in this PR: NOT EXECUTED.** No Bluetooth headset and no second input
device were available on the machine this branch was built on, so the AirPods
swap below has not been performed on hardware. Everything that *could* be
verified without it was, and is listed under "what was verified" — the split is
stated plainly here rather than blurred into a claim the branch cannot support.

## What was verified without AirPods

| Claim | How | Result |
| --- | --- | --- |
| The CoreAudio listeners actually install and remove on a real Mac | `cargo test --test input_format_change_handler -- --ignored --nocapture` | `armed=true`, `listeners removed cleanly` |
| A format-change event opens a segment, writes a `device_change` marker with the right host time, and retunes the ratio | `cargo test --test input_format_change_handler` (6 tests, synthetic `kAudioDevicePropertyStreamFormat` sequence) | green |
| Each swap of ONE take opens the NEXT segment — driven through the production seam (`LiveStream::observe_input`, the exact call `watchdog_poll` makes) and read back off the real marker sidecar | `cargo test --lib record::tests::every_swap_of_one_take_opens_the_next_segment` and `record::tests::a_reopen_is_not_a_segment_and_a_new_take_restarts_the_numbering` | green — `[1, 2, 3]` on disk; the pre-fix shape (a watch rebuilt per poll) produces `[1, 1, 1]` and fails the test |
| The anti-alias scratch is reserved off the audio thread, so the cascade never allocates inside cpal's callback | `cargo test --lib resample::tests::the_filter_scratch_is_reserved_before_the_callback_ever_runs` | green |
| A ratio never survives a format change, in the DSP itself | `cargo test --lib record::tests::stream_dsp_retunes_on_a_format_change_without_losing_the_take` | green — 0.5 s at the new 24 kHz rate produces ~8000 output samples, not the ~4000 a stale 48 kHz ratio would |
| The 60 s watchdog polls `LiveStream::has_failed` | `grep -n "has_failed" desktop/src-tauri/src/record.rs` → a call inside `watchdog_poll` | see the PR body |
| The anti-alias filter rejects ≥20 dB at 10 kHz | `cargo test --test biquad_lowpass_response -- --nocapture` | 28.46 dB measured |

## The procedure, for whoever has the hardware

1. Build and run the app (`npm run tauri dev` from `desktop/`, or a release
   build) with the log level at `info` or below, and keep the log tailing:
   `tail -f "$HOME/Library/Application Support/WilsonVoice/logs/yap.log"`.
2. Start a recording with the built-in microphone selected as the system input.
   Speak for ~20 seconds. The opening log line names the format the stream
   opened at: `mic device=MacBook Pro Microphone format=F32 rate=48000 ch=1 …`.
3. **Without stopping the recording**, put AirPods in and let macOS move the
   default input to them (or select them under System Settings → Sound → Input).
   Keep speaking for another ~30 seconds, then a further 90 seconds so the
   session outlives one full watchdog interval.
4. Stop the recording.

### What should happen (and what used to)

* Within a tick of the swap the log carries the watchdog line:
  `YV92 watchdog: input format changed (MacBook Pro Microphone 48000Hz/1ch → AirPods Pro 24000Hz/1ch, source=stream_format) — segment 1 opens at output sample N, new resample ratio 1.5000`
  followed by `YV92 watchdog: capture continues on AirPods Pro at 24000Hz/1ch`.
* **The segment number advances with each swap of the same take.** Engaging the
  AirPods mic (the HFP/SCO renegotiation) is a second change, so its line reads
  `segment 2 opens at output sample …, new resample ratio 1.0000`, and taking the
  AirPods back out reads `segment 3 … ratio 3.0000`. Watch for that: while the
  watch was rebuilt on every watchdog tick, all three lines said `segment 1` and
  the marker sidecar could not order its own records. A new take (the next
  dictation) starts again at segment 1.
* The audio after the swap plays back at **normal speed**. Before this item the
  ratio stayed at 3.0 and everything after the swap came out at half speed —
  clean-sounding, correctly pitched per-sample, and wrong by a factor of two in
  duration, which corrupts every timestamp downstream.
* The transcript covers the whole take. A stream that dies at the swap is now
  discovered by the watchdog within `WATCHDOG_TICK` (60 s) instead of at stop,
  and the take is reopened onto the new device with its buffered audio intact.
* `~/Library/Application Support/WilsonVoice/recovery/<id>.spill.markers.jsonl`
  carries one `device_change` line per swap — with `segment_index` 1, 2, 3 in
  the order they happened — while the take is in flight
  (the sidecar is retired with the take on a normal stop, so look while
  recording or after a crash, not after a clean finish).

### Known limitation, deliberately not fixed here

OS-9's fix (c) — "surface it in the pill at the moment it happens" — is not in
this item. There is no meeting pill yet (YV91/YV95), and adding a device-change
toast to the *dictation* pill would be a new interruption on a five-second path
where the change is already handled silently and correctly. The marker and the
log line are written now so the surface has something true to render when it
lands.
