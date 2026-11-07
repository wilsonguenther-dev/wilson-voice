/**
 * yap-1106-studio-room (+ FB-08) — the Home room becomes the STUDIO, one
 * station per friend, in the CHOSEN layered side view (decisions.studio_style
 * = layered; companions.html section 3 "C · Layered side view", home.html
 * section 5). A faithful port of the approved artifact
 * design-2026-11-06/assets/sideview.js: a 320 x 160 buffer in three depth
 * layers that shift 1, 3 and 6 whole pixels with the pointer, inside a wooden
 * pixel frame drawn in the buffer, shown at whole-number scales only.
 *
 * What the app adds to the artifact: every station reads REAL app state
 * (`StudioState`), and the sky follows the real clock (day, dusk, night, dawn):
 *   - Hoot's booth: the ON AIR light is lit ONLY while a meeting records, and
 *     Hoot wears his headphones at the mic only then.
 *   - Yappy's desk: Yappy types while you dictate, sleeps with the routine.
 *   - Tidy's bench: the counter shows the fixes Yap made to today's takes.
 *   - Gulp's jar: the cards fill with your Dictionary size.
 *   - Shelly's nook: plugged in while the Mac charges; the meter shows the real
 *     battery level (no meter on a Mac without a battery).
 *   - Ember's window: one firefly per streak day, at night only, at most 7.
 *   - The back room (yap-1106-sal-and-mochi): Sal sits on the furnace by the
 *     back door and LIGHTS it while the speech engine loads (the engine is
 *     ready when the fire is; a cold furnace = the model is not on this Mac
 *     yet). Mochi hangs over the door frame, and climbs down onto the door
 *     itself during a Focus / Do Not Disturb or a screen share (the quiet
 *     state pill_quiet.rs emits), the reason the whole crew goes silent.
 *     Both are ROOM-ONLY: they never appear in the pill (friendVisits.ts
 *     ROOM_ONLY, characterFit.ts PILL_CHARACTERS; salMochi.test.ts pins it).
 *   - Cinder delivers the news at the back door when a briefing is waiting.
 *
 * Budget (FB-08): idle redraw at most once a minute; reduce motion = one still
 * frame. `redrawPlan` is the single rule the component's scheduler follows.
 *
 * Everything below is plain code over a tiny canvas-shaped interface, so the
 * vitest pins it without a webview.
 */
import { FRIENDS, H, type PixelCtx, type SpriteName, type SpriteOpt } from "../../sprites/art";
import { undoAiEditText, type TranscriptEntry } from "../../appTypes";
import type { HabitatAppState } from "../habitat/habitat";

/** The studio buffer (art px). The approved artifact's W x H. */
export const STUDIO_W = 320;
export const STUDIO_H = 160;
/** Top of the floor (art px). */
export const FLOOR = 132;
/** Each layer is drawn this much wider on both sides so a shift never shows an edge. */
export const PAD = 8;
/** Whole-pixel shift per layer at full pointer deflection: back, stations, front. */
export const PARALLAX = [1, 3, 6] as const;
/** The pointer's largest deflection (artifact: (x - .5) * -1.2). */
export const PARALLAX_MAX = 0.6;

/** FB-08 budget: an idle Home redraws at most once a minute. */
export const IDLE_REDRAW_MS = 60_000;
/** A recording meeting: the room follows the 1 Hz meeting clock. */
export const MEETING_REDRAW_MS = 1_000;
/** While something moves (dictating, a reaction, the pointer settling): ~30 fps. */
export const LIVE_FRAME_MS = 33;

/** Ember's fireflies: one per streak day, never more than this. */
export const MAX_FIREFLIES = 7;
/** Gulp's jar holds this many cards; it reads full at JAR_FULL_WORDS words. */
export const JAR_CARDS = 8;
export const JAR_FULL_WORDS = 1000;
/** Tidy's counter shows three digits; more reads 999. */
export const TIDY_MAX = 999;

export type SkyBand = "dawn" | "day" | "dusk" | "night";
export type YappyPose = "rest" | "work" | "think" | "cheer" | "sleep";

/** Shelly's input: the governor's battery view. `percent` null = no battery. */
export interface StudioBattery {
  percent: number | null;
  charging: boolean;
}

/** The room's real-clock light: YappyHouse's SKY keyframes at this minute
 * (the same table the pill sky reads, decisions.pill_sky = C). */
export interface StudioSky {
  top: string;
  bot: string;
  amb: string;
  ambA: number;
}

/** Everything the studio draws from. Every field is REAL app state. */
export interface StudioState {
  /** skyPhase(new Date()): 0 dawn · .25 day · .5 dusk · .75 night. */
  phase: number;
  sky: StudioSky;
  meetingRecording: boolean;
  /** Fixes Yap made to today's takes; null = not loaded (counter blank). */
  fixesToday: number | null;
  /** Words in the Dictionary; null = not loaded (jar empty). */
  dictionarySize: number | null;
  /** null = the governor has not answered yet (Shelly rests, no meter). */
  battery: StudioBattery | null;
  /** Insights streak; null = not loaded (no fireflies). */
  streakDays: number | null;
  /** A morning briefing is waiting: Cinder stands at the back door with it. */
  newsReady: boolean;
  /** yap-1106-sal-and-mochi: the speech engine is loading (AppStatus.engineLoading). */
  engineLoading: boolean;
  /** The speech model is on this Mac (AppStatus.modelReady); null = not known yet. */
  modelReady: boolean | null;
  /** The screen is shared or presented (pill_quiet `screenSharing`). */
  screenSharing: boolean;
  /** A Focus / Do Not Disturb is on (pill_quiet `doNotDisturb`). */
  doNotDisturb: boolean;
  yappy: YappyPose;
}

/** The inputs Home passes in (the clock and Yappy's pose come from the room). */
export type StudioInputs = Omit<StudioState, "phase" | "sky" | "yappy" | "meetingRecording">;

