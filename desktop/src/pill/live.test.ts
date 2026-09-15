/**
 * live.test — the live-commentary state machine, driven by SYNTHETIC takes.
 *
 * Everything here is deterministic: a level trace is a function of time, frames
 * are a fixed dt, and no line is chosen at random. The bug this file exists to
 * keep dead (YV71): Yappy went quiet ~24s into a take and, on a quiet mic,
 * never said anything at all.
 */
import { describe, expect, it } from "vitest";
import {
  acceptProgress, advanceLive, createLiveState, frameIntervalMs, framePlan, liveTierForWords,
  phaseVisual, progressFraction, progressNumeral, reduceGatePhase, resetLive,
  speechThreshold, transcribeLine, transcribeTierForWords, wordsFromVoiced,
  phaseRank, winningPhase,
  errorSentence, fitsSideDockStrip, isLegalTransition, phaseCopy, phaseHoldMs, reduceTakePhase,
  AMBIENT_FRAME_MS, ALL_LIVE_PHASES, CANCELLED_SETTLE_MS, DEFAULT_LIVE, GATED_SETTLE_MS,
  IDLE_FRAME_MS, LIVE_TIERS, MAX_CHATTER_GAP, MIN_REPORTABLE_CHUNKS, PHASE_COPY, PHASE_HOLD_MS,
  PHASE_ID, PHASE_NEXT, PHASE_PRECEDENCE, SHIPPED_CHARACTERS, SIDE_DOCK_TEXT_PX,
  UNBOUNDED_PHASES,
  type ChatTone, type LiveFrame, type LivePhase, type TakeStatus, type TranscribeProgress,
} from "./live";
// Y5-C — the per-character art tables live in the components that draw them
// (Y5-K lifts them into a registry). Importing them here is what makes
// "adding a creature adds a ROW" true: a new face with a missing phase fails
// this file, not a hand-written test per character.
import { CLASSIC_PHASE_ART } from "./ClassicPill";
import { YAPPY_PHASE_ART } from "./YappyPill";

/** The pill redraws on rAF; `audio_level` lands at HUD_FPS underneath it. */
const FRAME = 1 / 60;

/**
 * A realistic speech level: ~3.5 syllables/sec of voiced peaks with the quiet
 * dips between them, plus a breath pause every 8s. `loud` defaults to a level
 * a MacBook internal mic actually produces — BELOW the old absolute 0.08 gate.
 */
const speech = (loud = 0.05, quiet = 0.008) => (t: number): number => {
  if (t % 8 > 7.4) return quiet;                       // breath between sentences
  const s = Math.sin(t * Math.PI * 2 * 3.5);
  return s > 0 ? quiet + (loud - quiet) * s : quiet;
};

interface Take { said: Array<{ t: number; line: string }>; frame: LiveFrame; state: ReturnType<typeof createLiveState> }

/** Run a synthetic take of `seconds` against a level trace. */
function take(seconds: number, level: (t: number) => number, tone: ChatTone = "friendly"): Take {
  const state = createLiveState();
  const said: Array<{ t: number; line: string }> = [];
  let t = 0;
  let frame: LiveFrame = { tier: "quick", prop: "none", estWords: 0, speaking: false, say: null };
  while (t < seconds) {
    t += FRAME;
    frame = advanceLive(state, FRAME, level(t), tone);
    if (frame.say) said.push({ t, line: frame.say });
  }
  return { said, frame, state };
}

/** Largest silent stretch, counting the start of the take as a gap. */
function widestGap(said: Array<{ t: number }>, seconds: number): number {
  let prev = 0, worst = 0;
  for (const s of said) { worst = Math.max(worst, s.t - prev); prev = s.t; }
  return Math.max(worst, seconds - prev);
}

describe("tier escalation", () => {
  it("escalates off quick on a 10s take — a line and a prop, not a blank pill", () => {
    const { frame, said } = take(10, speech());
    expect(LIVE_TIERS.indexOf(frame.tier)).toBeGreaterThanOrEqual(LIVE_TIERS.indexOf("notes"));
    expect(frame.prop).not.toBe("none");
    expect(said.map((s) => s.line)).toContain("ooh, lots to say!");   // notes crossing
  });

  it("reaches the desk tier on a 15s take", () => {
    const { frame, said } = take(15, speech());
    expect(frame.tier).toBe("desk");
    expect(frame.prop).toBe("desk");
    expect(said.map((s) => s.line)).toContain("okay, big one!");      // desk crossing
  });

  it("reaches the essay tier on a 60s take", () => {
    const { frame, said } = take(60, speech());
    expect(frame.tier).toBe("essay");
    expect(said.map((s) => s.line)).toContain("wow, keep going!");
  });

  it("reaches the saga tier on a 300s take and keeps escalating past essay", () => {
    const { frame, said } = take(300, speech());
    expect(frame.tier).toBe("saga");
    expect(said.map((s) => s.line)).toContain("still with you — go!");
    // every rung that has a crossing line was actually spoken, in order
    expect(LIVE_TIERS.indexOf(frame.tier)).toBe(LIVE_TIERS.length - 1);
  });

  it("never moves backwards: tier and voiced time are monotonic", () => {
    const state = createLiveState();
    const level = speech();
    let prevTier = -1, prevVoiced = -1;
    for (let i = 1; i <= 60 * 120; i++) {
      const f = advanceLive(state, FRAME, level(i * FRAME), "friendly");
      expect(LIVE_TIERS.indexOf(f.tier)).toBeGreaterThanOrEqual(prevTier);
      expect(state.voicedT).toBeGreaterThanOrEqual(prevVoiced);
      prevTier = LIVE_TIERS.indexOf(f.tier); prevVoiced = state.voicedT;
    }
  });
});

