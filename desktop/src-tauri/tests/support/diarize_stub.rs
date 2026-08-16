//! YV126 — a stub `yap-diarize` process that answers `diarize` with segments
//! whose embeddings the TEST chose.
//!
//! Zero model bytes, zero onnxruntime, zero audio: `DiarizePool` only ever sees
//! a `Command`, a stdin and a stdout, so a `/bin/sh` script is indistinguishable
//! from the real sidecar as far as every policy above the wire is concerned.
//! This is the same trick YV121's `diarize_sidecar_pool.rs` uses, extended with
//! the one thing YV126's clustering needs: a per-turn embedding.
//!
//! The stub echoes the request's own `id` back (`sed` off the line) rather than
//! answering `1`, so the tests drive the pool's real public API instead of a
//! hand-assembled request chosen to match a hard-coded answer.

#![allow(dead_code)]

use std::process::Command;
use std::time::Duration;

use wilson_voice_lib::diarize::{DiarizeError, DiarizeLauncher};

/// The `min_embed` floor these stub-driven tests hand `cluster_track`.
///
/// **A FIXTURE, not a shipped value and not a tuned one.** YV122 made the floor
/// a mandatory parameter with no default anywhere in either crate precisely so
/// that no file can quietly become its source; this one exists because a stub
/// that answers from a shell script never looks at it. Nothing here is scored
/// against it, and the number a real pass should use is a measurement that
/// belongs to the harness (`meeting_eval::ARM_MIN_UTTERANCE_SECONDS`) and not
/// to a test of the parent's arithmetic.
pub const STUB_MIN_EMBED: Duration = Duration::from_secs(2);

/// One turn the stub will report: a time span and the embedding measured for it.
#[derive(Debug, Clone)]
pub struct StubTurn {
    pub start: f64,
    pub end: f64,
    /// The cluster the CHILD claims. Deliberately settable and deliberately
    /// wrong in some tests: the parent re-clusters from the embeddings, and a
    /// test whose stub already agreed with the answer could not tell the two
    /// apart.
    pub child_cluster: u32,
    pub embedding: Vec<f32>,
}

impl StubTurn {
    pub fn new(start: f64, end: f64, child_cluster: u32, embedding: Vec<f32>) -> Self {
        Self {
            start,
            end,
            child_cluster,
            embedding,
        }
    }
}

/// A unit vector at `radians` in the first two dimensions — so the cosine
/// distance between two rays is exactly `1 - cos(Δ)` and a test can ask for a
/// distance instead of hoping for one.
pub fn ray(radians: f64) -> Vec<f32> {
    vec![radians.cos() as f32, radians.sin() as f32]
}

/// The angle between two rays that are exactly `distance` apart in cosine
/// distance.
pub fn angle_for_distance(distance: f64) -> f64 {
    (1.0 - distance).clamp(-1.0, 1.0).acos()
}

fn json_of(turns: &[StubTurn]) -> String {
    let body: Vec<String> = turns
        .iter()
        .map(|t| {
            let embedding: Vec<String> = t.embedding.iter().map(|v| format!("{v:.9}")).collect();
            format!(
                r#"{{"start":{:.6},"end":{:.6},"cluster":{},"embedding":[{}]}}"#,
                t.start,
                t.end,
                t.child_cluster,
                embedding.join(",")
            )
        })
        .collect();
    body.join(",")
}

/// A launcher whose child announces readiness and then answers every `diarize`
/// with `turns`.
pub fn stub_returning(turns: Vec<StubTurn>) -> DiarizeLauncher {
    stub_with_body(json_of(&turns))
}

/// A stub that SEGMENTS DIFFERENTLY depending on the distance it was sent —
/// which is what the real child does, and what every other stub in this file
/// deliberately does not.
///
/// sherpa clusters in order to segment, so `clustering_distance_threshold`
/// changes the turn set and not only the ids on it: YV122's
/// `a_two_voice_track_diarizes_and_a_tighter_distance_never_merges_more` prints
/// the count moving. A stub that answers the same turns at every distance makes
/// any test of "is this mode independent of the distance" independent BY
/// CONSTRUCTION — that was the review finding this exists to close.
///
/// At or above `cutoff` the child answers `loose` (the merged view); below it,
/// `tight`. The comparison is done by `awk` so a shell without floating-point
/// `test` cannot silently pick one arm forever — a `cutoff` that never
/// separated the two bodies would make the test vacuous, so the tests that use
/// this assert both arms really did come back different.
pub fn stub_segmenting_by_distance(
    loose: Vec<StubTurn>,
    tight: Vec<StubTurn>,
    cutoff: f64,
) -> DiarizeLauncher {
    let script = format!(
        concat!(
            r#"printf '{{"type":"ready","version":"stub"}}\n'"#,
            "\n",
            "while read -r line; do\n",
            r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
            "\n",
            r#"  d=$(printf '%s' "$line" | sed -n 's/.*"clustering_distance_threshold":\([0-9.]*\).*/\1/p')"#,
            "\n",
            r#"  if [ -z "$d" ]; then d=0; fi"#,
            "\n",
            r#"  if awk "BEGIN{{exit !($d >= {cutoff})}}"; then body='{loose}'; else body='{tight}'; fi"#,
            "\n",
            r#"  printf '{{"id":%s,"ok":true,"segments":[%s]}}\n' "$id" "$body""#,
            "\ndone\n"
        ),
        cutoff = cutoff,
        loose = json_of(&loose),
        tight = json_of(&tight),
    );
    Box::new(move || {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script.clone());
        Ok::<Command, DiarizeError>(command)
    })
}

/// A stub that echoes back the `min_embed_seconds` it was sent, as the `end` of
/// a single turn — so a test can assert what reached the WIRE rather than
/// trusting a parameter it passed one function earlier.
///
/// YV122 made the floor mandatory and defaultless; the failure this catches is
/// `cluster_track` accepting a caller's floor and then not forwarding it, which
/// no assertion on `cluster_track`'s own return value could see.
pub fn stub_echoing_min_embed() -> DiarizeLauncher {
    let script = concat!(
        r#"printf '{"type":"ready","version":"stub"}\n'"#,
        "\n",
        "while read -r line; do\n",
        r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
        "\n",
        r#"  m=$(printf '%s' "$line" | sed -n 's/.*"min_embed_seconds":\([0-9.]*\).*/\1/p')"#,
        "\n",
        r#"  if [ -z "$m" ]; then m=-1; fi"#,
        "\n",
        r#"  printf '{"id":%s,"ok":true,"segments":[{"start":0.0,"end":%s,"cluster":0,"embedding":[1.0,0.0]}]}\n' "$id" "$m""#,
        "\ndone\n"
    )
    .to_string();
    Box::new(move || {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script.clone());
        Ok::<Command, DiarizeError>(command)
    })
}

/// The same, for a caller that wants to hand over the raw `segments` body — the
/// no-embeddings fallback case, which is a shape no `StubTurn` can express.
pub fn stub_with_body(segments_json: String) -> DiarizeLauncher {
    let script = format!(
        concat!(
            r#"printf '{{"type":"ready","version":"stub"}}\n'"#,
            "\n",
            "while read -r line; do\n",
            r#"  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')"#,
            "\n",
            r#"  printf '{{"id":%s,"ok":true,"segments":[{}]}}\n' "$id""#,
            "\ndone\n"
        ),
        segments_json
    );
    Box::new(move || {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script.clone());
        Ok::<Command, DiarizeError>(command)
    })
}
