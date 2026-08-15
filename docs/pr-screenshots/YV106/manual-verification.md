# YV106 — manual verification, and the honest gaps

## No screenshots

This item changes no user-visible surface. The Meetings list, the detail view and
the Markdown export render exactly what they rendered before: a mic-only meeting
is byte-identical (`meeting_markdown_export` and
`a_mic_only_meeting_is_byte_for_byte_what_22a_produced` are the guards), and the
Me/Them rendering that reads `meeting_segments.track` is **YV108's**, not this
one's. There is nothing to photograph.

## No real two-track recording was made, and why

A genuine end-to-end two-track capture needs YV100's `syscapture.rs` CoreAudio
call sequence — `AudioHardwareCreateProcessTap` → aggregate device → IOProc — and
**that item is not merged at this commit**. `syscapture.rs` on `main` today is
the pure state machine YV104 landed (the ghost watchdog, the rebuild sequence as
data, the discriminator); it makes no FFI calls and there is no code path in this
repo that starts a tap. A real tap would additionally need the
`NSAudioCaptureUsageDescription` TCC grant on this Mac.

So: **no real system-audio capture was performed for this PR, and the pure tests
are the gate.** What that costs is stated rather than glossed — nothing here
proves that a live CoreAudio tap's drained blocks are shaped the way
`accept_track` expects. What it does prove is everything on this side of that
boundary, driven through the real journal, the real DSP path, the real finalize
and a real SQLite file:

* two producers → two spills → two playable wavs, through the consumer
  (`two_tracks_round_trip_through_the_consumer_into_two_playable_wavs`);
* each track's DSP/epoch state is its own, at two different native rates, with a
  ring overrun on one track and not the other
  (`each_track_keeps_its_own_dsp_epoch_and_index_state`);
* a loss on one track is spliced into that track only — the defect per-track
  index sequences exist for (`a_loss_on_one_track_splices_that_track_only`,
  `a_journal_queue_drop_on_one_track_is_spliced_into_that_track`);
* a crash mid two-track meeting recovers both tracks
  (`a_crash_mid_two_track_meeting_recovers_both_tracks`), and a journal left
  behind by the **shipped v0.8.0 build** — a hand-written 22-A marker, one track,
  no per-track index key — still recovers with its splice intact
  (`a_journal_abandoned_by_the_shipped_22a_build_still_recovers`);
* migration 3 upgrades a database full of 22-A meetings without a backfill, and
  a fresh install lands on `user_version = 3` in one pass
  (`docs/pr-screenshots/YV106/schema-after-migration-3.txt`).

## What was verified by hand

`sqlite3` was run against the database a test had just created, rather than
against a hand-built one — the file in `schema-after-migration-3.txt` is the real
artifact of `Database::open`, with `PRAGMA user_version` = 3 and all three new
columns in place (`track INTEGER notnull=1 default=0`, which is the no-backfill
claim as SQLite reports it).

## What is still mic-only in production

`SessionEngine::start` still builds a **one-track** `SessionConfig`, because a
second track with no producer would finalize an empty spill on every meeting.
`SessionConfig::virtual_meeting` (tracks: 2) and `meeting::fan_out_tap_block` are
the seams YV100 flips on when the tap exists; they are exercised by the tests
here, not by the shipping start path.
