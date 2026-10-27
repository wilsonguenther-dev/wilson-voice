//! PRIV-B — erasure is ONE operation over the whole product, not over SQLite.
//!
//! Three items in this loop turned a dictation clip from transient into days
//! of retained raw audio and partial transcript text: Y3-A spills a long take
//! to disk, Y3-D parks a cancelled take in the recovery dir instead of
//! deleting it, and DB-B persists per-take chunk TEXT in `take_chunks`. Before
//! this module "Clear History" was `db.clear_transcripts()` and nothing else,
//! so a user who dictated something regrettable, cancelled, and clicked Clear
//! had their rows VACUUMed and the full audio plus the partial words still on
//! disk for a week, with nothing saying so.
//!
//! Two operations, both refused by their command while a take or a meeting is
//! live (a live take's spill sits in the very directory these sweep):
//!
//! * [`clear_history_everywhere`] — Clear History. The SQLite half
//!   (`Database::clear_transcripts`: transcripts, FTS, harvested vocabulary,
//!   stats, `take_chunks`, every parked `failed_dictations` row, then VACUUM),
//!   followed by the FILESYSTEM half: every file in `recovery/` (parked WAVs,
//!   `.spill.pcm` journals, `.capture.wav` spills, recovered `<id>.wav`s),
//!   every file in `recordings/`, and every meeting audio file in `meetings/`
//!   that NO meeting row owns. Meetings themselves are their own surface with
//!   their own delete, so a meeting a row still owns is kept.
//! * [`delete_all_recordings`] — the separate, explicit control. Every byte of
//!   AUDIO the app holds goes: parked takes (row + chunk text, which is only
//!   resume state for audio that is about to stop existing), the scratch and
//!   recovery dirs, and every meeting's audio (the meeting keeps its words and
//!   is marked `audio_kept = 0`). Transcripts are kept.
//!
//! Every removal is reported, never silently swallowed: a file that could not
//! be removed is counted in [`EraseReport::failed`] and the command turns a
//! non-zero count into an error the user sees, because "Cleared" over audio
//! still on disk is exactly the lie this module exists to stop telling.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::app_paths::AppPaths;
use crate::db::Database;

/// What one erase actually removed from disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EraseReport {
    /// Files unlinked.
    pub files_removed: usize,
    /// Their total size, as stat'ed just before the unlink.
    pub bytes_removed: u64,
    /// Files that were there and could NOT be removed (permissions, I/O).
    pub failed: usize,
}

impl EraseReport {
    fn remove(&mut self, path: &Path) {
        let size = std::fs::symlink_metadata(path).map(|m| m.len()).ok();
        match std::fs::remove_file(path) {
            Ok(()) => {
                self.files_removed += 1;
                self.bytes_removed += size.unwrap_or(0);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                self.failed += 1;
                log::warn!("erase: {} not removed: {e}", path.display());
            }
        }
    }

    fn remove_tree(&mut self, path: &Path) {
        // Count what is inside before the tree goes, so the report is honest.
        for f in files_in(path) {
            self.remove(&f);
        }
        if let Err(e) = std::fs::remove_dir_all(path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                self.failed += 1;
                log::warn!("erase: {} not removed: {e}", path.display());
            }
        }
    }
}

/// Every regular file under `dir`, recursively. A missing dir is empty.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => out.extend(files_in(&p)),
            Ok(_) => out.push(p),
            Err(_) => out.push(p),
        }
    }
    out
}

/// Empty `dir` of EVERYTHING (files and sub-directories), keeping the dir.
fn empty_dir(dir: &Path, report: &mut EraseReport) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => report.remove_tree(&p),
            _ => report.remove(&p),
        }
    }
}

/// A file in `meetings/` that carries a meeting's AUDIO, or the journal
/// marker that would make startup try to rebuild audio from one: `.wav`,
/// raw `.pcm` spills and `*.meeting.in_progress.json`. Resume ledgers and
/// index sidecars hold timings, not sound or words, and are left alone.
pub fn is_meeting_audio_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    name.ends_with(".wav") || name.ends_with(".pcm") || name.ends_with(".meeting.in_progress.json")
}

