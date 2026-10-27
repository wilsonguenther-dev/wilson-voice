//! yap24-NT4 R2 — the MEASURED notes eval.
//!
//! The planted-commitment fixture (`support::notes_planted`: 6 explicit and 4
//! implicit commitments, 2 decisions, 6 "going to"/"'ll" small-talk
//! distractors, 2+ commitments in the last 20%) goes through the REAL notes
//! path — a `SummaryJobs` worker whose backend spawns the real `yap-polish`
//! sidecar on the real 1.5B notes model, exactly as `summarize_meeting_blocking`
//! does — and the notes are scored against what was planted. Gates (all
//! committed to docs/BUDGETS.md under "summary recall"):
//!
//!   action recall >= 0.8, action precision >= 0.8, decision recall >= 0.5,
//!   Summary 4-gram verbatim ratio <= 0.5, cross-section duplicates = 0,
//!   items per cited segment <= 2, owner accuracy >= 0.8 OR owner left blank.
//!
//! The model run is `#[ignore]` (the loop runs it when the model is
//! provisioned by scripts/provision-notetaker-eval.sh). Under
//! `YAP_EVAL_REQUIRE=1` a missing model or sidecar is a PANIC, not a skip.
//! The un-ignored tests check the fixture's shape and that every metric
//! scores a known answer correctly, so the gate cannot pass vacuously.
//!
//!   YAP_EVAL_REQUIRE=1 YAP_MODEL_DIR=$HOME/code/wilson-voice-loop/cache/models \
//!     cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1

mod support;

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::eval_metrics::summary_verbatim_ratio;
use support::notes_planted::{self, PlantedNote, MEETING_SECONDS, SCRIPT};
use wilson_voice_lib::meetings::{MeetingKind, MeetingSegment};
use wilson_voice_lib::summarize::{
    summarize_segments_with, MeetingSummary, SidecarSession, SidecarSummaryClient, SummaryItem,
    TrackSpeakers, DEFAULT_SUMMARY_MODEL,
};
use wilson_voice_lib::summary_jobs::{
    CancelToken, NotesBackend, NotesError, NotesPhase, NotesStatus, SummaryJobs,
};

/// A notes item is the planted one when this share of the planted keywords is
/// among its words (the call-bleed eval's `NOTE_MATCH_SHARE`).
const MATCH_SHARE: f64 = 0.6;
/// Two bullets in different sections are the same item at this word overlap.
const DUP_JACCARD: f64 = 0.8;

const GATE_ACTION_RECALL: f64 = 0.8;
const GATE_ACTION_PRECISION: f64 = 0.8;
const GATE_DECISION_RECALL: f64 = 0.5;
const GATE_VERBATIM_MAX: f64 = 0.5;
const GATE_DUPLICATES_MAX: usize = 0;
const GATE_ITEMS_PER_SEGMENT_MAX: usize = 2;
const GATE_OWNER_OK: f64 = 0.8;

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let w = w.to_lowercase().replace('\'', "");
            w.strip_suffix('s').filter(|s| s.len() > 3).map(str::to_string).unwrap_or(w)
        })
        .collect()
}

fn share(keywords: &[&str], text: &str) -> f64 {
    let have: HashSet<String> = words(text).into_iter().collect();
    let want: Vec<String> = keywords.iter().flat_map(|k| words(k)).collect();
    if want.is_empty() {
        return 0.0;
    }
    want.iter().filter(|w| have.contains(*w)).count() as f64 / want.len() as f64
}

/// Does `item` stand for `plant`? It cites the planted line, or it carries
/// the planted keywords.
fn is_item_for(item: &SummaryItem, plant: &PlantedNote) -> bool {
    (item.start_seconds - plant.start_seconds).abs() < 0.01 || share(plant.keywords, &item.text) >= MATCH_SHARE
}

/// The owner a notes item claims: its speaker, with "Someone here" (no claim)
/// and none both blank.
fn owner_of(item: &SummaryItem) -> String {
    match item.speaker.as_deref().map(str::trim) {
        None | Some("") | Some("Someone here") | Some("Speaker") => String::new(),
        Some(s) => s.to_string(),
    }
}

