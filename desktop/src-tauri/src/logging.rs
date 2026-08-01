//! Structured file logging + diagnostics (YV7).
//!
//! Tees every `env_logger` record to stderr (console — unchanged) AND a
//! size-rotating file at `data_dir()/logs/yap.log`, and installs a panic hook
//! that records Rust panics (message + location + backtrace) to the same log so
//! support can see crashes after the fact. Hand-rolled rotation keeps the log
//! bounded without pulling in a heavier appender crate.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Rotate the active log once it grows past ~2 MB.
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Keep `yap.log` plus this many rotated backups (`yap.log.1..=yap.log.N`).
const MAX_BACKUPS: usize = 4;

/// The logs directory (`<data_dir>/logs`), created if missing.
pub fn logs_dir(data_dir: &Path) -> PathBuf {
    let dir = data_dir.join("logs");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Initialize logging: keep console output and mirror it to a rotating file,
/// then install the panic hook. Safe to call exactly once at startup.
pub fn init(data_dir: &Path) {
    let dir = logs_dir(data_dir);
    let tee = TeeWriter::new(&dir);
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(tee)))
        // YV44: tauri-plugin-updater logs a hard ERROR for ANY non-2xx from the
        // release endpoint, including the 404 GitHub returns while no release
        // has published `latest.json` — the normal state, which opened every
        // user's log with an ERROR. Yap classifies the check itself (see
        // `check_for_update`: "nothing to update to" at DEBUG, real failures at
        // WARN and surfaced in the UI), so the plugin's own record is muted.
        .filter_module("tauri_plugin_updater", log::LevelFilter::Off)
        .init();
    install_panic_hook();
    log::info!("file logging → {}", dir.join("yap.log").display());
}

/// Install a global panic hook that writes the panic to the log (in addition to
/// whatever the previous hook did — we chain to it so console output is intact).
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let message = payload_message(info.payload());
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!("PANIC at {location}: {message}\nbacktrace:\n{backtrace}");
        previous(info);
    }));
}

fn payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".into()
    }
}

/// A `Write` sink that mirrors bytes to stderr and a rotating log file. The file
/// side never surfaces errors to the caller — logging must not break the app.
struct TeeWriter {
    file: Mutex<RollingFile>,
}

impl TeeWriter {
    fn new(dir: &Path) -> Self {
        Self {
            file: Mutex::new(RollingFile::open(dir.to_path_buf())),
        }
    }
}

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Console first — a file error must never suppress stderr logging.
        let _ = io::stderr().write_all(buf);
        if let Ok(mut f) = self.file.lock() {
            f.write_bytes(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Ok(mut f) = self.file.lock() {
            f.flush();
        }
        Ok(())
    }
}

/// Append-only handle to `yap.log` that rotates once it crosses `MAX_BYTES`.
struct RollingFile {
    dir: PathBuf,
    path: PathBuf,
    handle: Option<File>,
    written: u64,
}

impl RollingFile {
    fn open(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("yap.log");
        let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let handle = OpenOptions::new().create(true).append(true).open(&path).ok();
        Self {
            dir,
            path,
            handle,
            written,
        }
    }

    /// Shift `yap.log` → `yap.log.1`, aging older backups and dropping the oldest.
    fn rotate(&mut self) {
        self.handle = None;
        for i in (1..MAX_BACKUPS).rev() {
            let from = self.dir.join(format!("yap.log.{i}"));
            let to = self.dir.join(format!("yap.log.{}", i + 1));
            let _ = fs::rename(&from, &to);
        }
        let _ = fs::rename(&self.path, self.dir.join("yap.log.1"));
        self.handle = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }

    fn write_bytes(&mut self, buf: &[u8]) {
        if self.written > 0 && self.written + buf.len() as u64 > MAX_BYTES {
            self.rotate();
        }
        if let Some(f) = self.handle.as_mut() {
            if f.write_all(buf).is_ok() {
                self.written += buf.len() as u64;
            }
        }
    }

    fn flush(&mut self) {
        if let Some(f) = self.handle.as_mut() {
            let _ = f.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_file_rotates_and_caps_backups() {
        let base = std::env::temp_dir().join(format!("yap-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let dir = logs_dir(&base);
        let mut rf = RollingFile::open(dir.clone());
        // Force several rotations well past MAX_BYTES.
        let chunk = vec![b'x'; (MAX_BYTES / 2 + 1) as usize];
        for _ in 0..(MAX_BACKUPS + 4) {
            rf.write_bytes(&chunk);
            rf.write_bytes(&chunk);
            rf.write_bytes(&chunk);
        }
        rf.flush();
        // yap.log always exists.
        assert!(dir.join("yap.log").exists());
        // Backups are capped: yap.log.(MAX_BACKUPS+1) must never appear.
        assert!(!dir.join(format!("yap.log.{}", MAX_BACKUPS + 1)).exists());
        let _ = fs::remove_dir_all(&base);
    }
}
