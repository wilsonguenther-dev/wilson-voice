//! yap-1106-call-audio-truth — the stop path writes ONE `meeting_tracks` line
//! per meeting, with the row id and what each track delivered.
//!
//! A fixture session through the fake engine (mic only, no tap) is enough to
//! prove the line is written on the real stop path with the real row id; the
//! per-track figures of a tapped meeting are the engine's (`TrackEvidence`)
//! and are unit-tested in `meeting_control` against the line's format. The
//! line is observed through the `log` facade — a capturing logger installed
//! for this test process — because that is the one seam `logging.rs` tees
//! into `yap.log`.

mod support;

use std::sync::{Arc, Mutex};

use support::{open_db, temp_dir};
use wilson_voice_lib::meeting_control::{MeetingController, MeetingStatus, StatusSink};

struct CapturingLogger {
    lines: Arc<Mutex<Vec<String>>>,
}

impl log::Log for CapturingLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.lines.lock().unwrap().push(record.args().to_string());
        }
    }
    fn flush(&self) {}
}

fn quiet_sink() -> StatusSink {
    Arc::new(|_: &MeetingStatus| {})
}

#[test]
fn the_stop_path_writes_the_meeting_tracks_line_for_the_row() {
    let _guard = support::exclusive();
    support::install_fake_engine();
    support::set_fake_mode(support::FAKE_OK);
    let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    log::set_boxed_logger(Box::new(CapturingLogger {
        lines: Arc::clone(&lines),
    }))
    .expect("this test process installs the one logger");
    log::set_max_level(log::LevelFilter::Info);

    let dir = temp_dir("yap-1106-tracks");
    let db = Arc::new(open_db(&dir));
    let c = MeetingController::new(Arc::clone(&db), quiet_sink());
    let id = c.start(&dir, Some("Tracks fixture".into())).expect("start");
    // A fixture meeting reports the live chip as absent: the fake engine has
    // no tap and says so by not inventing one (the trait default).
    assert!(c.status().recording);
    assert!(c.status().call_audio.is_none());
    c.stop("user stopped").expect("stop");

    let captured = lines.lock().unwrap().clone();
    let tracks: Vec<&String> = captured
        .iter()
        .filter(|l| l.starts_with("meeting_tracks "))
        .collect();
    assert_eq!(
        tracks.len(),
        1,
        "exactly one meeting_tracks line per stop, got {captured:?}"
    );
    let line = tracks[0];
    assert!(
        line.starts_with(&format!("meeting_tracks id={id} mic_s=")),
        "the line names the row: {line}"
    );
    for key in [" mic_s=", " sys_s=", " sys_nonzero_s=", " tap_rebuilds=", " aec_share="] {
        assert!(line.contains(key), "missing {key} in {line}");
    }
    // Mic only through the fake: no system track, nothing to couple.
    assert!(line.ends_with(" sys_s=0.0 sys_nonzero_s=0.0 tap_rebuilds=0 aec_share=n/a"), "{line}");
    let _ = std::fs::remove_dir_all(&dir);
}