#[derive(Debug, Clone, Default)]
struct Scores {
    actions: usize,
    action_recall: f64,
    explicit_recall: f64,
    implicit_recall: f64,
    late_recall: f64,
    action_precision: f64,
    distractors_filed: usize,
    decision_recall: f64,
    verbatim: f64,
    duplicates: usize,
    max_items_per_segment: usize,
    owner_ok: f64,
    owner_right: usize,
    owner_blank: usize,
    owner_wrong: usize,
}

fn jaccard(a: &str, b: &str) -> f64 {
    let a: HashSet<String> = words(a).into_iter().collect();
    let b: HashSet<String> = words(b).into_iter().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    a.intersection(&b).count() as f64 / a.union(&b).count() as f64
}

fn score(s: &MeetingSummary) -> Scores {
    let commitments = notes_planted::commitments();
    let decisions = notes_planted::decisions();
    let late_from = MEETING_SECONDS * 0.8;
    let found = |p: &PlantedNote| s.actions.iter().any(|a| is_item_for(a, p));
    let recall_of = |ps: &[&PlantedNote]| {
        if ps.is_empty() {
            return 0.0;
        }
        ps.iter().filter(|p| found(p)).count() as f64 / ps.len() as f64
    };
    let all: Vec<&PlantedNote> = commitments.iter().collect();
    let explicit: Vec<&PlantedNote> = commitments.iter().filter(|p| p.explicit).collect();
    let implicit: Vec<&PlantedNote> = commitments.iter().filter(|p| !p.explicit).collect();
    let late: Vec<&PlantedNote> = commitments.iter().filter(|p| p.start_seconds >= late_from).collect();

    let true_actions = s.actions.iter().filter(|a| commitments.iter().any(|p| is_item_for(a, p))).count();
    let distractor_starts = notes_planted::distractor_starts();
    let distractors_filed = s
        .actions
        .iter()
        .filter(|a| distractor_starts.iter().any(|d| (a.start_seconds - d).abs() < 0.01))
        .count();

    let decision_recall = if decisions.is_empty() {
        0.0
    } else {
        decisions.iter().filter(|p| s.decisions.iter().any(|d| is_item_for(d, p))).count() as f64
            / decisions.len() as f64
    };

    // Duplicates: the same item filed twice — across sections, or twice in one.
    let sections: Vec<(usize, &SummaryItem)> = [&s.actions, &s.decisions, &s.questions]
        .iter()
        .enumerate()
        .flat_map(|(k, items)| items.iter().map(move |i| (k, i)))
        .collect();
    let mut duplicates = 0;
    for (x, (_, a)) in sections.iter().enumerate() {
        if sections[..x].iter().any(|(_, b)| jaccard(&a.text, &b.text) >= DUP_JACCARD) {
            duplicates += 1;
        }
    }
    let mut per_segment: BTreeMap<String, usize> = BTreeMap::new();
    for (_, i) in &sections {
        *per_segment.entry(format!("{:.2}", i.start_seconds)).or_default() += 1;
    }

    let (mut right, mut blank, mut wrong) = (0, 0, 0);
    for p in &commitments {
        if let Some(item) = s.actions.iter().find(|a| is_item_for(a, p)) {
            let owner = owner_of(item);
            if owner.is_empty() {
                blank += 1;
            } else if owner.eq_ignore_ascii_case(p.owner) {
                right += 1;
            } else {
                wrong += 1;
            }
        }
    }
    let matched = right + blank + wrong;
    Scores {
        actions: s.actions.len(),
        action_recall: recall_of(&all),
        explicit_recall: recall_of(&explicit),
        implicit_recall: recall_of(&implicit),
        late_recall: recall_of(&late),
        action_precision: if s.actions.is_empty() { 0.0 } else { true_actions as f64 / s.actions.len() as f64 },
        distractors_filed,
        decision_recall,
        verbatim: summary_verbatim_ratio(&s.narrative, &notes_planted::transcript_text()),
        duplicates,
        max_items_per_segment: per_segment.values().copied().max().unwrap_or(0),
        owner_ok: if matched == 0 { 0.0 } else { (right + blank) as f64 / matched as f64 },
        owner_right: right,
        owner_blank: blank,
        owner_wrong: wrong,
    }
}

