//! Fixture `call-bleed-2voice` (yap-audit-eval-call-bleed): the script, the
//! planted facts and the mix. A call between two people, rendered with `say`,
//! recorded the way a laptop records one: the far side arrives on the system
//! track, and ALSO leaks into the microphone.
//!
//! * `track1.wav` — the far side, clean.
//! * `track0.wav` — the near side, plus the far side at -12 dB, 60 ms late,
//!   with a short reverb tail, on a quiet room floor.
//!
//! The mix is here, not in `meeting_eval.rs`, because it is pure and therefore
//! testable without `say`: `tests/meeting_eval_metrics.rs` checks the gain, the
//! delay and the tail on a synthetic impulse.
//!
//! Nothing in the script is a name, a digit or an address (the corpus rule), so
//! a regenerated corpus can never contain anything resembling real speech.

use serde::{Deserialize, Serialize};

/// Who is talking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    /// The user, on the microphone. Rendered with [`NEAR_VOICE`].
    Me,
    /// The far side of the call, on the system track. [`FAR_VOICE`].
    Them,
    /// A second person in the user's room, on the microphone, for one stretch
    /// of the call. [`SECOND_NEAR_VOICE`].
    Here,
}

impl Who {
    /// The label the shipped renderer and the notes use for this speaker.
    pub fn label(self) -> &'static str {
        match self {
            Who::Me => "You",
            Who::Them => "Call",
            Who::Here => "Someone here",
        }
    }

    pub fn voice(self) -> &'static str {
        match self {
            Who::Me => NEAR_VOICE,
            Who::Them => FAR_VOICE,
            Who::Here => SECOND_NEAR_VOICE,
        }
    }

    /// Whether this speaker reaches the microphone directly (and so lands on
    /// `track0.wav`) rather than arriving over the call.
    pub fn is_near(self) -> bool {
        !matches!(self, Who::Them)
    }
}

/// Three voices measured apart in fundamental frequency in `meeting_eval.rs`'s
/// roster comments (Samantha / Fred / Ralph are all in the six-voice roster of
/// fixture (f) because they separate).
pub const NEAR_VOICE: &str = "Samantha";
pub const FAR_VOICE: &str = "Fred";
pub const SECOND_NEAR_VOICE: &str = "Ralph";

/// What a turn plants for the notes to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plant {
    Nothing,
    /// The speaker commits to do something. The words are the keywords the
    /// notes item must carry to count as the same commitment.
    Commitment(&'static [&'static str]),
    /// The group decides something.
    Decision(&'static [&'static str]),
}

#[derive(Debug, Clone, Copy)]
pub struct ScriptTurn {
    pub who: Who,
    pub text: &'static str,
    pub plant: Plant,
}

const fn t(who: Who, text: &'static str) -> ScriptTurn {
    ScriptTurn {
        who,
        text,
        plant: Plant::Nothing,
    }
}

const fn p(who: Who, text: &'static str, plant: Plant) -> ScriptTurn {
    ScriptTurn { who, text, plant }
}

use Who::{Here, Me, Them};

