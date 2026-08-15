# YV105 — what was verified by hand, and what could not be

## There are no screenshots in this folder, and there should not be

YV105 changes no UI. It adds rows to a const table, one pure decision function, five
test binaries and a regenerated document. Nothing a user can see moved, so a screenshot
would be a picture of an unrelated screen taken to satisfy a checklist. The evidence
that belongs here is the transcripts, and they are in this folder:

* `acceptance-tests.txt` — the five commands the backlog names, plus `matrix_coverage`.
* `published-sweep.txt` — the 15-binary sweep exactly as the document prints it.
* `row-count.txt` — the row-count criterion, computed two ways with the arithmetic shown.
* `non-vacuous-mutations.txt` — five mutations of the shipping code, and which
  assertions went red for each.

## The hardware this item cannot touch, stated plainly

Four of the five rows are about a CoreAudio process tap. **There is no tap on `main`** —
YV100 (#123) is still open — so there was nothing to grant permission to, nothing to
revoke, and nothing to lose mid-meeting. Even with YV100 merged, a real tap needs the
`NSAudioCaptureUsageDescription` TCC grant and macOS 14.4, and CI has neither.

So no manual tap repro was performed, and none is claimed. That is not a hole this item
could have filled by trying harder:

* **Row 1** (denied at start) needs a machine where the user has explicitly refused
  system-audio capture, and the code that would ask — `prewarm_tap` — is in #125.
* **Row 2** (revoked mid-meeting) needs a live tap to revoke. Its behaviour is also
  intermittent by nature when it arrives via OS-4's ghost rather than a revocation.
* **Row 14** (output device changes) is the one that could in principle be done by hand
  — connect AirPods mid-meeting — but nothing arms the output watch yet, so the only
  thing a real AirPods connect would demonstrate today is `record.rs` logging that a
  rebuild action arrived on the mic path, which is what it is written to do.

What replaces the manual repro is the substitution this repo already established for
YV104: the failure is replayed **as data** through the same functions the app runs —
`fold_block` over exact-zero buffers with real anchors, `ghost_tick` at the shipping 60 s
`WATCHDOG_INTERVAL`, `meeting::watchdog_tick` with a healthy mic and a dead tap, and
`InputFormatWatch` driven through a real AirPods burst with its selector events. The
numbers under test are the numbers the app would compute; what is missing is only the
kernel delivering them.

## What WAS checked on this machine

* macOS 26.5.2, arm64. `cargo test` in full: **66 binaries, 883 tests, 0 failures**.
* Row 12's gate was exercised against every OS version this app will meet, including
  `OsVersion::UNKNOWN`, and against the version the suite is actually running on — the
  one assertion in this branch that reads the real machine.
* Five mutations of the shipping code, each reverted (see `non-vacuous-mutations.txt`).
  One of them found a genuine weakness in this branch's own row-14 test, which was
  rewritten before this PR was opened rather than reported as a pass.