fn gate_failures(sc: &Scores) -> Vec<String> {
    let mut f = Vec::new();
    if sc.action_recall < GATE_ACTION_RECALL {
        f.push(format!("action recall {:.2} < {GATE_ACTION_RECALL}", sc.action_recall));
    }
    if sc.action_precision < GATE_ACTION_PRECISION {
        f.push(format!("action precision {:.2} < {GATE_ACTION_PRECISION}", sc.action_precision));
    }
    if sc.decision_recall < GATE_DECISION_RECALL {
        f.push(format!("decision recall {:.2} < {GATE_DECISION_RECALL}", sc.decision_recall));
    }
    if sc.verbatim > GATE_VERBATIM_MAX {
        f.push(format!("summary verbatim ratio {:.2} > {GATE_VERBATIM_MAX}", sc.verbatim));
    }
    if sc.duplicates > GATE_DUPLICATES_MAX {
        f.push(format!("cross-section duplicates {} > {GATE_DUPLICATES_MAX}", sc.duplicates));
    }
    if sc.max_items_per_segment > GATE_ITEMS_PER_SEGMENT_MAX {
        f.push(format!("items per cited segment {} > {GATE_ITEMS_PER_SEGMENT_MAX}", sc.max_items_per_segment));
    }
    if sc.owner_ok < GATE_OWNER_OK {
        f.push(format!("owner accuracy-or-blank {:.2} < {GATE_OWNER_OK}", sc.owner_ok));
    }
    f
}

// ── the fixture's shape, and the metrics on known answers ──────────────────

#[test]
fn the_fixture_plants_what_the_gate_counts() {
    let c = notes_planted::commitments();
    assert_eq!(c.iter().filter(|p| p.explicit).count(), 6, "explicit commitments");
    assert_eq!(c.iter().filter(|p| !p.explicit).count(), 4, "implicit commitments");
    assert_eq!(notes_planted::decisions().len(), 2);
    let distractors: Vec<_> =
        SCRIPT.iter().filter(|l| l.plant == notes_planted::Plant::Distractor).collect();
    assert_eq!(distractors.len(), 6);
    for d in &distractors {
        let t = d.text.to_lowercase();
        assert!(t.contains("going to") || t.contains("'ll"), "distractor shape: {}", d.text);
    }
    let late = c.iter().filter(|p| p.start_seconds >= MEETING_SECONDS * 0.8).count();
    assert!(late >= 2, "only {late} commitments in the last 20%");
    let segs = notes_planted::segments();
    let last = segs.last().unwrap();
    assert!(last.end_seconds <= MEETING_SECONDS && last.start_seconds >= MEETING_SECONDS * 0.95);
    assert!(segs.windows(2).all(|w| w[0].end_seconds < w[1].start_seconds), "segments overlap");
    assert!(segs.iter().any(|s| s.track == wilson_voice_lib::meetings::SYSTEM_TRACK));
    // Every keyword is a word of its own line (so a perfect note can match).
    for p in c.iter().chain(notes_planted::decisions().iter()) {
        let line = SCRIPT.iter().enumerate().find(|(i, _)| {
            (*i as f64 * MEETING_SECONDS / SCRIPT.len() as f64 - p.start_seconds).abs() < 0.01
        });
        assert!((share(p.keywords, line.unwrap().1.text) - 1.0).abs() < 1e-9);
    }
}

fn item(text: &str, at: f64, speaker: Option<&str>) -> SummaryItem {
    SummaryItem { text: text.into(), segment: String::new(), start_seconds: at, speaker: speaker.map(Into::into) }
}

fn summary(narrative: &str, actions: Vec<SummaryItem>, decisions: Vec<SummaryItem>) -> MeetingSummary {
    MeetingSummary {
        markdown: String::new(),
        narrative: narrative.into(),
        actions,
        decisions,
        questions: Vec::new(),
        chunks: 1,
        truncated_chunks: 0,
        truncated: false,
        dropped_items: 0,
        model: "stub".into(),
    }
}

fn perfect() -> MeetingSummary {
    let actions = notes_planted::commitments()
        .iter()
        .map(|p| item(&p.keywords.join(" "), p.start_seconds + 0.5, Some(p.owner)))
        .collect();
    let decisions = notes_planted::decisions()
        .iter()
        .map(|p| item(&p.keywords.join(" "), p.start_seconds, None))
        .collect();
    summary("A partner call about a regional rollout, a renewal and staff readiness.", actions, decisions)
}