export const EMPTY_STUDIO_INPUTS: StudioInputs = {
  fixesToday: null,
  dictionarySize: null,
  battery: null,
  streakDays: null,
  newsReady: false,
  engineLoading: false,
  modelReady: null,
  screenSharing: false,
  doNotDisturb: false,
};

/* ── pure station rules ─────────────────────────────────────────────────── */

const finite = (n: unknown): n is number => typeof n === "number" && Number.isFinite(n);

/** The real-clock sky band for a skyPhase value (wraps). */
export function skyBand(phase: number): SkyBand {
  const p = finite(phase) ? ((phase % 1) + 1) % 1 : 0.25;
  // phase 0 = 06:00. dawn 05:00-07:00, day 07:00-17:00, dusk 17:00-20:00, night 20:00-05:00.
  const h = (p * 24 + 6) % 24;
  if (h >= 5 && h < 7) return "dawn";
  if (h >= 7 && h < 17) return "day";
  if (h >= 17 && h < 20) return "dusk";
  return "night";
}

/** Hoot's ON AIR light: lit only while a meeting records. */
export function onAirLit(s: Pick<StudioState, "meetingRecording">): boolean {
  return s.meetingRecording === true;
}

/** Ember's fireflies: one per streak day, night only, at most MAX_FIREFLIES. */
export function fireflyCount(streakDays: number | null | undefined, band: SkyBand): number {
  if (band !== "night" || !finite(streakDays)) return 0;
  return Math.max(0, Math.min(MAX_FIREFLIES, Math.floor(streakDays)));
}

/** Gulp's jar: cards shown for a Dictionary size (0 = empty, any word = at least one). */
export function jarCards(dictionarySize: number | null | undefined): number {
  if (!finite(dictionarySize) || dictionarySize <= 0) return 0;
  return Math.max(1, Math.min(JAR_CARDS, Math.ceil((dictionarySize / JAR_FULL_WORDS) * JAR_CARDS)));
}

/** Shelly's meter: whole battery percent 0..100, or null when there is no battery. */
export function batteryLevel(b: StudioBattery | null | undefined): number | null {
  if (!b || !finite(b.percent)) return null;
  return Math.max(0, Math.min(100, Math.round(b.percent)));
}

/** Shelly is on the charging mat's cable only while the Mac is plugged in. */
export function shellyPluggedIn(b: StudioBattery | null | undefined): boolean {
  return !!b && b.charging === true;
}

/** Tidy's counter text: up to three digits, blank while not loaded. */
export function tidyCounter(fixes: number | null | undefined): string {
  if (!finite(fixes)) return "";
  return String(Math.max(0, Math.min(TIDY_MAX, Math.floor(fixes))));
}

/** A take's `createdAt` (ISO, or SQLite "YYYY-MM-DD HH:MM:SS" in UTC). */
export function parseTakeTime(s: string | null | undefined): Date | null {
  if (!s) return null;
  // SQLite's "YYYY-MM-DD HH:MM:SS" is UTC; `new Date` would read it as local time.
  const d = /^\d{4}-\d\d-\d\d \d\d:\d\d(:\d\d(\.\d+)?)?$/.test(s) ? new Date(`${s.replace(" ", "T")}Z`) : new Date(s);
  return Number.isNaN(d.getTime()) ? null : d;
}

/**
 * Tidy's fixes today: takes from today (local date) whose cleanup changed what
 * was pasted (the same rule as "Undo AI edit", undoAiEditText). Counts the
 * takes Home has loaded, which always include today's newest.
 */
export function fixesToday(history: readonly TranscriptEntry[] | null | undefined, now: Date): number {
  if (!Array.isArray(history)) return 0;
  let n = 0;
  for (const e of history) {
    const d = parseTakeTime(e.createdAt);
    if (!d) continue;
    if (d.getFullYear() !== now.getFullYear() || d.getMonth() !== now.getMonth() || d.getDate() !== now.getDate()) continue;
    if (undoAiEditText(e) != null) n++;
  }
  return n;
}

/** Yappy's pose from the habitat director (real dictation events). */
export function yappyPose(d: { appState: HabitatAppState; activity: string; reaction: "cheer" | null }): YappyPose {
  if (d.appState === "listening") return "work";
  if (d.appState === "thinking") return "think";
  if (d.appState === "done" || d.reaction === "cheer") return "cheer";
  if (d.activity === "sleep") return "sleep";
  return "rest";
}

/* ── the back room: Sal's furnace and Mochi's door (yap-1106-sal-and-mochi) ── */

/** lit = the engine is ready; lighting = Sal is lighting it (engine loading); cold = no model yet. */
export type FurnaceState = "lit" | "lighting" | "cold";

/** The furnace follows the real speech engine: the engine is ready when the fire is. */
export function furnaceState(s: Pick<StudioState, "engineLoading" | "modelReady">): FurnaceState {
  if (s.engineLoading === true) return "lighting";
  if (s.modelReady === false) return "cold";
  return "lit";
}

/** One lighting loop (seconds): the flame grows from a spark to the full fire, then Sal strikes again. */
export const LIGHTING_CYCLE_S = 1.2;
/** The lit furnace's flame (art px radii), the approved artifact's fire. */
export const FULL_FLAME = { rx: 6, ry: 5 } as const;

/** The flame's radii at `t`, or null for a cold furnace. Lighting never outgrows the lit fire. */
export function furnaceFlame(f: FurnaceState, t: number): { rx: number; ry: number } | null {
  if (f === "cold") return null;
  if (f === "lit") return { rx: FULL_FLAME.rx, ry: FULL_FLAME.ry };
  const tt = finite(t) && t > 0 ? t : 0;
  const k = Math.min(4, Math.floor(((tt % LIGHTING_CYCLE_S) / LIGHTING_CYCLE_S) * 5));
  return { rx: 2 + k, ry: 1 + k };
}

