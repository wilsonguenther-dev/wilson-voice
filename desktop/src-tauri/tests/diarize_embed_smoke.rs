//! YV122 acceptance — the embeddings are real, and the distance threshold
//! really reaches sherpa.
//!
//! ```sh
//! cargo test --test diarize_embed_smoke -- --nocapture
//! ```
//!
//! # What the spec asked for, and why it is asked for differently here
//!
//! The backlog's second acceptance criterion is: *"embeds two utterances of the
//! same synthetic voice and two of a different one, asserts
//! `cosine_similarity(same_voice) > cosine_similarity(different_voice)`."*
//!
//! **That pairwise claim is false on macOS `say` voices, and it is false in a
//! way that matters far past this test.** Measured here, 6 voices × 3
//! utterances, 18 embeddings, 18 genuine pairs and 135 impostor pairs through
//! the real sidecar:
//!
//! ```text
//! genuine  n=18   min 0.5295  mean 0.7837  max 0.9837
//! impostor n=135  min 0.2847  mean 0.5789  max 0.9362
//! EER  ≈ 0.272  at similarity 0.6425   (FAR 0.267 / FRR 0.278)
//! worst impostors: Fred~Ralph 0.936 / 0.921 / 0.918, Samantha~Karen 0.904
//! worst genuine  : Fred~Fred  0.529 / 0.567, Rishi~Rishi 0.586
//! ```
//!
//! A single genuine pair (Samantha at 0.555) scores *below* a single impostor
//! pair (Samantha~Daniel at 0.678), so the literal two-versus-two assertion
//! fails on the first pair anyone picks. The cause is not the model: `Fred` and
//! `Ralph` are both legacy MacinTalk formant synths, `Samantha` and `Karen` are
//! the same neural engine at different presets, and a speaker embedding trained
//! on VoxCeleb reads the SYNTHESIZER, not the persona. CAM++'s published
//! VoxCeleb EER is well under 1%; **27% is this substrate's number, not the
//! model's**.
//!
//! Two consequences, both larger than this file:
//!
//! * YV120's diarization fixtures (e) and (f) are rendered with `say`. No
//!   clustering threshold (YV126) and no enrollment band (YV129/YV131) may be
//!   tuned on them — a threshold fitted to an EER of 0.27 encodes macOS's TTS
//!   engine and would be re-derived the moment it met a room.
//! * The claim this file CAN make honestly is the population one, and it makes
//!   it at full strength: mean genuine > mean impostor, and *every* voice's own
//!   genuine mean clears the global impostor mean. That is enough to prove what
//!   this item is actually responsible for — the embeddings are real vectors
//!   that respond to who is speaking — without pretending the substrate is
//!   good enough to tune on.
//!
//! Scores are computed with YV120's own `cosine_similarity` and scored with its
//! `enrollment_eer`, so the harness that will judge YV129 is the harness that
//! judged its substrate first.
//!
//! # The roster is checked, not assumed
//!
//! Every number above is a claim about six speakers, and `say` will silently
//! give you fewer: `say -v <not-installed>` exits **0** and renders with a
//! fallback voice (two different nonsense names produce byte-identical audio —
//! transcript in `support/diarize_models.rs`). `Fred` and `Ralph` are legacy
//! MacinTalk voices Apple has been moving to download-on-demand, so this is the
//! roster's live failure mode, not a hypothetical one.
//!
//! A collapse would be invisible to the obvious guard and is therefore checked
//! twice, on both sides of the measurement:
//!
//! * **before** — `assert_voices_are_distinct` requires each voice to be listed
//!   AND to render a probe line to bytes no other voice in the roster produces;
//! * **after** — no cross-voice pair may reach `DEGENERATE_SIMILARITY` (0.99);
//!   every voice speaks the same three texts, so two collapsed voices produce
//!   an identical same-text rendering and a cosine of exactly 1.0.
//!
//! Both exist because the overlap tripwire at the end of the sweep cannot do
//! this job: `impostor_max > genuine_min` becomes *more* true when an impostor
//! pair is secretly a genuine pair. A guard that a defect strengthens is not a
//! guard against that defect.