#[test]
fn a_perfect_set_of_notes_passes_every_gate() {
    let sc = score(&perfect());
    assert_eq!(sc.action_recall, 1.0);
    assert_eq!(sc.action_precision, 1.0);
    assert_eq!(sc.decision_recall, 1.0);
    assert_eq!(sc.owner_right, 10);
    assert!(gate_failures(&sc).is_empty(), "{:?}", gate_failures(&sc));
}

#[test]
fn each_failure_mode_from_the_audit_trips_its_gate() {
    // The summary is a paste of the transcript.
    let mut s = perfect();
    s.narrative = SCRIPT.iter().take(12).map(|l| l.text).collect::<Vec<_>>().join(" ");
    assert!(score(&s).verbatim > 0.9);
    assert!(gate_failures(&score(&s)).iter().any(|f| f.contains("verbatim")));
    // Small talk filed as actions.
    let mut s = perfect();
    for at in notes_planted::distractor_starts() {
        s.actions.push(item("small talk", at, Some("Me")));
    }
    let sc = score(&s);
    assert_eq!(sc.distractors_filed, 6);
    assert!(gate_failures(&sc).iter().any(|f| f.contains("precision")));
    // The same item under Actions and Decisions.
    let mut s = perfect();
    let dup = s.actions[0].clone();
    s.decisions.push(dup);
    assert!(gate_failures(&score(&s)).iter().any(|f| f.contains("duplicates")));
    // Wrong owners under echo: every Them item filed as Me.
    let mut s = perfect();
    for a in &mut s.actions {
        a.speaker = Some("Me".into());
    }
    let sc = score(&s);
    assert!(sc.owner_wrong >= 4);
    assert!(gate_failures(&sc).iter().any(|f| f.contains("owner")));
    // Blank owners make no claim and pass.
    let mut s = perfect();
    for a in &mut s.actions {
        a.speaker = Some("Someone here".into());
    }
    assert_eq!(score(&s).owner_ok, 1.0);
    // Implicit commitments missed: recall 0.6.
    let mut s = perfect();
    let implicit: Vec<f64> =
        notes_planted::commitments().iter().filter(|p| !p.explicit).map(|p| p.start_seconds).collect();
    s.actions.retain(|a| !implicit.iter().any(|t| (a.start_seconds - 0.5 - t).abs() < 0.01));
    let sc = score(&s);
    assert!((sc.action_recall - 0.6).abs() < 1e-9);
    assert_eq!(sc.implicit_recall, 0.0);
    assert!(gate_failures(&sc).iter().any(|f| f.contains("action recall")));
}

// ── the real model, through summary_jobs ─────────────────────────────────────

fn model_path() -> Option<PathBuf> {
    let dir = std::env::var_os("YAP_MODEL_DIR").map(PathBuf::from)?;
    let model = wilson_voice_lib::models::polish_model(DEFAULT_SUMMARY_MODEL)?;
    let p = dir.join(&model.file.filename);
    p.is_file().then_some(p)
}

fn sidecar_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("YAP_POLISH_BIN").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let triple = if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" };
    let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("binaries/yap-polish-{triple}"));
    staged.is_file().then_some(staged)
}

/// Resident KB of this process and of its direct children (the sidecar).
fn rss_kb() -> (u64, u64) {
    let me = std::process::id();
    let Ok(out) = std::process::Command::new("/bin/ps").args(["-axo", "pid=,ppid=,rss="]).output() else {
        return (0, 0);
    };
    let (mut own, mut kids) = (0, 0);
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<u64> = line.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if f.len() == 3 {
            if f[0] == me as u64 {
                own = f[2];
            } else if f[1] == me as u64 {
                kids += f[2];
            }
        }
    }
    (own, kids)
}

/// The real notes backend, as the app's `AppNotes` runs a job: its own
/// sidecar session for this one job, track speakers, and the stored result.
struct RealNotes {
    binary: PathBuf,
    model: PathBuf,
    segments: Vec<MeetingSegment>,
    out: Mutex<Option<MeetingSummary>>,
}

