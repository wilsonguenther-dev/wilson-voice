//! Memory + disk hygiene (YV73).
//!
//! Two long-session problems, one module.
//!
//! **Disk.** Interrupted model downloads (`*.partial` from `models`, `*.part`
//! from `vad`), recovery WAVs whose failed-dictation row is long gone, rotated
//! logs and the `*.tmp` siblings the atomic settings write leaves behind on a
//! crash all accumulate under Application Support with nothing to sweep them.
//! [`sweep`] is that sweep: run once shortly after startup and daily after
//! that, on its own thread, so it can never sit in front of a dictation.
//!
//! The rules it sweeps by are deliberately NARROW, because the blast radius of
//! getting them wrong is a user's audio:
//!
//! * the retention window is `db::FAILED_TAKE_RETENTION_DAYS` — the SAME
//!   constant `purge_expired_failed_takes` uses, never a second one;
//! * a recovery WAV a `failed_dictations` row still points at is never a
//!   candidate, and when that row list cannot be read the recovery dir is
//!   skipped entirely rather than swept blind;
//! * a file whose mtime cannot be read counts as FRESH, so an unreadable
//!   timestamp can only ever spare a file;
//! * the DB (and its `-wal`/`-shm`/quarantined `.corrupt-*` copies), the
//!   models themselves, and the active `yap.log` are never candidates.
//!
//! Every "what may I delete" decision is a pure function over [`FileStat`], so
//! those rules are testable without touching a filesystem; only [`sweep`] and
//! the collectors below do I/O.
//!
//! **Memory.** [`MemoryReport::summary_line`] is the memory counterpart of the
//! YV40 latency line: one INFO record in the same stable `key=value` shape,
//! emitted every ten minutes and once per dictation, so a session's logs can
//! answer "did this build grow over eight hours" without attaching a profiler.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Total byte budget for the log directory — the active `yap.log` plus every
/// rotated backup. `logging` already caps a SINGLE file at ~2 MB and keeps 4
/// backups, but nothing bounded the directory across a re-install or a build
/// that changed those numbers, so this is the belt to that braces.
pub const LOG_BUDGET_BYTES: u64 = 20 * 1024 * 1024;

/// How often the memory telemetry line is emitted while the app is running.
pub const TELEMETRY_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// How often the disk sweep re-runs after the startup one.
pub const SWEEP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// How long after launch the FIRST sweep runs. Startup already pays for the
/// stale-wav sweep, the retention purge, crash ingest and the ASR preload;
/// this waits until that burst is over so the two never contend for the disk.
pub const STARTUP_SWEEP_DELAY: Duration = Duration::from_secs(30);

/// One file the sweep is allowed to consider. Split from the filesystem so the
/// selection rules below are pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}

/// The instant before which a file is "stale". Reuses the YV52 retention
/// constant — there is exactly ONE retention window in this app.
pub fn retention_cutoff(now: SystemTime) -> SystemTime {
    let days = crate::db::FAILED_TAKE_RETENTION_DAYS.max(0) as u64;
    now.checked_sub(Duration::from_secs(days * 24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case(ext))
        .unwrap_or(false)
}

/// An interrupted download: the `.partial` `models` resumes into, or the
/// `.part` `vad` curls into. Both are re-fetched from scratch when needed, so
/// a week-old one is pure dead weight.
pub fn is_interrupted_download(path: &Path) -> bool {
    has_extension(path, "partial") || has_extension(path, "part")
}

/// Interrupted downloads older than the cutoff — and nothing else. A finished
/// model sitting next to them is never selected, whatever its age.
pub fn select_stale_downloads(files: &[FileStat], cutoff: SystemTime) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| is_interrupted_download(&f.path) && f.modified < cutoff)
        .map(|f| f.path.clone())
        .collect()
}

/// Our own atomic-write leftovers (`settings.json.tmp`) older than the cutoff.
/// Scoped to `.tmp` on purpose: a quarantined `.corrupt-*` DB is forensics, not
/// garbage, and never matches.
pub fn select_stale_temp_files(files: &[FileStat], cutoff: SystemTime) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| has_extension(&f.path, "tmp") && f.modified < cutoff)
        .map(|f| f.path.clone())
        .collect()
}

