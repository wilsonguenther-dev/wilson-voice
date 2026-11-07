/**
 * live.reactions.test — yap-1106-live-reactions-during-take.
 *
 * DEFECT: Yappy's live lines landed AFTER you talked (a ~1 s rotation of
 * workLine chatter while the take decoded) instead of WHILE you talked, and on
 * a fan or a hot mic the live speaking-time estimate ran 20-25% over the VAD.
 *
 * These are REPLAY tests: a 30 s hands-free take is replayed, frame by frame,
 * against four level traces — a quiet room, a fan, music under the voice and a
 * hot (over-gained) mic. The traces are deterministic re-creations of those
 * rooms (seeded, so every run replays the same take): phrases of 1.5-4 s at
 * ~3.8 syllables/s with 0.35-1.45 s pauses, over the room's own noise. The VAD
 * side is `record.rs::voiced_frames` re-stated over the same trace — 20 ms
 * frames, 10th-percentile floor, the same gate rule and the same 0.30 s bridge —
 * so "within 20 percent" is measured against the app's own speech-time rule.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync } from "node:fs";
import {
  AFTER_TAKE_MAX_LINES, ESTIMATE_TOLERANCE, GLANCE_AFTER_SEC, NEW_USER_TAKES,
  advanceLive, afterTakeLineAllowed, armGlance, createLiveState, estimateDrift, estimateWithinTolerance,
  glanceLine, isNewUser, liveLinesLog, parseTakesSeen, resetLive,
  type ChatTone,
} from "./live";
import { duringLines, type VoiceId } from "./voices";

const readSibling = (rel: string): string =>
  readFileSync(new URL(rel, import.meta.url), "utf8") as string;

/** The pill redraws on rAF at 60 Hz. */
const FRAME = 1 / 60;
/** A hands-free take, press to release. */
const TAKE_SEC = 30;

/** mulberry32 — a tiny seeded PRNG so a trace is the same take every run. */
function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

interface Room { seed: number; noise: number; jitter: number; speech: number; music: number }
type RoomName = "quiet room" | "fan" | "music" | "hot mic";
const ROOMS: Record<RoomName, Room> = {
  "quiet room": { seed: 11, noise: 0.004, jitter: 0.002, speech: 0.05, music: 0 },
  fan: { seed: 23, noise: 0.03, jitter: 0.006, speech: 0.12, music: 0 },
  music: { seed: 37, noise: 0.01, jitter: 0.004, speech: 0.18, music: 0.05 },
  "hot mic": { seed: 51, noise: 0.12, jitter: 0.03, speech: 0.9, music: 0 },
};
const ROOM_NAMES = Object.keys(ROOMS) as RoomName[];

/** A level trace at the HUD's 50 Hz: room noise (+ music) with phrases of speech on top. */
interface Trace { level: Float64Array; fps: number }
function trace(room: Room, seconds = TAKE_SEC, fps = 50): Trace {
  const r = seeded(room.seed);
  const n = Math.round(seconds * fps);
  const level = new Float64Array(n);
  const plan: Array<[number, number]> = [];
  let t = 0.7;                                   // press, then start talking
  while (t < seconds - 0.6) {
    const len = 1.5 + r() * 2.5;
    const end = Math.min(t + len, seconds - 0.6);
    plan.push([t, end]);
    t = end + 0.35 + r() * 1.1;
  }
  const phase = r() * 6.28;
  for (let i = 0; i < n; i++) {
    const tt = i / fps;
    let sp = 0;
    for (const [a, b] of plan) {
      if (tt >= a && tt < b) {
        const env = Math.max(0, Math.sin((tt - a) * Math.PI * 2 * 3.8 + phase));
        sp = room.speech * (0.25 + 0.75 * env) * (0.7 + 0.3 * r());
        break;
      }
    }
    const noise = Math.max(0, room.noise + (r() * 2 - 1) * room.jitter + room.noise * 0.15 * Math.sin(tt * 0.9));
    const mus = room.music ? room.music * (0.5 + 0.5 * Math.max(0, Math.sin(tt * Math.PI * 2 * 2))) * (0.8 + 0.4 * r()) : 0;
    level[i] = Math.min(1, Math.sqrt(sp * sp + noise * noise + mus * mus));
  }
  return { level, fps };
}

