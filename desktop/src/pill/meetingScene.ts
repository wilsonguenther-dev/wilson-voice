/**
 * meetingScene — yap-1106-pill-rec-headphones-hoot-tape (+ FB-05), the pure half.
 *
 * During a meeting the pill shows a small world instead of a red dot and a
 * clock: the SUN crosses the sky (Wilson's option B), the elapsed time sits
 * under it, Hoot sits to Yappy's LEFT at a reel-to-reel tape machine (proposal
 * B), and the RIGHT ear cup of Yappy's headphones is the recording light.
 *
 * Everything that can be wrong — where the sun is at minute 37, when the sky
 * turns dusk, when Yappy glances up, how many times the canvas repaints in an
 * idle hour — is a pure function of the elapsed seconds the backend already
 * emits, so it is tested with a fake clock (meetingScene.test.ts) and needs no
 * canvas. `MeetingScene.tsx` only paints what this module decides.
 *
 * ## Energy (OS-12 still holds)
 * The canvas repaints when `meetingFrameKey` changes and at no other time:
 * once a minute (the sun moves a minute's worth) plus the two edges of each
 * 10-minute glance. The 1.6 s cup pulse and the turning reels are CSS
 * compositor animations over the canvas, so they never wake JS.
 */

/** The recording light on the right ear cup pulses on this period. */
export const REC_LIGHT_PULSE_MS = 1600;
/** One canvas redraw a minute while a meeting is idle. */
export const MEETING_IDLE_REDRAW_MS = 60_000;
/** FB-05 — every 10 minutes Yappy glances up at the sun… */
export const GLANCE_EVERY_S = 600;
/** …for one second. */
export const GLANCE_S = 1;
/** The sun crosses in the first hour; at 1 h the sky turns dusk. */
export const SUN_CROSS_S = 3600;
/** In hour two a moon crosses. */
export const MOON_CROSS_S = 3600;
/** A companion visit lasts at most this long (companions page). */
export const COMPANION_VISIT_CAP_MS = 4000;
/** On stop: Hoot hands over the tape and the pill reads "Saved" for this long. */
export const SAVED_HOLD_MS = 2400;
/** The pill's words for the stop beat. */
export const SAVED_LABEL = "Saved";

/** The scene's art buffer, in art pixels (painted at a whole-number scale). */
export const SCENE_W = 112;
export const SCENE_H = 40;
/** Where things stand in the buffer (Hoot left of Yappy, tape between). */
export const SCENE_LAYOUT = {
  groundY: 34,
  hoot: { x: 1, y: 12, size: 24 },
  tape: { x: 24, y: 20, w: 22, h: 14, reels: [{ x: 30, y: 26 }, { x: 40, y: 26 }] },
  /** feetY 36, not 38: the detail-pass Yappy (yap-1106-yappy-detail-pass) has
   * toes to feetY + 2 and a ground shadow to feetY + 3, so 36 is the lowest
   * he stands with every pixel inside the 40-row buffer. */
  yappy: { cx: 74, feetY: 36 },
  /** The right ear cup of Yappy's headphones — the recording light (moves with him). */
  cup: { x: 84, y: 22, w: 4, h: 6 },
  /**
   * yap-1106-main-character-switch — where a friend who SITS in the pill
   * stands for the meeting (Yappy's spot, at Hoot's 24 px), headphones on.
   */
  main: { x: 62, y: 11, size: 24 },
  /** That friend's right ear cup — the recording light. */
  mainCup: { x: 87, y: 19, w: 4, h: 6 },
} as const;

/**
 * yap-1106-main-character-switch — who is on stage during a meeting
 * (companions.html section 04, "Jobs never move"). Meetings are always
 * Hoot's: if Hoot sits in the pill he records the meeting himself, headphones
 * on, and there is NO visit; if anyone else sits in the pill, Hoot still comes
 * over to the tape machine, as before. Yappy is only on stage when he is the
 * one in the pill; otherwise he stays at the studio typewriter.
 */
export interface MeetingCast {
  /** Who sits in the pill (the `pill_style` character id; empty reads as Yappy). */
  main: string;
  /** Hoot walked over to record (false when Hoot IS the main character). */
  hootVisits: boolean;
}

/** The meeting's cast for whoever sits in the pill. Pure. */
export function meetingCast(occupant?: string | null): MeetingCast {
  const main = typeof occupant === "string" && occupant.trim() !== "" ? occupant.trim() : "yappy";
  return { main, hootVisits: main !== "hoot" };
}

export type SkyPhase = "day" | "dusk";

/**
 * How long a companion visit may last. Every visit is capped at 4 s — except
 * Hoot during a meeting: he is the recorder, and he stays at the tape machine
 * for as long as the meeting runs.
 */
export function visitCapMs(friend: string, ctx: { meeting: boolean }): number {
  if (ctx.meeting && friend === "hoot") return Infinity;
  return COMPANION_VISIT_CAP_MS;
}

const secs = (s: number) => (Number.isFinite(s) && s > 0 ? Math.floor(s) : 0);
const clamp01 = (n: number) => Math.max(0, Math.min(1, n));

/** The sky: day for the first hour, dusk from 1 h on. */
export function skyPhase(elapsedS: number): SkyPhase {
  return secs(elapsedS) >= SUN_CROSS_S ? "dusk" : "day";
}

/**
 * A body on its arc across the sky, in buffer pixels. `t` 0 = rising at the
 * left, 1 = setting at the right. Rounded to whole art pixels (crisp), and
 * computed from the WHOLE MINUTE so it moves once a minute, never between.
 */
function arc(t: number): { x: number; y: number } {
  const u = clamp01(t);
  const x = 6 + u * (SCENE_W - 12);
  const y = 13 - Math.sin(u * Math.PI) * 9; // 13 at the horizon edges, 4 at noon
  return { x: Math.round(x), y: Math.round(y) };
}

