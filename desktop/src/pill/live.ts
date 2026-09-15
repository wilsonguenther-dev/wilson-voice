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

// Y2-C — the refused-press copy is BORROWED from the Settings → License card
// through `./license`, which reads it out of `statusCopy`. `./license` imports
// only `../license/status` (both pure, neither imports this file), so this adds
// a leaf dependency and no cycle.
import { GATED_HEADLINE } from "./license";

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
/**
 * An ESTIMATE, and only ever an estimate — seconds of voiced audio multiplied by
 * an average speaking rate. On a ten-minute take its error is enormous.
 *
 * Y3-C: this is legal for the LISTENING phase ONLY, where nothing better exists
 * — no text has been decoded yet, so a guess is the only thing there is, and
 * "listening" states are playful rather than numeric. The TRANSCRIBING phase
 * MUST NOT use it: by then real chunks have returned real text with a real word
 * count (`TranscribeProgress.wordsSoFar`), and a numeric state that is wrong is
 * the dishonesty this item removes. See `transcribeTierForWords`.
 */
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

// ── Y3-C: real progress through a chunked decode ───────────────────────────
//
// Before this, the only mid-take number the pill had was `wordsFromVoiced` — an
// estimate off a stopwatch — and after the hold ended it had nothing at all:
// `busy` is a boolean, so a 15-minute take decoding across 12 chunks showed the
// same undifferentiated state for minutes.
//
// The backend now emits `transcribe_progress` once per COMPLETED chunk, with
// the chunk index, the total and the REAL cumulative word count of the text
// decoded so far. No timer, no interpolation, no smoothing: every value the
// pill draws was measured, and the only moments it moves are the moments a
// chunk actually finished.

/** The `transcribe_progress` payload — one per COMPLETED chunk, never a guess. */
export interface TranscribeProgress {
  /** 1-based index of the chunk that just completed. */
  chunk: number;
  /** Total chunks this take was split into. Never reported below 2. */
  of: number;
  /** REAL words in the text decoded so far. Cumulative and monotonic. */
  wordsSoFar: number;
}

/** Progress reported for a take that was never chunked is not progress. */
export const MIN_REPORTABLE_CHUNKS = 2;

/**
 * Fold one `transcribe_progress` event into the phase's state.
 *
 * Everything this rejects is a lie the pill would otherwise have drawn:
 *  - `of < 2` — a single-window take. There is nothing to report; a 1/1 flicker
 *    is worse than silence, so the phase is never entered at all.
 *  - `chunk > of` or `chunk < 1` — a fill past 100%, or before the start.
 *  - a chunk index that did not ADVANCE — progress bars do not go backwards,
 *    and a duplicate event must not re-animate one.
 *  - a word count that went DOWN — words already decoded cannot un-decode.
 *
 * Returns the state to draw: the accepted event, or `prev` untouched. Never
 * interpolates between events — a smooth bar that is lying is the defect being
 * removed, so the fill moves in exactly the steps the decoder actually took.
 */
export function acceptProgress(
  prev: TranscribeProgress | null,
  next: TranscribeProgress,
): TranscribeProgress | null {
  const { chunk, of, wordsSoFar } = next;
  if (!Number.isFinite(chunk) || !Number.isFinite(of) || !Number.isFinite(wordsSoFar)) return prev;
  if (of < MIN_REPORTABLE_CHUNKS) return prev;
  if (chunk < 1 || chunk > of) return prev;
  if (wordsSoFar < 0) return prev;
  if (prev) {
    if (of !== prev.of) return prev; // a different take's geometry — ignore it
    if (chunk <= prev.chunk) return prev;
    if (wordsSoFar < prev.wordsSoFar) return prev;
  }
  return { chunk, of, wordsSoFar };
}

/** Is the pill in the transcribing phase right now? Only real progress opens it. */
export const isTranscribing = (p: TranscribeProgress | null): p is TranscribeProgress => p !== null;

/**
 * The determinate fill, 0..1 — `chunk / of` and nothing else. Deliberately NOT
 * eased, interpolated or time-blended: each step is one chunk that finished.
 */
export const progressFraction = (p: TranscribeProgress): number =>
  Math.max(0, Math.min(1, p.chunk / p.of));

/** The numeral both pills draw (Departure Mono), e.g. "3/12". */
export const progressNumeral = (p: TranscribeProgress): string => `${p.chunk}/${p.of}`;

/** The capsule's accessible name while transcribing — states progress, not words. */
export const progressLabel = (p: TranscribeProgress): string =>
  `Transcribing — chunk ${p.chunk} of ${p.of}`;

/**
 * The live tier during TRANSCRIBING, keyed off REAL decoded words.
 *
 * Same rungs as `liveTierForWords`, deliberately: the ladder is the character,
 * what changes is that the number feeding it was measured instead of guessed.
 */
export const transcribeTierForWords = (realWords: number): LiveTier =>
  liveTierForWords(realWords);

