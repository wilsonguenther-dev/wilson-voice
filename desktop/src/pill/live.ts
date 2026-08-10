/**
 * live — the pill's LIVE commentary state machine: what Yappy reacts to WHILE
 * you are still talking (tier escalation, props, and the chatter lines above
 * the capsule).
 *
 * Pure + deterministic. It owns no timers, no events and no canvas: the caller
 * feeds it a frame delta and the live mic level from `audio_level`, and it hands
 * back the tier/prop to draw and the line to say (or `null`). So "does Yappy
 * still react 4 minutes into a take?" is a unit test, not a stopwatch — see
 * live.test.ts.
 *
 * Two things it fixes, both regressions the pill shipped with (YV71):
 *
 * 1. The escalation was a ONE-SHOT ladder — one line per tier crossing, top rung
 *    reached after ~24s of speech, then silence for the rest of the take. A
 *    3-minute dictation spent ~88% of its length frozen on one line. The ladder
 *    now keeps a REPEATING, tier-aware chatter schedule for as long as the take
 *    runs, and gains a `saga` rung so a long take still escalates.
 *
 * 2. Speech was detected as `level > 0.08` on the RAW, pre-AGC HUD mix. That
 *    constant sits mid-speech for a MacBook internal mic (the ASR path is
 *    allowed to boost that same signal up to 64x precisely because it is
 *    quiet), so on a quiet take the gate never opened and NOTHING fired. Speech
 *    is now relative to a tracked noise FLOOR, which is what "louder than this
 *    room" actually means — no absolute constant to be wrong about the mic.
 */

/** The user's companion tone (YV27). Anything unknown reads as friendly. */
export type ChatTone = "rude" | "friendly" | "rose";
export const toChatTone = (t?: string): ChatTone => (t === "rude" || t === "rose" ? t : "friendly");

/** Props the live tiers can put in the scene. */
export type LiveProp = "none" | "pad" | "desk";

/** Live escalation rungs, in order. `saga` keeps a long take escalating. */
export type LiveTier = "quick" | "notes" | "desk" | "essay" | "saga";
export const LIVE_TIERS: readonly LiveTier[] = ["quick", "notes", "desk", "essay", "saga"];
export const LIVE_PROP: Record<LiveTier, LiveProp> = {
  quick: "none", notes: "pad", desk: "desk", essay: "desk", saga: "desk",
};

/** Rough speaking rate → estimate words from seconds of ACTUAL voiced speech. */
export const WORDS_PER_SEC = 2.5;
export const wordsFromVoiced = (voicedSec: number): number => voicedSec * WORDS_PER_SEC;
/** Live tier keyed off ESTIMATED words — ~3s / 10s / 24s / 100s of speech. */
export const liveTierForWords = (estWords: number): LiveTier =>
  estWords < 8 ? "quick" : estWords < 25 ? "notes" : estWords < 60 ? "desk" : estWords < 250 ? "essay" : "saga";

/** Tone-aware line spoken the moment you cross INTO a tier (quick has none). */
const LIVE_LINE: Partial<Record<LiveTier, Record<ChatTone, string>>> = {
  notes: { rude: "ok, noting…", friendly: "ooh, lots to say!", rose: "tell me more 🌹" },
  desk: { rude: "a whole rant…", friendly: "okay, big one!", rose: "i'm all ears 🌹" },
  essay: { rude: "an ESSAY, live?!", friendly: "wow, keep going!", rose: "forever, love 🌹" },
  saga: { rude: "this is a SAGA now…", friendly: "still with you — go!", rose: "i could listen all day 🌹" },
};

/** Tone-aware REPEATING chatter, rotated per tier so a long take keeps talking. */
const LIVE_WORK: Record<LiveTier, Record<ChatTone, string[]>> = {
  quick: {
    rude: ["talk already…", "i'm waiting…"],
    friendly: ["i'm listening!", "go on…"],
    rose: ["i'm here 🌹", "whenever you're ready…"],
  },
  notes: {
    rude: ["mhm, noting…", "go on then…", "keep it moving…"],
    friendly: ["mhm, got that…", "keep going!", "noting it down…"],
    rose: ["every word 🌹", "keep talking to me…", "noting it, love…"],
  },
  desk: {
    rude: ["still typing…", "you done yet?", "my wings hurt…", "filing this rant…"],
    friendly: ["typing it up…", "still with you!", "receptionist mode!", "got it all so far…"],
    rose: ["typing every word 🌹", "still listening…", "so worth writing…", "don't stop, love…"],
  },
  essay: {
    rude: ["chapter two??", "wrist cramp…", "still going?!", "an ESSAY, live…"],
    friendly: ["wow, chapter two!", "still going strong!", "this is a big one!", "keep it coming!"],
    rose: ["your whole story 🌹", "i'm hanging on it…", "keep going, love…", "every word matters…"],
  },
  saga: {
    rude: ["chapter FIVE…", "write a book already…", "my wings gave up…", "am i your only friend?"],
    friendly: ["okay, this is a saga!", "still here, still typing!", "you're on a roll!", "chapter five!"],
    rose: ["forever, love 🌹", "i'd listen all day…", "still every word…", "never stop talking…"],
  },
};