/// The call: a planning check-in for a community workshop series. 48 turns of
/// 2-8 s, one stretch where a second voice sits on the near side, six planted
/// commitments (3 Them, 2 Me, 1 Someone here) and two planted decisions.
/// Decisions are made by the group, so they carry no owner.
pub const SCRIPT: [ScriptTurn; 48] = [
    t(Them, "Thanks for making time today, I wanted to walk through where the workshop series stands."),
    t(Me, "Happy to, I have a few notes on the venue and the budget, so go ahead and start."),
    t(Them, "The community hall has the second and third weekend open, but only one of them is free all day."),
    t(Me, "Which one is free all day, because the morning sessions need the stage and the projector."),
    t(Them, "The second weekend is the clear one, the third has a wedding booked in the afternoon."),
    t(Me, "Then the second weekend looks better to me, unless the parking situation changes anything."),
    t(Them, "Parking is fine, there is a lot behind the building that holds about sixty cars."),
    t(Me, "Good, that settles one worry, so let us talk about the money for a minute."),
    t(Them, "Last time we guessed the total cost would be about the same as the spring series."),
    t(Me, "It is close, but the banners and the printed handouts are more expensive this year."),
    p(Them, "I will ask the printer for a quote on the banners and email it over tomorrow.",
      Plant::Commitment(&["ask", "printer", "quote", "banners", "email"])),
    p(Me, "Perfect, and I will send the revised budget outline to the whole group by Friday.",
      Plant::Commitment(&["send", "revised", "budget", "outline", "group", "friday"])),
    t(Them, "While you are at it, include a line for refreshments, people always stay longer than planned."),
    t(Me, "Good point, a refreshments line and a small buffer for surprises would both make sense."),
    p(Them, "Speaking of refreshments, I will call the caterer and confirm the headcount before the end of the week.",
      Plant::Commitment(&["call", "caterer", "confirm", "headcount", "week"])),
    t(Me, "That works, although we should wait for the registration numbers before we give a final count."),
    t(Them, "Registration opened on Monday and we already have more sign ups than the spring series had in total."),
    t(Me, "That is encouraging, it means the second weekend might fill up faster than we expected."),
    t(Them, "Which brings me to the session size, because the hall gets crowded past a certain point."),
    t(Me, "I would be comfortable with about thirty people in each room, any more and the discussion gets lost."),
    p(Them, "Then it is decided, we cap each session at thirty people and open a waiting list for the rest.",
      Plant::Decision(&["cap", "session", "thirty", "people", "waiting", "list"])),
    t(Me, "Agreed, and the waiting list can live in the same sign up form so nobody has to track two lists."),
    t(Me, "One thing before we continue, a colleague just walked in to help with the equipment, so you will hear a second voice."),
    t(Here, "Hello everyone, sorry to interrupt, I was asked to help with the setup and the logistics."),
    t(Them, "Welcome, we were just talking about the tables and chairs, do you know what the hall already has?"),
    t(Here, "The hall has about twenty chairs, but nothing like enough tables for the craft stations."),
    t(Them, "That is what I was worried about, we will need at least a dozen more tables."),
    p(Here, "I can pick up the folding tables from the church on Thursday, they have plenty.",
      Plant::Commitment(&["pick", "folding", "tables", "church", "thursday"])),
    p(Them, "Wonderful, I will email the church office tonight to let them know you are coming on Thursday.",
      Plant::Commitment(&["email", "church", "office", "tonight", "coming", "thursday"])),
    t(Me, "Thanks for the help, he is heading out now, so it is just me on this side again."),
    t(Them, "Back to the schedule, I think the morning sessions should start at nine so people can leave by noon."),
    t(Me, "Nine is early for a weekend, but the hall is quiet then and the coffee will be fresh."),
    p(Them, "So we are agreed, the workshops will run on Saturday mornings from nine until noon.",
      Plant::Decision(&["workshops", "run", "saturday", "mornings", "nine", "noon"])),
    p(Me, "Yes, Saturday mornings it is, and I will draft the volunteer schedule and share it before the next call.",
      Plant::Commitment(&["draft", "volunteer", "schedule", "share", "next", "call"])),
    t(Them, "Great, and please keep the volunteer shifts short, nobody wants to stand at a registration desk all morning."),
    t(Me, "Two hour shifts with a break in the middle should keep everyone cheerful."),
    t(Them, "Do we have enough volunteers for that, or should we send another request to the mailing list?"),
    t(Me, "We have eleven so far, and I think another reminder would bring in a few more."),
    t(Them, "The reminder should be short, with the date, the time and a clear way to sign up."),
    t(Me, "Agreed, long emails get ignored, and a clear date in the first line does most of the work."),
    t(Them, "One more question, what do we do if the weather turns bad on the day of the event?"),
    t(Me, "The hall is indoors, so the weather only matters for the walk from the parking lot."),
    t(Them, "True, although a few umbrellas by the door would not hurt anyone."),
    t(Me, "That is easy, there is a basket in the storage room that nobody ever uses."),
    t(Them, "Then I think we have covered everything, let me read back the main points before we hang up."),
    t(Them, "The second weekend, thirty per session, Saturday mornings, and a short list of tasks for each of us."),
    t(Me, "That matches my notes exactly, thank you for pulling it all together."),
    t(Them, "Thank you both, talk to you soon, and have a good rest of the day."),
];

