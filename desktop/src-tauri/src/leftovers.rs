//! yap-1106-meeting-export-and-leftovers — the startup leftovers sweep.
//!
//! `hygiene::sweep` (YV73) is the DAILY sweep: interrupted downloads, orphan
//! recovery WAVs past retention, stale `.tmp` siblings, logs over budget. It is
//! deliberately narrow and runs thirty seconds after launch. This module is the
//! ONE pass that runs at every launch, before that, over the classes YV73 does
//! not cover — the things a data directory accumulates over months and nobody
//! sweeps, found by listing a real install (`docs/DATA-LIFECYCLE.md` has the
//! full ledger):
//!
//! * `tmp/` — scratch dirs the retired Python ASR sidecar left (`wv-asr-*`).
//!   Nothing in this build writes there, so at launch every entry is orphaned.
//! * `<id>.tN.asr-progress.json` + `<id>.attempts.json` in `meetings/` for a
//!   meeting whose row is COMPLETE: the resume ledger has done its job.
//! * `recovery/*.wav` past the 7-day rule the UI shows, not pointed at by a
//!   `failed_dictations` row — the same selector YV73 uses, run earlier.
//! * the retired Python sidecar: `venv/`, `__pycache__/`, `asr_worker.py`,
//!   `config.yaml` and a 0-byte `yap.db`. Removed ONCE, one log line each; a
//!   `yap.db` with bytes in it is never touched.
//! * stray `*.md` meeting exports in the data dir root (where exports landed
//!   before the export folder existed) — MOVED into the export folder, never
//!   deleted: they are a user's file.
//!
//! Every decision that does not need the filesystem is a pure function over a
//! name or a [`FileStat`], so the rules are unit-tested without I/O; [`sweep`]
//! is the only function that deletes, and it is tested over a fixture tree
//! built under `std::env::temp_dir()` — never a real data dir.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::hygiene::{retention_cutoff, select_orphan_recovery_wavs, FileStat};

/// The retired Python sidecar's files, by name, directly under the data root.
pub const RETIRED_SIDECAR_DIRS: [&str; 2] = ["venv", "__pycache__"];
pub const RETIRED_SIDECAR_FILES: [&str; 2] = ["asr_worker.py", "config.yaml"];
/// The sidecar's never-used database. Removed only when it is EMPTY.
pub const RETIRED_SIDECAR_EMPTY_DB: &str = "yap.db";

/// What one launch sweep released, per class. Same contract as
/// `hygiene::SweepOutcome`: stable fields, never conditionally dropped.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LeftoversOutcome {
    /// Entries removed from `tmp/` (files or whole directories).
    pub tmp: usize,
    /// Resume ledgers / attempt markers of complete meetings.
    pub progress: usize,
    /// Expired, unreferenced recovery clips.
    pub recovery: usize,
    /// Retired sidecar files and directories.
    pub sidecar: usize,
    /// Root `.md` exports moved into the export folder.
    pub exports_moved: usize,
    /// Bytes freed by deletions (moves free nothing).
    pub bytes: u64,
    /// Files the sweep wanted to remove or move but could not. Surfaced as a
    /// WARN line rather than an error: hygiene can never fail a launch.
    pub failed: usize,
}

impl LeftoversOutcome {
    pub fn swept(&self) -> usize {
        self.tmp + self.progress + self.recovery + self.sidecar + self.exports_moved
    }

    /// The launch line: `open leftovers swept=<n> bytes=<n> …`. Emitted on
    /// EVERY launch, zero or not, so a log can prove the sweep ran.
    pub fn log_line(&self) -> String {
        format!(
            "open leftovers swept={} bytes={} tmp={} progress={} recovery={} sidecar={} exports_moved={} failed={}",
            self.swept(),
            self.bytes,
            self.tmp,
            self.progress,
            self.recovery,
            self.sidecar,
            self.exports_moved,
            self.failed
        )
    }
}

/// `<id>.asr-progress.json`, `<id>.tN.asr-progress.json` or `<id>.attempts.json`
/// → `<id>`. Anything else → `None`. Mirrors the spellings
/// `meeting_pipeline::meeting_sidecar_paths` scans for.
pub fn progress_file_meeting_id(name: &str) -> Option<&str> {
    let rest = name.strip_suffix(".asr-progress.json");
    if let Some(rest) = rest {
        // `<id>` or `<id>.tN`
        let id = match rest.rsplit_once('.') {
            Some((id, track)) if track.starts_with('t') && track[1..].chars().all(|c| c.is_ascii_digit()) => id,
            _ => rest,
        };
        return (!id.is_empty()).then_some(id);
    }
    let id = name.strip_suffix(".attempts.json")?;
    (!id.is_empty()).then_some(id)
}

