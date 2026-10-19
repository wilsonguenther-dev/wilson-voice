//! focus-2026-10-19 — zero-setup meeting notes.
//!
//! With a stub model (no sidecar, no GGUF): a fixture transcript goes through
//! the real pipeline and comes back with a title, 3–6 key points, decisions,
//! action items with owners when said, and a one-paragraph summary; the model
//! plan never answers "no model"; and Cancel stops the run at its next call.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::summarize::{
    action_owner, cancel_requested, clear_cancel, parse_headline, plan_summary_model,
    request_cancel, summarize_segments, SummaryError, SummaryModelPlan, DEFAULT_SUMMARY_MODEL,
    HEADLINE_GRAMMAR,
};

const FIXTURE: &[&str] = &[
    "Okay, this is the pricing review for the Drivia consulting tier.",
    "We agreed the starter package stays at forty nine dollars a month.",
    "Jeisil will send the updated pricing doc to the team by Friday.",
    "Wilson to book the demo with Liberty University next week.",
    "The open question is whether annual billing gets a discount.",
    "We also decided to drop the free trial for enterprise accounts.",
];

fn answer(req: &wilson_voice_lib::polish_protocol::PolishRequest) -> Result<String, SummaryError> {
    {
        Ok(match req.mode.as_str() {
            "map" => {
                let ids = labels_in(&req.text);
                let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
                format!(
                    r#"{{"narrative":"The team reviewed pricing for the Drivia consulting tier. The starter package stays at forty nine dollars a month and the free trial is dropped for enterprise accounts.","actions":[{{"text":"Jeisil will send the updated pricing doc to the team by Friday.","segment":"{}"}},{{"text":"Wilson to book the demo with Liberty University next week.","segment":"{}"}}],"decisions":[{{"text":"The starter package stays at forty nine dollars a month.","segment":"{}"}},{{"text":"Drop the free trial for enterprise accounts.","segment":"{}"}}],"questions":[{{"text":"Whether annual billing gets a discount.","segment":"{}"}}]}}"#,
                    id(2), id(3), id(1), id(5), id(4)
                )
            }
            "reduce" => "The team reviewed pricing for the Drivia consulting tier.".to_string(),
            "headline" => r#"{"title":"Drivia consulting pricing review","key_points":["The starter package stays at forty nine dollars a month.","The free trial is dropped for enterprise accounts.","Jeisil will send the updated pricing doc by Friday.","Annual billing discount is still open."]}"#.to_string(),
            other => panic!("unexpected stage {other}"),
        })
    }
}

fn stub() -> StubModel {
    StubModel::with_headline(answer)
}

#[test]
fn fixture_transcript_becomes_full_notes() {
    clear_cancel();
    let segments = segments_from(FIXTURE);
    let model = stub();
    let s = summarize_segments(&segments, &model).expect("a summary");
    let md = &s.markdown;
    println!("{md}");
    assert!(
        md.starts_with("## Drivia consulting pricing review"),
        "{md}"
    );
    assert!(
        md.contains("### Summary\n\nThe team reviewed pricing"),
        "{md}"
    );
    let kp = md
        .split("### Key points\n\n")
        .nth(1)
        .expect("key points")
        .split("\n\n")
        .next()
        .unwrap();
    let n = kp.lines().filter(|l| l.starts_with("- ")).count();
    assert!((3..=6).contains(&n), "{n} key points: {kp}");
    assert!(md.contains("### Decisions"), "{md}");
    assert!(md.contains("- **Jeisil** — Jeisil will send"), "{md}");
    assert!(md.contains("- **Wilson** — Wilson to book"), "{md}");
    assert!(md.contains("### Open questions"), "{md}");
    assert!(!md.to_lowercase().contains("add a summary model"));
    assert_eq!(model.stage("headline").len(), 1);
    assert_eq!(
        model.stage("headline")[0].grammar.as_deref(),
        Some(HEADLINE_GRAMMAR)
    );
}

