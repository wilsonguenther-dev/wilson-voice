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
//! yap-audit-notes-strategy: every arm (A, B, E, and C when handed a 3B file)
//! runs on the planted fixture AND on `call-bleed-2voice` after the AEC
//! pre-pass and the shipped ASR (`call_bleed_arms_over_the_aec_cleaned_transcript`);
//! each leg prints a `NOTES-ARMS-TABLE`. The human half
//! (`human_half_real_meeting_copy_numbers_only`) runs the shipped arm on a
//! COPY of a real meeting DB and prints numbers only; it skips unless
//! YAP_EVAL_MEETING_DB and YAP_EVAL_MEETING_ID are set.
//!
//!   YAP_EVAL_REQUIRE=1 YAP_MODEL_DIR=$HOME/code/wilson-voice-loop/cache/models \
//!     cargo test --features custom-protocol --test meeting_notes_recall_eval -- --ignored --nocapture --test-threads=1

mod support;

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::call_bleed::CallBleedMeta;
use support::eval_metrics::summary_verbatim_ratio;
use support::notes_planted::{self, PlantedNote, MEETING_SECONDS, SCRIPT};
use wilson_voice_lib::asr_engine::TimedTranscript;
use wilson_voice_lib::meeting_asr::{plan_windows_fixed, ChunkConfig, ChunkOutcome, ResumePoint};
use wilson_voice_lib::meeting_pipeline::chunk_segments;
use wilson_voice_lib::meetings::{MeetingKind, MeetingSegment, NewMeetingSegment, MIC_TRACK, SYSTEM_TRACK};
use wilson_voice_lib::summarize::{
    summarize_segments_with_strategy, MeetingSummary, NotesStrategy, SidecarSession,
    SidecarSummaryClient, SummaryItem, TrackSpeakers, DEFAULT_SUMMARY_MODEL, NOTES_STRATEGY,
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

fn share<K: AsRef<str>>(keywords: &[K], text: &str) -> f64 {
    let have: HashSet<String> = words(text).into_iter().collect();
    let want: Vec<String> = keywords.iter().flat_map(|k| words(k.as_ref())).collect();
    if want.is_empty() {
        return 0.0;
    }
    want.iter().filter(|w| have.contains(*w)).count() as f64 / want.len() as f64
}

/// One planted commitment or decision, owned, so the planted 15-minute
/// fixture and the call-bleed corpus meta score through the same code.
#[derive(Debug, Clone)]
struct Plant {
    owner: String,
    keywords: Vec<String>,
    start_seconds: f64,
    explicit: bool,
}

/// What a fixture planted, and the transcript the notes were written from.
struct Truth {
    commitments: Vec<Plant>,
    decisions: Vec<Plant>,
    distractor_starts: Vec<f64>,
    /// The text the Summary's verbatim ratio is measured against.
    transcript: String,
    meeting_seconds: f64,
    /// The planted fixture's items cite the planted line's exact start; rows
    /// that came out of ASR never share a planted start, so on the call-bleed
    /// leg only the keywords count.
    match_by_start: bool,
}

fn plant_of(p: &PlantedNote) -> Plant {
    Plant {
        owner: p.owner.to_string(),
        keywords: p.keywords.iter().map(|k| k.to_string()).collect(),
        start_seconds: p.start_seconds,
        explicit: p.explicit,
    }
}

/// The planted 15-minute fixture of yap24-NT4 (R2).
fn planted_truth() -> Truth {
    Truth {
        commitments: notes_planted::commitments().iter().map(plant_of).collect(),
        decisions: notes_planted::decisions().iter().map(plant_of).collect(),
        distractor_starts: notes_planted::distractor_starts(),
        transcript: notes_planted::transcript_text(),
        meeting_seconds: MEETING_SECONDS,
        match_by_start: true,
    }
}

/// Does `item` stand for `plant`? It cites the planted line (planted fixture
/// only), or it carries the planted keywords.
fn is_item_for(item: &SummaryItem, plant: &Plant, by_start: bool) -> bool {
    (by_start && (item.start_seconds - plant.start_seconds).abs() < 0.01)
        || share(&plant.keywords, &item.text) >= MATCH_SHARE
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
    score_against(s, &planted_truth())
}

fn score_against(s: &MeetingSummary, truth: &Truth) -> Scores {
    let commitments = &truth.commitments;
    let decisions = &truth.decisions;
    let by_start = truth.match_by_start;
    let late_from = truth.meeting_seconds * 0.8;
    let found = |p: &Plant| s.actions.iter().any(|a| is_item_for(a, p, by_start));
    let recall_of = |ps: &[&Plant]| {
        if ps.is_empty() {
            return 0.0;
        }
        ps.iter().filter(|p| found(p)).count() as f64 / ps.len() as f64
    };
    let all: Vec<&Plant> = commitments.iter().collect();
    let explicit: Vec<&Plant> = commitments.iter().filter(|p| p.explicit).collect();
    let implicit: Vec<&Plant> = commitments.iter().filter(|p| !p.explicit).collect();
    let late: Vec<&Plant> = commitments.iter().filter(|p| p.start_seconds >= late_from).collect();

    let true_actions =
        s.actions.iter().filter(|a| commitments.iter().any(|p| is_item_for(a, p, by_start))).count();
    let distractor_starts = &truth.distractor_starts;
    let distractors_filed = s
        .actions
        .iter()
        .filter(|a| distractor_starts.iter().any(|d| (a.start_seconds - d).abs() < 0.01))
        .count();

    let decision_recall = if decisions.is_empty() {
        0.0
    } else {
        decisions.iter().filter(|p| s.decisions.iter().any(|d| is_item_for(d, p, by_start))).count() as f64
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
    for p in commitments {
        if let Some(item) = s.actions.iter().find(|a| is_item_for(a, p, by_start)) {
            let owner = owner_of(item);
            if owner.is_empty() {
                blank += 1;
            } else if owner.eq_ignore_ascii_case(&p.owner) {
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
        verbatim: summary_verbatim_ratio(&s.narrative, &truth.transcript),
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
        s.actions.push(item("small talk", at, Some("You")));
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
        a.speaker = Some("You".into());
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
    /// yap-audit-notes-strategy — the arm this backend runs.
    strategy: NotesStrategy,
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
        let s = summarize_segments_with_strategy(&self.segments, &client, &speakers, self.strategy)
            .map_err(|e| NotesError::Summary(format!("{e:?}")))?;
        *self.out.lock().unwrap() = Some(s);
        Ok(())
    }
}

/// One arm's measured row (yap-audit-notes-strategy).
struct ArmRun {
    summary: MeetingSummary,
    scores: Scores,
    wall_s: f64,
    peak_total_kb: u64,
    peak_sidecar_kb: u64,
    peak_host_kb: u64,
}

/// Run one arm end to end through `SummaryJobs`, the way the app runs a job,
/// sampling peak RSS of this process plus the sidecar every 200 ms. The ASR
/// engine is never resident here (the call-bleed leg decodes in a child
/// process that has exited), so the peak is the notes pass alone.
fn run_arm(
    binary: &PathBuf,
    model: &PathBuf,
    strategy: NotesStrategy,
    job: &str,
    segments: Vec<MeetingSegment>,
    truth: &Truth,
) -> ArmRun {
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
    let backend = Arc::new(RealNotes {
        binary: binary.clone(),
        model: model.clone(),
        segments,
        strategy,
        out: Mutex::new(None),
    });
    let jobs = SummaryJobs::spawn(backend.clone(), Arc::new(|_: &NotesStatus| {})).expect("notes worker");
    let started = Instant::now();
    assert!(jobs.submit(job));
    let end = jobs.wait_for(job, Duration::from_secs(30 * 60)).expect("a notes status");
    let wall = started.elapsed();
    stop.store(true, Ordering::SeqCst);
    sampler.join().unwrap();
    assert_eq!(end.phase, NotesPhase::Ready, "notes job {job} ended {:?}: {:?}", end.phase, end.reason);
    let summary = backend.out.lock().unwrap().clone().expect("a stored summary");
    let scores = score_against(&summary, truth);
    let (peak_total_kb, peak_sidecar_kb, peak_host_kb) = *peaks.lock().unwrap();
    ArmRun { summary, scores, wall_s: wall.as_secs_f64(), peak_total_kb, peak_sidecar_kb, peak_host_kb }
}

/// The table header docs/decisions/notes-strategy-2026-10.md and
/// docs/BUDGETS.md carry; every arm prints one row under it.
const ARM_TABLE_HEADER: &str = "| Arm | Action recall | Action precision | Decision recall | Owner accuracy | Summary 4-gram verbatim | Cross-section dups | Max items per segment | Wall s | Peak RSS MB |";

fn arm_row(arm: &str, r: &ArmRun) -> String {
    let sc = &r.scores;
    format!(
        "| {arm} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} | {:.1} | {:.1} |",
        sc.action_recall,
        sc.action_precision,
        sc.decision_recall,
        sc.owner_ok,
        sc.verbatim,
        sc.duplicates,
        sc.max_items_per_segment,
        r.wall_s,
        r.peak_total_kb as f64 / 1024.0,
    )
}

#[test]
fn every_arm_row_has_the_table_shape() {
    let r = ArmRun {
        summary: perfect(),
        scores: score(&perfect()),
        wall_s: 12.0,
        peak_total_kb: 1024 * 900,
        peak_sidecar_kb: 0,
        peak_host_kb: 0,
    };
    let row = arm_row("E", &r);
    assert!(row.starts_with("| E | "), "{row}");
    assert_eq!(row.matches('|').count(), ARM_TABLE_HEADER.matches('|').count(), "{row}");
    assert!(row.contains("| 900.0 |"), "{row}");
}

/// The arms every leg runs: A, B and E on the shipped 1.5B file; arm C is
/// arm E with a 3B model file, measured only, and only when the eval is
/// handed one (YAP_EVAL_ARM_C_MODEL); it must stay <= 2.2 GB peak.
fn arms_under_test(model: &PathBuf) -> Vec<(&'static str, NotesStrategy, PathBuf)> {
    let mut arms: Vec<(&'static str, NotesStrategy, PathBuf)> = vec![
        ("A", NotesStrategy::A, model.clone()),
        ("B", NotesStrategy::B, model.clone()),
        ("E", NotesStrategy::E, model.clone()),
    ];
    match std::env::var_os("YAP_EVAL_ARM_C_MODEL").map(PathBuf::from) {
        Some(c) if c.is_file() => arms.push(("C", NotesStrategy::E, c)),
        Some(c) => eprintln!("ARM C SKIPPED: YAP_EVAL_ARM_C_MODEL={c:?} is not a file"),
        None => eprintln!("ARM C SKIPPED: set YAP_EVAL_ARM_C_MODEL to a Qwen2.5-3B q4 GGUF to measure it"),
    }
    arms
}

/// One machine-readable line per arm per fixture (numbers only).
fn print_arm_line(fixture: &str, label: &str, r: &ArmRun) {
    let sc = &r.scores;
    println!(
        "NT4-EVAL fixture={fixture} arm={label} model={} wall_s={:.1} chunks={} actions={} action_recall={:.2} explicit_recall={:.2} implicit_recall={:.2} late_recall={:.2} action_precision={:.2} distractors_filed={} decision_recall={:.2} verbatim={:.2} duplicates={} max_items_per_segment={} owner_ok={:.2} owner_right={} owner_blank={} owner_wrong={} peak_rss_mb={:.0} peak_sidecar_mb={:.0} peak_host_mb={:.0}",
        r.summary.model,
        r.wall_s,
        r.summary.chunks,
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
        r.peak_total_kb as f64 / 1024.0,
        r.peak_sidecar_kb as f64 / 1024.0,
        r.peak_host_kb as f64 / 1024.0,
    );
    if label == "C" && r.peak_total_kb as f64 / 1024.0 / 1024.0 > 2.2 {
        eprintln!("ARM C over the 2.2 GB ceiling on {fixture}: {:.2} GB", r.peak_total_kb as f64 / 1024.0 / 1024.0);
    }
}

/// The model and the sidecar every leg needs; `None` is a skip, and under
/// `YAP_EVAL_REQUIRE=1` a missing input is a panic.
fn notes_inputs() -> Option<(PathBuf, PathBuf)> {
    match (model_path(), sidecar_binary()) {
        (Some(model), Some(binary)) => Some((model, binary)),
        (model, binary) => {
            let msg = format!(
                "notes eval inputs missing: model={model:?} (YAP_MODEL_DIR) sidecar={binary:?} (YAP_POLISH_BIN or binaries/)"
            );
            if eval_required() {
                panic!("YAP_EVAL_REQUIRE=1: {msg}");
            }
            eprintln!("SKIP {msg}");
            None
        }
    }
}

fn eval_required() -> bool {
    std::env::var("YAP_EVAL_REQUIRE").as_deref() == Ok("1")
}

#[test]
#[ignore = "runs the real 1.5B notes sidecar; provision with scripts/provision-notetaker-eval.sh"]
fn real_notes_model_meets_the_summary_recall_gates() {
    let Some((model, binary)) = notes_inputs() else { return };

    // yap-audit-notes-strategy: every arm on the same planted fixture.
    let arms = arms_under_test(&model);

    let mut rows = Vec::new();
    let mut shipped: Option<ArmRun> = None;
    for (label, strategy, arm_model) in arms {
        let r = run_arm(
            &binary,
            &arm_model,
            strategy,
            &format!("nt4-planted-{label}"),
            notes_planted::segments(),
            &planted_truth(),
        );
        println!("{}", r.summary.markdown);
        print_arm_line("nt4-planted", label, &r);
        rows.push(arm_row(label, &r));
        if label != "C" && strategy == NOTES_STRATEGY {
            shipped = Some(r);
        }
    }
    // One numeric table row per arm, ready to paste into
    // docs/decisions/notes-strategy-2026-10.md and docs/BUDGETS.md.
    println!("NOTES-ARMS-TABLE nt4-planted");
    println!("{ARM_TABLE_HEADER}");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for row in &rows {
        println!("{row}");
    }
    // The gates bind the SHIPPED arm; the others are measured, not gated.
    let shipped = shipped.expect("the shipped arm ran");
    let failures = gate_failures(&shipped.scores);
    assert!(failures.is_empty(), "summary recall gates failed for arm {}: {failures:?}", NOTES_STRATEGY.tag());
}

// ── the ASR + AEC leg: every arm on call-bleed-2voice ────────────────────────
//
// yap-audit-notes-strategy, bounce fix: the arms are measured on the
// AEC-cleaned input, never on echo-polluted text. The two-voice call goes
// through the shipped pipeline: the AEC pre-pass (yap24-NT11) cleans the far
// side out of the microphone, the shipped decoder (`wilson-voice
// --transcribe-file --timed`, a child process, so the ASR engine is never
// resident while notes run) decodes both tracks on the shipped window
// geometry, `chunk_segments` turns the windows into the rows the app would
// store, and every arm writes notes from those rows. The planted commitments
// and decisions of `meta.json` (`support::call_bleed::CallBleedMeta`) are the
// truth; matching is by keywords only, since ASR rows never share a planted
// start.

const CALL_BLEED: &str = "call-bleed-2voice";
const RATE: usize = 16_000;

fn corpus_root() -> PathBuf {
    std::env::var("YAP_EVAL_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs::home_dir().expect("home").join("yap-eval-corpus/meetings"))
}

/// `YAP_MODEL_DIR` -> link the pinned ASR model into `<YAP_DATA_DIR>/models`
/// and select it there (the contract `meeting_eval.rs` and
/// `meeting_utterance_segments.rs` use). Never decode without it: the binary
/// would fall back to the smallest model and DOWNLOAD it.
fn provision_pinned_asr_model() -> bool {
    if let Ok(model_dir) = std::env::var("YAP_MODEL_DIR") {
        let data_dir = std::env::var("YAP_DATA_DIR")
            .ok()
            .filter(|d| !d.trim().is_empty())
            .unwrap_or_else(|| panic!("YAP_MODEL_DIR is set but YAP_DATA_DIR is not"));
        let pinned = wilson_voice_lib::models::recommended_model();
        let file = pinned.default_file().expect("the pinned model lists a file");
        let src = PathBuf::from(&model_dir).join(&file.filename);
        if src.is_file() {
            let models_dir = PathBuf::from(&data_dir).join("models");
            fs::create_dir_all(&models_dir).expect("models dir");
            let dest = models_dir.join(&file.filename);
            if !dest.exists() {
                std::os::unix::fs::symlink(&src, &dest).expect("link the pinned model");
            }
            let settings = PathBuf::from(&data_dir).join("settings.json");
            if !settings.exists() {
                fs::write(&settings, serde_json::json!({ "nativeModel": pinned.id }).to_string())
                    .expect("settings");
            }
        }
    }
    wilson_voice_lib::models::is_downloaded(wilson_voice_lib::models::recommended_model())
}

/// A scratch dir that is removed even when a decode panics.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_wav_16k(path: &Path) -> Vec<i16> {
    let mut r = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = r.spec();
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.sample_rate as usize, RATE);
    r.samples::<i16>().map(|s| s.expect("sample")).collect()
}

fn write_wav_16k(path: &Path, samples: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE as u32,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("wav");
    for s in samples {
        w.write_sample(*s).expect("sample");
    }
    w.finalize().expect("finalize");
}

/// The shipped geometry, the shipped decoder, the shipped shift-and-clip:
/// one [`ChunkOutcome`] per window.
fn decode_windows(samples: &[i16], scratch: &Path, label: &str) -> Vec<ChunkOutcome> {
    let total = samples.len() as f64 / RATE as f64;
    let plan = plan_windows_fixed(total, ResumePoint::start(), &ChunkConfig::default(), 0);
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_wilson-voice"));
    let mut out = Vec::new();
    for window in &plan {
        let from = (window.audio_start_seconds * RATE as f64) as usize;
        let to = ((window.audio_end_seconds * RATE as f64) as usize).min(samples.len());
        let wav = scratch.join(format!("{label}-{:03}.wav", window.index));
        write_wav_16k(&wav, &samples[from..to]);
        let run = Command::new(&bin)
            .arg("--transcribe-file")
            .arg(&wav)
            .arg("--timed")
            .output()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", bin.display()));
        let _ = fs::remove_file(&wav);
        assert!(
            run.status.success(),
            "decode of {label} window {} failed: {}",
            window.index,
            String::from_utf8_lossy(&run.stderr).trim()
        );
        let transcript: TimedTranscript =
            serde_json::from_str(String::from_utf8_lossy(&run.stdout).trim()).expect("a timed transcript");
        out.push(ChunkOutcome::from_transcript(window, transcript));
    }
    out
}

/// The rows the app would store for the call: track 0 AFTER the AEC
/// pre-pass, track 1 as recorded, merged in time order.
fn call_bleed_segments(root: &Path, scratch: &Path) -> Vec<MeetingSegment> {
    let dir = root.join(CALL_BLEED);
    let cleaned = scratch.join("track0.aec.wav");
    let report = wilson_voice_lib::meeting_aec::aec_prepass(&dir.join("track0.wav"), &dir.join("track1.wav"), &cleaned)
        .expect("the AEC pre-pass (yap24-NT11)");
    println!("NT4-CALL-BLEED aec call_atten_p50_db={:.1}", report.call_atten_p50_db);
    let mut rows: Vec<(i64, NewMeetingSegment)> = Vec::new();
    for (track, wav, label) in [(MIC_TRACK, cleaned.clone(), "mic-aec"), (SYSTEM_TRACK, dir.join("track1.wav"), "system")] {
        let samples = read_wav_16k(&wav);
        for chunk in decode_windows(&samples, scratch, label) {
            rows.extend(chunk_segments(&chunk, track).into_iter().map(|r| (track, r)));
        }
    }
    rows.sort_by(|a, b| a.1.start_seconds.total_cmp(&b.1.start_seconds));
    rows.into_iter()
        .enumerate()
        .map(|(i, (track, r))| MeetingSegment {
            id: format!("cb-{i:03}"),
            meeting_id: CALL_BLEED.to_string(),
            start_seconds: r.start_seconds,
            end_seconds: r.end_seconds,
            text: r.text,
            confidence: r.confidence,
            created_at: chrono::Utc::now(),
            track,
            cluster_index: None,
        })
        .collect()
}

/// The call's planted commitments and decisions, from the corpus meta.
fn call_bleed_truth(root: &Path, segments: &[MeetingSegment]) -> Truth {
    let path = root.join(CALL_BLEED).join("meta.json");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let value: serde_json::Value = serde_json::from_str(&raw).expect("meta.json parses");
    let meta: CallBleedMeta =
        serde_json::from_value(value.get("call_bleed").cloned().expect("meta.json has a call_bleed block"))
            .expect("the call_bleed block parses");
    let duration = value.get("duration_seconds").and_then(|d| d.as_f64()).unwrap_or_else(|| {
        segments.iter().map(|s| s.end_seconds).fold(0.0, f64::max)
    });
    Truth {
        commitments: meta
            .commitments
            .iter()
            .map(|c| Plant {
                owner: c.owner.clone(),
                keywords: c.keywords.clone(),
                start_seconds: c.start_seconds,
                explicit: true,
            })
            .collect(),
        decisions: meta
            .decisions
            .iter()
            .map(|d| Plant {
                owner: String::new(),
                keywords: d.keywords.clone(),
                start_seconds: d.start_seconds,
                explicit: true,
            })
            .collect(),
        distractor_starts: Vec::new(),
        transcript: segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n"),
        meeting_seconds: duration,
        match_by_start: false,
    }
}

#[test]
fn call_bleed_scoring_matches_by_keywords_and_never_by_start() {
    let truth = Truth {
        commitments: vec![Plant {
            owner: "Them".into(),
            keywords: vec!["ask".into(), "printer".into(), "quote".into(), "banners".into(), "email".into()],
            start_seconds: 54.65,
            explicit: true,
        }],
        decisions: Vec::new(),
        distractor_starts: Vec::new(),
        transcript: "I will ask the printer for a quote on the banners and email it over tomorrow.".into(),
        meeting_seconds: 275.0,
        match_by_start: false,
    };
    // Same start, wrong words: not the commitment on the ASR leg.
    let s = summary("", vec![item("Book the hall for the second weekend.", 54.65, Some("Them"))], vec![]);
    assert_eq!(score_against(&s, &truth).action_recall, 0.0);
    // The planted words, at an ASR row's start: the commitment, owner right.
    let s = summary("", vec![item("Ask the printer for a banners quote and email it.", 53.9, Some("Them"))], vec![]);
    let sc = score_against(&s, &truth);
    assert_eq!(sc.action_recall, 1.0);
    assert_eq!(sc.action_precision, 1.0);
    assert_eq!(sc.owner_right, 1);
}

#[test]
#[ignore = "decodes call-bleed-2voice through the AEC pre-pass and the shipped ASR, then runs the real notes sidecar"]
fn call_bleed_arms_over_the_aec_cleaned_transcript() {
    let Some((model, binary)) = notes_inputs() else { return };
    let require = eval_required();
    let root = corpus_root();
    if !root.join(CALL_BLEED).join("track0.wav").is_file() {
        assert!(!require, "YAP_EVAL_REQUIRE=1 but {CALL_BLEED} is not under {}", root.display());
        eprintln!("SKIP {CALL_BLEED} not under {} (YAP_EVAL_CORPUS)", root.display());
        return;
    }
    if !provision_pinned_asr_model() {
        assert!(!require, "YAP_EVAL_REQUIRE=1 but the pinned ASR model is not installed (YAP_MODEL_DIR)");
        eprintln!("SKIP the pinned ASR model is not installed (YAP_MODEL_DIR)");
        return;
    }
    let scratch = Scratch(std::env::temp_dir().join(format!("yap-notes-call-bleed-{}", std::process::id())));
    let _ = fs::remove_dir_all(&scratch.0);
    fs::create_dir_all(&scratch.0).expect("scratch");
    let asr_started = Instant::now();
    let segments = call_bleed_segments(&root, &scratch.0);
    let asr_wall = asr_started.elapsed().as_secs_f64();
    drop(scratch);
    assert!(!segments.is_empty(), "the AEC-cleaned decode stored no rows");
    let truth = call_bleed_truth(&root, &segments);
    println!(
        "NT4-CALL-BLEED asr_aec_wall_s={asr_wall:.1} rows={} mic_rows={} system_rows={} commitments={} decisions={}",
        segments.len(),
        segments.iter().filter(|s| s.track == MIC_TRACK).count(),
        segments.iter().filter(|s| s.track == SYSTEM_TRACK).count(),
        truth.commitments.len(),
        truth.decisions.len(),
    );

    let mut rows = Vec::new();
    let mut shipped: Option<ArmRun> = None;
    for (label, strategy, arm_model) in arms_under_test(&model) {
        let r = run_arm(&binary, &arm_model, strategy, &format!("call-bleed-{label}"), segments.clone(), &truth);
        println!("{}", r.summary.markdown);
        print_arm_line(CALL_BLEED, label, &r);
        rows.push(arm_row(&format!("{label} (call-bleed)"), &r));
        if label != "C" && strategy == NOTES_STRATEGY {
            shipped = Some(r);
        }
    }
    println!("NOTES-ARMS-TABLE {CALL_BLEED} (AEC-cleaned ASR)");
    println!("{ARM_TABLE_HEADER}");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for row in &rows {
        println!("{row}");
    }
    // The same NT4 gates bind the shipped arm on the call as on the planted
    // fixture; the other arms are measured, not gated.
    let shipped = shipped.expect("the shipped arm ran");
    let failures = gate_failures(&shipped.scores);
    assert!(
        failures.is_empty(),
        "summary recall gates failed on {CALL_BLEED} for arm {}: {failures:?}",
        NOTES_STRATEGY.tag()
    );
}

// ── the human half: the shipped arm on a COPY of a real meeting ──────────────
//
// Numbers only, never text. Point YAP_EVAL_MEETING_DB at a COPY of
// wilson_voice.db (never the live Application Support file) and
// YAP_EVAL_MEETING_ID at the 31-minute meeting. The shipped arm rewrites the
// notes from the stored rows (echo-suppressed rows excluded, the stored
// equivalent of AEC-cleaned input), prints the numbers, and stores the notes
// IN THE COPY so `scripts/eval/compare-reference.py --notes <copy> <id>` reads
// the same notes. Without the two variables it skips, even under
// YAP_EVAL_REQUIRE=1 (Wilson's meeting is not a loop fixture).

/// "Me" / "Them" / "Someone here" / blank / other, from the item's speaker.
fn owner_bucket(item: &SummaryItem) -> &'static str {
    match item.speaker.as_deref().map(str::trim) {
        None | Some("") | Some("Speaker") => "blank",
        Some("Me") => "Me",
        Some("Them") => "Them",
        Some("Someone here") => "Someone here",
        Some(_) => "other",
    }
}

#[test]
fn owner_buckets_name_the_four_labels_and_other() {
    let at = |who: Option<&str>| owner_bucket(&item("Send the outline.", 1.0, who));
    assert_eq!(at(Some("Me")), "Me");
    assert_eq!(at(Some("Them")), "Them");
    assert_eq!(at(Some("Someone here")), "Someone here");
    assert_eq!(at(None), "blank");
    assert_eq!(at(Some("")), "blank");
    assert_eq!(at(Some("Speaker 3")), "other");
}

#[test]
#[ignore = "human half: the shipped arm on a COPY of a real meeting DB; prints numbers only"]
fn human_half_real_meeting_copy_numbers_only() {
    let (Ok(db), Ok(meeting_id)) = (std::env::var("YAP_EVAL_MEETING_DB"), std::env::var("YAP_EVAL_MEETING_ID")) else {
        eprintln!("SKIP human half: set YAP_EVAL_MEETING_DB (a COPY of wilson_voice.db) and YAP_EVAL_MEETING_ID");
        return;
    };
    assert!(
        !db.contains("Application Support"),
        "YAP_EVAL_MEETING_DB must be a COPY outside Application Support, never the live data dir"
    );
    let Some((model, binary)) = notes_inputs() else { return };
    let conn = rusqlite::Connection::open(&db).expect("open the DB copy");
    let duration: f64 = conn
        .query_row("SELECT duration_seconds FROM meetings WHERE id = ?1", [&meeting_id], |r| r.get(0))
        .expect("the meeting row in the copy");
    let segments: Vec<MeetingSegment> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, start_seconds, end_seconds, text, confidence, track FROM meeting_segments \
                 WHERE meeting_id = ?1 AND suppressed_as_echo = 0 ORDER BY start_seconds",
            )
            .expect("the segments query");
        let rows = stmt
            .query_map([&meeting_id], |r| {
                Ok(MeetingSegment {
                    id: r.get(0)?,
                    meeting_id: meeting_id.clone(),
                    start_seconds: r.get(1)?,
                    end_seconds: r.get(2)?,
                    text: r.get(3)?,
                    confidence: r.get(4)?,
                    created_at: chrono::Utc::now(),
                    track: r.get(5)?,
                    cluster_index: None,
                })
            })
            .expect("segment rows");
        rows.collect::<Result<Vec<_>, _>>().expect("segment rows decode")
    };
    assert!(!segments.is_empty(), "the meeting has no stored rows in the copy");
    let truth = Truth {
        commitments: Vec::new(),
        decisions: Vec::new(),
        distractor_starts: Vec::new(),
        transcript: segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n"),
        meeting_seconds: duration,
        match_by_start: false,
    };
    let n_segments = segments.len();
    let r = run_arm(&binary, &model, NOTES_STRATEGY, "human-half", segments, &truth);
    let mut owners: BTreeMap<&'static str, usize> = BTreeMap::new();
    for a in &r.summary.actions {
        *owners.entry(owner_bucket(a)).or_default() += 1;
    }
    let owner = |k: &str| owners.get(k).copied().unwrap_or(0);
    println!(
        "HUMAN-HALF arm={} minutes={:.1} segments={n_segments} actions={} decisions={} questions={} verbatim={:.2} duplicates={} max_items_per_segment={} owner_me={} owner_them={} owner_someone_here={} owner_blank={} owner_other={} wall_s={:.1} peak_rss_mb={:.0}",
        NOTES_STRATEGY.tag(),
        duration / 60.0,
        r.summary.actions.len(),
        r.summary.decisions.len(),
        r.summary.questions.len(),
        r.scores.verbatim,
        r.scores.duplicates,
        r.scores.max_items_per_segment,
        owner("Me"),
        owner("Them"),
        owner("Someone here"),
        owner("blank"),
        owner("other"),
        r.wall_s,
        r.peak_total_kb as f64 / 1024.0,
    );
    conn.execute(
        "UPDATE meetings SET summary = ?1, summary_model = ?2 WHERE id = ?3",
        rusqlite::params![r.summary.markdown, r.summary.model, meeting_id],
    )
    .expect("store the notes in the copy");
}