/** Why Mochi is on the door, or null when she is up on the frame (a screen share outranks a Focus). */
export function mochiQuietReason(s: Pick<StudioState, "screenSharing" | "doNotDisturb">): "screen-sharing" | "do-not-disturb" | null {
  if (s.screenSharing === true) return "screen-sharing";
  if (s.doNotDisturb === true) return "do-not-disturb";
  return null;
}

/** Mochi hangs on the door during a Focus / Do Not Disturb or a screen share. */
export function mochiOnDoor(s: Pick<StudioState, "screenSharing" | "doNotDisturb">): boolean {
  return mochiQuietReason(s) !== null;
}

/** Mochi over the door frame (the approved spot) and on the door's upper panel (quiet). */
export const MOCHI_ON_FRAME = { x: 278, y: 12 } as const;
export const MOCHI_ON_DOOR = { x: 279, y: 36 } as const;

/** Where Mochi hangs (her sprite's top-left, art px). */
export function mochiSpot(s: Pick<StudioState, "screenSharing" | "doNotDisturb">): { x: number; y: number } {
  return mochiOnDoor(s) ? { ...MOCHI_ON_DOOR } : { ...MOCHI_ON_FRAME };
}

/** Sal's sprite frame: busy while lighting, dozing by a cold furnace, a slow blink when lit. */
export function salFrame(f: FurnaceState, t: number): number {
  const tt = finite(t) ? t : 0;
  if (f === "lighting") return Math.floor(tt * 4) % 2 === 0 ? 0 : 1;
  if (f === "cold") return 1;
  return Math.floor(tt * 2) % 2 === 0 ? 0 : 1;
}

/** Reduce motion's one still frame is drawn at this t (no friend mid-blink). */
export const STILL_T = 1;

/** Whole-pixel layer shifts for an eased pointer value (artifact rounding). */
export function parallaxOffsets(px: number): [number, number, number] {
  const p = finite(px) ? Math.max(-PARALLAX_MAX, Math.min(PARALLAX_MAX, px)) : 0;
  const half = Math.round(p * 2) / 2;
  const o = PARALLAX.map((k) => Math.round(half * k) || 0);
  return [o[0], o[1], o[2]];
}

/** The pointer's target deflection over the stage (0 = centred / left). */
export function parallaxTarget(clientX: number, left: number, width: number): number {
  if (!finite(clientX) || !finite(left) || !finite(width) || width <= 0) return 0;
  const f = Math.max(0, Math.min(1, (clientX - left) / width));
  return (f - 0.5) * -2 * PARALLAX_MAX || 0;
}

export type RedrawMode = "still" | "live" | "tick" | "idle";

export interface RedrawInputs {
  reduceMotion: boolean;
  appState: HabitatAppState;
  reacting: boolean;
  parallaxSettling: boolean;
  meetingRecording: boolean;
  /** yap-1106-sal-and-mochi: Sal is lighting the furnace (the engine is loading) — the flame moves. */
  furnaceLighting?: boolean;
}

/**
 * The ONE redraw rule. still = reduce motion (one frame, repainted only when the
 * state changes); live = something moves; tick = a meeting records (1 Hz);
 * idle = at most one redraw a minute (FB-08 budget).
 */
export function redrawPlan(i: RedrawInputs): { mode: RedrawMode; delayMs: number | null } {
  if (i.reduceMotion) return { mode: "still", delayMs: null };
  if (i.appState !== "idle" || i.reacting || i.parallaxSettling || i.furnaceLighting === true) return { mode: "live", delayMs: LIVE_FRAME_MS };
  if (i.meetingRecording) return { mode: "tick", delayMs: MEETING_REDRAW_MS };
  return { mode: "idle", delayMs: IDLE_REDRAW_MS };
}

/* ── drawing (port of sideview.js) ──────────────────────────────────────── */

/** The canvas members the studio draws with (a 2D context satisfies it). */
export interface StudioCtx extends PixelCtx {
  save(): void;
  restore(): void;
  translate(x: number, y: number): void;
}

/** One approved friend at 1x, top-left at (x, y), whole pixels. */
export function stamp(o: StudioCtx, name: SpriteName, x: number, y: number, f: number, opt?: SpriteOpt): void {
  o.save();
  o.translate(Math.round(x), Math.round(y));
  FRIENDS[name].draw(o, H(o), f || 0, opt);
  o.restore();
}

const DIGITS: Record<string, readonly string[]> = {
  "0": ["111", "101", "101", "101", "111"],
  "1": ["010", "110", "010", "010", "111"],
  "2": ["111", "001", "111", "100", "111"],
  "3": ["111", "001", "111", "001", "111"],
  "4": ["101", "101", "111", "001", "001"],
  "5": ["111", "100", "111", "001", "111"],
  "6": ["111", "100", "111", "101", "111"],
  "7": ["111", "001", "001", "001", "001"],
  "8": ["111", "101", "111", "101", "111"],
  "9": ["111", "101", "111", "001", "111"],
};

/** 3 x 5 pixel digits, 1 px apart (Tidy's counter). */
export function drawDigits(o: PixelCtx, text: string, x: number, y: number, col: string): void {
  const h = H(o);
  let cx = x;
  for (const ch of text) {
    const g = DIGITS[ch];
    if (g) g.forEach((row, j) => { for (let i = 0; i < 3; i++) if (row[i] === "1") h.r(cx + i, y + j, 1, 1, col); });
    cx += 4;
  }
}

/** The room light never darkens the picture by more than this (lights stay readable). */
export const MAX_TINT = 0.3;
/** Fixed firefly spots near Ember's window (art px). */
export const FIREFLY_SPOTS: readonly (readonly [number, number])[] = [
  [20, 70], [44, 76], [30, 88], [56, 68], [14, 98], [40, 104], [60, 92],
];