/// Pure: the progress ledgers / attempt markers whose meeting is complete.
pub fn select_completed_progress_files(files: &[FileStat], completed_ids: &[String]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| {
            f.path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(progress_file_meeting_id)
                .is_some_and(|id| completed_ids.iter().any(|c| c == id))
        })
        .map(|f| f.path.clone())
        .collect()
}

/// Pure: a root `.md` file is a stray export (only `.md`, only a plain file, no
/// dotfile — `.foo.md.part` is a half-written export the writer owns).
pub fn is_stray_root_export(name: &str) -> bool {
    name.ends_with(".md") && !name.starts_with('.')
}

/// Pure: is this root entry a retired-sidecar leftover? `size` matters only for
/// `yap.db`, which goes only when empty.
pub fn is_retired_sidecar(name: &str, is_dir: bool, size: u64) -> bool {
    if is_dir {
        RETIRED_SIDECAR_DIRS.contains(&name)
    } else {
        RETIRED_SIDECAR_FILES.contains(&name) || (name == RETIRED_SIDECAR_EMPTY_DB && size == 0)
    }
}

fn read_stats(dir: &Path, now: SystemTime) -> Vec<FileStat> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| FileStat {
                path: entry.path(),
                size: meta.len(),
                modified: meta.modified().unwrap_or(now),
            })
        })
        .collect()
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// Remove a file or a whole directory. Returns the bytes it held.
fn remove_entry(path: &Path, is_dir: bool) -> Result<u64, std::io::Error> {
    if is_dir {
        let bytes = dir_size(path);
        std::fs::remove_dir_all(path)?;
        Ok(bytes)
    } else {
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        std::fs::remove_file(path)?;
        Ok(bytes)
    }
}

/// A destination name that does not collide: `a.md`, then `a-1.md`, `a-2.md`…
pub fn free_destination(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.strip_suffix(".md").unwrap_or(name);
    for n in 1..1000 {
        let candidate = dir.join(format!("{stem}-{n}.md"));
        if !candidate.exists() {
            return candidate;
        }
    }
    first
}

/// Move one file; falls back to copy + unlink when `rename` crosses a volume.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
    }
}

