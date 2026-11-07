# Yap data lifecycle

Every file Yap writes, who writes it, when it is deleted, and what happens to it
on a crash, a clean quit and an uninstall. Added by `yap-1106-meeting-export-and-leftovers`;
keep it current when a writer is added or retired.

Two roots:

- **The data dir** — `~/Library/Application Support/WilsonVoice/` (the test
  channel uses `WilsonVoice-Test/`; `YAP_DATA_DIR` moves the whole root for
  automation). The directory NAME is frozen: TCC grants and the SQLite history
  hang off it. Resolved once in `app_paths.rs`.
- **The export folder** — `~/Documents/Yap/Meetings/` by default, changeable in
  Settings › Meetings (`meeting_export_dir` in `settings.json`). Created on the
  first export, never at launch. It holds the user's documents, not app state:
  nothing in Yap deletes anything in it.

"Launch" below means the leftovers sweep (`leftovers.rs`, run on the hygiene
thread at every launch, logged as `open leftovers swept=<n> bytes=<n> …`) or the
daily hygiene sweep (`hygiene.rs`, 30 s after launch and every 24 h, logged as
`YV73 hygiene: …`). A clean quit logs `exit clean closed=<steps> ms=<n>`; a quit
where a step timed out logs `exit unclean … missed=<steps>`.

## The data dir

| Path | Written by | When | Deleted | On crash | On clean quit | On uninstall |
|---|---|---|---|---|---|---|
| `wilson_voice.db` (+ `-wal`, `-shm`) | `db.rs` — the SQLite history: transcripts, meetings, segments, failed takes, settings_kv | first launch, every take | never by Yap; Settings › Privacy › Erase everything (`erase.rs`) | WAL replays on next open; a corrupt file is quarantined as `.corrupt-<stamp>` | WAL checkpointed (`exit teardown … db_checkpoint`) | stays — user data; delete the folder to remove it |
| `settings.json` (+ `.json.tmp`) | `write_settings_file` (atomic temp + rename) | every settings save | never; a corrupt file is moved to `.bak` and defaults load | the `.tmp` sibling is swept by hygiene after 7 days | nothing to do | stays |
| `license.json` | `license.rs` | activation | deactivation | n/a | n/a | stays (re-activation reads it) |
| `yap.instance.lock` | `lifecycle.rs` | launch (advisory `flock`) | never — the kernel drops the lock on any exit | lock released by the kernel; the stale file is harmless | lock released | stays, 0 bytes |
| `.hide-hint-shown` | `lifecycle.rs` | the first time the window is hidden | never | n/a | n/a | stays |
| `last-run-signature.json` | `perm_probe.rs` | every launch | overwritten each launch | n/a | n/a | stays |
| `permission-signature.json` | `perm_truth.rs` | every permission probe | overwritten | n/a | n/a | stays |
| `notes-jobs.json` | `summary_jobs.rs` — the automatic-notes upgrade ledger | first automatic notes run | never | n/a | n/a | stays |
| `logs/yap.log` (+ `yap.log.1…4`) | `logging.rs` | continuously | rotated at ~2 MB, 4 backups; the directory is capped at 20 MB by hygiene | the tail is what a crash report reads | flushed | stays |
| `logs/crash-summary.txt` | `open_logs_dir` | when Settings › Diagnostics opens the logs folder | overwritten | n/a | n/a | stays |
| `recordings/*.wav` | `record.rs` — the in-flight dictation take | press-to-talk start | at every launch (`record::sweep_stale_wavs`) and when the take is transcribed | left in place; swept at next launch | the in-flight take is awaited (`await_take`) then removed | stays until the folder is deleted |
| `recovery/<id>.wav` | `record.rs` — a failed take, parked for Retry | a take whose transcription failed | with its `failed_dictations` row after 7 days (`db::FAILED_TAKE_RETENTION_DAYS`); an unreferenced clip past 7 days is removed by the leftovers sweep at launch and by hygiene daily | left in place; Home offers Retry | nothing to do | stays |
| `recovery/<id>.in_progress.json`, `recovery/<id>.tN.spill.pcm`, `recovery/<id>.tN.capture.wav` | `meeting.rs` — the live meeting journal | while a meeting records | finalized into `meetings/<id>.tN.wav` when the meeting stops | the orphan journal is finalized at next launch into a `partial` meeting with Resume | the meeting is finalized (`exit teardown … meeting_stop`, budget-exempt) | stays |
| `meetings/<id>.tN.wav`, `meetings/<id>.tN.aec.wav` | `meeting.rs` / `meeting_aec.rs` — meeting audio per track | meeting stop | 7 days after a complete transcript (`meetings::AUDIO_RETENTION_DAYS`), 30 days or at the size ceiling for an untranscribed one; immediately on Delete meeting | kept; transcription resumes | kept | stays |
| `meetings/<id>.index.jsonl`, `meetings/<id>.tN.index.jsonl` | `meeting.rs` — host-time anchors per track | with the WAV | with the WAV | kept | kept | stays |
| `meetings/<id>.tN.asr-progress.json` | `meeting_asr.rs` — the transcription resume ledger | while transcribing | with the WAV; for a meeting already `complete`, at the next launch (leftovers sweep) | kept — it is what the resume reads | kept | stays |
| `meetings/<id>.attempts.json` | `meeting_pipeline.rs` — the crash-attempt marker | when a transcription attempt starts | as above | read at next launch to count unprogressed crashes | kept | stays |
| `models/*.bin`, `models/*.onnx`, `models/*.gguf` | `models.rs` / `diarize.rs` — downloaded engines | Settings › Dictation / the Notetaker setup | Settings › Dictation › Remove, or Erase everything | a `*.partial` / `*.part` download resumes; one older than 7 days is swept by hygiene | nothing to do | stays (largest files; delete the folder) |
| `tmp/` | **nobody** — the retired Python sidecar's scratch (`wv-asr-*`) | never, since YV34 | every entry at launch (leftovers sweep), then the directory | n/a | n/a | gone after one launch of this build |
| `venv/`, `__pycache__/`, `asr_worker.py`, `config.yaml`, 0-byte `yap.db` | **nobody** — the retired Python sidecar | never | once, at the first launch of this build, one `leftovers retired-sidecar removed name=…` line each; a `yap.db` with bytes is never touched | n/a | n/a | gone |
| `*.md` in the root | the pre-1106 meeting export | never | moved into the export folder at launch (`leftovers export moved name=…`), never deleted; a name clash gets a `-1`, `-2` suffix | n/a | n/a | moved on first launch |
| `cache/` | unknown — present on the July install, no writer in this build | — | **not swept**: provenance unproven; listed here so the next sweep can decide | n/a | n/a | stays |