/** Seconds between repeating lines per tier — longer up the ladder so it does
 *  not nag, never long enough for the bubble to read as frozen. */
const CHATTER_GAP: Record<LiveTier, number> = { quick: 11, notes: 8, desk: 9, essay: 10, saga: 11 };
/** The worst case of the above — tests assert no gap ever exceeds it. */
export const MAX_CHATTER_GAP = 11;

export interface LiveTuning {
  /** Floor never reads quieter than this (a truly silent digital mic is 0). */
  floorMin: number;
  /** Per-second rate the floor falls toward a quieter room (fast). */
  floorFall: number;
  /** Per-second rate the floor rises toward a louder room (slow). */
  floorRise: number;
  /** Per-second rate the tracked peak decays back down. */
  peakFall: number;
  /** Speech sits this far up the floor→peak range… */
  speakRatio: number;
  /** …but never less than this above the floor (a steady hiss is not speech). */
  speakMargin: number;
  /** Keep counting voiced time through this long a gap (between words). */
  holdSec: number;
}

export const DEFAULT_LIVE: LiveTuning = {
  floorMin: 0.002,
  floorFall: 4,
  floorRise: 0.4,
  peakFall: 0.25,
  speakRatio: 0.3,
  speakMargin: 0.006,
  holdSec: 0.35,
};

export interface LiveState {
  /** Wall seconds since the take started. */
  wallT: number;
  /** Seconds of ACTUAL speech — silence must not escalate the tier. */
  voicedT: number;
  /** Tracked room tone (falls fast, rises slow). */
  floor: number;
  /** Tracked loud end of the take (rises instantly, decays back down). */
  peak: number;
  /** Remaining hold: >0 means a between-words gap still counts as speech. */
  hold: number;
  /** Highest tier index already reacted to, -1 before the first line. */
  said: number;
  /** Seconds until the next repeating line. */
  chatterT: number;
  /** Monotonic line counter — rotates the pool deterministically. */
  chatterN: number;
}

export function createLiveState(cfg: LiveTuning = DEFAULT_LIVE): LiveState {
  return {
    wallT: 0, voicedT: 0, floor: cfg.floorMin, peak: cfg.floorMin, hold: 0,
    said: -1, chatterT: CHATTER_GAP.quick, chatterN: 0,
  };
}

/** Reset in place — a NEW take, not a mid-take status event. */
export function resetLive(s: LiveState, cfg: LiveTuning = DEFAULT_LIVE): void {
  const fresh = createLiveState(cfg);
  s.wallT = fresh.wallT; s.voicedT = fresh.voicedT; s.floor = fresh.floor; s.peak = fresh.peak;
  s.hold = fresh.hold; s.said = fresh.said; s.chatterT = fresh.chatterT; s.chatterN = fresh.chatterN;
}

export interface LiveFrame {
  tier: LiveTier;
  prop: LiveProp;
  /** Words estimated from voiced seconds (what the tier is keyed off). */
  estWords: number;
  /** Did THIS frame read as speech (after the between-words hold)? */
  speaking: boolean;
  /** A line to put in the bubble, or null to leave it as it is. */
  say: string | null;
}

/**
 * The level a frame has to beat to read as speech: RELATIVE to the take's own
 * room tone and loud end, never an absolute constant — a MacBook internal mic
 * and a headset mic are an order of magnitude apart on this scale.
 */
export function speechThreshold(floor: number, peak: number, cfg: LiveTuning = DEFAULT_LIVE): number {
  return floor + Math.max(cfg.speakMargin, (peak - floor) * cfg.speakRatio);
}

/**
 * Advance one frame of live commentary. `dt` seconds, `level` the raw 0..1 HUD
 * mic level. Mutates `s` (the caller owns one per take) and returns what to
 * render/say. Deterministic: same inputs, same line, every run.
 */
