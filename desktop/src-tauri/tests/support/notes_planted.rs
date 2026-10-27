//! yap24-NT4 R2 — the planted-commitment notes fixture: a 15-minute two-person
//! call (Me on the microphone, Them on the system track) whose script plants
//! exactly what the notes must find and what they must NOT file as an action.
//!
//! * 6 EXPLICIT commitments ("I will send the deck by Friday"),
//! * 4 IMPLICIT commitments ("I can get you the specs"),
//! * 2 decisions,
//! * 6 small-talk DISTRACTORS that use "going to" or "'ll" — the shapes
//!   `generic_commitment` lets through — and must never become action items,
//! * at least 2 commitments inside the last 20% of the meeting (end-of-call
//!   wrap-ups are where real notes lose items).
//!
//! The script is pure data on the call's clock, so it is the transcript the
//! notes pipeline reads in `tests/meeting_notes_recall_eval.rs` and the same
//! script a `say` render can voice (the voices are the call-bleed roster's,
//! `support::call_bleed::{NEAR_VOICE, FAR_VOICE}`). Like the corpus rule,
//! nothing here is a name or a digit.

use wilson_voice_lib::meetings::{MeetingSegment, MIC_TRACK, SYSTEM_TRACK};

/// The fixture's length on the call clock, in seconds.
pub const MEETING_SECONDS: f64 = 900.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Me,
    Them,
}

impl Speaker {
    pub fn label(self) -> &'static str {
        match self {
            Speaker::Me => "Me",
            Speaker::Them => "Them",
        }
    }
    pub fn track(self) -> i64 {
        match self {
            Speaker::Me => MIC_TRACK,
            Speaker::Them => SYSTEM_TRACK,
        }
    }
}

/// What a line plants. The keywords are words of the spoken line a notes item
/// must carry (share >= `NOTE_MATCH_SHARE`) to count as the same item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plant {
    Filler,
    Explicit(&'static [&'static str]),
    Implicit(&'static [&'static str]),
    Decision(&'static [&'static str]),
    Distractor,
}

#[derive(Debug, Clone, Copy)]
pub struct Line {
    pub who: Speaker,
    pub text: &'static str,
    pub plant: Plant,
}

use Plant::*;
use Speaker::*;

const fn l(who: Speaker, text: &'static str, plant: Plant) -> Line {
    Line { who, text, plant }
}

/// The script, in call order. Spread evenly over [`MEETING_SECONDS`].
pub const SCRIPT: &[Line] = &[
    l(Me, "Hey, thanks for jumping on, can you hear me okay?", Filler),
    l(Them, "Yes, loud and clear. Sorry I am a minute late, the last call ran over.", Filler),
    l(Me, "No problem at all. It's going to rain all weekend apparently, so I am glad we are inside.", Distractor),
    l(Them, "Same here. My kids are going to a soccer camp this summer, so the weekends are about to get busy.", Distractor),
    l(Me, "Alright, the agenda today is the regional pilot, the contract renewal, and the onboarding work.", Filler),
    l(Them, "Sounds good. Let me start with where the pilot stands.", Filler),
    l(Them, "The field team finished the site surveys last week and the feedback was mostly positive.", Filler),
    l(Me, "That is great to hear. Were there any blockers from the surveys?", Filler),
    l(Them, "Mostly connectivity in the rural sites, the tablets drop the signal for a few minutes at a time.", Filler),
    l(Me, "Okay, the offline mode should cover that if the sync queue holds up.", Filler),
    l(Them, "Right, and the sync queue held up fine in the warehouse test.", Filler),
    l(Them, "I will send the revised deck to your team by Friday.", Explicit(&["send", "revised", "deck", "Friday"])),
    l(Me, "Perfect, the leadership review is the week after, so Friday works.", Filler),
    l(Me, "On the sensor side, we still do not have the hardware details from your engineers.", Filler),
    l(Them, "I can get you the specs for the new sensor.", Implicit(&["specs", "new", "sensor"])),
    l(Me, "That would unblock the integration work on our end.", Filler),
    l(Them, "We'll see how the traffic is on the way home, it has been rough this week.", Distractor),
    l(Me, "Tell me about it. Okay, so where do we want the pilot to start?", Filler),
    l(Them, "The northern region has the most stores ready and the strongest local managers.", Filler),
    l(Me, "And the southern region still needs new tablets, which pushes it out anyway.", Filler),
    l(Them, "So we agreed to launch the pilot in the northern region first.", Decision(&["launch", "pilot", "northern", "region"])),
    l(Me, "Agreed. That keeps the rollout simple for the first phase.", Filler),
    l(Me, "For the offsite where we present the results, we need a room for the whole group.", Filler),
    l(Me, "I will book the venue for the offsite before the end of the month.", Explicit(&["book", "venue", "offsite"])),
    l(Them, "Great. Let us move to the contract renewal.", Filler),
    l(Them, "The current terms expire at the end of the quarter, and legal wants the new language reviewed early.", Filler),
    l(Them, "I will draft the contract renewal and share it on Tuesday.", Explicit(&["draft", "contract", "renewal", "Tuesday"])),
    l(Me, "Good. Please flag anything that changes the support hours.", Filler),
    l(Them, "Will do. The support hours should stay the same.", Filler),
    l(Me, "I think the coffee machine on our floor is going to break again, it was making that noise all morning.", Distractor),
    l(Them, "Ha, ours finally got replaced last month.", Filler),
    l(Me, "Lucky you. Okay, the pilot also needs invoicing set up before the first stores go live.", Filler),
    l(Me, "Let me take care of the invoice for the pilot.", Implicit(&["invoice", "pilot"])),
    l(Them, "Thank you. Finance asked about that twice already.", Filler),
    l(Them, "Another thing, procurement is still comparing the two hardware vendors.", Filler),
    l(Me, "Do you want us to weigh in on that comparison?", Filler),
    l(Them, "Leave the vendor comparison with me.", Implicit(&["vendor", "comparison"])),
    l(Me, "Sounds good. On onboarding, the new store staff keep missing the same setup steps.", Filler),
    l(Me, "I will update the onboarding checklist tonight.", Explicit(&["update", "onboarding", "checklist"])),
    l(Them, "That would help a lot. The managers keep asking for one page they can print.", Filler),
    l(Them, "You'll love the new cafe downstairs, by the way, the sandwiches are great.", Distractor),
    l(Me, "Next time I am in the office I have to try it.", Filler),
    l(Them, "On the paperwork, the stores still use the paper forms for returns.", Filler),
    l(Me, "Those forms are why the return numbers never match.", Filler),
    l(Them, "Okay, decided, we drop the paper forms and go fully digital.", Decision(&["drop", "paper", "forms", "digital"])),
    l(Me, "Great, that removes a whole reconciliation step.", Filler),
    l(Them, "The game tonight is going to be a close one, are you watching?", Distractor),
    l(Me, "Probably the second half. Okay, let us wrap up with next steps.", Filler),
    l(Them, "The budget is the last open item before the leadership review.", Filler),
    l(Them, "I will email the final budget numbers tomorrow morning.", Explicit(&["email", "final", "budget", "numbers"])),
    l(Me, "Perfect. And the design team wants to walk through the store screens.", Filler),
    l(Me, "I will schedule the follow up call with the design team for next week.", Explicit(&["schedule", "follow", "call", "design", "team"])),
    l(Them, "Also, the support staff will need training before the pilot starts.", Filler),
    l(Me, "I can put together the training slides for the support staff.", Implicit(&["training", "slides", "support", "staff"])),
    l(Them, "That covers it from my side. Thanks for the time today.", Filler),
    l(Me, "Thanks, talk soon.", Filler),
];