#[path = "support/diarize_models.rs"]
mod models;

use wilson_voice_lib::diarize_metrics::{cosine_similarity, enrollment_eer, CosineSimilarity};
use wilson_voice_lib::diarize_protocol::DiarizeRequest;

/// The sample rate the segmentation model runs at and the extractor is fed.
/// Asserted against what the loaded model reports rather than trusted.
const RATE_HZ: u32 = 16_000;

/// The shortest span of audio these tests consider worth embedding, in seconds.
///
/// **A fixture, not a shipped threshold.** `min_embed_seconds` is a request
/// field with no default anywhere in either crate; the sweep in this PR shows a
/// floor is necessary and cannot say where it belongs, because the only speech
/// this repo has is the macOS `say` output this very file measures an EER of
/// 0.272 on. 2.0 s is what these tests pass so that the gate is exercised in
/// both directions — nothing here asserts the number is right for real speech,
/// and `no_default_embedding_floor_is_written_down_anywhere` is what stops it
/// migrating into `src/`.
const TEST_FLOOR: f32 = 2.0;

/// Six voices spanning the two macOS synthesizers, deliberately including the
/// pair that confuses CAM++ worst (`Fred`/`Ralph`, both legacy MacinTalk). A
/// roster picked for separability would have hidden the finding above.
const VOICES: [&str; 6] = ["Samantha", "Daniel", "Karen", "Rishi", "Fred", "Ralph"];

/// Three utterances long enough for CAM++ to have something to work with, and
/// different enough that a same-voice pair is not scoring lexical overlap.
const UTTERANCES: [&str; 3] = [
    "The quarterly numbers came in this morning and the growth line finally crossed the mark we set in January.",
    "I would still like to see the churn breakdown before we commit to any hiring plan for the next two quarters.",
    "Let me pull up the dashboard and walk through the retention curve with everyone who is on this call today.",
];

