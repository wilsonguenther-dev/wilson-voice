# YV109 — manual verification, and what could NOT be verified

## The honest headline

**The camera demo the backlog asks for could not be run, and not for want of a
TCC grant.** There is no process tap in this build to grant anything to.
`desktop/src-tauri/src/syscapture.rs` on `main` is YV104's ghost watchdog — a
pure state machine over a tap's liveness — and it contains no CoreAudio FFI at
all. The tap itself is **PR #123 (YV100), still open**, as are the TCC pre-warm
step (#125, YV102) and the host-time merge (#130, YV107). A build of this branch
records one track, from the microphone, exactly as 22-A shipped.

So the manual half of this item is a script with its preconditions written down
(`docs/yap22b-phase-demo.md`, which names the three pull requests it waits on and
carries an empty results table), and the gate for this PR is the automated half:
the pure state-machine / fixture tests, per the standing rule that where hardware
cannot run in CI the pure tests are the gate.

Saying "verified on a real Zoom call" here would have been a false capability
claim about a code path that does not exist on this branch.

## What WAS run by hand

### 1. The corpus was grown, hashed, and verified without cargo

```
$ cargo test --test meeting_eval meeting_eval_generate_two_track_ordering -- --ignored --nocapture
wrote /Users/wilsonguenther/yap-eval-corpus/meetings/two-track-ordering — two tracks, 76.0s, 13 markers
wrote .../tests/fixtures/meeting_eval_manifest.json and .../meeting_eval_manifest.sha256 (17 files)

$ cd ~/yap-eval-corpus/meetings && shasum -a 256 -c .../meeting_eval_manifest.sha256
… all 17 OK, including
two-track-ordering/meta.json: OK
two-track-ordering/reference.txt: OK
two-track-ordering/track0-anchors.json: OK
two-track-ordering/track0.wav: OK
two-track-ordering/track1-anchors.json: OK
two-track-ordering/track1.wav: OK
```

The three fixtures YV90 grew are untouched — their hashes in the committed
manifest are byte-identical before and after this item, which is the check that
the writer re-hashed rather than regenerated.

### 2. The fixture was read back and the transcript read by eye

`docs/pr-screenshots/YV109/two-track-transcript.md` — the decoded, merged,
rendered conversation, with the four sub-second Me/Them pairs in the order they
were spoken. Read on screen, not only asserted.

### 3. One marker was changed because the decoder disagreed with it

The first cut of fixture (d) used `quilt` at 44 s on the tap track. The real
model heard **`quill`**, the gate failed with the whole decoded token list, and
the word was changed to `walrus` and the fixture regrown. Worth recording
because it is the gate doing its job on its first run: a marker the decoder
cannot reproduce makes the ordering comparison shorter and quietly weaker, which
is why "every marker comes back exactly once" is asserted before the order is
scored rather than after.

### 4. The skip path was exercised as a missing corpus, not as a flag

```
$ YAP_EVAL_CORPUS=/tmp/does-not-exist-yv109 cargo test --test meeting_eval
test result: ok. 23 passed; 0 failed; 4 ignored; finished in 0.02s
```

Seven prints of `meeting eval corpus not found at ~/yap-eval-corpus/meetings,
skipping` (six before this item, seven after — the new gate joined the same
path rather than growing a second one), and `grep` still matches the constant.

## What a reviewer should NOT read into this PR

* That a Zoom call has been recorded end to end. It has not, on this branch.
* That YV107's merge is wired into the pipeline. It is not; this item rebases
  with the eval harness's own independent reference and says so in the file
  header, in the E2E's doc comment, and in the changelog entry.
* That the 10 ms residual is a hardware measurement. It is a measurement of the
  chain around a synthetic capture whose clock error is declared rather than
  observed. The number that IS hardware-shaped is the one the demo script exists
  to collect, and it is not collected yet.