/// Run the launch sweep against `data_dir`.
///
/// * `completed_ids` — the meeting rows in state `complete` (their ledgers go).
/// * `keep_recovery` — the `failed_dictations` wav list; `None` = the DB could
///   not be asked, so `recovery/` is left ALONE (same posture as YV73).
/// * `export_dir` — where stray root exports are moved. Created on demand only
///   when there is something to move.
pub fn sweep(
    data_dir: &Path,
    completed_ids: &[String],
    keep_recovery: Option<&[String]>,
    export_dir: &Path,
    now: SystemTime,
) -> LeftoversOutcome {
    let mut out = LeftoversOutcome::default();
    let name_of = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    // 1. tmp/ — everything is orphaned at launch; the dir itself goes last.
    let tmp = data_dir.join("tmp");
    if let Ok(entries) = std::fs::read_dir(&tmp) {
        for entry in entries.filter_map(|e| e.ok()) {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            match remove_entry(&entry.path(), is_dir) {
                Ok(b) => {
                    out.tmp += 1;
                    out.bytes = out.bytes.saturating_add(b);
                }
                Err(e) => {
                    out.failed += 1;
                    log::warn!("leftovers tmp remove failed name={} err={e}", name_of(&entry.path()));
                }
            }
        }
        if std::fs::remove_dir(&tmp).is_ok() {
            log::info!("leftovers tmp dir removed");
        }
    }

    // 2. ledgers of complete meetings.
    let meetings = read_stats(&data_dir.join("meetings"), now);
    for path in select_completed_progress_files(&meetings, completed_ids) {
        match remove_entry(&path, false) {
            Ok(b) => {
                out.progress += 1;
                out.bytes = out.bytes.saturating_add(b);
            }
            Err(e) => {
                out.failed += 1;
                log::warn!("leftovers progress remove failed name={} err={e}", name_of(&path));
            }
        }
    }

    // 3. expired recovery clips — the 7-day rule, the same selector YV73 uses.
    if let Some(keep) = keep_recovery {
        let recovery = read_stats(&data_dir.join("recovery"), now);
        for path in select_orphan_recovery_wavs(&recovery, keep, retention_cutoff(now)) {
            match remove_entry(&path, false) {
                Ok(b) => {
                    out.recovery += 1;
                    out.bytes = out.bytes.saturating_add(b);
                    log::info!("leftovers recovery expired removed name={} bytes={b}", name_of(&path));
                }
                Err(e) => {
                    out.failed += 1;
                    log::warn!("leftovers recovery remove failed name={} err={e}", name_of(&path));
                }
            }
        }
    }

    // 4 + 5. the data root: retired sidecar files, stray exports.
    if let Ok(entries) = std::fs::read_dir(data_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if is_retired_sidecar(&name, meta.is_dir(), meta.len()) {
                match remove_entry(&path, meta.is_dir()) {
                    Ok(b) => {
                        out.sidecar += 1;
                        out.bytes = out.bytes.saturating_add(b);
                        log::info!("leftovers retired-sidecar removed name={name} bytes={b}");
                    }
                    Err(e) => {
                        out.failed += 1;
                        log::warn!("leftovers retired-sidecar remove failed name={name} err={e}");
                    }
                }
            } else if meta.is_file() && is_stray_root_export(&name) {
                if let Err(e) = std::fs::create_dir_all(export_dir) {
                    out.failed += 1;
                    log::warn!("leftovers export dir create failed err={e}");
                    continue;
                }
                let dest = free_destination(export_dir, &name);
                match move_file(&path, &dest) {
                    Ok(()) => {
                        out.exports_moved += 1;
                        log::info!("leftovers export moved name={} bytes={}", name_of(&dest), meta.len());
                    }
                    Err(e) => {
                        out.failed += 1;
                        log::warn!("leftovers export move failed name={name} err={e}");
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    #[test]
    fn progress_names_resolve_to_their_meeting() {
        assert_eq!(progress_file_meeting_id("abc.asr-progress.json"), Some("abc"));
        assert_eq!(progress_file_meeting_id("abc.t0.asr-progress.json"), Some("abc"));
        assert_eq!(progress_file_meeting_id("abc.t12.asr-progress.json"), Some("abc"));
        assert_eq!(progress_file_meeting_id("abc.attempts.json"), Some("abc"));
        // A dotted id keeps its dots when the suffix is not a track.
        assert_eq!(progress_file_meeting_id("a.b.asr-progress.json"), Some("a.b"));
        assert_eq!(progress_file_meeting_id("abc.t0.wav"), None);
        assert_eq!(progress_file_meeting_id("abc.index.jsonl"), None);
        assert_eq!(progress_file_meeting_id(".asr-progress.json"), None);
    }

    #[test]
    fn only_complete_meetings_lose_their_ledgers() {
        let now = SystemTime::UNIX_EPOCH + DAY * 400;
        let f = |p: &str| FileStat { path: PathBuf::from(p), size: 1, modified: now };
        let files = vec![
            f("/m/done.t0.asr-progress.json"),
            f("/m/done.t1.asr-progress.json"),
            f("/m/done.attempts.json"),
            f("/m/done.t0.wav"),
            f("/m/live.t0.asr-progress.json"),
            f("/m/live.attempts.json"),
        ];
        let got = select_completed_progress_files(&files, &["done".to_string()]);
        assert_eq!(
            got,
            vec![
                PathBuf::from("/m/done.t0.asr-progress.json"),
                PathBuf::from("/m/done.t1.asr-progress.json"),
                PathBuf::from("/m/done.attempts.json"),
            ]
        );
    }

    #[test]
    fn retired_sidecar_rules_are_exact() {
        assert!(is_retired_sidecar("venv", true, 0));
        assert!(is_retired_sidecar("__pycache__", true, 0));
        assert!(is_retired_sidecar("asr_worker.py", false, 14_000));
        assert!(is_retired_sidecar("config.yaml", false, 393));
        assert!(is_retired_sidecar("yap.db", false, 0));
        // A yap.db with bytes is NOT ours to delete.
        assert!(!is_retired_sidecar("yap.db", false, 10));
        // Never the real history or anything beside it.
        assert!(!is_retired_sidecar("wilson_voice.db", false, 0));
        assert!(!is_retired_sidecar("settings.json", false, 0));
        assert!(!is_retired_sidecar("models", true, 0));
        assert!(!is_retired_sidecar("venv", false, 0));
    }

    #[test]
    fn stray_exports_are_plain_md_files_only() {
        assert!(is_stray_root_export("meeting-9-19-am-20261106-091957.md"));
        assert!(!is_stray_root_export(".meeting.md.part"));
        assert!(!is_stray_root_export("settings.json"));
        assert!(!is_stray_root_export("README.markdown"));
    }

    /// The sweep over a fixture tree shaped like a real install. Built under
    /// the system temp dir — NEVER a real data dir — and removed after.
    #[test]
    fn sweep_removes_leftovers_and_spares_live_data() {
        let now = SystemTime::now();
        let base = std::env::temp_dir().join(format!("yap-leftovers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        let export = base.join("Documents").join("Yap").join("Meetings");
        for d in ["tmp/wv-asr-aaa", "tmp/wv-asr-bbb", "meetings", "recovery", "venv/lib", "__pycache__", "models", "logs"] {
            std::fs::create_dir_all(data.join(d)).unwrap();
        }
        let w = |rel: &str, bytes: &[u8]| std::fs::write(data.join(rel), bytes).unwrap();
        w("tmp/wv-asr-aaa/x.wav", b"12345");
        w("tmp/stray.txt", b"1");
        w("meetings/done.t0.asr-progress.json", b"{}");
        w("meetings/done.attempts.json", b"{}");
        w("meetings/done.t0.wav", b"RIFF");
        w("meetings/done.index.jsonl", b"{}");
        w("meetings/live.t0.asr-progress.json", b"{}");
        w("recovery/old.wav", b"old");
        w("recovery/kept.wav", b"kept");
        w("recovery/fresh.wav", b"fresh");
        w("recovery/take.in_progress.json", b"{}");
        w("venv/lib/site.py", b"x");
        w("__pycache__/a.pyc", b"x");
        w("asr_worker.py", b"print()");
        w("config.yaml", b"a: 1");
        w("yap.db", b"");
        w("wilson_voice.db", b"not empty");
        w("settings.json", b"{}");
        w("models/ggml-small.bin", b"model");
        w("logs/yap.log", b"log");
        w("meeting-9-19-am.md", b"# Meeting");
        w(".meeting.md.part", b"half");
        // An export already in the folder with the same name: never overwritten.
        std::fs::create_dir_all(&export).unwrap();
        std::fs::write(export.join("meeting-9-19-am.md"), b"earlier").unwrap();

        let old = now - DAY * 9;
        for n in ["old.wav", "kept.wav"] {
            let f = std::fs::File::options().write(true).open(data.join("recovery").join(n)).unwrap();
            f.set_modified(old).unwrap();
        }

        let keep = vec![data.join("recovery/kept.wav").display().to_string()];
        let out = sweep(&data, &["done".to_string()], Some(&keep), &export, now);

        assert_eq!(out.tmp, 3, "{out:?}");
        assert!(!data.join("tmp").exists(), "tmp dir goes with its contents");
        assert_eq!(out.progress, 2);
        assert!(!data.join("meetings/done.t0.asr-progress.json").exists());
        assert!(!data.join("meetings/done.attempts.json").exists());
        assert!(data.join("meetings/done.t0.wav").exists(), "audio is never a candidate");
        assert!(data.join("meetings/done.index.jsonl").exists(), "anchors stay with the audio");
        assert!(data.join("meetings/live.t0.asr-progress.json").exists(), "a live ledger stays");
        assert_eq!(out.recovery, 1);
        assert!(!data.join("recovery/old.wav").exists());
        assert!(data.join("recovery/kept.wav").exists(), "a row still points here");
        assert!(data.join("recovery/fresh.wav").exists(), "inside the 7-day window");
        assert!(data.join("recovery/take.in_progress.json").exists());
        assert_eq!(out.sidecar, 5);
        for gone in ["venv", "__pycache__", "asr_worker.py", "config.yaml", "yap.db"] {
            assert!(!data.join(gone).exists(), "{gone} should be gone");
        }
        for kept in ["wilson_voice.db", "settings.json", "models/ggml-small.bin", "logs/yap.log", ".meeting.md.part"] {
            assert!(data.join(kept).exists(), "{kept} must survive");
        }
        assert_eq!(out.exports_moved, 1);
        assert!(!data.join("meeting-9-19-am.md").exists());
        assert_eq!(std::fs::read(export.join("meeting-9-19-am.md")).unwrap(), b"earlier");
        assert_eq!(std::fs::read(export.join("meeting-9-19-am-1.md")).unwrap(), b"# Meeting");
        assert_eq!(out.failed, 0);
        assert_eq!(out.swept(), 12);
        assert!(out.bytes > 0);
        assert!(out.log_line().starts_with("open leftovers swept=12 bytes="), "{}", out.log_line());

        // A second launch finds nothing — the sidecar removal is a one-time event.
        let again = sweep(&data, &["done".to_string()], Some(&keep), &export, now);
        assert_eq!(again, LeftoversOutcome::default());

        std::fs::remove_dir_all(&base).unwrap();
    }

    /// With the DB unreadable, recovery is left alone; everything else still runs.
    #[test]
    fn unreadable_row_list_spares_recovery() {
        let now = SystemTime::now();
        let base = std::env::temp_dir().join(format!("yap-leftovers-norows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let data = base.join("data");
        std::fs::create_dir_all(data.join("recovery")).unwrap();
        std::fs::write(data.join("recovery/old.wav"), b"old").unwrap();
        let f = std::fs::File::options().write(true).open(data.join("recovery/old.wav")).unwrap();
        f.set_modified(now - DAY * 30).unwrap();
        std::fs::write(data.join("config.yaml"), b"a: 1").unwrap();

        let out = sweep(&data, &[], None, &base.join("exports"), now);
        assert_eq!(out.recovery, 0);
        assert!(data.join("recovery/old.wav").exists());
        assert_eq!(out.sidecar, 1);
        assert!(!base.join("exports").exists(), "export dir is created only when something moves");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