#[test]
fn a_failed_headline_pass_still_yields_title_and_three_points() {
    clear_cancel();
    let model = StubModel::with_headline(|req| match req.mode.as_str() {
        "headline" => Err(SummaryError::Protocol),
        _ => answer(req),
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    assert!(s.markdown.starts_with("## "), "{}", s.markdown);
    let kp = s
        .markdown
        .split("### Key points\n\n")
        .nth(1)
        .expect("key points");
    assert!(
        kp.lines().take_while(|l| l.starts_with("- ")).count() >= 3,
        "{kp}"
    );
}

#[test]
fn the_model_plan_never_says_no_model() {
    let none = |_: &str| false;
    assert_eq!(
        plan_summary_model("", &none),
        SummaryModelPlan::Download(DEFAULT_SUMMARY_MODEL.into())
    );
    let default_only = |m: &str| m == DEFAULT_SUMMARY_MODEL;
    assert_eq!(
        plan_summary_model("", &default_only),
        SummaryModelPlan::Use(DEFAULT_SUMMARY_MODEL.into())
    );
    assert_eq!(
        plan_summary_model("qwen2.5-0.5b-instruct-q4_k_m", &default_only),
        SummaryModelPlan::Use(DEFAULT_SUMMARY_MODEL.into())
    );
    let all = |_: &str| true;
    assert_eq!(
        plan_summary_model("qwen2.5-0.5b-instruct-q4_k_m", &all),
        SummaryModelPlan::Use("qwen2.5-0.5b-instruct-q4_k_m".into())
    );
}

#[test]
fn headline_parse_drops_invented_points_and_owner_needs_a_name_up_front() {
    let source =
        "The team reviewed pricing. The starter package stays at forty nine dollars a month.";
    let h = parse_headline(
        r#"```json
{"title":"Pricing review","key_points":["The starter package stays at forty nine dollars a month.","Revenue hit 2 million dollars in 2025."]}
```"#,
        source,
    )
    .expect("parsed");
    assert_eq!(h.title, "Pricing review");
    assert_eq!(h.key_points.len(), 1, "{:?}", h.key_points);
    // A title is held to the notes too: a topic they never mention is dropped
    // (the mechanical title fills in later), and so is an invented number.
    let off_topic = parse_headline(
        r#"{"title":"Quarterly hiring forecast","key_points":["The starter package stays at forty nine dollars a month."]}"#,
        source,
    )
    .expect("the point survives");
    assert_eq!(off_topic.title, "", "an ungrounded title is not kept");
    let invented = parse_headline(r#"{"title":"Pricing review 2025","key_points":[]}"#, source);
    assert_eq!(
        invented, None,
        "a title with a number the notes lack is not kept"
    );
    assert_eq!(
        action_owner("Jeisil will send the doc"),
        Some("Jeisil".into())
    );
    assert_eq!(action_owner("We will send the doc"), None);
    assert_eq!(action_owner("send the doc"), None);
}

#[test]
fn cancel_flag_round_trips() {
    clear_cancel();
    assert!(!cancel_requested());
    request_cancel();
    assert!(cancel_requested());
    clear_cancel();
}

/// The real run: the fixture through the REAL sidecar and the REAL default
/// model, headless (no app window). Opt-in — needs a built sidecar + GGUF:
///   YAP_REAL_SIDECAR=<yap-polish> YAP_REAL_MODEL=<qwen2.5-1.5b…gguf> \
///   cargo test --test summary_default_notes real_model -- --ignored --nocapture
#[test]
#[ignore]
fn real_model_fixture_run() {
    use std::path::Path;
    let bin = std::env::var("YAP_REAL_SIDECAR").expect("YAP_REAL_SIDECAR");
    let model = std::env::var("YAP_REAL_MODEL").expect("YAP_REAL_MODEL");
    let session = wilson_voice_lib::summarize::SidecarSession::spawn(
        Path::new(&bin),
        Path::new(&model),
        DEFAULT_SUMMARY_MODEL,
    )
    .expect("sidecar starts");
    let client = wilson_voice_lib::summarize::SidecarSummaryClient::new(session);
    let t0 = std::time::Instant::now();
    let s = summarize_segments(&segments_from(FIXTURE), &client).expect("a summary");
    println!(
        "REAL_RUN_BEGIN ({} ms)\n{}\nREAL_RUN_END",
        t0.elapsed().as_millis(),
        s.markdown
    );
    // The raw HEADLINE answer for the same notes, so a run shows whether the
    // title + key points came from the model or from the mechanical fallback.
    let raw = wilson_voice_lib::summarize::SummaryClient::generate(
        &client,
        &wilson_voice_lib::polish_protocol::PolishRequest::summarize(
            900_001,
            "headline",
            wilson_voice_lib::polish_protocol::SUMMARY_HEADLINE_MAX_OUT,
            120_000,
            s.markdown.clone(),
            Some(HEADLINE_GRAMMAR.to_string()),
        ),
    );
    match raw {
        Ok(g) => println!("RAW_HEADLINE truncated={} {}", g.truncated, g.text),
        Err(e) => println!("RAW_HEADLINE error {}", e.tag()),
    }
    assert!(s.markdown.starts_with("## "));

    // Memory: the sidecar (and the GGUF it mapped) lives only for the job.
    // Alive while the client is held, gone the moment it is dropped — which is
    // what `summarize_meeting_blocking` does when it returns.
    let sidecars = || -> Vec<(u32, u64)> {
        let out = std::process::Command::new("/bin/ps")
            .args(["-ax", "-o", "pid=,rss=,command="])
            .output()
            .expect("ps runs");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| {
                let mut f = l.split_whitespace();
                let pid = f.next()?.parse().ok()?;
                let rss = f.next()?.parse().ok()?;
                (f.next()? == bin).then_some((pid, rss))
            })
            .collect()
    };
    let during = sidecars();
    println!("SIDECAR_DURING {during:?} (pid, rss KiB)");
    assert_eq!(during.len(), 1, "one sidecar for the job: {during:?}");
    drop(client);
    let after = sidecars();
    println!("SIDECAR_AFTER {after:?}");
    assert!(
        after.is_empty(),
        "the sidecar exits with the job: {after:?}"
    );
}