describe("chatter cadence", () => {
  it("keeps talking for the whole of Wilson's 210s take — never a frozen bubble", () => {
    const seconds = 210;
    const { said } = take(seconds, speech());
    expect(said.length).toBeGreaterThanOrEqual(8);
    expect(widestGap(said, seconds)).toBeLessThanOrEqual(MAX_CHATTER_GAP + 0.5);
  });

  it("advances the line during continuous speech instead of repeating one", () => {
    const { said } = take(210, speech());
    for (let i = 1; i < said.length; i++) expect(said[i].line).not.toBe(said[i - 1].line);
    expect(new Set(said.map((s) => s.line)).size).toBeGreaterThanOrEqual(6);
  });

  it("speaks in the user's tone", () => {
    const rude = take(30, speech(), "rude").said.map((s) => s.line);
    const rose = take(30, speech(), "rose").said.map((s) => s.line);
    expect(rude).toContain("a whole rant…");
    expect(rose).toContain("i'm all ears 🌹");
  });

  it("is deterministic — the same take says the same lines", () => {
    const a = take(120, speech()).said;
    const b = take(120, speech()).said;
    expect(a).toEqual(b);
  });
});

describe("speech detection", () => {
  it("escalates on a QUIET mic — the level the old absolute 0.08 gate ignored", () => {
    const level = speech(0.05);
    let peak = 0;
    for (let i = 0; i < 600; i++) peak = Math.max(peak, level(i * FRAME));
    expect(peak).toBeLessThan(0.08);            // the old gate never opened here
    expect(take(30, level).frame.tier).toBe("essay");
  });

  it("escalates on a LOUD mic too — the threshold is relative, not absolute", () => {
    expect(take(30, speech(0.6, 0.05)).frame.tier).toBe("essay");
  });

  it("counts a sentence as one stretch — between-word dips do not stall it", () => {
    const { state } = take(20, speech());
    expect(state.voicedT).toBeGreaterThan(20 * 0.6);
  });

  it("does not escalate on digital silence", () => {
    const { frame, state } = take(60, () => 0.001);
    expect(frame.tier).toBe("quick");
    expect(state.voicedT).toBeLessThan(1);
  });

  it("stops counting a steady room hiss once the floor adapts to it", () => {
    const { frame, state } = take(60, () => 0.03);
    expect(state.voicedT).toBeLessThan(8);
    expect(["quick", "notes"]).toContain(frame.tier);
  });

  it("a one-off bang does not raise the bar for the rest of the take", () => {
    const voice = speech();
    const { frame } = take(30, (t) => (t > 1 && t < 1.2 ? 0.95 : voice(t)));
    expect(LIVE_TIERS.indexOf(frame.tier)).toBeGreaterThanOrEqual(LIVE_TIERS.indexOf("desk"));
  });

  it("threshold sits between the room tone and the speech level", () => {
    const thr = speechThreshold(0.008, 0.05, DEFAULT_LIVE);
    expect(thr).toBeGreaterThan(0.008);
    expect(thr).toBeLessThan(0.05);
  });
});

describe("take boundaries", () => {
  it("a mid-take status event must not rewind the ladder — only a new take resets", () => {
    const state = createLiveState();
    const level = speech();
    for (let i = 1; i <= 60 * 30; i++) advanceLive(state, FRAME, level(i * FRAME), "friendly");
    const midVoiced = state.voicedT, midSaid = state.said;
    expect(midSaid).toBeGreaterThan(0);
    // the pill's `status` listener now re-affirms the phase WITHOUT resetting:
    // advancing again must carry the ladder forward, not restart it
    const after = advanceLive(state, FRAME, level(30), "friendly");
    expect(state.voicedT).toBeGreaterThanOrEqual(midVoiced);
    expect(state.said).toBeGreaterThanOrEqual(midSaid);
    expect(after.tier).toBe("essay");
    // a genuinely NEW take does start from scratch
    resetLive(state);
    expect(state.voicedT).toBe(0);
    expect(state.said).toBe(-1);
    expect(advanceLive(state, FRAME, 0.001, "friendly").tier).toBe("quick");
  });

  it("word estimate maps to the documented rungs", () => {
    expect(liveTierForWords(0)).toBe("quick");
    expect(liveTierForWords(8)).toBe("notes");
    expect(liveTierForWords(25)).toBe("desk");
    expect(liveTierForWords(60)).toBe("essay");
    expect(liveTierForWords(250)).toBe("saga");
  });
});

describe("frame policy", () => {
  const quiet = { level: 0, busyVisuals: false, reduceMotion: false };

  it("never parks while a take is live", () => {
    for (const phase of ["listening", "thinking", "done"] as LivePhase[]) {
      for (const reduceMotion of [false, true]) {
        const ms = frameIntervalMs(phase, { ...quiet, reduceMotion });
        expect(Number.isFinite(ms)).toBe(true);
      }
    }
  });

  it("draws every frame while listening (full motion)", () => {
    expect(frameIntervalMs("listening", quiet)).toBe(0);
  });

  it("throttles — but still runs — when idle", () => {
    expect(frameIntervalMs("idle", quiet)).toBe(IDLE_FRAME_MS);
    expect(frameIntervalMs("sleepy", quiet)).toBe(IDLE_FRAME_MS);
  });

  it("keeps running while sparkles or a live level are in flight", () => {
    expect(frameIntervalMs("idle", { ...quiet, busyVisuals: true })).toBe(0);
    expect(frameIntervalMs("idle", { ...quiet, level: 0.4 })).toBe(0);
  });

  it("parks only under reduced motion with nothing live", () => {
    expect(frameIntervalMs("idle", { ...quiet, reduceMotion: true })).toBe(Infinity);
    expect(frameIntervalMs("listening", { ...quiet, reduceMotion: true })).toBeLessThan(Infinity);
  });
});