/** Back layer: wall, posters, clock, shelves, Tidy's pegboard, back door, floor. */
export function drawBack(o: StudioCtx, s: StudioState, _t: number): void {
  const h = H(o), W = STUDIO_W, Hh = STUDIO_H;
  h.r(-PAD, 0, W + PAD * 2, FLOOR, "#e9cf98"); h.r(-PAD, 0, W + PAD * 2, 6, "#c39a5f"); h.r(-PAD, 6, W + PAD * 2, 2, "#f6e0b0");
  for (let x = -PAD; x < W + PAD; x += 20) h.r(x, 8, 1, FLOOR - 40, "#dfc28a");
  h.r(-PAD, FLOOR - 32, W + PAD * 2, 32, "#b88a56"); h.r(-PAD, FLOOR - 32, W + PAD * 2, 2, "#d8a870");
  for (let x2 = -PAD; x2 < W + PAD; x2 += 12) h.r(x2, FLOOR - 30, 1, 30, "#a07444");
  // Ember's window frame + sill (the glass is a light, drawn in drawLights).
  h.r(10, 16, 46, 44, "#5f3c22"); h.r(8, 60, 50, 3, "#a5704a");
  // Hoot's booth panel (the ON AIR box is a light).
  h.r(64, 22, 46, 78, "#3b3f4a");
  for (let k = 0; k < 4; k++) for (let j = 0; j < 6; j++) h.r(67 + k * 11, 26 + j * 12, 9, 10, "#4a4f5c");
  h.r(74, 12, 26, 8, "#2a2d35");
  // posters
  h.r(120, 18, 30, 38, "#2a2d35"); h.r(122, 20, 26, 34, "#ffd24a"); h.ell(135, 34, 7, 7, "#f2b52e"); h.r(131, 32, 2, 2, "#2b2118"); h.r(137, 32, 2, 2, "#2b2118"); h.r(133, 37, 4, 2, "#ff9f1c"); h.r(126, 46, 18, 4, "#2a2d35"); h.r(127, 47, 3, 2, "#ffd24a"); h.r(132, 47, 3, 2, "#ffd24a"); h.r(137, 47, 3, 2, "#ffd24a");
  h.r(156, 24, 36, 26, "#5f3c22"); h.r(158, 26, 32, 22, "#8fd6ff"); h.ell(166, 48, 12, 8, "#6fb84e"); h.ell(184, 48, 10, 10, "#4f9f42"); h.r(178, 32, 6, 6, "#fffbe0");
  // the clock: its hands read the REAL time (hour hand + minute hand).
  h.ell(204, 30, 8, 8, "#5a3420"); h.ell(204, 30, 7, 7, "#fffdf5");
  const p = finite(s.phase) ? ((s.phase % 1) + 1) % 1 : 0;
  const hours = (p * 24 + 6) % 24;
  const ma = ((hours % 1) * 2 - 0.5) * Math.PI, ha = (((hours % 12) / 12) * 2 - 0.5) * Math.PI;
  for (let r = 1; r <= 5; r++) h.r(204 + Math.round(Math.cos(ma) * r), 30 + Math.round(Math.sin(ma) * r), 1, 1, "#2b2118");
  for (let r = 1; r <= 3; r++) h.r(204 + Math.round(Math.cos(ha) * r), 30 + Math.round(Math.sin(ha) * r), 1, 1, "#b93a26");
  // shelves with books and plants
  [[218, 46], [218, 74]].forEach((sh, i) => {
    h.r(sh[0], sh[1], 54, 3, "#8a5a36"); h.r(sh[0], sh[1], 54, 1, "#b8763f");
    let bx = sh[0] + 2;
    ["#c8443a", "#3f6fb0", "#e8c86a", "#3fb0a0", "#8a5ab0", "#e0574a"].forEach((c, n) => { const bh = 10 + ((n * 3 + i * 2) % 6); h.r(bx, sh[1] - bh, 5, bh, c); h.r(bx, sh[1] - bh, 1, bh, "#ffffff"); bx += 6; });
    h.r(sh[0] + 42, sh[1] - 8, 8, 8, "#c8443a"); h.ell(sh[0] + 46, sh[1] - 11, 5, 4, "#4f9f42");
  });
  // Tidy's pegboard
  h.r(222, 84, 50, 14, "#c9a878");
  [228, 236, 246, 256, 264].forEach((x, n) => h.r(x, 86, 2, 10, ["#7d8c96", "#c8443a", "#e8c86a", "#3fb0a0", "#7d8c96"][n]));
  // Tidy's counter over her pegboard: the fixes Yap made to today's takes.
  h.r(240, 76, 14, 7, "#2a2d35");
  const txt = tidyCounter(s.fixesToday);
  drawDigits(o, txt, 253 - txt.length * 4, 77, "#7fdc7a");
  // the back door (the back room: Mochi's frame, Sal's furnace)
  h.r(282, 30, 34, FLOOR - 30, "#5f3c22"); h.r(285, 33, 28, FLOOR - 33, "#7a4b2e"); h.r(288, 36, 22, 30, "#6b4428"); h.r(288, 70, 22, 30, "#6b4428"); h.ell(309, 84, 1.5, 1.5, "#e8c86a");
  // floor
  h.r(-PAD, FLOOR, W + PAD * 2, Hh - FLOOR, "#a5673f"); h.r(-PAD, FLOOR, W + PAD * 2, 2, "#c08a5a");
  for (let f = -PAD; f < W + PAD; f += 26) h.r(f, FLOOR + 2, 1, Hh - FLOOR, "#8a5434");
  h.r(-PAD, FLOOR + 14, W + PAD * 2, 1, "#94593a");
}