impl NotesBackend for RealNotes {
    fn model_ready(&self) -> bool {
        true
    }
    fn fetch_model(&self, _p: &mut dyn FnMut(u8), _c: &CancelToken) -> Result<(), NotesError> {
        Ok(())
    }
    fn summarize(&self, _id: &str, cancel: &CancelToken) -> Result<(), NotesError> {
        let session = SidecarSession::spawn(&self.binary, &self.model, DEFAULT_SUMMARY_MODEL)
            .map_err(|_| NotesError::ModelStart)?;
        let client = SidecarSummaryClient::with_cancel(session, cancel.clone());
        let speakers = TrackSpeakers::for_meeting(MeetingKind::Virtual, &self.segments);
        let s = summarize_segments_with(&self.segments, &client, &speakers)
            .map_err(|e| NotesError::Summary(format!("{e:?}")))?;
        *self.out.lock().unwrap() = Some(s);
        Ok(())
    }
}

#[test]
#[ignore = "runs the real 1.5B notes sidecar; provision with scripts/provision-notetaker-eval.sh"]
fn real_notes_model_meets_the_summary_recall_gates() {
    let require = std::env::var("YAP_EVAL_REQUIRE").as_deref() == Ok("1");
    let (Some(model), Some(binary)) = (model_path(), sidecar_binary()) else {
        let msg = format!(
            "notes eval inputs missing: model={:?} (YAP_MODEL_DIR) sidecar={:?} (YAP_POLISH_BIN or binaries/)",
            model_path(),
            sidecar_binary()
        );
        if require {
            panic!("YAP_EVAL_REQUIRE=1: {msg}");
        }
        eprintln!("SKIP {msg}");
        return;
    };

    // Peak RSS of this process (the app's stand-in) plus the sidecar, sampled
    // every 200 ms for the whole job.
    let stop = Arc::new(AtomicBool::new(false));
    let peaks = Arc::new(Mutex::new((0u64, 0u64, 0u64)));
    let sampler = {
        let (stop, peaks) = (stop.clone(), peaks.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (own, kids) = rss_kb();
                let mut p = peaks.lock().unwrap();
                p.0 = p.0.max(own + kids);
                p.1 = p.1.max(kids);
                p.2 = p.2.max(own);
                drop(p);
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };

    let backend = Arc::new(RealNotes { binary, model, segments: notes_planted::segments(), out: Mutex::new(None) });
    let jobs = SummaryJobs::spawn(backend.clone(), Arc::new(|_: &NotesStatus| {})).expect("notes worker");
    let started = Instant::now();
    assert!(jobs.submit("nt4-planted"));
    let end = jobs.wait_for("nt4-planted", Duration::from_secs(30 * 60)).expect("a notes status");
    let wall = started.elapsed();
    stop.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    assert_eq!(end.phase, NotesPhase::Ready, "notes job ended {:?}: {:?}", end.phase, end.reason);
    let s = backend.out.lock().unwrap().clone().expect("a stored summary");
    let sc = score(&s);
    let (peak_total, peak_sidecar, peak_own) = *peaks.lock().unwrap();

    println!("{}", s.markdown);
    println!(
        "NT4-EVAL model={} wall_s={:.1} chunks={} actions={} action_recall={:.2} explicit_recall={:.2} implicit_recall={:.2} late_recall={:.2} action_precision={:.2} distractors_filed={} decision_recall={:.2} verbatim={:.2} duplicates={} max_items_per_segment={} owner_ok={:.2} owner_right={} owner_blank={} owner_wrong={} peak_rss_mb={:.0} peak_sidecar_mb={:.0} peak_host_mb={:.0}",
        s.model,
        wall.as_secs_f64(),
        s.chunks,
        sc.actions,
        sc.action_recall,
        sc.explicit_recall,
        sc.implicit_recall,
        sc.late_recall,
        sc.action_precision,
        sc.distractors_filed,
        sc.decision_recall,
        sc.verbatim,
        sc.duplicates,
        sc.max_items_per_segment,
        sc.owner_ok,
        sc.owner_right,
        sc.owner_blank,
        sc.owner_wrong,
        peak_total as f64 / 1024.0,
        peak_sidecar as f64 / 1024.0,
        peak_own as f64 / 1024.0,
    );
    let failures = gate_failures(&sc);
    assert!(failures.is_empty(), "summary recall gates failed: {failures:?}");
}