/// The meeting audio NO meeting row owns. Pure: `owned` is what
/// `Database::meeting_audio_paths` returned.
pub fn select_orphan_meeting_audio(files: &[PathBuf], owned: &[PathBuf]) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| is_meeting_audio_file(f))
        .filter(|f| !owned.iter().any(|o| o == *f))
        .cloned()
        .collect()
}

/// Clear History, as the user means it. See the module doc.
pub fn clear_history_everywhere(db: &Database, paths: &AppPaths) -> Result<EraseReport, String> {
    // Owned meeting audio is read BEFORE anything is deleted, so a failure
    // here leaves everything intact rather than half-erased.
    let owned = db.meeting_audio_paths()?;
    let parked = db.clear_transcripts()?;

    let mut report = EraseReport::default();
    for wav in &parked {
        report.remove(wav);
    }
    empty_dir(&paths.recovery, &mut report);
    empty_dir(&paths.recordings, &mut report);
    for orphan in select_orphan_meeting_audio(&files_in(&paths.meetings), &owned) {
        report.remove(&orphan);
    }
    log::info!(
        "clear history: rows erased; {} file(s) / {} bytes removed from disk, {} failed",
        report.files_removed,
        report.bytes_removed,
        report.failed
    );
    Ok(report)
}

/// Delete all recordings — every byte of audio, transcripts kept. See the
/// module doc.
pub fn delete_all_recordings(db: &Database, paths: &AppPaths) -> Result<EraseReport, String> {
    let referenced = db.forget_all_recordings()?;
    let mut report = EraseReport::default();
    for wav in &referenced {
        report.remove(wav);
        // yap-meetings-fix — a meeting's resume ledgers live beside its WAV
        // and are meaningless once the audio is gone.
        if let (Some(dir), Some(stem)) = (wav.parent(), meeting_id_of(wav)) {
            for ledger in crate::meeting_pipeline::ledger_paths(dir, &stem) {
                report.remove(&ledger);
            }
        }
    }
    empty_dir(&paths.recovery, &mut report);
    empty_dir(&paths.recordings, &mut report);
    for f in files_in(&paths.meetings) {
        if is_meeting_audio_file(&f) {
            report.remove(&f);
        }
    }
    log::info!(
        "delete all recordings: {} file(s) / {} bytes removed from disk, {} failed",
        report.files_removed,
        report.bytes_removed,
        report.failed
    );
    Ok(report)
}

/// `meetings/<id>.wav` / `meetings/<id>.sys.wav` → `<id>`. Only used to find
/// the resume ledgers beside a meeting WAV; a name it cannot parse just means
/// no ledger is looked for.
fn meeting_id_of(wav: &Path) -> Option<String> {
    let name = wav.file_name()?.to_string_lossy().to_string();
    let stem = name.strip_suffix(".wav")?;
    Some(stem.split('.').next().unwrap_or(stem).to_string())
}

/// The sentence the command returns as an error when some file survived.
pub fn failure_message(report: &EraseReport) -> Option<String> {
    (report.failed > 0).then(|| {
        format!(
            "{} file(s) could not be removed from Yap's data folder — open it from Settings and delete them by hand.",
            report.failed
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphan_selection_keeps_owned_audio_and_non_audio() {
        let owned = vec![PathBuf::from("/m/a.wav"), PathBuf::from("/m/a.sys.wav")];
        let files = vec![
            PathBuf::from("/m/a.wav"),
            PathBuf::from("/m/a.sys.wav"),
            PathBuf::from("/m/a.mic.progress.json"),
            PathBuf::from("/m/b.wav"),
            PathBuf::from("/m/c.t0.spill.pcm"),
            PathBuf::from("/m/c.meeting.in_progress.json"),
        ];
        assert_eq!(
            select_orphan_meeting_audio(&files, &owned),
            vec![
                PathBuf::from("/m/b.wav"),
                PathBuf::from("/m/c.t0.spill.pcm"),
                PathBuf::from("/m/c.meeting.in_progress.json"),
            ]
        );
    }

    #[test]
    fn meeting_id_is_the_first_dotted_segment() {
        assert_eq!(
            meeting_id_of(Path::new("/m/abc.wav")).as_deref(),
            Some("abc")
        );
        assert_eq!(
            meeting_id_of(Path::new("/m/abc.sys.wav")).as_deref(),
            Some("abc")
        );
        assert_eq!(meeting_id_of(Path::new("/m/abc.json")), None);
    }
}