/** `record.rs::voiced_frames` + `voiced_seconds`, over a level trace. */
function vadSeconds(tr: Trace): number {
  const rms = Array.from(tr.level);
  const sorted = [...rms].sort((a, b) => a - b);
  const floor = sorted[Math.floor(sorted.length * 0.1)];
  const peak = sorted[sorted.length - 1];
  const thresh = floor > peak * 0.4 ? Math.max(peak * 0.3, 1e-4) : Math.max(floor * 3, peak * 0.06, 1e-4);
  const mask = rms.map((v) => v > thresh);
  const bridged = [...mask];
  const maxGap = Math.round(0.3 * tr.fps);
  let last = -1;
  for (let i = 0; i < mask.length; i++) {
    if (!mask[i]) continue;
    if (last >= 0 && i - last <= maxGap + 1) for (let k = last + 1; k < i; k++) bridged[k] = true;
    last = i;
  }
  return bridged.filter(Boolean).length / tr.fps;
}

interface Replay { lines: Array<{ t: number; line: string }>; glances: number[]; voicedSec: number; counted: number; glanceVoiced: number[] }
/** Replay a take against the live machine exactly as YappyPill drives it. */
function replay(tr: Trace, opts: { tone?: ChatTone; voice?: VoiceId; newUser?: boolean } = {}): Replay {
  const s = createLiveState();
  if (opts.newUser) armGlance(s);
  const lines: Replay["lines"] = [];
  const glances: number[] = [];
  const glanceVoiced: number[] = [];
  const seconds = tr.level.length / tr.fps;
  let t = 0;
  while (t < seconds) {
    t += FRAME;
    const lv = tr.level[Math.min(tr.level.length - 1, Math.floor(t * tr.fps))];
    const f = advanceLive(s, FRAME, lv, opts.tone ?? "friendly", undefined, opts.voice);
    if (f.say) lines.push({ t, line: f.say });
    if (f.glance) { glances.push(t); glanceVoiced.push(s.voicedT); }
  }
  return { lines, glances, voicedSec: s.voicedT, counted: s.lines, glanceVoiced };
}

describe("replay: live lines fire WHILE you talk", () => {
  for (const name of ROOM_NAMES) {
    it(`replay_${name.replace(" ", "_")}_30s_hands_free_take_says_3_live_lines_before_release`, () => {
      const tr = trace(ROOMS[name]);
      for (const opts of [{ tone: "friendly" as ChatTone }, { tone: "rude" as ChatTone }, { voice: "bestfriend" as VoiceId }, { voice: "professional" as VoiceId }]) {
        const r = replay(tr, opts);
        expect(r.lines.length).toBeGreaterThanOrEqual(3);
        // every one of them landed before release, and the first well inside the take
        for (const l of r.lines) expect(l.t).toBeLessThanOrEqual(TAKE_SEC);
        expect(r.lines[0].t).toBeLessThan(10);
        // the tally the telemetry reports is the lines actually said
        expect(r.counted).toBe(r.lines.length);
      }
    });
  }
});

describe("replay: the speaking-time estimate tracks the VAD", () => {
  for (const name of ROOM_NAMES) {
    it(`estimate_within_20pct_of_vad_${name.replace(" ", "_")}`, () => {
      const tr = trace(ROOMS[name]);
      const vad = vadSeconds(tr);
      const { voicedSec } = replay(tr);
      expect(vad).toBeGreaterThan(10);                 // the trace really is a talked take
      const drift = estimateDrift(voicedSec, vad);
      expect(drift).not.toBeNull();
      expect(Math.abs(drift as number)).toBeLessThanOrEqual(ESTIMATE_TOLERANCE);
      expect(estimateWithinTolerance(voicedSec, vad)).toBe(true);
    });
  }

  it("a steady fan with nobody talking is not counted as speech (floor seeded from the room)", () => {
    const s = createLiveState();
    for (let i = 0; i < 60 * 10; i++) advanceLive(s, FRAME, 0.03, "friendly");
    expect(s.voicedT).toBeLessThan(0.5);
  });

  it("a phrase's trailing hold is not banked — only a gap that speech resumes inside is", () => {
    const s = createLiveState();
    advanceLive(s, FRAME, 0.004, "friendly");                         // room tone seeds the floor
    for (let i = 0; i < 60; i++) advanceLive(s, FRAME, 0.05, "friendly");  // 1 s of speech
    const spoken = s.voicedT;
    for (let i = 0; i < 120; i++) advanceLive(s, FRAME, 0.004, "friendly"); // 2 s of silence
    expect(s.voicedT).toBeCloseTo(spoken, 5);
    expect(s.pending).toBe(0);
  });
});