/** Tone-aware commentary while the decode runs, escalating on REAL words. */
const WORK_LINE: Record<LiveTier, Record<ChatTone, string>> = {
  quick: { rude: "typing it…", friendly: "typing it up!", rose: "writing it down 🌹" },
  notes: { rude: "still typing…", friendly: "got a page of this!", rose: "every word, love 🌹" },
  desk: { rude: "this is a lot…", friendly: "big one — nearly there!", rose: "so much to keep 🌹" },
  essay: { rude: "an ESSAY to type…", friendly: "wow, what an essay!", rose: "your whole story 🌹" },
  saga: { rude: "a SAGA to type…", friendly: "a whole saga — hang on!", rose: "i'd type all day 🌹" },
};

/**
 * What Yappy says while the decode runs.
 *
 * The escalation is driven by `p.wordsSoFar` — real text from real chunks —
 * NOT by `wordsFromVoiced`. That is the whole point of the phase: a persona
 * promised off a stopwatch is unearned, and on a long take the stopwatch is
 * wrong by a wide margin.
 */
export const transcribeLine = (tone: ChatTone, p: TranscribeProgress): string =>
  WORK_LINE[transcribeTierForWords(p.wordsSoFar)][tone];

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
//
// YV95 / OS-12 — that ladder assumed "visible" means "a take is happening or
// just did", i.e. seconds. A meeting pins the pill on screen for up to three
// hours with `hidden` false the whole time, so the bottom rung it lands on is
// the 10fps ambient tick: 108,000 canvas redraws over three hours, which is ten
// times the JS wakes the 1 Hz elapsed-clock emit costs. A settled pill during a
// meeting has nothing to animate — the clock is DOM text, the recording pulse is
// a CSS compositor animation — so `meetingRecording` parks the canvas the way
// `hidden` does. This is the fix OS-12 (1) asks for; without it "the canvas loop
// can stay parked through a three-hour meeting" is false.

/**
 * Every phase the pill can be in — the WHOLE union, landed in one commit on
 * purpose (PERM-C).
 *
 * Six items across two parallel lanes need a variant of this type, each branched
 * off main independently. A builder that finds its predecessor's PR unlanded
 * invents its own variant, and the merge conflicts land on the one file that
 * carries the pill's defects. So the earliest of the six declares the complete
 * union up front: variants nobody renders yet are PLACEHOLDERS with a named
 * owner, and later items add rendering and copy WITHOUT editing this union.
 *
 * If you are here to add a phase: don't. Claim one of the placeholders below and
 * give it a real `phaseVisual` treatment instead.
 */
export type LivePhase =
  // ── shipped, rendered by both pills ──────────────────────────────────────
  | "idle"
  | "listening"
  | "thinking"
  | "done"
  | "sleepy"
  // ── PERM-C (this item): the microphone gate's two refusals ───────────────
  /** Denied or Restricted: macOS is blocking the mic and will not re-prompt. */
  | "blocked" // OWNED BY PERM-C
  /** NotDetermined: the system dialog is up; the press did not wait for it. */
  | "waiting" // OWNED BY PERM-C
  // ── placeholders: declared here, rendered by the item that owns them ─────
  /** Trial over / no license — a refused take that is NOT a permission fault. */
  | "gated" // OWNED BY Y2-C
  /**
   * Audio captured, ASR running over a take that was split into MORE THAN ONE
   * chunk — so the pill can state real, measured progress (Y3-C). Distinct from
   * `thinking`, which is the polish/LLM gap AFTER a transcript exists. A take
   * decoded in a single window never enters this phase: there is no progress to
   * report and a flicker of 1/1 is worse than silence.
   */
  | "transcribing" // OWNED BY Y3-C
  /** Transcript in hand, the polish sidecar is rewriting it. */
  | "polishing" // OWNED BY Y5-C
  /** Text going into the focused app. */
  | "pasting" // OWNED BY Y7-D
  /** The take failed and the user has to know. */
  | "error" // OWNED BY Y8-D
  /** The speech engine is loading before it can decode. */
  | "model-loading" // OWNED BY Y3-C
  /** The take produced no words at all. */
  | "empty" // OWNED BY Y8-D
  // ── Y3-D (this item) ─────────────────────────────────────────────────────
  /**
   * The user cancelled the take. Deliberately `calm`, not `warn`: nothing went
   * wrong, so the pill must not paint a failure at someone who pressed cancel
   * on purpose. It SETTLES — see [`CANCELLED_SETTLE_MS`] and the
   * `cancel_settled` event — rather than being a resting state, because a pill
   * parked on "Cancelled" forever is just a stuck pill.
   */
  | "cancelled"; // OWNED BY Y3-D

/** How a phase is drawn, and what a click on it means. */
export interface PhaseVisual {
  /**
   * The treatment bucket both pills key off. `warn` is the muted, struck-through
   * capsule; `live`/`busy`/`good` are the existing take states; `calm` is at rest.
   */
  tone: "calm" | "live" | "busy" | "good" | "warn";
  /** Short human label — also the accessible name of the capsule. */
  label: string;
  /** Clicking this phase should open Permissions, not start/stop a take. */
  needsPermission: boolean;
  /** Declared but not yet designed: its owning item still has to render it. */
  placeholder: boolean;
}

/**
 * The single place a phase becomes a treatment. Both pills read it, so a phase
 * can never be drawn two different ways, and a variant added to the union
 * without a case here is a compile error rather than a silently invisible state.
 */