/// Recovery WAVs that NO `failed_dictations` row points at and that are past
/// the retention window.
///
/// `keep` is the row list — a WAV named there is audio the user can still
/// retry, so it is never a candidate no matter how old the file is (the row's
/// own expiry is `purge_expired_failed_takes`' job, and it deletes the wav
/// itself). Only `.wav` is considered: a live take's `.spill.pcm` and
/// `.in_progress.json` belong to the YV63 crash-journal path.
pub fn select_orphan_recovery_wavs(
    files: &[FileStat],
    keep: &[String],
    cutoff: SystemTime,
) -> Vec<PathBuf> {
    files
        .iter()
        .filter(|f| has_extension(&f.path, "wav"))
        .filter(|f| f.modified < cutoff)
        .filter(|f| !keep.iter().any(|k| Path::new(k) == f.path))
        .map(|f| f.path.clone())
        .collect()
}

/// Rotated logs to drop, OLDEST FIRST, until the directory fits `budget`.
///
/// `active` (the live `yap.log`) counts toward the budget but is never
/// returned — the log being written to is not garbage. Returned in deletion
/// order, so the caller unlinking them in sequence frees the oldest bytes
/// first and stops as soon as the budget is met.
pub fn select_logs_over_budget(files: &[FileStat], active: &Path, budget: u64) -> Vec<PathBuf> {
    let mut total: u64 = files.iter().map(|f| f.size).sum();
    let mut candidates: Vec<&FileStat> = files.iter().filter(|f| f.path != active).collect();
    candidates.sort_by(|a, b| a.modified.cmp(&b.modified).then(a.path.cmp(&b.path)));
    let mut doomed = Vec::new();
    for f in candidates {
        if total <= budget {
            break;
        }
        total = total.saturating_sub(f.size);
        doomed.push(f.path.clone());
    }
    doomed
}

/// What one sweep removed. Logged as a single line; all zeroes stays silent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepOutcome {
    pub downloads: usize,
    pub recovery: usize,
    pub temp: usize,
    pub logs: usize,
    pub bytes: u64,
}

impl SweepOutcome {
    pub fn removed(&self) -> usize {
        self.downloads + self.recovery + self.temp + self.logs
    }
}

/// Read one directory (NOT recursive) into stats. Anything unreadable is
/// skipped, and a file whose mtime cannot be read is reported as `now` — the
/// conservative direction, since every rule above only deletes what is OLDER
/// than the cutoff.
fn read_stats(dir: &Path, now: SystemTime) -> Vec<FileStat> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        out.push(FileStat {
            path: entry.path(),
            size: meta.len(),
            modified: meta.modified().unwrap_or(now),
        });
    }
    out
}

/// Run the whole sweep against `data_dir`.
///
/// `keep_recovery` is the `failed_dictations` wav list. `None` means the DB
/// could not be asked — the recovery directory is then left ALONE rather than
/// swept without knowing which clips are still reachable.
pub fn sweep(data_dir: &Path, keep_recovery: Option<&[String]>, now: SystemTime) -> SweepOutcome {
    let cutoff = retention_cutoff(now);
    let mut outcome = SweepOutcome::default();

    let models = read_stats(&data_dir.join("models"), now);
    let doomed = select_stale_downloads(&models, cutoff);
    let (n, bytes) = unlink_all(&models, &doomed);
    outcome.downloads = n;
    outcome.bytes += bytes;

    if let Some(keep) = keep_recovery {
        let recovery = read_stats(&data_dir.join("recovery"), now);
        let doomed = select_orphan_recovery_wavs(&recovery, keep, cutoff);
        let (n, bytes) = unlink_all(&recovery, &doomed);
        outcome.recovery = n;
        outcome.bytes += bytes;
    }

    let root = read_stats(data_dir, now);
    let doomed = select_stale_temp_files(&root, cutoff);
    let (n, bytes) = unlink_all(&root, &doomed);
    outcome.temp = n;
    outcome.bytes += bytes;

    let logs_dir = data_dir.join("logs");
    let logs = read_stats(&logs_dir, now);
    let doomed = select_logs_over_budget(&logs, &logs_dir.join("yap.log"), LOG_BUDGET_BYTES);
    let (n, bytes) = unlink_all(&logs, &doomed);
    outcome.logs = n;
    outcome.bytes += bytes;

    outcome
}