describe("new users: one listening glance after 1.5 s of speech", () => {
  it("new_user_gets_one_glance_after_1_5s_of_speech", () => {
    for (const name of ROOM_NAMES) {
      const r = replay(trace(ROOMS[name]), { newUser: true });
      expect(r.glances).toHaveLength(1);
      expect(r.glanceVoiced[0]).toBeGreaterThanOrEqual(GLANCE_AFTER_SEC);
      expect(r.glanceVoiced[0]).toBeLessThan(GLANCE_AFTER_SEC + 0.5);
    }
  });

  it("returning users get no glance", () => {
    const r = replay(trace(ROOMS["quiet room"]));
    expect(r.glances).toHaveLength(0);
  });

  it("the glance says a listening line in the user's voice, and only when nothing else is said that frame", () => {
    expect(glanceLine("friendly")).toBe("i'm listening!");
    expect(glanceLine("friendly", "bestfriend")).toBe(duringLines("bestfriend", 0)[0]);
    const r = replay(trace(ROOMS.fan), { newUser: true, voice: "bestfriend" });
    const atGlance = r.lines.find((l) => Math.abs(l.t - r.glances[0]) < 1e-9);
    expect(atGlance?.line).toBe(duringLines("bestfriend", 0)[0]);
  });

  it("a new take resets the glance; arming after 1.5 s is a no-op", () => {
    const s = createLiveState();
    armGlance(s);
    expect(s.glance).toBe(0);
    resetLive(s);
    expect(s.glance).toBe(-1);
    s.voicedT = GLANCE_AFTER_SEC + 1;
    armGlance(s);
    expect(s.glance).toBe(-1);
  });

  it("a user is new for the first NEW_USER_TAKES takes", () => {
    expect(isNewUser(0)).toBe(true);
    expect(isNewUser(NEW_USER_TAKES - 1)).toBe(true);
    expect(isNewUser(NEW_USER_TAKES)).toBe(false);
    expect(parseTakesSeen(null)).toBe(0);
    expect(parseTakesSeen("garbage")).toBe(0);
    expect(parseTakesSeen("7")).toBe(7);
    expect(parseTakesSeen(3.9)).toBe(3);
    expect(parseTakesSeen(-4)).toBe(0);
  });
});

describe("after release: workLine chatter is capped at 2 lines", () => {
  it("after_take_chatter_capped_at_2_lines", () => {
    expect(AFTER_TAKE_MAX_LINES).toBe(2);
    expect(afterTakeLineAllowed(0)).toBe(true);
    expect(afterTakeLineAllowed(1)).toBe(true);
    expect(afterTakeLineAllowed(2)).toBe(false);
    expect(afterTakeLineAllowed(9)).toBe(false);
  });

  it("YappyPill gates every after-take workLine through the cap", () => {
    const src = readSibling("./YappyPill.tsx");
    // the only workLine call is inside afterLine(), which checks the cap first
    const calls = src.match(/workLine\(chatTone/g) ?? [];
    expect(calls).toHaveLength(1);
    // yap-1106-chatter-setting adds the "Yappy talks" policy gate after the cap
    expect(src).toMatch(/if \(!chatty\(chatTone, words\) \|\| !afterTakeLineAllowed\(afterLines\)( \|\| !allowAfterLine\(policy, afterLines\))?\) return "";/);
    expect(src).toMatch(/phase === "thinking" && chatty\(chatTone, words\) && afterTakeLineAllowed\(afterLines\)/);
    // and every take logs its live.lines tally
    expect(src).toMatch(/console\.info\(liveLinesLog\(/);
  });
});

describe("telemetry: live.lines {during, after, voicedSec, vadSec}", () => {
  it("live_lines_telemetry_line_shape", () => {
    expect(liveLinesLog({ during: 4, after: 2, voicedSec: 21.04, vadSec: 20 }))
      .toBe("live.lines during=4 after=2 voicedSec=21.0 vadSec=20.0 driftPct=5 within20=true");
    expect(liveLinesLog({ during: 3, after: 0, voicedSec: 30, vadSec: 20 }))
      .toBe("live.lines during=3 after=0 voicedSec=30.0 vadSec=20.0 driftPct=50 within20=false");
    expect(liveLinesLog({ during: 1, after: 1, voicedSec: 2.25, vadSec: null }))
      .toBe("live.lines during=1 after=1 voicedSec=2.3 vadSec=na driftPct=na within20=na");
    expect(liveLinesLog({ during: Number.NaN, after: -1, voicedSec: Number.NaN, vadSec: Number.NaN }))
      .toBe("live.lines during=0 after=0 voicedSec=0.0 vadSec=na driftPct=na within20=na");
  });

  it("drift is null without a VAD time to compare with", () => {
    expect(estimateDrift(10, 0)).toBeNull();
    expect(estimateDrift(10, null)).toBeNull();
    expect(estimateDrift(10, undefined)).toBeNull();
    expect(estimateDrift(Number.NaN, 10)).toBeNull();
    expect(estimateDrift(12, 10)).toBeCloseTo(0.2, 9);
    expect(estimateWithinTolerance(10, null)).toBeNull();
  });
});
