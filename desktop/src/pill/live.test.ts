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
  advanceLive, createLiveState, frameIntervalMs, liveTierForWords, resetLive,
  speechThreshold, DEFAULT_LIVE, IDLE_FRAME_MS, LIVE_TIERS, MAX_CHATTER_GAP,
  type ChatTone, type LiveFrame, type LivePhase,
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