/// The embeddings are real vectors that respond to who is speaking.
#[test]
fn camplus_embeddings_track_the_speaker_across_utterances() {
    let Some((segmentation, embedding_model)) = models::models() else {
        return;
    };
    let dir = models::scratch("embed-smoke");
    // Before a single number exists: prove the six voices are six voices.
    // `say` exits 0 on a name it does not have and renders a fallback, and a
    // collapsed roster makes the tripwire at the bottom of this test EASIER,
    // not harder — see `assert_voices_are_distinct`.
    models::assert_voices_are_distinct(&dir, &VOICES);
    let mut sidecar = models::Spawned::start();

    let dim = sidecar
        .ask(&DiarizeRequest::load_models(
            1,
            &segmentation,
            &embedding_model,
        ))
        .into_embedding_dim()
        .expect("both models load") as usize;

    // (voice, embedding), in roster order.
    let mut embeddings: Vec<(&str, Vec<f32>)> = Vec::new();
    let mut id = 2u64;
    for voice in VOICES {
        for (n, text) in UTTERANCES.iter().enumerate() {
            let wav = models::synthesize(&dir, voice, &n.to_string(), text, RATE_HZ);
            let vector = sidecar
                .ask(&DiarizeRequest::embed(id, &wav, TEST_FLOOR))
                .into_embedding()
                .unwrap_or_else(|| panic!("{voice} utterance {n} produced no embedding"));
            assert_eq!(
                vector.len(),
                dim,
                "every embedding is the width the model reported at load time"
            );
            assert!(
                vector.iter().any(|v| *v != 0.0),
                "{voice} utterance {n} embedded to all zeroes"
            );
            embeddings.push((voice, vector));
            id += 1;
        }
    }

    let mut genuine: Vec<CosineSimilarity> = Vec::new();
    let mut impostor: Vec<CosineSimilarity> = Vec::new();
    let mut per_voice: Vec<(&str, Vec<f64>)> = VOICES.iter().map(|v| (*v, Vec::new())).collect();
    for (i, (voice_a, a)) in embeddings.iter().enumerate() {
        for (voice_b, b) in embeddings.iter().skip(i + 1) {
            let score = cosine_similarity(a, b);
            if voice_a == voice_b {
                genuine.push(score);
                let slot = per_voice
                    .iter_mut()
                    .find(|(v, _)| v == voice_a)
                    .expect("roster");
                slot.1.push(score.get() as f64);
            } else {
                impostor.push(score);
            }
        }
    }

    let mean = |scores: &[CosineSimilarity]| -> f64 {
        scores.iter().map(|s| s.get() as f64).sum::<f64>() / scores.len() as f64
    };
    let genuine_mean = mean(&genuine);
    let impostor_mean = mean(&impostor);
    let genuine_min = genuine
        .iter()
        .map(|s| s.get() as f64)
        .fold(f64::INFINITY, f64::min);
    let impostor_max = impostor
        .iter()
        .map(|s| s.get() as f64)
        .fold(f64::NEG_INFINITY, f64::max);

    eprintln!(
        "genuine  n={:3}  mean {:.4}  min {:.4}",
        genuine.len(),
        genuine_mean,
        genuine_min
    );
    eprintln!(
        "impostor n={:3}  mean {:.4}  max {:.4}",
        impostor.len(),
        impostor_mean,
        impostor_max
    );

    // The population claim — the honest form of the spec's pairwise one.
    assert!(
        genuine_mean > impostor_mean,
        "same-voice pairs must score higher on average than different-voice \
         pairs: genuine {genuine_mean:.4} vs impostor {impostor_mean:.4}"
    );
    // …and it is not one voice carrying the average.
    for (voice, scores) in &per_voice {
        let own = scores.iter().sum::<f64>() / scores.len() as f64;
        eprintln!("  {voice:<9} genuine mean {own:.4}");
        assert!(
            own > impostor_mean,
            "{voice}'s own utterances must resemble each other more than the \
             average stranger does: {own:.4} vs {impostor_mean:.4}"
        );
    }

    // The ceiling, scored with the harness that will judge YV129.
    let report = enrollment_eer(&genuine, &impostor);
    eprintln!(
        "EER {:.3} at similarity {:.4} (FAR {:.3} / FRR {:.3}) — \
         THIS IS THE SYNTHESIZER'S NUMBER, NOT CAM++'S",
        report.eer,
        report.threshold_at_eer.get(),
        report.far_at_eer,
        report.frr_at_eer
    );
    // A tripwire, not a quality bar. The distributions OVERLAP on macOS `say`
    // voices, which is precisely why YV126/YV129 may not tune a threshold on
    // YV120's `say`-rendered fixtures. If this ever stops being true, the
    // substrate changed and the module docs above have to be re-derived before
    // anyone treats a number measured on it as a speaker-identity number.
    assert!(
        impostor_max > genuine_min,
        "the genuine and impostor distributions no longer overlap on this \
         substrate (genuine min {genuine_min:.4}, impostor max {impostor_max:.4}) \
         — re-derive this file's header before tuning anything on it"
    );
    // The second net under the roster, at the measurement layer this time.
    //
    // The tripwire above is satisfied MORE easily when two voices collapse into
    // one, so it cannot be the thing that catches a collapse. This can: every
    // voice renders the SAME three utterance texts, so if two of them are one
    // synthesizer, their same-text cross pair is byte-identical audio and
    // embeds to cosine 1.0 exactly. Measured max on a healthy roster is 0.9362
    // (Fred~Ralph, the worst legitimate pair on this substrate) — comfortably
    // clear of the bar, and the bar is degeneracy, not quality.
    assert!(
        impostor_max < DEGENERATE_SIMILARITY,
        "a cross-voice pair scored {impostor_max:.4} — at or past {DEGENERATE_SIMILARITY} \
         two of {VOICES:?} are the same voice, which `say` will not tell you \
         (it exits 0 on an absent voice and renders a fallback). Every number \
         above is then measured on fewer speakers than it names"
    );
}

/// The cosine similarity at which a cross-voice pair is not a hard pair but the
/// same audio twice. Set well above the 0.9362 worst legitimate impostor pair
/// measured on this substrate and well below the 1.0 an identical rendering
/// produces — this is a degeneracy detector, not a separability bar.
const DEGENERATE_SIMILARITY: f64 = 0.99;