/**
 * YV81 — the energy half of the same policy. A pill that is on screen all day
 * must not hold a 60Hz rAF open for a chick that is standing still.
 */
describe("frame policy — parking (YV81)", () => {
  const quiet = { level: 0, busyVisuals: false, reduceMotion: false };

  it("pill_parks_when_idle_and_settled", () => {
    // Idle + settled: the rAF is parked and only the 10fps ambient tick is left.
    const plan = framePlan("idle", { ...quiet, settled: true });
    expect(plan.mode).toBe("ambient");
    expect(plan.intervalMs).toBe(AMBIENT_FRAME_MS);
    expect(plan.intervalMs).toBeGreaterThanOrEqual(100);      // ≤ 10fps, never 60
    expect(frameIntervalMs("idle", { ...quiet, settled: true })).toBe(Infinity);
    // Sleepy is idle's deeper cousin and parks the same way.
    expect(framePlan("sleepy", { ...quiet, settled: true }).mode).toBe("ambient");

    // Hidden (panel ordered out / page occluded): nothing is scheduled at all.
    expect(framePlan("idle", { ...quiet, settled: true, hidden: true }).mode).toBe("parked");
    expect(framePlan("idle", { ...quiet, hidden: true }).mode).toBe("parked");

    // RECORDING NEVER PARKS — not settled, not hidden, not under reduced
    // motion. The mouth follows the mic and the commentary is on a schedule.
    for (const phase of ["listening", "thinking", "done"] as LivePhase[]) {
      for (const settled of [false, true]) {
        for (const hidden of [false, true]) {
          for (const reduceMotion of [false, true]) {
            const live = framePlan(phase, { ...quiet, settled, hidden, reduceMotion });
            expect(live.mode).toBe("raf");
            expect(Number.isFinite(live.intervalMs)).toBe(true);
          }
        }
      }
    }
  });

  /**
   * YV95 / OS-12 fix (1). The PR claimed "the canvas loop can stay parked
   * through a three-hour meeting" while `framePlan` parked only on
   * `hidden || reduceMotion` — and a meeting deliberately SHOWS the pill, so a
   * settled Yappy fell through to the 10fps ambient tick for the whole session:
   * 108,000 canvas redraws + spring updates + bubble repositions over three
   * hours, ten times the JS wakes the 1 Hz clock costs. This is that claim as a
   * test.
   */
  it("a_meeting_parks_the_canvas_loop", () => {
    const meeting = { ...quiet, settled: true, hidden: false, meetingRecording: true };
    expect(framePlan("idle", meeting).mode).toBe("parked");
    expect(framePlan("idle", meeting).intervalMs).toBe(Infinity);
    expect(frameIntervalMs("idle", meeting)).toBe(Infinity);
    // Sleepy is idle's deeper cousin and parks the same way.
    expect(framePlan("sleepy", meeting).mode).toBe("parked");
    // The exact regression: without the flag this same frame is the ambient
    // tick, i.e. 3h at 100ms = 108,000 redraws.
    expect(framePlan("idle", { ...meeting, meetingRecording: false }).mode).toBe("ambient");
    expect((3 * 3600 * 1000) / AMBIENT_FRAME_MS).toBe(108_000);

    // A meeting does NOT silence a live take — dictation during a meeting still
    // animates at full rate, because the live branch is answered first.
    for (const phase of ["listening", "thinking", "done"] as LivePhase[]) {
      expect(framePlan(phase, meeting).mode).toBe("raf");
    }
    // …nor does it park a scene that has not settled yet: a pill frozen
    // mid-capsule-close would stay frozen ON SCREEN for the whole meeting.
    expect(framePlan("idle", { ...meeting, settled: false }).mode).toBe("raf");
    expect(framePlan("idle", { ...meeting, busyVisuals: true }).mode).toBe("raf");
    // A meeting cannot un-park a hidden pill either.
    expect(framePlan("idle", { ...meeting, hidden: true }).mode).toBe("parked");
  });

  it("keeps animating while the idle scene is still settling", () => {
    // The capsule is still shutting / a sparkle is still in flight: frames
    // keep coming, so nothing freezes mid-transition.
    expect(framePlan("idle", { ...quiet, settled: false }).mode).toBe("raf");
    expect(framePlan("idle", quiet).intervalMs).toBe(IDLE_FRAME_MS);
    expect(framePlan("idle", { ...quiet, settled: true, busyVisuals: true }).mode).toBe("raf");
    expect(framePlan("idle", { ...quiet, settled: true, level: 0.4 }).mode).toBe("raf");
  });
});

/** Every variant of the union PERM-C owns — kept literal so a new one is a type error here too. */
// Y5-C — was a SECOND, hand-typed copy of the union, and it had already
// drifted: Y3-D added `cancelled` to `LivePhase` and not to this list, so that
// phase was silently exempt from "every declared phase has a treatment". The
// canonical list is exported from live.ts now; this alias keeps the existing
// assertions reading the way they did.
const ALL_PHASES: readonly LivePhase[] = ALL_LIVE_PHASES;