/** Middle layer: every friend at their station. */
export function drawMid(o: StudioCtx, s: StudioState, t: number): void {
  const h = H(o), fr = Math.floor(t * 2) % 2;
  [[34, 18], [86, 16], [206, 14], [246, 16], [292, 18]].forEach((sh) => h.ell(sh[0] + 3, FLOOR + 3, sh[1], 2, "#3a2418", 0.2));
  // Shelly's charging nook: mat, outlet, cable only while plugged in, real meter.
  h.r(12, FLOOR - 1, 46, 4, "#5f8a4e"); h.r(12, FLOOR - 1, 46, 1, "#8aae6e");
  h.r(4, FLOOR - 24, 6, 8, "#e7e1d0"); h.r(6, FLOOR - 21, 1, 2, "#3b3f4a"); h.r(8, FLOOR - 21, 1, 2, "#3b3f4a");
  const plugged = shellyPluggedIn(s.battery);
  if (plugged) { h.r(7, FLOOR - 16, 1, 15, "#2a2d35"); h.r(7, FLOOR - 1, 6, 1, "#2a2d35"); }
  else { h.r(7, FLOOR - 16, 1, 2, "#2a2d35"); h.r(13, FLOOR + 2, 3, 2, "#e7e1d0"); h.r(16, FLOOR + 3, 6, 1, "#2a2d35"); }
  stamp(o, "shelly", 12, FLOOR - 34, plugged ? 0 : fr);
  const lvl = batteryLevel(s.battery);
  if (lvl != null) {
    const bx = 40, by = FLOOR - 44, fill = Math.round((lvl / 100) * 8);
    h.r(bx, by, 10, 5, "#2a2d35"); h.r(bx + 10, by + 1, 1, 3, "#2a2d35"); h.r(bx + 1, by + 1, 8, 3, "#e7e1d0");
    if (fill > 0) h.r(bx + 1, by + 1, fill, 3, lvl > 50 ? "#5fb35a" : lvl > 20 ? "#e8c86a" : "#e5484d");
    if (plugged) { h.r(bx + 4, by - 3, 2, 2, "#ffd23f"); h.r(bx + 3, by - 1, 2, 1, "#ffd23f"); }
  }
  // Hoot's booth desk and mic: headphones on only while the meeting records.
  h.r(68, FLOOR - 18, 40, 4, "#8a5a36"); h.r(68, FLOOR - 18, 40, 1, "#b8763f"); h.r(70, FLOOR - 14, 2, 14, "#5f3c22"); h.r(104, FLOOR - 14, 2, 14, "#5f3c22"); h.r(100, FLOOR - 34, 1, 16, "#2a2d35"); h.ell(100, FLOOR - 36, 2, 3, "#5a606e");
  stamp(o, "hoot", 66, FLOOR - 40, (t % 3.1) < 0.15 ? 1 : 0, { bare: !onAirLit(s) });
  // Yappy stands behind his desk (the desk is in the front layer).
  const yf = s.yappy === "sleep" ? 1 : s.yappy === "work" ? (Math.floor(t * 6) % 3 === 0 ? 2 : 0) : s.yappy === "cheer" ? 2 : (t % 3.4) < 0.15 ? 1 : 0;
  stamp(o, "yappy", 140, FLOOR - 50, yf);
  // Tidy's bench (her counter hangs over the pegboard, back layer).
  h.r(190, FLOOR - 22, 36, 4, "#6b4428"); h.r(190, FLOOR - 22, 36, 1, "#8a5a36"); h.r(192, FLOOR - 18, 2, 18, "#4a2c1e"); h.r(222, FLOOR - 18, 2, 18, "#4a2c1e"); h.r(196, FLOOR - 27, 10, 5, "#9aa3ad");
  stamp(o, "tidy", 186, FLOOR - 34, fr);
  // Gulp's jar: one card per eighth of a full Dictionary.
  h.r(262, FLOOR - 30, 14, 30, "#9fc8d0"); h.r(262, FLOOR - 30, 14, 1, "#ffffff"); h.r(261, FLOOR - 33, 16, 3, "#8a5a36"); h.r(264, FLOOR - 28, 1, 26, "#d8eef2");
  const cards = jarCards(s.dictionarySize);
  for (let n = 0; n < cards; n++) h.r(265 + ((n * 3) % 4), FLOOR - 4 - n * 3, 6, 2, n % 2 ? "#fff2b0" : "#fffdf5");
  stamp(o, "gulp", 220, FLOOR - 37, (t % 5) < 0.6 ? 1 : 0);
  // The back room: Sal's furnace + Sal on top (lighting it while the engine
  // loads); Mochi over the door frame, or ON the door during a Focus / DND / share.
  h.r(282, FLOOR - 28, 30, 28, "#2a2d35"); h.r(284, FLOOR - 26, 26, 24, "#3b3f4a"); h.r(296, FLOOR - 70, 4, 42, "#2a2d35");
  const furnace = furnaceState(s);
  if (furnace === "cold") { h.r(291, FLOOR - 8, 12, 2, "#6a6a72"); h.r(293, FLOOR - 9, 3, 1, "#8a8a92"); h.r(299, FLOOR - 9, 2, 1, "#8a8a92"); }
  stamp(o, "sal", 274, FLOOR - 56, salFrame(furnace, t));
  const mochi = mochiSpot(s);
  stamp(o, "mochi", mochi.x, mochi.y, 1);
  // Cinder's news delivery at the back door, when a briefing is waiting.
  if (s.newsReady) {
    stamp(o, "cinder", 240, FLOOR - 40, Math.floor(t * 3) % 2, { talk: Math.floor(t * 3) % 2 === 1, t });
    h.r(256, FLOOR - 20, 8, 6, "#fffdf5"); h.r(257, FLOOR - 18, 6, 1, "#4a4f5c"); h.r(257, FLOOR - 16, 4, 1, "#4a4f5c");
  }
  // Ember works the window.
  stamp(o, "ember", 26 + Math.round(Math.sin(t * 1.5) * 3), 34 + Math.round(Math.sin(t * 2) * 2), fr);
}

