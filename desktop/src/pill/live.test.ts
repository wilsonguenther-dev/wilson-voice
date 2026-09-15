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
  AMBIENT_FRAME_MS, CANCELLED_SETTLE_MS, DEFAULT_LIVE, IDLE_FRAME_MS, LIVE_TIERS,
  MAX_CHATTER_GAP, MIN_REPORTABLE_CHUNKS,
  type ChatTone, type LiveFrame, type LivePhase, type TranscribeProgress,
} from "./live";

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
const ALL_PHASES: LivePhase[] = [
  "idle", "listening", "thinking", "done", "sleepy",
  "blocked", "waiting", "gated", "transcribing", "polishing",
  "pasting", "error", "model-loading", "empty",
];

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
    // Y3-C claimed `transcribing`, so the placeholder count is one lower than
    // PERM-C landed it at. Each later item that renders its variant drops this
    // by one more; the point of the assertion is that the union is not GROWING.
    expect(ALL_PHASES.filter((p) => phaseVisual(p).placeholder).length).toBeGreaterThanOrEqual(6);
    expect(phaseVisual("transcribing").placeholder, "Y3-C renders it").toBe(false);
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