export function phaseVisual(phase: LivePhase): PhaseVisual {
  switch (phase) {
    case "idle":
      return { tone: "calm", label: "Start dictation", needsPermission: false, placeholder: false };
    case "listening":
      return { tone: "live", label: "Stop dictation", needsPermission: false, placeholder: false };
    case "thinking":
      return { tone: "busy", label: "Transcribing", needsPermission: false, placeholder: false };
    case "done":
      return { tone: "good", label: "Done", needsPermission: false, placeholder: false };
    case "sleepy":
      return { tone: "calm", label: "Start dictation", needsPermission: false, placeholder: false };
    case "blocked":
      return {
        tone: "warn",
        label: "Microphone blocked — open Permissions",
        needsPermission: true,
        placeholder: false,
      };
    case "waiting":
      return {
        tone: "warn",
        label: "Waiting for microphone permission",
        needsPermission: true,
        placeholder: false,
      };
    case "gated":
      // Y2-C — no longer a placeholder. `warn` is the same muted, drained
      // capsule `blocked` wears, which is right: both are a press that produced
      // no take. `needsPermission` stays FALSE — the fix is a license, not a
      // macOS grant, and sending someone to the Permissions pane over a trial
      // that ended is the wrong sentence twice over.
      return { tone: "warn", label: GATED_HEADLINE, needsPermission: false, placeholder: false };
    case "transcribing":
      return { tone: "busy", label: "Transcribing", needsPermission: false, placeholder: false };
    case "polishing":
      // Y5-C — `busy` like `transcribing`, but it is NOT the same state and the
      // copy says so: this is the LLM rewriting a transcript that already
      // exists, and it has its own bounded deadline (see `PHASE_HOLD_MS`).
      return { tone: "busy", label: "Polishing", needsPermission: false, placeholder: false };
    case "pasting":
      // Y5-C — its own phase because the paste has its own failure: a receipt
      // that never comes back is an ACCESSIBILITY problem (YV39), not a
      // transcription one, and `error`'s sentence has to be able to say which.
      return { tone: "busy", label: "Pasting", needsPermission: false, placeholder: false };
    case "error":
      // Y5-C — the generic terminal state. The LABEL is the accessible name;
      // the one-line REASON comes off the take's `last_error` through
      // [`errorSentence`], never a code and never this placeholder sentence.
      return { tone: "warn", label: "Something went wrong", needsPermission: false, placeholder: false };
    case "model-loading":
      // Y5-C — YV80's lazy arm: capture is already live while the engine comes
      // off disk, so the pill says it is WARMING instead of looking stuck.
      return { tone: "busy", label: "Loading speech engine", needsPermission: false, placeholder: false };
    case "empty":
      // Y5-C — YV16's payoff. `calm`, not `warn`: refusing to paste garbage is
      // Yap working correctly, and today it says nothing at all, so the user
      // experiences a dead hotkey. This state IS the visible part of that gate.
      return { tone: "calm", label: "Nothing to type", needsPermission: false, placeholder: false };
    case "cancelled":
      return { tone: "calm", label: "Cancelled", needsPermission: false, placeholder: false };
  }
}

/** The events that can move the pill in or out of a permission phase. */
export type GateEvent =
  /** The backend refused a press: `mic_permission_required`. */
  | { type: "mic_permission_required"; status: string; prompting?: boolean }
  /** TCC answered: the `microphone-status` event. */
  | { type: "microphone-status"; status: string }
  /** A take started or stopped. */
  | { type: "recording"; recording: boolean }
  /** Y3-D — the backend's `take_cancelled`: the user stopped this take. */
  | { type: "take_cancelled" }
  /** Y3-D — [`CANCELLED_SETTLE_MS`] has elapsed; the pill returns to rest. */
  | { type: "cancel_settled" }
  /**
   * Y2-C — the backend refused a NEW take for the license: `license_required`
   * (lib.rs:1111-1126). Carries nothing, because the pill re-decides nothing:
   * the payload's own `days_left` is already handled by `pillLicense`.
   */
  | { type: "license_required" }
  /** Y2-C — [`GATED_SETTLE_MS`] has elapsed; the refusal stops shouting. */
  | { type: "gate_settled" };

/**
 * Y3-D — how long the pill holds "Cancelled" before settling to idle.
 *
 * Long enough to be read as an acknowledgement (the press produced a visible
 * effect, which is the whole reason the state exists) and short enough that it
 * never reads as a state the pill is stuck in. Exported so the settle is a unit
 * test rather than a stopwatch held against a running app.
 */
export const CANCELLED_SETTLE_MS = 1400;

/**
 * Y2-C — how long a refused press holds the `gated` phase before settling.
 *
 * Longer than [`CANCELLED_SETTLE_MS`] on purpose: a cancel is an acknowledgement
 * of something the USER just did, while this is news the user did not ask for
 * and has to READ ("Dictation is paused"). Still bounded, because a pill parked
 * on a refusal forever is indistinguishable from a stuck pill — and because the
 * persistent `ended` chip is what carries the state between presses. Leaning on
 * the hotkey re-enters the phase every time; it never becomes permanent noise.
 *
 * This is also what makes `should_announce_gate`'s throttle SAFE to keep as it
 * is: the SYSTEM notification stays rate-limited, and this cheap in-place signal
 * answers every single press.
 */
export const GATED_SETTLE_MS = 2500;