/// Gap before each turn, in seconds. Varied (deterministically) so the pauses
/// are not a metronome a VAD could learn.
pub fn gap_before(index: usize) -> f64 {
    if index == 0 {
        0.5
    } else {
        [0.40, 0.55, 0.70][index % 3]
    }
}

/// Far side level in the mic, relative to the far side's own track.
pub const BLEED_GAIN_DB: f64 = -12.0;
/// How late the far side arrives in the mic, in seconds.
pub const BLEED_DELAY_SECONDS: f64 = 0.060;
/// The short reverb tail applied to the bleed: `(extra delay after the direct
/// bleed in seconds, gain relative to it)`. The longest tap is the tail length.
pub const BLEED_TAIL: [(f64, f32); 4] =
    [(0.020, 0.35), (0.045, 0.22), (0.080, 0.14), (0.120, 0.08)];

/// Linear gain for [`BLEED_GAIN_DB`].
pub fn bleed_gain() -> f32 {
    10f64.powf(BLEED_GAIN_DB / 20.0) as f32
}

/// The far side as the microphone hears it: `far` at [`BLEED_GAIN_DB`], delayed
/// [`BLEED_DELAY_SECONDS`], plus [`BLEED_TAIL`]. Same length as `far` plus the
/// delay and the longest tap, at `rate_hz`.
pub fn bleed_of(far: &[f32], rate_hz: u32) -> Vec<f32> {
    let rate = rate_hz as f64;
    let delay = (BLEED_DELAY_SECONDS * rate).round() as usize;
    let tail = BLEED_TAIL
        .iter()
        .map(|(extra, _)| (extra * rate).round() as usize)
        .max()
        .unwrap_or(0);
    let gain = bleed_gain();
    let mut out = vec![0.0f32; far.len() + delay + tail];
    for (i, s) in far.iter().enumerate() {
        out[i + delay] += s * gain;
    }
    for (extra, tap_gain) in BLEED_TAIL {
        let offset = delay + (extra * rate).round() as usize;
        for (i, s) in far.iter().enumerate() {
            out[i + offset] += s * gain * tap_gain;
        }
    }
    out
}

/// One rendered turn on the call's clock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallTurn {
    pub speaker: String,
    pub voice: String,
    /// `track0.wav` (microphone) or `track1.wav` (system) as the speaker's own
    /// track — the one the speech is on without any bleed.
    pub track: String,
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// A planted commitment: the sentence, who owns it, the words a notes item must
/// carry to be the same commitment, and when it was said.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlantedCommitment {
    pub text: String,
    pub owner: String,
    pub keywords: Vec<String>,
    pub start_seconds: f64,
}

/// A planted decision (no owner: the group decided).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlantedDecision {
    pub text: String,
    pub keywords: Vec<String>,
    pub start_seconds: f64,
}

/// Everything about the call that `FixtureMeta` has no place for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallBleedMeta {
    /// `[microphone, system]`, in track order.
    pub tracks: Vec<String>,
    pub bleed_gain_db: f64,
    pub bleed_delay_ms: f64,
    /// Length of the longest reverb tap, in milliseconds.
    pub bleed_tail_ms: f64,
    /// The span of the second near-side voice: first turn start to last turn end.
    pub second_voice_window: (f64, f64),
    pub turns: Vec<CallTurn>,
    pub commitments: Vec<PlantedCommitment>,
    pub decisions: Vec<PlantedDecision>,
}

/// `M:SS - Speaker` blocks, one per turn, as `compare-reference.py` reads them.
pub fn reference_blocks(turns: &[CallTurn]) -> String {
    let mut out = String::new();
    for (i, turn) in turns.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let whole = turn.start_seconds.floor() as u64;
        out.push_str(&format!(
            "{}:{:02} - {}\n{}\n",
            whole / 60,
            whole % 60,
            turn.speaker,
            turn.text
        ));
    }
    out
}