## The export folder (`~/Documents/Yap/Meetings/` by default)

| Path | Written by | When | Deleted | On crash | On clean quit | On uninstall |
|---|---|---|---|---|---|---|
| `<YYYY-MM-DD> <title>.md` | `write_meeting_markdown` — Export Markdown… | on the user's click (list row menu or detail header) | never by Yap; a re-export of the same meeting replaces its own file | the `.<name>.part` temp file is left; the export is written with temp + rename so the final name is never half-written | nothing to do | stays — it is the user's document |

Show in Finder selects the meeting's export when it exists, else opens the
folder. Copy as Markdown writes the same bytes to the clipboard and touches no
file.

## Elsewhere on the Mac

| Path | Written by | When | Deleted |
|---|---|---|---|
| `~/Desktop/Yap-Support-<stamp>.zip` | `support.rs` — the support bundle | Settings › Diagnostics › Send a report | by the user |
| `~/Library/LaunchAgents/…yap…plist` | tauri-plugin-autostart | Settings › Advanced › Launch at login ON | the same toggle OFF |
| the clipboard | `paste.rs` | every dictation paste (restored after a sequenced paste), Copy as Markdown | by the next copy |

## Uninstall

Drag `Yap.app` to the Trash. The data dir and the export folder stay (macOS
does not remove Application Support for a deleted app). To remove everything:
Settings › Privacy › Erase everything first (it clears the history, models,
recordings and meeting audio in the data dir), then delete
`~/Library/Application Support/WilsonVoice/` and, if wanted, `~/Documents/Yap/`.