/** The sun, or null once it has set (from 1 h). */
export function sunPosition(elapsedS: number): { x: number; y: number } | null {
  const s = secs(elapsedS);
  if (s >= SUN_CROSS_S) return null;
  return arc(Math.floor(s / 60) * 60 / SUN_CROSS_S);
}

/** The moon: crosses during hour two, then rests where it set. Null before 1 h. */
export function moonPosition(elapsedS: number): { x: number; y: number } | null {
  const s = secs(elapsedS);
  if (s < SUN_CROSS_S) return null;
  return arc((Math.floor((s - SUN_CROSS_S) / 60) * 60) / MOON_CROSS_S);
}

/**
 * Is Yappy glancing up at the sun right now? Every 10 minutes for 1 s, never at
 * 0, skipped while dictating (he has the notepad), and never under reduce
 * motion (the time still updates).
 */
export function isGlancing(elapsedS: number, o: { dictating?: boolean; reduceMotion?: boolean } = {}): boolean {
  if (o.dictating || o.reduceMotion) return false;
  const s = secs(elapsedS);
  return s >= GLANCE_EVERY_S && s % GLANCE_EVERY_S < GLANCE_S;
}

/** Elapsed time under the sun: `0:00`, `9:59`, `1:01:00`. Short — it is pixels. */
export function sceneTimeLabel(elapsedS: number): string {
  const s = secs(elapsedS);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  const p = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(r)}` : `${m}:${p(r)}`;
}

export type YappyMeetingPose = "listening" | "glance" | "notepad";

export interface MeetingSceneInput {
  elapsedS: number;
  /** A dictation take is live mid-meeting. */
  dictating?: boolean;
  reduceMotion?: boolean;
  /** The stop beat: Hoot is handing over the tape. */
  handover?: boolean;
  /** yap-1106-main-character-switch — who sits in the pill (absent = Yappy). */
  occupant?: string | null;
}

export interface MeetingSceneModel {
  sky: SkyPhase;
  sun: { x: number; y: number } | null;
  moon: { x: number; y: number } | null;
  pose: YappyMeetingPose;
  /** The tape keeps running through dictation; it stops for the hand-over. */
  reelsTurning: boolean;
  /**
   * Hoot records every meeting; `visitCapMs` is Infinity for the meeting.
   * `visiting` is false when Hoot is the main character: he records from the
   * pill himself, headphones on, with no visit.
   */
  hoot: { present: true; visiting: boolean; handingTape: boolean; visitCapMs: number };
  /** yap-1106-main-character-switch — who is on stage. */
  cast: MeetingCast;
  /** The recording light: the right ear cup, pulsing (steady under reduce motion). */
  recLight: { pulseMs: number; pulsing: boolean; on: boolean };
  /** Where that ear cup is: on Yappy's headphones, or on the main friend's. */
  cup: { x: number; y: number; w: number; h: number };
  timeLabel: string;
  /** Where the time sits: under the sun (or the moon) inside the scene. */
  timeAnchor: { x: number; y: number };
}

/** The whole scene for one moment of the meeting. Pure. */
export function meetingScene(i: MeetingSceneInput): MeetingSceneModel {
  const s = secs(i.elapsedS);
  const sun = sunPosition(s);
  const moon = moonPosition(s);
  const body = (sun ?? moon) as { x: number; y: number };
  const handover = i.handover === true;
  const pose: YappyMeetingPose = i.dictating ? "notepad" : isGlancing(s, i) ? "glance" : "listening";
  const cast = meetingCast(i.occupant);
  const cup = cast.main === "yappy" ? SCENE_LAYOUT.cup : SCENE_LAYOUT.mainCup;
  return {
    sky: skyPhase(s),
    sun,
    moon,
    pose,
    reelsTurning: !handover && !i.reduceMotion,
    hoot: { present: true, visiting: cast.hootVisits, handingTape: handover, visitCapMs: visitCapMs("hoot", { meeting: true }) },
    cast,
    recLight: { pulseMs: REC_LIGHT_PULSE_MS, pulsing: !handover && !i.reduceMotion, on: !handover },
    cup: { x: cup.x, y: cup.y, w: cup.w, h: cup.h },
    timeLabel: sceneTimeLabel(s),
    timeAnchor: { x: body.x, y: body.y + 5 },
  };
}

/**
 * The canvas repaints when, and only when, this string changes. Idle in a
 * meeting that is once a minute (the minute bucket) plus each glance's start
 * and end; a dictation or a hand-over is its own frame. Reduce motion drops
 * the glance but keeps the minute, so the time and the sky still move.
 */
export function meetingFrameKey(i: MeetingSceneInput): string {
  const s = secs(i.elapsedS);
  const m = meetingScene(i);
  return `${Math.floor(s / 60)}|${m.sky}|${m.pose}|${i.handover ? "h" : "-"}|${m.cast.main}`;
}

/** Count of canvas repaints across [fromS, toS] stepping one backend tick a second. */
export function meetingRedraws(fromS: number, toS: number, o: Omit<MeetingSceneInput, "elapsedS"> = {}): number {
  let n = 0;
  let prev: string | null = null;
  for (let s = fromS; s <= toS; s++) {
    const k = meetingFrameKey({ ...o, elapsedS: s });
    if (k !== prev) n++;
    prev = k;
  }
  return n;
}

/**
 * The stop beat: recording → not recording starts the hand-over and "Saved".
 * Returns the deadline (ms epoch) or null. Pure; the badge arms one timer.
 */
export function stopBeatUntil(wasRecording: boolean, recording: boolean, nowMs: number): number | null {
  return wasRecording && !recording ? nowMs + SAVED_HOLD_MS : null;
}