/** Front layer: Yappy's desk + typewriter + mug, a plant, crates, a cable. */
export function drawFront(o: StudioCtx, s: StudioState, t: number): void {
  const h = H(o), Hh = STUDIO_H;
  h.r(116, FLOOR - 16, 90, 6, "#5a3420"); h.r(117, FLOOR - 16, 88, 5, "#b8763f"); h.r(117, FLOOR - 16, 88, 1, "#d8955a");
  for (let g = 122; g < 200; g += 19) h.r(g, FLOOR - 13, 9, 1, "#a8683a");
  h.r(120, FLOOR - 10, 4, 22, "#5a3420"); h.r(198, FLOOR - 10, 4, 22, "#5a3420"); h.r(124, FLOOR - 10, 74, 8, "#8a5a36"); h.r(124, FLOOR - 10, 74, 1, "#6b4428"); h.r(156, FLOOR - 7, 10, 2, "#e8c86a");
  const typing = s.yappy === "work";
  const ux = 164, uy = FLOOR - 26, lift = typing && Math.floor(t * 6) % 3 === 0 ? 1 : 0;
  h.r(ux, uy + 3, 22, 7, "#2a2d35"); h.r(ux + 1, uy + 3, 20, 6, "#4a5060"); h.r(ux + 1, uy + 3, 20, 1, "#6a7284"); h.r(ux + 3, uy, 16, 4, "#2a2d35");
  h.r(ux + 5, uy - 7 - lift, 12, 8, "#c9c3ad"); h.r(ux + 6, uy - 7 - lift, 10, 7, "#fffdf5"); h.r(ux + 7, uy - 5 - lift, 7, 1, "#4a4f5c");
  for (let q = 0; q < 6; q++) h.r(ux + 2 + q * 3, uy + 6, 2, 2, typing && q === Math.floor(t * 9) % 6 ? "#ffd23f" : "#e8e6e1");
  h.r(124, FLOOR - 22, 7, 6, "#5a3420"); h.r(125, FLOOR - 22, 5, 5, "#e8e6e1"); h.r(125, FLOOR - 21, 5, 1, "#7a4626"); h.r(131, FLOOR - 21, 2, 3, "#5a3420");
  h.ell(0, Hh - 22, 14, 22, "#2f5a2c"); h.ell(4, Hh - 30, 8, 14, "#3e7a38"); h.ell(14, Hh - 18, 7, 12, "#356a30"); h.r(-6, Hh - 12, 26, 12, "#7a3a2a"); h.r(-6, Hh - 12, 26, 2, "#9a4a36");
  h.r(298, Hh - 22, 30, 22, "#6b4428"); h.r(298, Hh - 22, 30, 2, "#8a5a36"); h.r(298, Hh - 12, 30, 1, "#4a2c1e"); h.r(312, Hh - 22, 1, 22, "#4a2c1e"); h.r(306, Hh - 34, 22, 12, "#7a4b2e"); h.r(306, Hh - 34, 22, 1, "#a5704a");
  for (let c = 206; c < 296; c++) h.r(c, Hh - 6 + Math.round(Math.sin(c / 9) * 1), 1, 1, "#2a2d35");
}

/**
 * The lights, drawn over the room's tint so they glow: the window's real sky,
 * the ON AIR box, the string lights (back layer, shifted `backDx`), the
 * furnace and Ember's fireflies (station layer, shifted `midDx`).
 */
export function drawLights(o: StudioCtx, s: StudioState, t: number, backDx: number, midDx: number): void {
  const band = skyBand(s.phase);
  // A NaN or negative clock must still pick a real bunting colour (raster takes hex only).
  const fr = finite(t) ? ((Math.floor(t * 2) % 2) + 2) % 2 : 0;
  o.save();
  o.translate(backDx, 0);
  const h = H(o);
  // The window shows the room's real-clock sky: upper pane the zenith, lower the horizon.
  h.r(13, 19, 40, 18, s.sky.top); h.r(13, 39, 40, 18, s.sky.bot);
  if (band === "night") {
    h.ell(46, 26, 3, 3, "#f6f0cf");
    [[18, 24], [24, 44], [42, 48], [20, 52]].forEach((q) => h.r(q[0], q[1], 1, 1, "#fffbe0"));
  } else if (band === "day") {
    h.ell(22, 27, 4, 4, "#ffd23f"); h.ell(44, 46, 7, 2, "#ffffff");
  } else {
    h.ell(band === "dusk" ? 44 : 20, 50, 4, 4, "#ff7a3c"); h.ell(30, 30, 6, 2, "#fff2d8");
  }
  h.r(32, 19, 2, 38, "#5f3c22"); h.r(13, 37, 40, 2, "#5f3c22");
  // ON AIR: lit only while a meeting records; dark glass otherwise.
  h.r(76, 14, 22, 4, onAirLit(s) ? ((t % 1.6) < 1.1 ? "#e5484d" : "#c23a3a") : "#4a2c2a");
  if (onAirLit(s)) for (let i = 0; i < 4; i++) h.r(79 + i * 4, 15, 2, 2, "#ffe6e0");
  for (let k = 0; k < 16; k++) {
    const lx = 112 + k * 12, ly = 10 + Math.round(Math.sin(k * 0.8) * 2);
    h.r(lx, ly, 2, 2, ["#ffd23f", "#ff8a8a", "#7fdc7a", "#8fd6ff"][(k + fr) % 4]);
  }
  o.restore();
  o.save();
  o.translate(midDx, 0);
  const m = H(o);
  // Sal's furnace: the full fire when the engine is ready, a growing flame plus
  // the sparks from Sal's tail while it loads, nothing when it is cold.
  const furnace = furnaceState(s);
  const flame = furnaceFlame(furnace, t);
  if (flame) {
    m.ell(297, FLOOR - 12, flame.rx, flame.ry, fr ? "#ff8a3c" : "#ffb04a");
    m.ell(297, FLOOR - 11, Math.min(3, flame.rx - 1), Math.min(3, flame.ry), "#ffe27a");
  }
  if (furnace === "lighting") {
    for (let i = 0; i < 3; i++) {
      const p = (((finite(t) ? t : 0) * 1.5 + i / 3) % 1 + 1) % 1;
      m.r(316 - Math.round(p * 12), FLOOR - 38 + Math.round(p * 24), 1, 1, i % 2 ? "#ffd23f" : "#fff2b0");
    }
  }
  const flies = fireflyCount(s.streakDays, band);
  for (let i = 0; i < flies; i++) {
    const [fx, fy] = FIREFLY_SPOTS[i];
    const on = ((t * 0.9 + i * 0.37) % 1) < 0.75;
    m.r(fx, fy, 1, 1, on ? "#fff27a" : "#c9b84a");
    if (on) m.r(fx, fy, 1, 1, "#fffbe0", 0.9);
  }
  o.restore();
}