/**
 * Y2-C — which phase wins when more than one is true at once, most urgent
 * first. Anything absent ranks below everything present.
 *
 *   blocked (no mic)  >  gated (no license)  >  listening  >  thinking  >  done
 *
 * THE MICROPHONE REASON WINS. Telling someone to buy a license when Yap cannot
 * hear them is the wrong sentence, and it is the one they would act on. This is
 * the same order the backend enforces in `start_recording` (PERM-C), and the
 * pill is not allowed to disagree with the backend about why a press did
 * nothing.
 */
export const PHASE_PRECEDENCE: readonly LivePhase[] = [
  "blocked",
  "waiting",
  "gated",
  // Y5-C — a take that FAILED, then a take the user CANCELLED, both above the
  // happy path: either one means the pipeline the happy path describes is no
  // longer running, so painting "Polishing" over a failure would be a lie
  // about the only thing the user needs to know. `error` outranks `cancelled`
  // because a cancel is expected and a failure is not.
  "error",
  "cancelled",
  "listening",
  "model-loading",
  "transcribing",
  "polishing",
  "pasting",
  "thinking",
  "done",
  // `empty` sits below `done`: both are "the take is over", and if a transcript
  // did arrive it outranks the absence of one. `idle`/`sleepy` stay unranked
  // (they are the absence of a phase, and must never win a tie).
  "empty",
];

/** Rank of a phase in [`PHASE_PRECEDENCE`]; lower wins, unranked sorts last. */
export function phaseRank(phase: LivePhase): number {
  const i = PHASE_PRECEDENCE.indexOf(phase);
  return i === -1 ? PHASE_PRECEDENCE.length : i;
}

/** The more urgent of two phases. Ties keep `a`, so it is stable. */
export function winningPhase(a: LivePhase, b: LivePhase): LivePhase {
  return phaseRank(a) <= phaseRank(b) ? a : b;
}

/** Denied / restricted / an unknown string all mean "Yap cannot hear you". */
const micUsable = (status: string): boolean => status === "authorized";

/**
 * The permission phase, as a pure reducer — the part of the pill that a test can
 * hold still.
 *
 * Two rules the pill got wrong before, and the reason this is not just a
 * `setState` in a listener:
 *  - `blocked` OUTRANKS a take. A refused press still emits `recording` churn
 *    around it; if a recording event could overwrite `blocked`, the refusal
 *    would flash and vanish, which is the invisible refusal being fixed.
 *  - Only the authoritative status clears it. The grant is macOS state, so
 *    nothing but TCC answering `authorized` may put the pill back to idle.
 */
export function reduceGatePhase(prev: LivePhase, ev: GateEvent): LivePhase {
  // NB: this is the MICROPHONE refusal, not Y2-C's `gated` license phase. The
  // two are different reasons a press did nothing and the mic one outranks it.
  const micRefused = prev === "blocked" || prev === "waiting";
  switch (ev.type) {
    case "mic_permission_required":
      if (micUsable(ev.status)) return micRefused ? "idle" : prev;
      return ev.status === "not_determined" ? "waiting" : "blocked";
    case "microphone-status":
      if (micUsable(ev.status)) return micRefused ? "idle" : prev;
      return ev.status === "not_determined" ? "waiting" : "blocked";
    case "recording":
      // A permission phase survives both edges of a take.
      if (micRefused) return prev;
      // Y3-D: so does the cancelled acknowledgement. Cancelling a take emits
      // `recording:false` right behind `take_cancelled`, and if that could
      // overwrite the phase the acknowledgement would flash and vanish — the
      // same bug `blocked` is protected from one case above.
      if (prev === "cancelled" && !ev.recording) return prev;
      // Y2-C: and so does the license refusal. A refused press emits `recording`
      // churn around it exactly the way a refused MIC press does, so if
      // `recording:false` could clear `gated` the explanation would flash and
      // vanish — the invisible refusal all over again. `recording:true` DOES
      // clear it: a take that really started means the gate is no longer shut.
      if (prev === "gated" && !ev.recording) return prev;
      return ev.recording ? "listening" : "idle";
    case "take_cancelled":
      // A permission refusal still outranks it: a cancel cannot make a blocked
      // mic look resolved.
      return micRefused ? prev : "cancelled";
    case "cancel_settled":
      // Only ever moves the phase it owns. A take that has already started
      // again by the time the timer fires keeps its own phase.
      return prev === "cancelled" ? "idle" : prev;
    case "license_required":
      // The microphone reason wins — see [`PHASE_PRECEDENCE`].
      if (micRefused) return prev;
      // AND a take that was ALREADY ALLOWED TO START keeps its own phase. The
      // gate stops NEW dictation only; `license.rs` promises it "never takes
      // back the ones already spoken", so the trial ending mid-flight must not
      // eat the take that is still on its way to `done`.
      if (prev === "listening" || prev === "thinking" || prev === "transcribing") return prev;
      return "gated";
    case "gate_settled":
      // Only ever moves the phase it owns, for the same reason `cancel_settled`
      // does: a later press may already have moved the pill on.
      return prev === "gated" ? "idle" : prev;
  }
}

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
  /**
   * Is a meeting recording right now (YV95 / OS-12 fix 1)? The pill is then
   * deliberately VISIBLE for hours rather than seconds, so `hidden` — the only
   * thing that used to park a shown-but-quiet pill — is false for the whole
   * session and the loop would otherwise sit on the 10fps ambient tick from the
   * first minute of a lecture to the last. Absent reads as "no meeting".
   */
  meetingRecording?: boolean;
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
  // PERM-C: `blocked`/`waiting` are deliberately NOT "active". They are static
  // refusals — a muted capsule and a struck-through glyph, both CSS — so they
  // must repaint once and then let the loop settle, never hold a 60Hz rAF for
  // as long as a grant stays revoked.
  // Y3-C — `transcribing` joins the active set. It is the phase that fills the
  // dead time after talking stops, and a determinate fill that does not repaint
  // is the undifferentiated busy state this item removes.
  // Y5-C — `model-loading`, `polishing` and `pasting` join the active set: each
  // is a bounded stage with something moving in it, and a parked loop through
  // one of them is the frozen pill this item exists to delete. `empty`,
  // `error` and `cancelled` deliberately DO NOT join: they are settled states
  // that repaint once and then let the loop go quiet, exactly like `blocked`.
  const active = phase === "listening" || phase === "thinking" || phase === "done"
    || phase === "transcribing" || phase === "model-loading" || phase === "polishing"
    || phase === "pasting";
  if (active || f.busyVisuals || f.level >= 0.02) {
    return { mode: "raf", intervalMs: f.reduceMotion ? REDUCED_FRAME_MS : 0 };
  }
  // Nothing live. A hidden pill draws nothing; reduced motion asked for one
  // calm static frame and no loop at all.
  if (f.hidden || f.reduceMotion) return { mode: "parked", intervalMs: Infinity };
  // A meeting is recording (YV95). The pill is pinned on screen for up to three
  // hours, so the ambient tick — 10fps of canvas redraw, spring update and
  // bubble reposition — is no longer "the idle scene between takes", it is
  // 108,000 frames of a chick breathing next to a clock that is DOM and a pulse
  // that is a compositor animation. Once the transients have settled there is
  // nothing on the canvas left to move, so park it outright, exactly as `hidden`
  // does. Gated on `settled` (unlike `hidden`, which nobody can see): a pill
  // frozen mid-capsule-close would be frozen in view for the whole meeting.
  if (f.meetingRecording && f.settled) return { mode: "parked", intervalMs: Infinity };
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