// ── PERM-C — the microphone refusal, as a state machine ─────────────────────
// The defect: a denied grant still opened the capture stream (a denied stream
// delivers SILENCE), so the pill painted a normal recording of nothing. The fix
// is a refusal the user can SEE, and the thing that can silently regress is not
// the drawing — it is the ranking. These are the three ways `blocked` used to
// get wiped off the pill before anyone read it.
describe("PERM-C — the permission phase", () => {
  it("blocked outranks listening", () => {
    // A refused press still produces `recording` churn around it. If a take
    // event could overwrite the refusal, it would flash and vanish.
    expect(reduceGatePhase("blocked", { type: "recording", recording: true })).toBe("blocked");
    expect(reduceGatePhase("idle", { type: "recording", recording: true })).toBe("listening");
  });

  it("blocked survives a recording:false event", () => {
    expect(reduceGatePhase("blocked", { type: "recording", recording: false })).toBe("blocked");
    expect(reduceGatePhase("waiting", { type: "recording", recording: false })).toBe("waiting");
    // …while an ordinary take still falls back to idle.
    expect(reduceGatePhase("listening", { type: "recording", recording: false })).toBe("idle");
  });

  it("blocked clears on an authorized status event", () => {
    expect(reduceGatePhase("blocked", { type: "microphone-status", status: "authorized" })).toBe("idle");
    expect(reduceGatePhase("waiting", { type: "microphone-status", status: "authorized" })).toBe("idle");
    // Nothing else clears it: macOS owns the grant, so only TCC's own answer may.
    expect(reduceGatePhase("blocked", { type: "microphone-status", status: "denied" })).toBe("blocked");
    expect(reduceGatePhase("blocked", { type: "microphone-status", status: "restricted" })).toBe("blocked");
    // An authorized answer while nothing is gated must not disturb a live take.
    expect(reduceGatePhase("listening", { type: "microphone-status", status: "authorized" })).toBe("listening");
  });

  it("routes the backend's refusal by status: not_determined waits, everything else blocks", () => {
    expect(reduceGatePhase("idle", { type: "mic_permission_required", status: "not_determined", prompting: true })).toBe("waiting");
    expect(reduceGatePhase("idle", { type: "mic_permission_required", status: "denied" })).toBe("blocked");
    expect(reduceGatePhase("idle", { type: "mic_permission_required", status: "restricted" })).toBe("blocked");
    // An unknown status is never treated as a grant.
    expect(reduceGatePhase("idle", { type: "mic_permission_required", status: "" })).toBe("blocked");
  });

  it("the refusal phases are the only ones a click sends to Permissions", () => {
    const needs = ALL_PHASES.filter((p) => phaseVisual(p).needsPermission);
    expect(needs).toEqual(["blocked", "waiting"]);
  });

  it("every declared phase has a treatment — placeholders included", () => {
    // PERM-C lands the WHOLE union so five later items can render into it
    // without touching the type. A variant with no case here would fall through
    // `phaseVisual` as undefined and paint nothing at all.
    for (const p of ALL_PHASES) {
      const v = phaseVisual(p);
      expect(v.label.length, `${p} has no label`).toBeGreaterThan(0);
      expect(["calm", "live", "busy", "good", "warn"]).toContain(v.tone);
    }
    // Y3-C claimed `transcribing` and Y2-C claimed `gated`, so the placeholder
    // count is two lower than PERM-C landed it at. Each later item that renders
    // its variant drops this by one more; the point of the assertion is that the
    // union is not GROWING.
    // Y5-C renders the LAST five placeholders, so the count is now zero and
    // stays zero: the union is complete and nothing may be declared without a
    // treatment again.
    expect(ALL_PHASES.filter((p) => phaseVisual(p).placeholder)).toEqual([]);
    expect(phaseVisual("transcribing").placeholder, "Y3-C renders it").toBe(false);
    expect(phaseVisual("gated").placeholder, "Y2-C renders it").toBe(false);
    for (const p of ["polishing", "pasting", "error", "model-loading", "empty"] as LivePhase[]) {
      expect(phaseVisual(p).placeholder, `Y5-C renders ${p}`).toBe(false);
    }
  });

  it("a blocked pill never holds a 60Hz rAF open", () => {
    // The refusal is CSS. It must repaint, then let the loop settle — a revoked
    // grant can last for days.
    const plan = framePlan("blocked", { level: 0, busyVisuals: false, reduceMotion: false, settled: true });
    expect(plan.mode).not.toBe("raf");
  });
});

/**
 * Y3-C — progress through a chunked decode.
 *
 * Two dishonesty bugs live here, both named by Wilson. The escalation used to be
 * a TIMER (`wordsFromVoiced`, an estimate off a stopwatch), and after the hold
 * ended there was no progress signal AT ALL — `busy` is a boolean, so a
 * 15-minute take across 12 chunks showed one undifferentiated state for minutes.
 */