/** The room's light for the clock, over the whole picture (before the lights). */
export function drawTint(o: PixelCtx, s: StudioState): void {
  const a = roomTintAlpha(s.sky);
  if (a > 0) H(o).r(0, 0, STUDIO_W, STUDIO_H, s.sky.amb, a);
}

/** The room light's strength for a sky (0 by day, at most MAX_TINT at night). */
export function roomTintAlpha(sky: Pick<StudioSky, "ambA"> | null | undefined): number {
  const a = sky && finite(sky.ambA) ? sky.ambA : 0;
  return Math.max(0, Math.min(MAX_TINT, a * 0.7));
}

/* ── the wooden pixel frame (yap-1106-fb-24, home.html section 5) ───────────
 * "The room now hangs in a wooden pixel frame: a dark edge, a lit top and left,
 * shaded bottom and right, brass corner pegs and a small brass plate. It's drawn
 * inside the picture at 3x, so it stays crisp and the room is still edge to edge."
 * A byte-exact port of sideview.js `frame()`. It lives in the BUFFER (so it
 * scales with the room at the same whole-number cell, no smoothing, no margin),
 * it never moves with the parallax, and it is drawn ONCE into an offscreen
 * canvas (`createFrameCache`) that every redraw blits, so the budget is unchanged.
 * Pixel-art palette: named here beside the room art, not in tokens.css. */

/** The frame's rail width in buffer px (artifact `F`). */
export const FRAME_F = 5;
/** The brass plate's width in buffer px, centred on the bottom rail. */
export const PLATE_W = 28;
/** The plate's left edge in buffer px (artifact `Math.floor(W / 2) - 14`). */
export const PLATE_X = Math.floor(STUDIO_W / 2) - PLATE_W / 2;
/** The four 2 x 2 brass corner pegs (top-left corners, buffer px). */
export const FRAME_PEGS: readonly (readonly [number, number])[] = [
  [2, 2],
  [STUDIO_W - 4, 2],
  [2, STUDIO_H - 4],
  [STUDIO_W - 4, STUDIO_H - 4],
];

/** The frame's palette (artifact colours, verbatim). */
export const FRAME_PAL = {
  edgeLit: "#3a2418", // the dark outer edge, top + left (and the inner edge)
  edgeShade: "#2a1a10", // the dark outer edge, bottom + right
  faceTop: "#8a5a36",
  faceLeft: "#7a4b2e",
  faceShade: "#6b4428", // bottom + right rails
  litTop: "#c48a52",
  litLeft: "#b8763f",
  shade: "#4a2c1e", // the shaded bottom + right bevel
  grainLit: "#9a6a40", // grain dashes on the lit rails (every 11 / 13 px)
  grainShade: "#5a3420", // grain dashes on the shaded rails, and the inner lower/right edge
  innerShadow: "#000000", // drawn at INNER_SHADOW_TOP / INNER_SHADOW_LEFT alpha
  peg: "#e8c86a",
  pegShine: "#fff2b0",
  plate: "#c9a03a",
  plateLit: "#f0d27a",
  plateShade: "#8a6a1f",
  plateTick: "#6b4f15",
} as const;
/** The inner shadow under the top / left rails (artifact rgba(0,0,0,.18) / .12). */
export const INNER_SHADOW_TOP = 0.18;
export const INNER_SHADOW_LEFT = 0.12;

/** The wooden pixel frame (artifact `frame`), in the buffer so it scales with the room. */
export function drawFrame(o: PixelCtx): void {
  const h = H(o), F = FRAME_F, W = STUDIO_W, Hh = STUDIO_H, P = FRAME_PAL;
  h.r(0, 0, W, F, P.faceTop); h.r(0, Hh - F, W, F, P.faceShade); h.r(0, 0, F, Hh, P.faceLeft); h.r(W - F, 0, F, Hh, P.faceShade);
  h.r(1, 1, W - 2, 1, P.litTop); h.r(1, 1, 1, Hh - 2, P.litLeft); h.r(1, Hh - 2, W - 2, 1, P.shade); h.r(W - 2, 1, 1, Hh - 2, P.shade);
  for (let x = 8; x < W - 8; x += 11) { h.r(x, 2, 4, 1, P.grainLit); h.r(x + 5, Hh - 3, 3, 1, P.grainShade); }
  for (let y = 9; y < Hh - 9; y += 13) { h.r(2, y, 1, 4, P.grainLit); h.r(W - 3, y + 5, 1, 3, P.grainShade); }
  h.r(0, 0, W, 1, P.edgeLit); h.r(0, Hh - 1, W, 1, P.edgeShade); h.r(0, 0, 1, Hh, P.edgeLit); h.r(W - 1, 0, 1, Hh, P.edgeShade);
  h.r(F, F, W - F * 2, 1, P.edgeLit); h.r(F, F, 1, Hh - F * 2, P.edgeLit); h.r(F, Hh - F - 1, W - F * 2, 1, P.grainShade); h.r(W - F - 1, F, 1, Hh - F * 2, P.grainShade);
  h.r(F + 1, F + 1, W - F * 2 - 2, 1, P.innerShadow, INNER_SHADOW_TOP); h.r(F + 1, F + 1, 1, Hh - F * 2 - 2, P.innerShadow, INNER_SHADOW_LEFT);
  FRAME_PEGS.forEach((c) => { h.r(c[0], c[1], 2, 2, P.peg); h.r(c[0], c[1], 1, 1, P.pegShine); });
  const px = PLATE_X;
  h.r(px, Hh - F, PLATE_W, F, P.plate); h.r(px, Hh - F, PLATE_W, 1, P.plateLit); h.r(px, Hh - 1, PLATE_W, 1, P.plateShade); h.r(px + 1, Hh - F + 1, 1, 1, P.plateShade); h.r(px + PLATE_W - 2, Hh - F + 1, 1, 1, P.plateShade);
  for (let q = 0; q < 6; q++) h.r(px + 5 + q * 3, Hh - 3, 2, 1, P.plateTick);
}