/// The segments the notes read, on the call clock: line `i` starts at
/// `i * MEETING_SECONDS / n` and lasts long enough to say it.
pub fn segments() -> Vec<MeetingSegment> {
    let n = SCRIPT.len() as f64;
    let slot = MEETING_SECONDS / n;
    SCRIPT
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let start = i as f64 * slot;
            let words = line.text.split_whitespace().count() as f64;
            let end = (start + (words * 0.38).max(1.5)).min(start + slot - 0.2);
            MeetingSegment {
                id: format!("nt4-{i:03}"),
                meeting_id: "nt4-planted".to_string(),
                start_seconds: start,
                end_seconds: end,
                text: line.text.to_string(),
                confidence: None,
                created_at: chrono::Utc::now(),
                track: line.who.track(),
                cluster_index: None,
            }
        })
        .collect()
}

/// One planted item with where it was said.
#[derive(Debug, Clone)]
pub struct PlantedNote {
    pub owner: &'static str,
    pub keywords: &'static [&'static str],
    pub start_seconds: f64,
    pub explicit: bool,
}

fn planted(pick: impl Fn(Plant) -> Option<(&'static [&'static str], bool)>) -> Vec<PlantedNote> {
    let slot = MEETING_SECONDS / SCRIPT.len() as f64;
    SCRIPT
        .iter()
        .enumerate()
        .filter_map(|(i, line)| {
            pick(line.plant).map(|(keywords, explicit)| PlantedNote {
                owner: line.who.label(),
                keywords,
                start_seconds: i as f64 * slot,
                explicit,
            })
        })
        .collect()
}

/// Every commitment, explicit and implicit.
pub fn commitments() -> Vec<PlantedNote> {
    planted(|p| match p {
        Explicit(k) => Some((k, true)),
        Implicit(k) => Some((k, false)),
        _ => None,
    })
}

pub fn decisions() -> Vec<PlantedNote> {
    planted(|p| match p {
        Decision(k) => Some((k, false)),
        _ => None,
    })
}

/// The distractor lines' start times.
pub fn distractor_starts() -> Vec<f64> {
    let slot = MEETING_SECONDS / SCRIPT.len() as f64;
    SCRIPT
        .iter()
        .enumerate()
        .filter(|(_, l)| l.plant == Distractor)
        .map(|(i, _)| i as f64 * slot)
        .collect()
}

/// The transcript as one text, for the verbatim ratio.
pub fn transcript_text() -> String {
    SCRIPT.iter().map(|l| l.text).collect::<Vec<_>>().join("\n")
}