describe("transcribe progress (Y3-C)", () => {
  const ev = (chunk: number, of: number, wordsSoFar: number): TranscribeProgress => ({
    chunk, of, wordsSoFar,
  });

  it("transcribing_uses_real_chunk_words_not_the_voiced_estimate", () => {
    // A 10-minute take that was mostly SILENCE: 600s of wall clock, but only
    // ~40s of it voiced. The estimate says ~100 words ("desk"); the decoder came
    // back with 12 real words across 4 chunks ("quick"). They disagree, and the
    // transcribing phase must follow the DECODER.
    const estimate = wordsFromVoiced(40);
    expect(estimate).toBeGreaterThan(60);
    expect(liveTierForWords(estimate)).toBe("essay");

    const p = acceptProgress(null, ev(4, 4, 5))!;
    expect(p).not.toBeNull();
    expect(transcribeTierForWords(p.wordsSoFar)).toBe("quick");
    // …and the line Yappy says comes off the REAL count, not the estimate.
    for (const tone of ["rude", "friendly", "rose"] as ChatTone[]) {
      expect(transcribeLine(tone, p)).toBe(transcribeLine(tone, ev(4, 4, 5)));
      expect(transcribeLine(tone, p)).not.toBe(transcribeLine(tone, ev(4, 4, Math.round(estimate))));
    }

    // The other direction too: a fast talker whose voiced seconds under-read.
    const many = acceptProgress(null, ev(2, 9, 300))!;
    expect(transcribeTierForWords(many.wordsSoFar)).toBe("saga");
    expect(transcribeTierForWords(many.wordsSoFar)).not.toBe(liveTierForWords(wordsFromVoiced(2)));
  });

  it("single_window_take_emits_no_progress_phase", () => {
    // A short take decodes in ONE window. There is no progress to report, and a
    // flicker of 1/1 is worse than silence — so the phase is never entered.
    expect(MIN_REPORTABLE_CHUNKS).toBe(2);
    expect(acceptProgress(null, ev(1, 1, 7))).toBeNull();
    expect(acceptProgress(null, ev(1, 0, 7))).toBeNull();
    // Two chunks IS reportable — the boundary is not off by one.
    expect(acceptProgress(null, ev(1, 2, 7))).not.toBeNull();
  });

  it("progress_is_monotonic_and_never_exceeds_of", () => {
    let p: TranscribeProgress | null = null;
    const seen: number[] = [];
    for (const e of [ev(1, 12, 20), ev(2, 12, 55), ev(3, 12, 91)]) {
      p = acceptProgress(p, e);
      seen.push(progressFraction(p!));
    }
    expect(progressNumeral(p!)).toBe("3/12");
    expect(seen).toEqual([1 / 12, 2 / 12, 3 / 12]);
    // A duplicate, a backwards chunk and a shrinking word count all change nothing.
    expect(acceptProgress(p, ev(3, 12, 91))).toBe(p);
    expect(acceptProgress(p, ev(2, 12, 55))).toBe(p);
    expect(acceptProgress(p, ev(4, 12, 10))).toBe(p);
    // Nothing can fill past 100%: an out-of-range chunk is rejected outright…
    expect(acceptProgress(p, ev(13, 12, 400))).toBe(p);
    // …and the fill is clamped for every accepted value along the way.
    let q: TranscribeProgress | null = null;
    for (let i = 1; i <= 12; i++) {
      q = acceptProgress(q, ev(i, 12, i * 30));
      expect(progressFraction(q!)).toBeLessThanOrEqual(1);
      expect(progressFraction(q!)).toBeGreaterThan(0);
    }
    expect(progressFraction(q!)).toBe(1);
  });

  it("listening_tier_still_uses_the_estimate", () => {
    // The deliberate exception. While LISTENING nothing has been decoded, so the
    // voiced-seconds estimate is the only signal there is and the ladder keeps
    // running off it — a playful state, never a stated number.
    const s = createLiveState();
    let f: LiveFrame | null = null;
    for (let t = 0; t < 30; t += FRAME) f = advanceLive(s, FRAME, 0.05, "friendly");
    expect(f!.estWords).toBeGreaterThan(0);
    expect(f!.tier).toBe(liveTierForWords(wordsFromVoiced(s.voicedT)));
    expect(f!.estWords).toBeCloseTo(wordsFromVoiced(s.voicedT), 6);
  });

  it("transcribing is a first-class phase, no longer a placeholder", () => {
    const v = phaseVisual("transcribing");
    expect(v.placeholder).toBe(false);
    expect(v.tone).toBe("busy");
    // …and it is NOT `thinking`, which is the polish/LLM gap after a transcript.
    expect(phaseVisual("thinking").label).toBe("Transcribing");
    expect(v.needsPermission).toBe(false);
  });
});

/**
 * Y3-D — the pill's cancelled state.
 *
 * A cancel is not a failure, and a pill that paints red at someone who pressed
 * cancel on purpose is wrong. It is also not a resting state: it settles.
 */
describe("Y3-D cancelled phase", () => {
  it("is calm, not a failure, and is a real state rather than a placeholder", () => {
    const v = phaseVisual("cancelled");
    expect(v.tone).toBe("calm");
    expect(v.tone).not.toBe("warn");
    expect(v.needsPermission).toBe(false);
    expect(v.placeholder).toBe(false);
    // The failure phase is what it must NOT look like.
    expect(v.tone).not.toBe(phaseVisual("error").tone);
  });

  it("settles back to idle rather than parking on Cancelled", () => {
    const cancelled = reduceGatePhase("thinking", { type: "take_cancelled" });
    expect(cancelled).toBe("cancelled");
    expect(reduceGatePhase(cancelled, { type: "cancel_settled" })).toBe("idle");
    expect(CANCELLED_SETTLE_MS).toBeGreaterThan(0);
  });

  it("survives the recording:false that arrives right behind the cancel", () => {
    // The backend emits `recording:false` on the cancel path. If that could
    // overwrite the phase, the acknowledgement would flash and vanish — the
    // same bug `blocked` is protected from.
    const cancelled = reduceGatePhase("listening", { type: "take_cancelled" });
    expect(reduceGatePhase(cancelled, { type: "recording", recording: false })).toBe("cancelled");
  });

  it("never clears a permission refusal, and never overwrites a NEW take", () => {
    expect(reduceGatePhase("blocked", { type: "take_cancelled" })).toBe("blocked");
    expect(reduceGatePhase("waiting", { type: "take_cancelled" })).toBe("waiting");
    // A settle timer that fires after the next take started leaves it alone.
    expect(reduceGatePhase("listening", { type: "cancel_settled" })).toBe("listening");
  });
});

/**
 * Y2-C — the refused press, made visible.
 *
 * The defect these keep dead: past the trial, `license_allows_new_dictation`
 * emitted `license_required`, maybe notified (throttled), logged, and returned
 * false — and THE PILL DID NOT MOVE. Holding the hotkey produced nothing a human
 * could see, which is indistinguishable from a broken app.
 */