/// A two-voice track segments into turns that carry embeddings, and the
/// clustering value on the wire is a DISTANCE the whole way down.
///
/// The unit assertion is the monotonicity, and it is the only form of this
/// claim that survives the process boundary. YV121's review found the inversion
/// (`CosineSimilarity::from_distance(clustering).get()`) surviving a green
/// suite because every test built the request it then inspected; the fix there
/// was to read the value back off the child's stdin. Here it is read off the
/// child's ANSWER: a smaller distance is a stricter cluster, so a tight
/// threshold must never yield fewer clusters than a loose one. Invert the value
/// anywhere between `DiarizePool::diarize` and `FastClusteringConfig` and this
/// ordering reverses.
#[test]
fn a_two_voice_track_diarizes_and_a_tighter_distance_never_merges_more() {
    let Some((segmentation, embedding_model)) = models::models() else {
        return;
    };
    let dir = models::scratch("diarize-smoke");
    // This track is only a two-voice track if the two voices are two voices.
    models::assert_voices_are_distinct(&dir, &VOICES[..2]);
    let mut sidecar = models::Spawned::start();
    let dim = sidecar
        .ask(&DiarizeRequest::load_models(
            1,
            &segmentation,
            &embedding_model,
        ))
        .into_embedding_dim()
        .expect("both models load") as usize;

    // Two voices, three turns, half a second of silence between them.
    let track = dir.join("two-voice-track.wav");
    let turns = [
        (VOICES[0], UTTERANCES[0]),
        (VOICES[1], UTTERANCES[1]),
        (VOICES[0], UTTERANCES[2]),
    ];
    let mut pcm: Vec<i16> = Vec::new();
    for (n, (voice, text)) in turns.iter().enumerate() {
        let wav = models::synthesize(&dir, voice, &format!("turn{n}"), text, RATE_HZ);
        pcm.extend(read_wav_i16(&wav, RATE_HZ));
        pcm.extend(std::iter::repeat_n(0i16, RATE_HZ as usize / 2));
    }
    write_wav_i16(&track, &pcm, RATE_HZ);

    // Tight first, loose second — sherpa's clustering runs on DISTANCE.
    let tight = 0.05f32;
    let loose = 1.2f32;
    let clusters_at = |sidecar: &mut models::Spawned, id: u64, distance: f32| -> usize {
        let segments = sidecar
            .ask(&DiarizeRequest::diarize(id, &track, distance, TEST_FLOOR))
            .into_segments()
            .unwrap_or_else(|| panic!("diarize at {distance} produced no segments"));
        assert!(
            !segments.is_empty(),
            "three spoken turns must not diarize to silence"
        );
        for segment in &segments {
            assert!(segment.end > segment.start, "a turn ends after it starts");
            assert!(
                segment.embedding.is_empty() || segment.embedding.len() == dim,
                "a turn's embedding is the reported width or absent, never a \
                 third thing: {}",
                segment.embedding.len()
            );
        }
        let distinct: std::collections::BTreeSet<u32> =
            segments.iter().map(|s| s.cluster).collect();
        eprintln!(
            "clustering distance {distance:<5} -> {} turns, {} clusters {distinct:?}",
            segments.len(),
            distinct.len()
        );
        // Every turn long enough to embed did embed — the "empty is allowed"
        // escape hatch must not be swallowing every turn.
        assert!(
            segments.iter().any(|s| s.embedding.len() == dim),
            "no turn produced an embedding"
        );
        distinct.len()
    };

    let tight_clusters = clusters_at(&mut sidecar, 2, tight);
    let loose_clusters = clusters_at(&mut sidecar, 3, loose);
    assert!(
        tight_clusters > loose_clusters,
        "a cosine DISTANCE of {tight} is STRICTER than {loose} and must not \
         produce fewer clusters ({tight_clusters} vs {loose_clusters}) — an \
         inverted unit anywhere on this path reverses exactly this ordering \
         (merged finding #20)"
    );

    sidecar.assert_stdout_is_only_protocol();

    // A track at the wrong rate is REFUSED, not silently mis-timed.
    //
    // `OfflineSpeakerDiarization::process` takes samples and no sample rate: it
    // reads them at whatever the segmentation model runs at. Hand it 44.1 kHz
    // audio for a 16 kHz model and every turn boundary comes back 2.76x off, at
    // times that still look like times — a whole meeting attributed to the
    // wrong moments, with nothing downstream in a position to notice.
    let wrong_rate = models::synthesize(&dir, VOICES[0], "44k", UTTERANCES[0], 44_100);
    let response = sidecar.ask(&DiarizeRequest::diarize(4, &wrong_rate, 0.35, TEST_FLOOR));
    assert_eq!(
        response.err_tag(),
        Some(wilson_voice_lib::diarize_protocol::ERR_SAMPLE_RATE),
        "a rate the segmentation model does not run at is a refusal; \
         resampling belongs upstream, behind the anti-alias filter (OS-8)"
    );
}