export function advanceLive(
  s: LiveState,
  dt: number,
  level: number,
  tone: ChatTone,
  cfg: LiveTuning = DEFAULT_LIVE,
): LiveFrame {
  s.wallT += dt;

  // ── speech vs room tone ──────────────────────────────────────────────────
  // Floor falls fast / rises slowly, so the between-word dips of a long take
  // hold it at room tone and the loud stretches barely lift it. Peak jumps to
  // any loud frame and decays back, so a one-off bang cannot raise the bar for
  // the rest of the take.
  const rate = level < s.floor ? cfg.floorFall : cfg.floorRise;
  s.floor += (level - s.floor) * Math.min(1, dt * rate);
  if (s.floor < cfg.floorMin) s.floor = cfg.floorMin;
  if (level > s.peak) s.peak = level;
  else s.peak += (Math.max(level, s.floor) - s.peak) * Math.min(1, dt * cfg.peakFall);
  const loud = level > speechThreshold(s.floor, s.peak, cfg);
  // A hold carries the estimate across the gaps between words/syllables, so a
  // normal sentence counts as one stretch of speech instead of a dotted line.
  if (loud) s.hold = cfg.holdSec; else s.hold = Math.max(0, s.hold - dt);
  const speaking = loud || s.hold > 0;
  if (speaking) s.voicedT += dt;

  // ── tier ladder ──────────────────────────────────────────────────────────
  const estWords = wordsFromVoiced(s.voicedT);
  const tier = liveTierForWords(estWords);
  const idx = LIVE_TIERS.indexOf(tier);
  let say: string | null = null;
  if (idx > s.said) {
    // crossed into a new tier — react to it immediately
    s.said = idx;
    const line = LIVE_LINE[tier];
    if (line) { say = line[tone]; s.chatterN = 0; }
    s.chatterT = CHATTER_GAP[tier];
  } else {
    // ── repeating chatter ── the take keeps running, so Yappy keeps talking.
    s.chatterT -= dt;
    if (s.chatterT <= 0) {
      const pool = LIVE_WORK[tier][tone];
      say = pool[s.chatterN % pool.length];
      s.chatterN++;
      s.chatterT = CHATTER_GAP[tier];
    }
  }

  return { tier, prop: LIVE_PROP[tier], estWords, speaking, say };
}

// ── frame policy ───────────────────────────────────────────────────────────
// The always-on pill throttles its redraw while nothing is happening, but it
// must NEVER park while a take is live — the mouth follows the mic and the
// commentary is on a schedule, both of which need frames to advance.
//
// YV81 — the energy pass added the other end of that ladder. A rAF loop that
// only SKIPS its redraws is still a 60Hz wake: the callback runs, the compositor
// stays awake, and the pill is on screen all day. So once a take is over and the
// scene has SETTLED (capsule closed, no sparkles, nothing springing), the rAF is
// parked outright and Yappy's ambient life — breathing, sway, drifting clouds,
// the next blink — moves onto a 10fps timer. Hidden (the panel ordered out, the
// page reporting itself occluded) it parks completely: nothing renders for a
// window nobody can see.

export type LivePhase = "idle" | "listening" | "thinking" | "done" | "sleepy";
/** Redraw budget while idle: ~18fps keeps the breathing perceptible. */
export const IDLE_FRAME_MS = 55;
/** Reduced-motion redraw budget while a take is live: slow, but never parked. */
export const REDUCED_FRAME_MS = 120;
/** Ambient tick once the idle scene has settled: 10fps, off a timer, rAF parked. */
export const AMBIENT_FRAME_MS = 100;

export interface FrameInputs {
  level: number;
  /** Sparkles in flight / a hop mid-air — visuals that must not stutter. */
  busyVisuals: boolean;
  reduceMotion: boolean;
  /**
   * Has the idle scene finished settling — capsule shut, props gone, springs
   * at rest? Only then may the rAF park; what is left to animate is ambient.
   * Absent reads as "still settling", which can only ever keep frames coming.
   */
  settled?: boolean;
  /** Is the pill window hidden/occluded (nothing it draws can be seen)? */
  hidden?: boolean;
}

/** How the loop is being driven right now. */
export type FrameMode =
  /** requestAnimationFrame, at `intervalMs` between redraws (0 = every frame). */
  | "raf"
  /** rAF parked; ambient life redraws off a slow timer at `intervalMs`. */
  | "ambient"
  /** Nothing is scheduled at all — woken by the next phase/visibility change. */
  | "parked";

export interface FramePlan {
  mode: FrameMode;
  /** `Infinity` when parked. */
  intervalMs: number;
}

/**
 * What should be driving the pill's canvas right now.
 *
 * The order of these branches IS the contract: a live take is answered before
 * anything else can park the loop, so no visibility flag, settle test or
 * reduced-motion setting can silence the mouth or stall the commentary
 * (`live.test.ts` — "never parks while a take is live").
 */
export function framePlan(phase: LivePhase, f: FrameInputs): FramePlan {
  const active = phase === "listening" || phase === "thinking" || phase === "done";
  if (active || f.busyVisuals || f.level >= 0.02) {
    return { mode: "raf", intervalMs: f.reduceMotion ? REDUCED_FRAME_MS : 0 };
  }
  // Nothing live. A hidden pill draws nothing; reduced motion asked for one
  // calm static frame and no loop at all.
  if (f.hidden || f.reduceMotion) return { mode: "parked", intervalMs: Infinity };
  // Settled: the transients are done, so drop the rAF and keep only the
  // ambient tick. Still settling → keep animating, throttled.
  if (f.settled) return { mode: "ambient", intervalMs: AMBIENT_FRAME_MS };
  return { mode: "raf", intervalMs: IDLE_FRAME_MS };
}

/**
 * The rAF-only view of [`framePlan`]: milliseconds the animation-frame loop may
 * wait before the next redraw. `0` = every animation frame; `Infinity` = the
 * rAF is parked (whatever, if anything, replaces it — see `framePlan`).
 */
export function frameIntervalMs(phase: LivePhase, f: FrameInputs): number {
  const plan = framePlan(phase, f);
  return plan.mode === "raf" ? plan.intervalMs : Infinity;
}