describe("Y2-C — the license gate on the pill", () => {
  it("gated_holds_then_settles_to_the_ended_chip", () => {
    // A refused press moves the pill. This is the whole item.
    const gated = reduceGatePhase("idle", { type: "license_required" });
    expect(gated).toBe("gated");
    // It is a rendered state now, not a declared placeholder.
    const visual = phaseVisual("gated");
    expect(visual.placeholder).toBe(false);
    expect(visual.tone).toBe("warn");
    // The copy is BORROWED from statusCopy's license_required branch.
    expect(visual.label).toBe("Dictation is paused");
    // The fix is a license, not a macOS grant: never the Permissions route.
    expect(visual.needsPermission).toBe(false);

    // It HOLDS: the `recording:false` churn around a refused press must not
    // clear it, or the explanation flashes and vanishes all over again.
    expect(reduceGatePhase(gated, { type: "recording", recording: false })).toBe("gated");

    // …and then it SETTLES, so the pill is never parked on a refusal. What
    // remains between presses is the persistent `ended` chip (pillLicense),
    // which is a property of the license and not of this phase.
    expect(GATED_SETTLE_MS).toBeGreaterThan(CANCELLED_SETTLE_MS);
    expect(reduceGatePhase(gated, { type: "gate_settled" })).toBe("idle");

    // Leaning on the hotkey is answered EVERY time: re-entry is not throttled.
    const again = reduceGatePhase("idle", { type: "license_required" });
    expect(again).toBe("gated");

    // The settle only ever moves the phase it owns.
    expect(reduceGatePhase("listening", { type: "gate_settled" })).toBe("listening");
  });

  it("blocked_outranks_gated", () => {
    // The microphone reason WINS. Telling someone to buy a license when Yap
    // cannot hear them is the wrong sentence, and it is the one they would act
    // on. Same order the backend enforces in start_recording (PERM-C).
    expect(phaseRank("blocked")).toBeLessThan(phaseRank("gated"));
    expect(phaseRank("waiting")).toBeLessThan(phaseRank("gated"));
    expect(winningPhase("blocked", "gated")).toBe("blocked");
    expect(winningPhase("gated", "blocked")).toBe("blocked");

    // A license refusal cannot overwrite a mic refusal…
    expect(reduceGatePhase("blocked", { type: "license_required" })).toBe("blocked");
    expect(reduceGatePhase("waiting", { type: "license_required" })).toBe("waiting");
    // …but a mic refusal DOES overwrite a license refusal.
    expect(reduceGatePhase("gated", { type: "mic_permission_required", status: "denied" }))
      .toBe("blocked");
    expect(reduceGatePhase("gated", { type: "microphone-status", status: "denied" }))
      .toBe("blocked");

    // The full order the item asserts, end to end.
    expect(PHASE_PRECEDENCE.indexOf("blocked")).toBeLessThan(PHASE_PRECEDENCE.indexOf("gated"));
    const order: LivePhase[] = ["blocked", "gated", "listening", "thinking", "done"];
    for (let i = 1; i < order.length; i++) {
      expect(phaseRank(order[i - 1])).toBeLessThan(phaseRank(order[i]));
    }
  });

  it("gated_never_suppresses_the_done_state_of_a_take_already_in_flight", () => {
    // license.rs's own doc (lib.rs:1100-1108): the gate stops NEW dictation and
    // "never takes back the ones already spoken". A trial that ends while a take
    // is decoding must not eat that take's result.
    expect(reduceGatePhase("listening", { type: "license_required" })).toBe("listening");
    expect(reduceGatePhase("thinking", { type: "license_required" })).toBe("thinking");
    expect(reduceGatePhase("transcribing", { type: "license_required" })).toBe("transcribing");

    // The in-flight take still reaches its own end, un-gated: the refusal never
    // entered the phase at all, so there is nothing to clear.
    const stillLive = reduceGatePhase("listening", { type: "license_required" });
    expect(reduceGatePhase(stillLive, { type: "recording", recording: false })).toBe("idle");

    // And a take that really starts clears a stale refusal rather than being
    // suppressed by it — a gate that is no longer shut must not keep saying so.
    expect(reduceGatePhase("gated", { type: "recording", recording: true })).toBe("listening");

    // The other affordances are untouched: `gated` is not `needsPermission`, is
    // not an error tone, and history/search/export/settings keep working past
    // the trial (KEEP_FOREVER_LINE). The pill must not imply otherwise.
    expect(phaseVisual("gated").needsPermission).toBe(false);
  });
});


/**
 * Y5-C — THE COMPLETE PHASE VOCABULARY.
 *
 * The union has been complete since PERM-C; what was missing is everything
 * that makes a variant a STATE: a duration, a successor, copy in every tone,
 * art in every character, and a place to be drawn on all three docks. Every
 * one of those is a table here, because the failure mode is not a crash — it
 * is a pill that is blank, or frozen, or lying, on someone's screen.
 *
 * The defect in Wilson's words: "fill the dead time after talking stops and
 * before text appears."
 */
