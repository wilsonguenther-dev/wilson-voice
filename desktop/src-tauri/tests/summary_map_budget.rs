//! #250 round 2, defect 1 — the MAP pass ran out of its fixed 288-token
//! output budget on a six-line meeting, and the sidecar reported that as
//! `truncated`, which renders "_Part of this meeting did not fit…_" on a
//! meeting that fit whole. The budget is now sized to the chunk, an output
//! cap is its own flag (`out_capped`), and the note appears only when input
//! was really dropped.

mod support;

use support::{labels_in, segments_from, StubModel};
use wilson_voice_lib::polish_protocol::{
    summary_map_max_out, PolishRequest, SUMMARY_MAP_MAX_OUT, SUMMARY_MAP_MAX_OUT_CEILING,
};
use wilson_voice_lib::summarize::{summarize_segments, Generated, MAP_CHUNK_TOKENS};

const FIXTURE: &[&str] = &[
    "Okay, this is the pricing review for the Drivia consulting tier.",
    "We agreed the starter package stays at forty nine dollars a month.",
    "Jeisil will send the updated pricing doc to the team by Friday.",
    "Wilson to book the demo with Liberty University next week.",
    "The open question is whether annual billing gets a discount.",
    "We also decided to drop the free trial for enterprise accounts.",
];

/// The real-model MAP answer's bucketing from the PR #250 run, verbatim: the
/// question and the "decided to drop" line came back as ACTIONS.
fn misfiled_map(req: &PolishRequest) -> String {
    let ids = labels_in(&req.text);
    let id = |i: usize| ids.get(i).cloned().unwrap_or_else(|| ids[0].clone());
    format!(
        r#"{{"narrative":"The team reviewed pricing for the Drivia consulting tier.","actions":[{{"text":"Jeisil will send the updated pricing doc to the team by Friday","segment":"{}"}},{{"text":"Wilson to book the demo with Liberty University next week","segment":"{}"}},{{"text":"The open question is whether annual billing gets a discount","segment":"{}"}},{{"text":"We also decided to drop the free trial for enterprise accounts","segment":"{}"}}],"decisions":[{{"text":"The starter package will remain at forty nine dollars a month","segment":"{}"}}],"questions":[]}}"#,
        id(2),
        id(3),
        id(4),
        id(5),
        id(1)
    )
}

fn reduce_or_headline(req: &PolishRequest) -> Option<String> {
    match req.mode.as_str() {
        "reduce" => Some("The team reviewed pricing for the Drivia consulting tier.".into()),
        "headline" => Some(r#"{"title":"Drivia consulting pricing review","key_points":["The starter package stays at forty nine dollars a month.","The free trial is dropped for enterprise accounts.","Jeisil will send the updated pricing doc by Friday."]}"#.into()),
        _ => None,
    }
}

const NOTE: &str = "did not fit the model";

// ── defect 1: output budget + the "did not fit" note ────────────────────────

#[test]
fn map_output_budget_is_sized_to_the_chunk_and_fits_the_context() {
    let model = StubModel::with_headline(|req| {
        Ok(reduce_or_headline(req).unwrap_or_else(|| misfiled_map(req)))
    });
    summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    let map = model.stage("map");
    assert_eq!(map.len(), 1);
    assert!(
        map[0].max_out > SUMMARY_MAP_MAX_OUT,
        "six lines still get the old fixed 288: {}",
        map[0].max_out
    );
    assert!(map[0].max_out <= SUMMARY_MAP_MAX_OUT_CEILING);
    // The deadline grows with the budget, so a bigger budget cannot become a
    // `deadline` failure.
    assert!(map[0].deadline_ms >= u64::from(map[0].max_out) * 80);
    // A FULL chunk's budget + ~600 tokens of system turn / template still fits
    // the sidecar's 4096 context.
    let full = summary_map_max_out(MAP_CHUNK_TOKENS);
    assert!(MAP_CHUNK_TOKENS + 600 + full as usize <= 4096, "{full}");
}

#[test]
fn an_output_capped_map_answer_keeps_its_items_and_shows_no_note() {
    // What a pass that hit its OUTPUT budget looks like to the parent: the
    // JSON stops mid-array, and nothing of the input was dropped
    // (`truncated:false`; the sidecar now reports the cap as `out_capped`).
    let model = StubModel::with_headline(|req| {
        if let Some(a) = reduce_or_headline(req) {
            return Ok(a);
        }
        let full = misfiled_map(req);
        let cut = full.find("{\"text\":\"The open question").expect("cut point");
        Ok(full[..cut + 30].to_string())
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    assert!(!s.truncated, "an output cap is not an input cut");
    assert!(!s.markdown.contains(NOTE), "{}", s.markdown);
    assert_eq!(s.actions.len(), 2, "the closed items survive: {:?}", s.actions);
}

#[test]
fn an_input_truncated_pass_still_shows_the_note() {
    let model = StubModel::truncating(|req| {
        if let Some(a) = reduce_or_headline(req) {
            return Ok(Generated::whole(a));
        }
        Ok(Generated {
            text: misfiled_map(req),
            truncated: true,
        })
    });
    let s = summarize_segments(&segments_from(FIXTURE), &model).expect("a summary");
    assert!(s.truncated);
    assert!(s.markdown.contains(NOTE), "{}", s.markdown);
}