// ═══════════════════════════════════════════════════════════════════════════
// Y5-C — THE PHASE VOCABULARY, COMPLETE: policy here, rendering in the pills.
//
// `phaseVisual` already said how a phase is DRAWN. What was missing is
// everything that makes a phase a state rather than a label:
//
//   * a DURATION POLICY  — how long it may hold before it is a stuck pill
//   * a SUCCESSOR SET    — what it is allowed to become
//   * COPY, in every tone — a phase with no line in one tone is a blank pill
//   * a canonical ID     — the name the product, the CSS and a screenshot use
//
// And one defect, in Wilson's words: "fill the dead time after talking stops
// and before text appears". Before this, a take ran `listening → thinking →
// done`, where `thinking` was a single undifferentiated boolean (`status.busy`)
// covering engine load, ASR and polish — minutes of one frozen state on a long
// take, and on a SHORT take `busy` could come and go between two status events
// so the pill went `listening → done` with nothing in between at all. The
// transition table below makes that sequence ILLEGAL, and `reduceTakePhase`
// refuses to produce it.
// ═══════════════════════════════════════════════════════════════════════════

/**
 * Every phase, once, in one array — the fixture every table test drives off.
 *
 * Exported because the test file kept its OWN copy of this list and it had
 * already drifted: `cancelled` was in the union and not in the list, so Y3-D's
 * phase was silently exempt from "every declared phase has a treatment". One
 * list, or the table tests test whatever someone remembered to type.
 */
export const ALL_LIVE_PHASES: readonly LivePhase[] = [
  "idle", "sleepy", "listening", "model-loading", "transcribing", "polishing",
  "pasting", "thinking", "done", "empty", "cancelled", "error", "gated",
  // `blocked` before `waiting`: PERM-C asserts that order for the two phases a
  // click sends to the Permissions pane, and it is also the precedence order.
  "blocked", "waiting",
];

/**
 * The canonical PRODUCT id of each phase — snake_case, which is the vocabulary
 * Wilson enumerates (`model_loading`) and the one the backend's own event
 * payloads already speak (`mic_permission_required`, `take_cancelled`,
 * `words_so_far`). The TypeScript variant name is an implementation detail that
 * a refactor may rename; this string is the NAME, and it is stable.
 *
 * It is not decoration: the shell writes it to `<html data-phase>`, which is
 * the single CSS hook every phase treatment keys off and the only handle a
 * test — or a screenshot of a running pill — has on "which state is this".
 */
export const PHASE_ID: Record<LivePhase, string> = {
  idle: "idle",
  sleepy: "sleepy",
  listening: "listening",
  "model-loading": "model_loading",
  transcribing: "transcribing",
  polishing: "polishing",
  pasting: "pasting",
  thinking: "thinking",
  done: "done",
  empty: "empty",
  cancelled: "cancelled",
  error: "error",
  gated: "gated",
  waiting: "waiting",
  blocked: "blocked",
};