describe("Y5-C — the phase vocabulary", () => {
  const TONES: ChatTone[] = ["rude", "friendly", "rose"];
  const st = (o: Partial<TakeStatus>): TakeStatus =>
    ({ recording: false, busy: false, ...o });

  it("the_transition_table_is_total_and_has_no_unreachable_phase", () => {
    // (a) every phase has a successor set, and every successor is a real phase.
    for (const p of ALL_LIVE_PHASES) {
      const next = PHASE_NEXT[p];
      expect(Array.isArray(next), `${p} has no successor set`).toBe(true);
      expect(next.length, `${p} is a dead end`).toBeGreaterThan(0);
      for (const n of next) expect(ALL_LIVE_PHASES, `${p} -> ${n}`).toContain(n);
      expect(new Set(next).size, `${p} lists a successor twice`).toBe(next.length);
      expect(next, `${p} lists itself`).not.toContain(p);
    }
    // (b) no phase is unreachable: walk the graph from `idle` and reach all.
    const seen = new Set<LivePhase>(["idle"]);
    const queue: LivePhase[] = ["idle"];
    while (queue.length) {
      for (const n of PHASE_NEXT[queue.pop()!]) if (!seen.has(n)) { seen.add(n); queue.push(n); }
    }
    expect([...ALL_LIVE_PHASES].filter((p) => !seen.has(p)), "unreachable").toEqual([]);
    // (c) and every phase has a way BACK to rest, or it is a pill you have to
    //     quit the app to clear.
    for (const p of ALL_LIVE_PHASES) {
      const back = new Set<LivePhase>([p]);
      const q: LivePhase[] = [p];
      while (q.length) {
        for (const n of PHASE_NEXT[q.pop()!]) if (!back.has(n)) { back.add(n); q.push(n); }
      }
      expect(back.has("idle"), `${p} never reaches idle again`).toBe(true);
    }
  });

  it("no_path_from_listening_to_done_without_an_intermediate_phase", () => {
    // The gap Wilson named. Releasing the hotkey CANNOT put a check mark on
    // the pill: something is always happening between "talking stopped" and
    // "text appeared", and the pill has to say which thing.
    expect(PHASE_NEXT.listening).not.toContain("done");
    expect(isLegalTransition("listening", "done")).toBe(false);
    // …and the reducer refuses to produce it, which is the part that matters:
    // a table nothing obeys is a comment. `busy` has not been set yet here —
    // the status events race, and this exact race is what used to show a check
    // mark over a take that had not been decoded.
    const afterHold = reduceTakePhase("listening", { type: "status", status: st({}) });
    expect(afterHold).not.toBe("done");
    expect(afterHold).toBe("thinking");
    // Nor by the other road: a transcript cannot arrive while still listening
    // without the intermediate phase the reducer just inserted.
    let ph: LivePhase = "listening";
    const trace: LivePhase[] = [ph];
    for (const ev of [
      { type: "status" as const, status: st({ busy: true, engineLoading: true }) },
      { type: "progress" as const },
      { type: "transcript" as const, words: 42 },
    ]) { ph = reduceTakePhase(ph, ev); trace.push(ph); }
    expect(trace).toEqual(["listening", "model-loading", "transcribing", "done"]);
    // Every step of that real trace is a legal transition.
    for (let i = 1; i < trace.length; i++) {
      expect(isLegalTransition(trace[i - 1], trace[i]), `${trace[i - 1]} -> ${trace[i]}`).toBe(true);
    }
  });

  it("every_phase_has_copy_in_every_tone", () => {
    // The tone presets are rude|friendly|rose and `friendly` is the default
    // (companion_tone, lib.rs:412). A phase with no line in ONE tone is a
    // blank pill for whoever chose that tone.
    for (const p of ALL_LIVE_PHASES) {
      for (const tone of TONES) {
        const line = phaseCopy(p, tone);
        expect(typeof line, `${p}/${tone}`).toBe("string");
        expect(line.trim().length, `${p} has no ${tone} line`).toBeGreaterThan(0);
        expect(line, `${p}/${tone} is a placeholder`).not.toMatch(/^(TODO|TBD|\?+)$/i);
      }
      // Three tones means three DIFFERENT voices, not one string copied.
      expect(new Set(TONES.map((t) => phaseCopy(p, t))).size, `${p} reuses one line`).toBe(3);
    }
    expect(Object.keys(PHASE_COPY).sort()).toEqual([...ALL_LIVE_PHASES].sort());
  });

  it("every_phase_renders_within_the_side_dock_strip", () => {
    // On a left/right dock the capsule hugs the screen edge and its max-width
    // is the hard budget for anything drawn inside it, so a phrase that is
    // comfortable on a bottom dock is the phrase that clips on a side one
    // (`white-space: nowrap`). This is why it is a test and not a look.
    for (const p of ALL_LIVE_PHASES) {
      for (const tone of TONES) {
        const line = phaseCopy(p, tone);
        expect(fitsSideDockStrip(line), `${p}/${tone} overflows the strip: "${line}"`).toBe(true);
      }
    }
    // And the one line that is NOT a preset — a backend error string — is cut
    // to the same budget rather than clipped mid-word.
    const long = "Error: accessibility permission was revoked so the paste receipt never came back from the focused application";
    const cut = errorSentence(long)!;
    expect(fitsSideDockStrip(cut), cut).toBe(true);
    expect(cut.startsWith("accessibility"), "the word `Error:` is chrome, not the reason").toBe(true);
    expect(cut.endsWith("\u2026")).toBe(true);
    // A short one is passed through untouched; nothing is a code.
    expect(errorSentence("Clipboard was empty")).toBe("Clipboard was empty");
    expect(errorSentence(null)).toBe(null);
    expect(errorSentence("   ")).toBe(null);
    // The CSS max-width and the TS budget have to agree or one of them lies.
    expect(SIDE_DOCK_TEXT_PX).toBe(192);
  });

  it("every_shipped_character_has_copy_and_art_for_every_phase", () => {
    // Driven off the list of characters that SHIP, so adding a creature adds a
    // row and not a test file — the fixture matrix Y5-K formalises.
    const ART = { classic: CLASSIC_PHASE_ART, yappy: YAPPY_PHASE_ART } as const;
    expect(Object.keys(ART).sort()).toEqual([...SHIPPED_CHARACTERS].sort());
    for (const character of SHIPPED_CHARACTERS) {
      const table = ART[character];
      expect(Object.keys(table).sort(), `${character} art table`).toEqual([...ALL_LIVE_PHASES].sort());
      for (const p of ALL_LIVE_PHASES) {
        const art = table[p];
        expect(art?.sprite?.length, `${character} has no sprite for ${p}`).toBeGreaterThan(0);
        expect(["still", "breathe", "work", "pulse", "shake"], `${character}/${p} motion`)
          .toContain(art.motion);
        // The COPY is shared — that is the point of putting it in live.ts —
        // so every character inherits a line for every phase in every tone.
        for (const tone of TONES) expect(phaseCopy(p, tone).length).toBeGreaterThan(0);
      }
      // A settled state must not be given a working animation, or the pill
      // looks busy while nothing is happening.
      for (const p of ["done", "empty", "cancelled", "blocked", "gated"] as LivePhase[]) {
        expect(table[p].motion, `${character} animates ${p} like work`).not.toBe("work");
      }
    }
  });

  it("precedence: blocked > gated > error > cancelled > the happy path", () => {
    const at = (p: LivePhase) => PHASE_PRECEDENCE.indexOf(p);
    expect(at("blocked")).toBeLessThan(at("gated"));
    expect(at("gated")).toBeLessThan(at("error"));
    expect(at("error")).toBeLessThan(at("cancelled"));
    expect(at("cancelled")).toBeLessThan(at("listening"));
    for (const happy of ["listening", "model-loading", "transcribing", "polishing", "pasting", "thinking", "done"] as LivePhase[]) {
      expect(at("cancelled"), `cancelled must outrank ${happy}`).toBeLessThan(at(happy));
      expect(winningPhase("blocked", happy)).toBe("blocked");
      expect(winningPhase(happy, "error")).toBe("error");
    }
    // `idle`/`sleepy` are unranked: the absence of a phase never wins a tie.
    expect(phaseRank("idle")).toBeGreaterThanOrEqual(PHASE_PRECEDENCE.length);
    expect(winningPhase("idle", "polishing")).toBe("polishing");
  });

  it("only the phases that wait on the user or the OS may hold forever", () => {
    // Everything Yap is DOING either finishes or has failed, so everything
    // else has a deadline and the table says it.
    expect([...UNBOUNDED_PHASES].sort())
      .toEqual(["blocked", "idle", "listening", "sleepy", "waiting"]);
    for (const p of ALL_LIVE_PHASES) {
      const hold = phaseHoldMs(p);
      if (hold === null) continue;
      expect(hold, `${p} holds for no time at all`).toBeGreaterThan(0);
      expect(hold, `${p} holds so long it reads as stuck`).toBeLessThanOrEqual(30_000);
    }
    // The polish stage must outlive the sidecar it is narrating, or the pill
    // paints a failure over work that is still running (polish.rs:59 = 1200ms).
    expect(phaseHoldMs("polishing")!).toBeGreaterThan(1200);
    expect(phaseHoldMs("cancelled")).toBe(CANCELLED_SETTLE_MS);
    expect(phaseHoldMs("gated")).toBe(GATED_SETTLE_MS);
    expect(PHASE_HOLD_MS.done).toBeLessThan(PHASE_HOLD_MS.error!);
  });

  it("a hold that runs out settles the pill instead of freezing it", () => {
    // A WORKING phase that ran out of time has failed; an acknowledgement that
    // ran out of time has simply been read.
    expect(reduceTakePhase("polishing", { type: "hold_elapsed", phase: "polishing" })).toBe("error");
    expect(reduceTakePhase("done", { type: "hold_elapsed", phase: "done" })).toBe("idle");
    // A STALE timer may never move a phase the pill has already left — the
    // flashing-refusal bug in another costume.
    expect(reduceTakePhase("listening", { type: "hold_elapsed", phase: "done" })).toBe("listening");
    expect(reduceTakePhase("listening", { type: "hold_elapsed", phase: "listening" })).toBe("listening");
  });

  it("the no-speech exit is visible as `empty` rather than a dead hotkey", () => {
    // YV16: the hallucination / no-speech gate refuses to paste garbage and
    // emits NO `transcript` at all (lib.rs:2026), so `busy` simply goes false
    // with no error. That used to be silence — a user pressing a hotkey and
    // getting nothing, with no way to tell it from a crash.
    let ph = reduceTakePhase("listening", { type: "status", status: st({ busy: true }) });
    expect(ph).toBe("thinking");
    ph = reduceTakePhase(ph, { type: "status", status: st({}) });
    expect(ph).toBe("empty");
    // A take that DID produce text goes to `done`, never `empty`.
    expect(reduceTakePhase("thinking", { type: "transcript", words: 7 })).toBe("done");
    // And a failure is a failure, not an absence.
    expect(reduceTakePhase("thinking", { type: "status", status: st({ lastError: "no mic" }) }))
      .toBe("error");
  });

  it("the engine warming up is its own phase, not an indefinite wait", () => {
    // YV80's lazy arm (lib.rs:1156-1158): the first take of a session loads the
    // model while capture is already live. Saying "Transcribing" there is a lie
    // about which multi-second thing is happening.
    const ph = reduceTakePhase("listening", {
      type: "status", status: st({ busy: true, engineLoading: true }),
    });
    expect(ph).toBe("model-loading");
    expect(PHASE_ID[ph]).toBe("model_loading");
    expect(phaseHoldMs(ph)).not.toBe(null);
    // …and it keeps the canvas alive, because something is moving in it.
    expect(framePlan(ph, { level: 0, busyVisuals: false, reduceMotion: false, settled: true }).mode)
      .toBe("raf");
    // while a settled failure does NOT hold a 60Hz loop open for four seconds.
    expect(framePlan("error", { level: 0, busyVisuals: false, reduceMotion: false, settled: true }).mode)
      .not.toBe("raf");
    expect(framePlan("empty", { level: 0, busyVisuals: false, reduceMotion: false, settled: true }).mode)
      .not.toBe("raf");
  });

  it("the canonical phase ids are snake_case, unique, and cover the union", () => {
    // `<html data-phase>` is the CSS hook and the only handle a screenshot has.
    expect(Object.keys(PHASE_ID).sort()).toEqual([...ALL_LIVE_PHASES].sort());
    const ids = ALL_LIVE_PHASES.map((p) => PHASE_ID[p]);
    expect(new Set(ids).size, "two phases share an id").toBe(ids.length);
    for (const id of ids) expect(id, id).toMatch(/^[a-z][a-z_]*$/);
    expect(PHASE_ID["model-loading"]).toBe("model_loading");
  });
});
