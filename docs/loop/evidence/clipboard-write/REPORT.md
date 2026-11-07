# yap-1107-clipboard-write-never-fails-silently — clipboard write report

Written 2026-11-07T16:59Z by loop three coder B4 (code only, nothing compiled).

## Acceptance (1) — the write paths and the observed cause

Write paths before this branch (base 31f07d0):

- `paste.rs` `copy_text` — tauri-plugin-clipboard-manager `write_text`, plugin `Ok` trusted, never read back. Used by the take's copy, the Copy card, the no-receipt recopy and "Copy Last Transcript".
- `paste_tx.rs` `mac::settle` — `let _ = clipboard.write_text(saved)` (restore), `let _ = clipboard.clear()`, `let _ = write_text(transcript)` (leave the transcript after a dead ⌘V). Three discarded results.
- `paste_tx.rs` `mac::settle_then_copy` — plugin `write_text`, not read back.
- `paste_tx.rs` `mac::settle_then_copy_transient` — NSPasteboard declareTypes + setString, not read back.
- `paste_tx.rs` `mac::publish_and_chord` — the NSPasteboard promise path (declareTypes:owner: with the `YapPasteProvider` owner). Its changeCount was already checked.

Observed failure (Yap Test logs, read from a copy, `WilsonVoice-Test.before-20261107T165422Z/logs/yap.log*`):

```
[2026-11-07T16:11:18Z INFO  wilson_voice_lib::paste] insert path=CopyCard app=unknown fallback=secure_field
[2026-11-07T16:11:18Z INFO  wilson_voice_lib::take_stage] take_stage emit stage=paste_not_confirmed ...
[2026-11-07T16:11:18Z WARN  wilson_voice_lib] take 01829888-…: Not pasted: that's a password field. Held in the pill, not on your clipboard.
```

That take was a Secure Input / password-field hold. Yap deliberately does NOT put the take on the clipboard there (`hold_with(.., clipboard: false)`), so `copied` was false. The take path read `copied == false` as a clipboard failure and showed BOTH messages: `take_error::paste_last_error` set CLIPBOARD_FAILED ("Yap could not put the text on the clipboard — it is saved in History") and lib.rs emitted `paste_failed` with PASTE_NOT_COPIED ("The paste may not have landed, and the text is not on your clipboard — press Copy again"). Nothing failed to write. No clipboard-write error line exists in today's logs. The other write-side risks found by reading the code: three discarded results in `settle`, and no read-back anywhere, so a second writer racing our write (changeCount race) was never noticed.

## Fixes

- Every write goes through `paste_tx::write_verified`: write, then check that changeCount moved and the read-back string matches, retry after 20/60/180 ms, and only then return an error. The cause is logged by name (refused / unchanged / clobbered / empty).
- `PasteOutcome::clipboard_withheld()` + `take_error::take_paste_last_error` / `should_emit_paste_failed`: the secure-field hold gets neither message. A real clipboard failure (all retries used up) still gets both, once.
- `errors.ts` `createPasteFailedGate`: at most one `paste_failed` toast per take id (the ×2).

## Acceptance (3) — contention test (the integrator runs it)

`cargo test --lib paste_tx::tests::clipboard_contention_200_writes -- --ignored --nocapture` writes `contention-<unix>.json` into this folder: 200 writes against a `pbcopy` loop, each one counted as verified or reported failed, with `silent_failures` required to be 0.
