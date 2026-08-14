# YV103 — manual verification, and what could NOT be verified

## What was run on real hardware

`cargo test --test input_format_change_handler -- --ignored --nocapture`, on
macOS 26.5.2 (25F84), arm64. Transcript in `hal-listeners-on-this-mac.txt`.

Both HAL-touching tests pass:

* `output_format_listeners_install_on_this_mac` (new in YV103) — the two
  output-side addresses (`kAudioHardwarePropertyDefaultOutputDevice` = `'dOut'`
  and `kAudioHardwarePropertyDevices` = `'dev#'`, both on
  `kAudioObjectSystemObject`) install via `AudioObjectAddPropertyListener`,
  arming is idempotent, and `AudioObjectRemovePropertyListener` removes exactly
  what was added. This is the real evidence that the fourccs and the scope are
  right — a wrong selector installs cleanly and then never fires, which is the
  quietest possible failure and is not detectable from a pure test.
* `input_format_listeners_install_on_this_mac` (YV92, unchanged) — still passes,
  so the output-side arming did not disturb the input side. They are separately
  armed and separately disarmed on purpose.

## What could NOT be run, stated plainly

The backlog's last YV103 criterion is *"Manual repro documented in the PR:
connecting AirPods mid-meeting (system audio being tapped from a different app)
does not produce a rebuild storm in the logs and the meeting continues recording
both tracks."*

**That repro is not runnable at this commit, and this PR does not claim it.**
There is no tap to storm yet: `syscapture.rs` is YV100's module and YV100 is not
merged (`main` is at YV101, `fbb2f0e`). A tap also needs the
`SystemAudioCaptureRequests` TCC grant against a signed build, which YV101's own
spec records as a precondition. Both are absent here.

This is why OS-3 makes the *pure* test the acceptance criterion in the first
place, in its own words: make the pure `event sequence in → rebuild decisions
out` function the gate "rather than the current 'device-change mid-meeting
doesn't lose audio', which is a manual repro nobody will run 40 times." The
storm, the burst and the re-entrancy are all driven end-to-end in
`aggregate_rebuild_debounce.rs` and `aggregate_rebuild_idempotent_teardown.rs`
— including a 100-round soak that interleaves rebuilds with the notification
storm each rebuild emits.

When YV100 lands the tap, the AirPods-mid-meeting repro belongs to the item that
wires `RebuildAggregate` into `syscapture`'s 7-step sequence, where there is
something real to observe.

## Also not wired here, and why

The backlog lists `syscapture.rs` under YV103's "files/functions touched" (the
aggregate-rebuild call, wired to the new action). That file does not exist on
`main` yet. Rather than create a stub of YV100's module — which would fork the
module YV100 owns, the exact failure mode this item exists to avoid on the
listener side — YV103 ships the decision half only, with the action, the guard
API (`begin_aggregate_work` / `finish_aggregate_work`) and the arming seam
(`arm_output_listeners` / `take_output_event`) that YV100 and YV104 call.