/// Unlink each selected path, returning how many went and how many bytes that
/// freed. Best-effort: a file that will not delete (permissions, a race with
/// the app itself) is logged at DEBUG and skipped — hygiene must never be able
/// to fail a launch.
fn unlink_all(stats: &[FileStat], doomed: &[PathBuf]) -> (usize, u64) {
    let mut removed = 0;
    let mut freed: u64 = 0;
    for path in doomed {
        match std::fs::remove_file(path) {
            Ok(()) => {
                removed += 1;
                if let Some(s) = stats.iter().find(|s| &s.path == path) {
                    freed = freed.saturating_add(s.size);
                }
            }
            Err(e) => log::debug!("hygiene: could not remove {}: {e}", path.display()),
        }
    }
    (removed, freed)
}

// ── Memory telemetry ────────────────────────────────────────────────────────

/// One sample of what Yap is holding, in memory and on disk. Same contract as
/// [`crate::latency::PipelineSpans`]: stable keys, never reordered, never
/// conditionally dropped — a field it could not measure reports `0`, so every
/// line parses the same way.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MemoryReport {
    /// Resident set size of this process, MB.
    pub rss_mb: f64,
    /// `wilson_voice.db` on disk, MB.
    pub db_size_mb: f64,
    /// The whole log directory, MB (bounded by [`LOG_BUDGET_BYTES`]).
    pub logs_mb: f64,
    /// How many files the recovery directory is holding.
    pub recovery_files: usize,
}

impl MemoryReport {
    /// The single machine-parsable INFO line. `memory` stays the first token so
    /// it greps the way `latency` does.
    pub fn summary_line(&self) -> String {
        format!(
            "memory rss_mb={:.1} db_size_mb={:.1} logs_mb={:.1} recovery_files={}",
            self.rss_mb, self.db_size_mb, self.logs_mb, self.recovery_files
        )
    }
}

const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

/// Resident set size in MB, read from `ps`.
///
/// Deliberately a subprocess rather than a `task_info`/`proc_pidinfo` FFI: this
/// is a diagnostic line sampled every ten minutes and once per take (off the
/// dictation thread, see `log_snapshot_async`), and it is not worth an `unsafe`
/// block plus a hand-declared C struct layout in a dictation app. Returns `0.0`
/// rather than erroring — telemetry never fails anything.
pub fn resident_mb() -> f64 {
    let pid = std::process::id().to_string();
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
    else {
        return 0.0;
    };
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<f64>()
        // `ps` reports RSS in KB.
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

fn file_mb(path: &Path) -> f64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) as f64 / BYTES_PER_MB
}

fn dir_mb(dir: &Path) -> f64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0.0;
    };
    let bytes: u64 = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    bytes as f64 / BYTES_PER_MB
}

fn dir_file_count(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.metadata().map(|m| m.is_file()).unwrap_or(false))
                .count()
        })
        .unwrap_or(0)
}

/// Sample the current report against `data_dir`.
pub fn collect(data_dir: &Path) -> MemoryReport {
    MemoryReport {
        rss_mb: resident_mb(),
        db_size_mb: file_mb(&data_dir.join("wilson_voice.db")),
        logs_mb: dir_mb(&data_dir.join("logs")),
        recovery_files: dir_file_count(&data_dir.join("recovery")),
    }
}