/** The polish sidecar's own wall-clock deadline (polish.rs:59). */
export const POLISH_DEADLINE_MS = 1200;

/**
 * How long a phase may HOLD before it is indistinguishable from a stuck pill.
 *
 * `null` means "waits on the user or the OS, and may wait forever" — and that
 * is the WHOLE list of phases allowed to: `idle` and `sleepy` (at rest, waiting
 * for a press), `listening` (waiting for the user to stop talking), `blocked`
 * and `waiting` (waiting on macOS, which can take days or never answer).
 * Everything else has a number, because everything else is Yap working and
 * anything Yap is doing either finishes or has failed.
 *
 * Each number is the deadline of the thing it describes plus slack, never less:
 * a pill that times out BEFORE the stage it is narrating would paint a failure
 * over work that is still running.
 */
export const PHASE_HOLD_MS: Record<LivePhase, number | null> = {
  // ── wait on the user or the OS ───────────────────────────────────────────
  idle: null,
  sleepy: null,
  listening: null,
  blocked: null,
  waiting: null,
  // ── bounded work ─────────────────────────────────────────────────────────
  /** YV80's lazy arm — a large model off a cold disk. Generous on purpose. */
  "model-loading": 20_000,
  /** One CHUNK of ASR. Real progress re-arms it, so a 12-chunk take is fine. */
  transcribing: 30_000,
  /** The sidecar gives up at [`POLISH_DEADLINE_MS`]; the pill outlives it. */
  polishing: POLISH_DEADLINE_MS + 600,
  /** A receipt-sequenced paste (YV39) is fast or it has failed. */
  pasting: 1_500,
  /** The undifferentiated legacy busy state, still the fallback. */
  thinking: 30_000,
  // ── bounded acknowledgements ─────────────────────────────────────────────
  /** Matches the check-mark hold both capsules already run. */
  done: 1_100,
  /** Long enough to READ that nothing was heard, short enough not to nag. */
  empty: 2_200,
  cancelled: CANCELLED_SETTLE_MS,
  gated: GATED_SETTLE_MS,
  /** A failure is news the user did not ask for, so it holds the longest. */
  error: 4_000,
};

/** The phases that may hold forever, derived so the two can never disagree. */
export const UNBOUNDED_PHASES: readonly LivePhase[] =
  ALL_LIVE_PHASES.filter((p) => PHASE_HOLD_MS[p] === null);

/** How long `phase` may hold, or `null` if it legitimately waits forever. */
export const phaseHoldMs = (phase: LivePhase): number | null => PHASE_HOLD_MS[phase];

/**
 * What each phase is allowed to become next.
 *
 * Read the `listening` row first: `done` IS NOT IN IT, and that is the defect
 * Wilson named. Stopping the hotkey cannot put a check mark on the pill,
 * because between "talking stopped" and "text appeared" there is always at
 * least one thing happening and the pill has to say which one.
 *
 * Every phase reaches `idle` again, directly or through an acknowledgement:
 * `every_phase_has_a_way_back_to_idle` is a test, not a hope.
 */
export const PHASE_NEXT: Record<LivePhase, readonly LivePhase[]> = {
  idle: ["listening", "sleepy", "blocked", "waiting", "gated"],
  sleepy: ["idle", "listening", "blocked", "waiting", "gated"],
  // NB: no "done". See above.
  listening: ["model-loading", "transcribing", "thinking", "cancelled", "error", "blocked"],
  "model-loading": ["transcribing", "thinking", "error", "cancelled"],
  // `done` directly: polish is SKIPPABLE (`polish_skip_reason`), so a decoded
  // take can land its text without an LLM stage ever running.
  transcribing: ["polishing", "pasting", "thinking", "done", "empty", "error", "cancelled"],
  thinking: ["polishing", "pasting", "done", "empty", "error", "cancelled"],
  polishing: ["pasting", "done", "empty", "error"],
  pasting: ["done", "error"],
  done: ["idle", "sleepy", "listening"],
  empty: ["idle", "listening"],
  cancelled: ["idle", "listening"],
  error: ["idle", "listening"],
  gated: ["idle", "listening"],
  waiting: ["idle", "blocked", "listening"],
  blocked: ["idle", "listening"],
};

/** May the pill move straight from `from` to `to`? */
export const isLegalTransition = (from: LivePhase, to: LivePhase): boolean =>
  from === to || PHASE_NEXT[from].includes(to);

/**
 * The line the pill SAYS in each phase, in each tone.
 *
 * Distinct from `phaseVisual().label`, which is the accessible NAME of the
 * capsule (read aloud, unbounded, tone-free). This is the text that is DRAWN,
 * so every one of these has to fit the side-dock strip — see
 * [`fitsSideDockStrip`], which is a test and not a promise.
 *
 * `friendly` is the default (`companion_tone: "friendly"`, lib.rs:412); a phase
 * missing a line in ONE tone is a blank pill for whoever chose that tone, which
 * is why this is a total `Record` and not a `Partial`.
 */