/** True for a buffer pixel the frame paints OPAQUE: the four rails plus the
 *  1 px inner edge (the rgba inner shadow beyond it blends with the room). */
export function isFramePixel(x: number, y: number): boolean {
  const b = FRAME_F + 1;
  return x < b || y < b || x >= STUDIO_W - b || y >= STUDIO_H - b;
}

/** The offscreen frame: drawn once on first use, blitted on every later redraw. */
export interface FrameCache<C> {
  /** The cached frame surface (STUDIO_W x STUDIO_H, transparent inside the rails). */
  get(): C;
  /** renders = times the frame was painted (1 after the first get); hits = later gets served from the cache. */
  readonly stats: { renders: number; hits: number };
}

/** `make` returns a fresh STUDIO_W x STUDIO_H surface and its 2D context. */
export function createFrameCache<C>(make: () => { surface: C; ctx: PixelCtx }): FrameCache<C> {
  const stats = { renders: 0, hits: 0 };
  let cached: C | null = null;
  return {
    stats,
    get() {
      if (cached !== null) {
        stats.hits++;
        return cached;
      }
      const { surface, ctx } = make();
      drawFrame(ctx);
      stats.renders++;
      cached = surface;
      return surface;
    },
  };
}

/** What the compositor needs (a 2D canvas context and the test Raster both satisfy it). */
export interface StudioBlitter<S> extends StudioCtx {
  setTransform(a: number, b: number, c: number, d: number, e: number, f: number): void;
  clearRect(x: number, y: number, w: number, h: number): void;
  drawImage(src: S, sx: number, sy: number, sw: number, sh: number, dx: number, dy: number, dw: number, dh: number): void;
}

/**
 * One studio picture into the STUDIO_W x STUDIO_H buffer `o`: the three
 * painted layers (each PAD wider per side) at their parallax offsets, the sky
 * tint, the lights, then the cached frame at (0, 0) — the frame never shifts.
 */
export function composeStudio<S>(
  o: StudioBlitter<S>,
  layers: readonly S[],
  frame: S,
  offs: readonly [number, number, number],
  s: StudioState,
  t: number,
): void {
  const W = STUDIO_W, Hh = STUDIO_H, LW = STUDIO_W + PAD * 2;
  o.setTransform(1, 0, 0, 1, 0, 0);
  o.clearRect(0, 0, W, Hh);
  layers.forEach((c, i) => o.drawImage(c, 0, 0, LW, Hh, -PAD + offs[i], 0, LW, Hh));
  drawTint(o, s);
  drawLights(o, s, t, offs[0], offs[1]);
  o.drawImage(frame, 0, 0, W, Hh, 0, 0, W, Hh);
}

/** The three layer painters, back to front (each is drawn into a PAD-wide layer). */
export const LAYERS: readonly ((o: StudioCtx, s: StudioState, t: number) => void)[] = [drawBack, drawMid, drawFront];

/** A plain-words label for screen readers, from the same state the picture draws. */
export function studioAriaLabel(s: StudioState): string {
  const band = skyBand(s.phase);
  const parts = [`Yappy's studio at ${band === "day" ? "daytime" : band}.`];
  parts.push(s.yappy === "work" ? "Yappy is typing at his desk." : s.yappy === "sleep" ? "Yappy is dozing at his desk." : "Yappy is at his desk.");
  parts.push(onAirLit(s) ? "Hoot's ON AIR light is on: a meeting is recording." : "Hoot's booth is quiet.");
  const fixes = tidyCounter(s.fixesToday);
  if (fixes) parts.push(`Tidy's counter shows ${fixes} ${fixes === "1" ? "fix" : "fixes"} today.`);
  if (finite(s.dictionarySize)) parts.push(`Gulp's jar holds ${Math.max(0, Math.floor(s.dictionarySize))} Dictionary ${s.dictionarySize === 1 ? "word" : "words"}.`);
  const lvl = batteryLevel(s.battery);
  if (lvl != null) parts.push(`Shelly shows ${lvl}% battery${shellyPluggedIn(s.battery) ? ", charging" : ""}.`);
  else if (shellyPluggedIn(s.battery)) parts.push("Shelly is plugged in.");
  const flies = fireflyCount(s.streakDays, band);
  if (flies > 0) parts.push(`Ember has ${flies} ${flies === 1 ? "firefly" : "fireflies"} out for your streak.`);
  if (s.newsReady) parts.push("Cinder is at the back door with the news.");
  parts.push(studioBackRoomLabel(s));
  return parts.join(" ");
}

/** The back room in plain words (Sal's furnace, Mochi's spot), from the same rules the picture draws. */
export function studioBackRoomLabel(s: Pick<StudioState, "engineLoading" | "modelReady" | "screenSharing" | "doNotDisturb">): string {
  const furnace = furnaceState(s);
  const sal =
    furnace === "lighting"
      ? "Sal is lighting the furnace in the back room while the speech engine loads."
      : furnace === "cold"
        ? "Sal waits by a cold furnace in the back room: the speech engine is not ready yet."
        : "Sal keeps the furnace warm in the back room.";
  const why = mochiQuietReason(s);
  const mochi =
    why === "screen-sharing"
      ? "Mochi hangs on the door, so the crew stays quiet while your screen is shared."
      : why === "do-not-disturb"
        ? "Mochi hangs on the door, so the crew stays quiet during Do Not Disturb."
        : "Mochi hangs over the door.";
  return `${sal} ${mochi}`;
}