/// Emit one telemetry line from a detached thread.
///
/// Used by the dictation path: the take is already pasted by then, but reading
/// RSS forks `ps` and the rest stats four paths, and NONE of that belongs on
/// the thread that still has a transcript row to write.
pub fn log_snapshot_async(data_dir: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("wv-hygiene-snap".into())
        .spawn(move || log::info!("{}", collect(&data_dir).summary_line()));
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn stat(path: &str, size: u64, age: Duration, now: SystemTime) -> FileStat {
        FileStat {
            path: PathBuf::from(path),
            size,
            modified: now.checked_sub(age).expect("test clock underflow"),
        }
    }

    /// The download sweep takes interrupted downloads past the retention window
    /// and NOTHING else: not a fresh `.partial` a resume is still waiting on,
    /// and never a finished model however old it is.
    #[test]
    fn gc_selects_stale_partials_only() {
        let now = SystemTime::UNIX_EPOCH + DAY * 400;
        let cutoff = retention_cutoff(now);
        let files = vec![
            stat("/m/ggml-small.bin.partial", 900, DAY * 8, now),
            stat("/m/silero_vad_v4.onnx.part", 300, DAY * 30, now),
            // Inside the window — a resume may still pick this up.
            stat("/m/ggml-medium.bin.partial", 900, DAY * 6, now),
            // A real model. Age is irrelevant: user data is never a candidate.
            stat("/m/ggml-small.bin", 500_000, DAY * 365, now),
            stat("/m/silero_vad_v4.onnx", 2_000, DAY * 365, now),
        ];

        assert_eq!(
            select_stale_downloads(&files, cutoff),
            vec![
                PathBuf::from("/m/ggml-small.bin.partial"),
                PathBuf::from("/m/silero_vad_v4.onnx.part"),
            ],
            "only interrupted downloads past the {}-day window may go",
            crate::db::FAILED_TAKE_RETENTION_DAYS
        );
    }

    /// The retention window is the hard floor for audio. Nothing inside it is
    /// ever selected, and a WAV a failed-dictation row still points at is spared
    /// even when it is older — that row's expiry belongs to the DB purge, which
    /// deletes the audio itself.
    #[test]
    fn gc_never_selects_within_retention() {
        let now = SystemTime::UNIX_EPOCH + DAY * 400;
        let cutoff = retention_cutoff(now);
        let keep = vec!["/r/live-row.wav".to_string()];
        let files = vec![
            // In flight / recent — the whole point of the window.
            stat("/r/today.wav", 40_000, Duration::from_secs(30), now),
            stat("/r/six-days.wav", 40_000, DAY * 6, now),
            // Old, but still reachable from a row.
            stat("/r/live-row.wav", 40_000, DAY * 40, now),
            // The YV63 journal of a take in flight.
            stat("/r/abc.spill.pcm", 9_000, Duration::from_secs(2), now),
            stat("/r/abc.in_progress.json", 120, Duration::from_secs(2), now),
            // Genuinely orphaned and past the window.
            stat("/r/orphan.wav", 40_000, DAY * 9, now),
        ];

        assert_eq!(
            select_orphan_recovery_wavs(&files, &keep, cutoff),
            vec![PathBuf::from("/r/orphan.wav")],
            "only a row-less WAV past retention may go"
        );

        // And with no rows readable at all, the six-day clip is STILL safe —
        // the window, not the row list, is what protects it.
        assert_eq!(
            select_orphan_recovery_wavs(&files, &[], cutoff),
            vec![
                PathBuf::from("/r/live-row.wav"),
                PathBuf::from("/r/orphan.wav")
            ],
            "recent clips stay untouched however the row list reads"
        );
        // (`sweep` skips the directory entirely in that case — see its docs.)
    }

    /// The log directory is capped by TOTAL bytes, shedding the oldest backups
    /// first and stopping the moment it fits. The live `yap.log` counts toward
    /// the budget but is never deleted.
    #[test]
    fn log_cap_deletes_oldest_first_to_budget() {
        let now = SystemTime::UNIX_EPOCH + DAY * 400;
        let mb = 1024 * 1024;
        // 1 + 6 + 6 + 6 + 6 + 6 = 31 MB against a 20 MB budget.
        let files = vec![
            stat("/l/yap.log", mb, Duration::from_secs(1), now),
            stat("/l/yap.log.1", 6 * mb, Duration::from_secs(60), now),
            stat("/l/yap.log.2", 6 * mb, Duration::from_secs(120), now),
            stat("/l/yap.log.3", 6 * mb, Duration::from_secs(180), now),
            stat("/l/yap.log.4", 6 * mb, Duration::from_secs(240), now),
            stat("/l/yap.log.5", 6 * mb, Duration::from_secs(300), now),
        ];
        let active = PathBuf::from("/l/yap.log");

        // 31 → 25 → 19: exactly the two oldest, oldest first.
        assert_eq!(
            select_logs_over_budget(&files, &active, 20 * mb),
            vec![PathBuf::from("/l/yap.log.5"), PathBuf::from("/l/yap.log.4")]
        );

        // Already inside the budget → nothing is touched.
        assert!(select_logs_over_budget(&files[..2], &active, 20 * mb).is_empty());

        // Even an absurd budget can never select the file being written to.
        let doomed = select_logs_over_budget(&files, &active, 0);
        assert_eq!(doomed.len(), files.len() - 1);
        assert!(!doomed.contains(&active), "the active log is never garbage");
    }

    /// Same contract as the YV40 latency line: one leading token, then pure
    /// key=value pairs in a fixed order, with unmeasured fields printed as 0.
    #[test]
    fn memory_summary_line_is_stable_key_value() {
        let report = MemoryReport {
            rss_mb: 214.75,
            db_size_mb: 4.2,
            logs_mb: 1.06,
            recovery_files: 3,
        };
        assert_eq!(
            report.summary_line(),
            "memory rss_mb=214.8 db_size_mb=4.2 logs_mb=1.1 recovery_files=3"
        );

        // A sample that could not read RSS keeps every key.
        assert_eq!(
            MemoryReport::default().summary_line(),
            "memory rss_mb=0.0 db_size_mb=0.0 logs_mb=0.0 recovery_files=0"
        );

        let line = report.summary_line();
        let mut tokens = line.split(' ');
        assert_eq!(tokens.next(), Some("memory"));
        let keys: Vec<&str> = tokens
            .map(|kv| {
                kv.split_once('=')
                    .unwrap_or_else(|| panic!("token is not key=value: {kv}"))
                    .0
            })
            .collect();
        assert_eq!(
            keys,
            vec!["rss_mb", "db_size_mb", "logs_mb", "recovery_files"]
        );
    }

    /// End-to-end against a real temp dir: the sweep removes exactly what the
    /// pure rules select, and `keep_recovery: None` leaves the recovery
    /// directory completely alone.
    #[test]
    fn sweep_honours_the_selection_rules_on_disk() {
        let base = std::env::temp_dir().join(format!("yap-yv73-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for sub in ["models", "recovery", "logs"] {
            std::fs::create_dir_all(base.join(sub)).unwrap();
        }
        let write = |p: PathBuf, bytes: usize| {
            std::fs::write(&p, vec![b'x'; bytes]).unwrap();
            p
        };
        let stale_partial = write(base.join("models/ggml.bin.partial"), 4);
        let model = write(base.join("models/ggml.bin"), 4);
        let orphan = write(base.join("recovery/orphan.wav"), 4);
        let tmp = write(base.join("settings.json.tmp"), 4);
        let db = write(base.join("wilson_voice.db"), 4);

        // A clock far enough ahead that every file written above is past the
        // retention window — so what survives, survives on TYPE, not on age.
        let now = SystemTime::now() + DAY * 30;

        // No row list → the recovery dir is not swept at all, but everything
        // that does not depend on the DB still is.
        let skipped = sweep(&base, None, now);
        assert_eq!(skipped.downloads, 1, "a stale .partial must go");
        assert_eq!(skipped.temp, 1, "our own stale .tmp goes");
        assert_eq!(skipped.recovery, 0);
        assert!(orphan.exists(), "recovery is untouched without a row list");
        assert!(!stale_partial.exists());
        assert!(!tmp.exists());

        // With a (here empty) row list the orphan is reachable from nothing.
        let outcome = sweep(&base, Some(&[]), now);
        assert_eq!(outcome.recovery, 1);
        assert!(
            !orphan.exists(),
            "a row-less recovery wav past retention goes"
        );
        assert!(model.exists(), "a real model is never a candidate");
        assert!(db.exists(), "the DB is never a candidate");

        let _ = std::fs::remove_dir_all(&base);
    }
}