/// The turns sherpa returns OVERLAP IN TIME, and a turn is embedded on the
/// audio no other turn claims — or on nothing at all.
///
/// # The two defects this test exists for
///
/// **1. `process` does not return disjoint turns.** `diarize_protocol.rs` and
/// merged finding #22 both say overlapped frames are deleted before embedding.
/// That is true of the vectors sherpa computes for its OWN clustering
/// (`ExcludeOverlap`, inside `process`) and false of the turn list it hands
/// back. The per-turn embeddings this repo ships are a second pass over that
/// list, so before the fix, a turn's "voiceprint" was computed on whatever
/// audio its span covered — including another speaker.
///
/// **2. an empty vector was not a gate.** `audio_too_short` fires below ~10
/// samples on the shipped CAM++; a 0.2 s clip returns a full 512 floats. The
/// second half of this test measures that directly, because it is the reason a
/// floor has to exist at all.
///
/// # Why the numbers here are derived and not written down
///
/// Every threshold in this test is computed from the turns this run produced.
/// The floor is a request field with no default in either crate (see
/// `diarize_wire_unit_discipline.rs::no_default_embedding_floor_is_written_down_anywhere`),
/// and this file is the one that measured why nobody may tune a speaker number
/// on `say` voices — so it does not tune one. Three arms, one track:
/// floor below everything, floor above everything, floor between.
#[test]
fn a_turn_is_embedded_on_its_own_audio_and_the_floor_is_what_decides() {
    let Some((segmentation, embedding_model)) = models::models() else {
        return;
    };
    let dir = models::scratch("overlap");
    models::assert_voices_are_distinct(&dir, &VOICES[..2]);
    let mut sidecar = models::Spawned::start();
    let dim = sidecar
        .ask(&DiarizeRequest::load_models(
            1,
            &segmentation,
            &embedding_model,
        ))
        .into_embedding_dim()
        .expect("both models load") as usize;

    // Two voices, summed, with the second starting 2 s into the first — a
    // deliberate collision, which is exactly the case the segmentation model's
    // own ceiling says it cannot resolve and therefore the case where the
    // returned spans overlap.
    const SECOND_VOICE_ENTERS_AT: f64 = 2.0;
    let a = models::synthesize(&dir, VOICES[0], "ov0", UTTERANCES[0], RATE_HZ);
    let b = models::synthesize(&dir, VOICES[1], "ov1", UTTERANCES[1], RATE_HZ);
    let (pcm_a, pcm_b) = (read_wav_i16(&a, RATE_HZ), read_wav_i16(&b, RATE_HZ));

    // With models LOADED, a request that omits the floor is refused. The
    // sidecar's own unit test can only reach the `no_models` arm; this is the
    // one that proves the field is not quietly optional once there is a backend
    // to run — i.e. that an older parent beside a newer staged sidecar cannot
    // silently go back to embedding every fragment.
    let mut floorless = DiarizeRequest::diarize(90, &a, 0.35, 1.0);
    floorless.min_embed_seconds = None;
    assert_eq!(
        sidecar.ask(&floorless).err_tag(),
        Some(wilson_voice_lib::diarize_protocol::ERR_MISSING_FIELD),
        "a diarize request with no floor must be refused, never defaulted"
    );
    let mut floorless = DiarizeRequest::embed(91, &a, 1.0);
    floorless.min_embed_seconds = None;
    assert_eq!(
        sidecar.ask(&floorless).err_tag(),
        Some(wilson_voice_lib::diarize_protocol::ERR_MISSING_FIELD),
        "an embed request with no floor must be refused, never defaulted"
    );

    let offset = (SECOND_VOICE_ENTERS_AT * RATE_HZ as f64) as usize;
    let mut mixed = vec![0i32; pcm_a.len().max(offset + pcm_b.len())];
    for (i, s) in pcm_a.iter().enumerate() {
        mixed[i] += i32::from(*s) / 2;
    }
    for (i, s) in pcm_b.iter().enumerate() {
        mixed[offset + i] += i32::from(*s) / 2;
    }
    let mixed: Vec<i16> = mixed
        .into_iter()
        .map(|s| s.clamp(-32768, 32767) as i16)
        .collect();
    let track = dir.join("overlapped.wav");
    write_wav_i16(&track, &mixed, RATE_HZ);

    // A floor of zero: nothing is gated, so this run is the turn list itself.
    let turns = sidecar
        .ask(&DiarizeRequest::diarize(2, &track, 0.35, 0.0))
        .into_segments()
        .expect("the overlapped track diarizes");
    assert!(turns.len() >= 2, "two voices, at least two turns");
    for turn in &turns {
        eprintln!(
            "turn {:.2}-{:.2}s cluster {} embedding {}",
            turn.start,
            turn.end,
            turn.cluster,
            turn.embedding.len()
        );
    }

    // The premise, asserted rather than assumed. If sherpa ever starts
    // returning disjoint turns, the masking below becomes a no-op and this test
    // would otherwise keep passing while proving nothing.
    let overlap: f64 = turns
        .iter()
        .enumerate()
        .flat_map(|(i, x)| {
            turns
                .iter()
                .skip(i + 1)
                .map(move |y| (x.end.min(y.end) - x.start.max(y.start)).max(0.0))
        })
        .sum();
    assert!(
        overlap > 0.5,
        "the turns sherpa returned do not overlap in time ({overlap:.2}s total), \
         so this track no longer exercises the masking it was built for — \
         rebuild the fixture before trusting the assertions below"
    );

    // Exclusive seconds per turn, computed HERE from the returned times, so the
    // sidecar's arithmetic is checked against a second implementation rather
    // than against itself.
    let exclusive: Vec<f64> = turns
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            let mut seconds = turn.end - turn.start;
            for (j, other) in turns.iter().enumerate() {
                if i != j {
                    seconds -= (turn.end.min(other.end) - turn.start.max(other.start)).max(0.0);
                }
            }
            seconds.max(0.0)
        })
        .collect();
    eprintln!("exclusive seconds per turn: {exclusive:?}");
    assert!(
        exclusive.iter().any(|s| *s > 0.0),
        "at least one turn has audio of its own"
    );

    // At a floor of zero, every turn with exclusive audio embedded. That is
    // what makes an empty vector in the next two arms attributable to the
    // floor and not to a broken masking path.
    for (turn, seconds) in turns.iter().zip(&exclusive) {
        if *seconds > 0.0 {
            assert_eq!(
                turn.embedding.len(),
                dim,
                "a turn with {seconds:.2}s of its own audio embedded to nothing \
                 at a floor of zero"
            );
        } else {
            assert!(
                turn.embedding.is_empty(),
                "a turn with no audio of its own must not produce a vector"
            );
        }
    }

    let longest = exclusive.iter().cloned().fold(0.0f64, f64::max);
    let embedded_at = |sidecar: &mut models::Spawned, id: u64, floor: f64| -> Vec<usize> {
        sidecar
            .ask(&DiarizeRequest::diarize(id, &track, 0.35, floor as f32))
            .into_segments()
            .expect("diarize")
            .iter()
            .map(|t| t.embedding.len())
            .collect()
    };

    // Above everything: not one turn may embed.
    let above = embedded_at(&mut sidecar, 3, longest + 1.0);
    assert!(
        above.iter().all(|len| *len == 0),
        "a floor above every turn's exclusive audio must leave no vector at \
         all, got {above:?}"
    );

    // Between: exactly the turns at or above the floor embed. Derived from this
    // run's own turn list, so `say`'s timing can move without making this test
    // flaky or making it vacuous.
    let mut sorted: Vec<f64> = exclusive.iter().cloned().filter(|s| *s > 0.0).collect();
    sorted.sort_by(|x, y| x.partial_cmp(y).expect("no NaN"));
    if sorted.len() >= 2 && sorted[0] < sorted[sorted.len() - 1] {
        let between = (sorted[0] + sorted[sorted.len() - 1]) / 2.0;
        let lens = embedded_at(&mut sidecar, 4, between);
        eprintln!("floor {between:.2}s -> embedding widths {lens:?}");
        for (len, seconds) in lens.iter().zip(&exclusive) {
            let expected = if *seconds >= between { dim } else { 0 };
            assert_eq!(
                *len,
                expected,
                "a turn with {seconds:.2}s of exclusive audio against a {between:.2}s \
                 floor must {} — the empty/non-empty answer is the gate now, and it \
                 has to mean what it says",
                if expected == 0 {
                    "come back empty"
                } else {
                    "embed"
                }
            );
        }
        assert!(
            lens.contains(&0) && lens.contains(&dim),
            "this arm proves nothing unless the floor actually split the turns: {lens:?}"
        );
    }

    // ── And the measurement the floor exists for ────────────────────────────
    //
    // A 0.2 s clip through the enrollment path. At a floor of zero it comes
    // back as a full-width vector — which is the defect: nothing about that
    // answer says "there was not enough audio", and `into_embedding()` reports
    // it as a voiceprint. With a floor it is a refusal that names itself.
    let clip = dir.join("fragment.wav");
    let short = read_wav_i16(&a, RATE_HZ);
    let from = short.len() / 2;
    write_wav_i16(
        &clip,
        &short[from..(from + (0.2 * RATE_HZ as f64) as usize).min(short.len())],
        RATE_HZ,
    );
    let ungated = sidecar
        .ask(&DiarizeRequest::embed(5, &clip, 0.0))
        .into_embedding()
        .expect("a 0.2s clip is not refused by the extractor");
    assert_eq!(
        ungated.len(),
        dim,
        "0.2s of audio returns a FULL-WIDTH vector — this is the measurement \
         that makes `audio_too_short` a liveness check and not a sufficiency \
         one, and it is why the floor is a request field"
    );
    assert_eq!(
        sidecar
            .ask(&DiarizeRequest::embed(6, &clip, TEST_FLOOR))
            .err_tag(),
        Some(wilson_voice_lib::diarize_protocol::ERR_AUDIO_TOO_SHORT),
        "with a floor, the same clip is a refusal that says why"
    );

    sidecar.assert_stdout_is_only_protocol();
}

// ── Minimal WAV I/O, so this file owns its own fixture ─────────────────────

fn read_wav_i16(path: &std::path::Path, expect_rate: u32) -> Vec<i16> {
    let bytes = std::fs::read(path).expect("read wav");
    let rate = u32::from_le_bytes(bytes[24..28].try_into().expect("rate"));
    assert_eq!(rate, expect_rate, "afconvert ignored the requested rate");
    // Walk the chunk list rather than assuming `data` is at a fixed offset.
    let mut offset = 12;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size =
            u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().expect("size")) as usize;
        let body = offset + 8;
        if id == b"data" {
            return bytes[body..(body + size).min(bytes.len())]
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        offset = body + size + (size & 1);
    }
    panic!("no data chunk in {}", path.display());
}

fn write_wav_i16(path: &std::path::Path, samples: &[i16], rate: u32) {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, out).expect("write wav");
}