export const PHASE_COPY: Record<LivePhase, Record<ChatTone, string>> = {
  idle: {
    rude: "well? press it.",
    friendly: "ready when you are!",
    rose: "ready for you 🌹",
  },
  sleepy: {
    rude: "zzz. wake me.",
    friendly: "just napping…",
    rose: "resting my eyes 🌹",
  },
  listening: {
    rude: "talking. finally.",
    friendly: "listening!",
    rose: "i'm all yours 🌹",
  },
  "model-loading": {
    rude: "warming up. wait.",
    friendly: "warming up the engine…",
    rose: "just waking up 🌹",
  },
  transcribing: {
    rude: "decoding your mumbling…",
    friendly: "writing it down…",
    rose: "every word, love 🌹",
  },
  polishing: {
    rude: "fixing your grammar…",
    friendly: "tidying it up…",
    rose: "making it pretty 🌹",
  },
  pasting: {
    rude: "dropping it in…",
    friendly: "popping it in!",
    rose: "handing it over 🌹",
  },
  thinking: {
    rude: "working on it…",
    friendly: "one sec…",
    rose: "almost there 🌹",
  },
  done: {
    rude: "there. happy?",
    friendly: "done!",
    rose: "all yours 🌹",
  },
  empty: {
    // YV16: this is the gate WORKING. Never an apology, never an error face.
    rude: "you said nothing.",
    friendly: "i didn't hear words!",
    rose: "i heard only quiet 🌹",
  },
  cancelled: {
    rude: "fine, forget it.",
    friendly: "cancelled, no worries!",
    rose: "another time 🌹",
  },
  error: {
    rude: "that broke.",
    friendly: "that didn't work…",
    rose: "something went wrong 🌹",
  },
  gated: {
    rude: "pay up first.",
    friendly: "dictation is paused",
    rose: "paused for now 🌹",
  },
  waiting: {
    rude: "answer the dialog.",
    friendly: "allow the mic…",
    rose: "let me hear you? 🌹",
  },
  blocked: {
    rude: "mic's blocked. fix it.",
    friendly: "mic blocked",
    rose: "i can't hear you 🌹",
  },
};

/** The line for a phase in the user's tone. */
export const phaseCopy = (phase: LivePhase, tone: ChatTone): string => PHASE_COPY[phase][tone];

// ── the side-dock strip: the narrowest place any of this has to fit ─────────
//
// On a left/right dock the window is parked flush to the screen edge and the
// capsule hugs it (`:root[data-dock] .stage` in float.css). The capsule's own
// `max-width` is therefore the hard budget for anything drawn INSIDE it, and
// the shell's phase line is drawn inside it — so a phrase that is comfortable
// on a bottom dock can be the phrase that overflows on a side one.
//
// These mirror float.css. They are an ESTIMATE of text width (a real advance
// width needs a laid-out browser), deliberately PESSIMISTIC: 6.0px per
// character against an 11px system sans whose true average is nearer 5.6.

/** `.pill { max-width }` in float.css. */
export const PILL_MAX_WIDTH_PX = 260;
/** `:root[data-dock="left"] .stage { padding-left }` — the screen-edge inset. */
export const SIDE_DOCK_INSET_PX = 10;
/** `.pill { padding: 0 10px }`, both sides. */
export const PILL_PADDING_PX = 20;
/** The mic/stop control plus the flex gap that always sits beside the text. */
export const PILL_CONTROL_PX = 30;
/** `.pill.blocked .gate-note { padding-right: 8px }`. */
export const STRIP_TAIL_PX = 8;
/** Pessimistic average advance width at `11px` system sans. */
export const STRIP_CHAR_PX = 6.0;

/** The drawable width left for a phase line on a SIDE dock, in px. */
export const SIDE_DOCK_TEXT_PX =
  PILL_MAX_WIDTH_PX - SIDE_DOCK_INSET_PX - PILL_PADDING_PX - PILL_CONTROL_PX - STRIP_TAIL_PX;

/** Estimated drawn width of a phase line, in px. */
export const stripTextWidthPx = (text: string): number => text.length * STRIP_CHAR_PX;

/** Does a line fit the side-dock strip without clipping (`white-space: nowrap`)? */
export const fitsSideDockStrip = (text: string): boolean =>
  stripTextWidthPx(text) <= SIDE_DOCK_TEXT_PX;

/** Every dock the shell has to place the phase line at (float-main's `DOCKS`). */
export const DOCK_SIDES = ["bottom", "left", "right"] as const;
export type DockSide = (typeof DOCK_SIDES)[number];

// ── the characters that ship ────────────────────────────────────────────────

/**
 * The faces that ship today. Y5-K lifts the per-character art tables out of the
 * two components into a registry; until it does, this list is the fixture the
 * coverage test drives off, so ADDING A CREATURE ADDS A ROW and not a test file.
 */
export const SHIPPED_CHARACTERS = ["classic", "yappy"] as const;
export type PillCharacter = (typeof SHIPPED_CHARACTERS)[number];

/**
 * What one character owes ONE phase: something to draw, and how it moves.
 * `motion` is a hint the component's own renderer reads — the components keep
 * their art, this type keeps them honest about covering every phase.
 */
export interface PhaseArt {
  /** The sprite/glyph key this character draws for the phase. */
  sprite: string;
  /** How it animates while the phase holds. `still` is a settled state. */
  motion: "still" | "breathe" | "work" | "pulse" | "shake";
}

/** A character's art, one entry per phase — no `Partial`, by design. */
export type PhaseArtTable = Record<LivePhase, PhaseArt>;

// ── the take, as a reducer ──────────────────────────────────────────────────

/**
 * The part of the backend's `status` payload that decides a take's phase. Every
 * field here is one the backend ALREADY emits (`build_status`, lib.rs:873) —
 * this adds no new channel and no new Rust.
 */
export interface TakeStatus {
  recording: boolean;
  busy: boolean;
  /** YV80 — the engine is coming off disk while capture is already live. */
  engineLoading?: boolean;
  /** Set by lib.rs:1460 / 2497; the sentence `error` shows comes from here. */
  lastError?: string | null;
}

/** What can move a take's phase. Both are events the frontend already gets. */
export type TakeEvent =
  | { type: "status"; status: TakeStatus }
  /** The `transcript` event — emitted ONLY on a take that produced text. */
  | { type: "transcript"; words: number }
  /** Y3-C's per-chunk progress: real chunks, so really `transcribing`. */
  | { type: "progress" }
  /** The phase's [`PHASE_HOLD_MS`] elapsed without anything else arriving. */
  | { type: "hold_elapsed"; phase: LivePhase };

/** The phases that mean "Yap is working on this take right now". */
const WORKING: readonly LivePhase[] = [
  "model-loading", "transcribing", "polishing", "pasting", "thinking",
];
const isWorking = (p: LivePhase): boolean => WORKING.includes(p);

/**
 * A backend error string as ONE HUMAN LINE — never a code, never a stack.
 *
 * `status_message` prefixes the same string with `Error: ` for the main window;
 * the pill has a warn treatment and does not need the word, and a 200-character
 * paste-receipt message has to be cut somewhere or it clips mid-word on a side
 * dock. Empty/absent falls back to the phase's own copy, which is why this
 * returns `null` rather than an empty string.
 */
export function errorSentence(lastError?: string | null): string | null {
  if (!lastError) return null;
  const one = lastError.replace(/^\s*error:\s*/i, "").replace(/\s+/g, " ").trim();
  if (!one) return null;
  const budget = Math.floor(SIDE_DOCK_TEXT_PX / STRIP_CHAR_PX);
  if (one.length <= budget) return one;
  // Cut on a word boundary when there is one in the last quarter of the budget.
  const cut = one.slice(0, budget - 1);
  const sp = cut.lastIndexOf(" ");
  return `${(sp > budget * 0.6 ? cut.slice(0, sp) : cut).trimEnd()}…`;
}

/**
 * The take's phase, as a pure reducer over events the frontend already
 * receives. The permission/license phases are NOT its business — they live in
 * `reduceGatePhase` and outrank it; the shell combines the two with
 * [`winningPhase`].
 *
 * The rule that closes Wilson's gap is the `recording: false` case: a take that
 * stopped while the pill was `listening` becomes `thinking` — NOT `done`, and
 * NOT `idle`. Something is always happening between the hotkey release and the
 * text landing, and if the backend has not said WHICH thing yet, the pill says
 * the honest generic one rather than skipping the whole pipeline.
 */
export function reduceTakePhase(prev: LivePhase, ev: TakeEvent): LivePhase {
  switch (ev.type) {
    case "progress":
      // Real chunks came back, so this is really `transcribing` (Y3-C). It may
      // not resurrect a take that has already ended.
      return isWorking(prev) || prev === "listening" ? "transcribing" : prev;
    case "transcript":
      // A transcript event is only ever emitted for a take that produced text
      // (lib.rs:2509), so words <= 0 should be impossible — handled anyway,
      // because a `done` check mark over nothing is the dead hotkey again.
      return ev.words > 0 ? "done" : "empty";
    case "hold_elapsed": {
      // Only the phase that armed the timer may be moved by it: by the time it
      // fires the pill may already have moved on, and a stale timer that
      // clobbers a live phase is the flashing refusal bug in another costume.
      if (prev !== ev.phase || PHASE_HOLD_MS[prev] === null) return prev;
      // A WORKING phase that ran out of time has failed; an acknowledgement
      // that ran out of time has simply been read. Both settle, differently.
      return isWorking(prev) ? "error" : "idle";
    }
    case "status": {
      const s = ev.status;
      if (s.recording) return "listening";
      if (s.busy) {
        if (s.engineLoading) return "model-loading";
        // Y3-C owns `transcribing` and only real progress opens it, so a busy
        // status never downgrades a pill that is already reporting chunks.
        return prev === "transcribing" || prev === "polishing" || prev === "pasting"
          ? prev
          : "thinking";
      }
      // Not recording, not busy: the take is over. WHY it is over is the
      // interesting part.
      if (s.lastError) return isWorking(prev) || prev === "listening" ? "error" : prev;
      // The hold just ended and `busy` has not been set yet — the status
      // events race. Never `done` (see above), never `idle`.
      if (prev === "listening") return "thinking";
      // Working, then quiet, with NO transcript event: that is YV16's
      // no-speech / hallucination-gate exit, which emits no `transcript` at
      // all (lib.rs:2026). Today the pill says nothing and the user
      // experiences a dead hotkey. This is the whole visible payoff of the
      // gate.
      if (isWorking(prev)) return "empty";
      return prev;
    }
  }
}
