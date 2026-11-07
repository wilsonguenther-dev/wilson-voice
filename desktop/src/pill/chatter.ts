/**
 * yap-1106-chatter-setting (absorbs FB-21) — "Yappy talks": always / less over
 * time / only milestones.
 *
 * Before this the quieting for heavy users was a hidden rule. It is now ONE
 * visible, switchable setting (Settings › Companion), stored on the backend as
 * `yappy_talks` (wire key `yappyTalks`), with one plain sentence and the take
 * count that drives it shown beside it.
 *
 *   always      — every take gets the full show, however many you do.
 *   less        — the DEFAULT. Everything is on until your 50th take; from the
 *                 50th take on, bubbles come less often and are shorter, hops
 *                 get smaller, friends visit less, and first-time tips stop.
 *   milestones  — Yappy stays quiet and only speaks for milestone takes.
 *
 * The world, the props and the check mark always stay: they tell you the app is
 * working. What fades is talking. Pure and deterministic — YappyPill, NudgeRow
 * and Settings render what this decides; nothing here touches the DOM.
 */

export type ChatterMode = "always" | "less" | "milestones";

export const CHATTER_MODES: readonly ChatterMode[] = ["always", "less", "milestones"];

/** A fresh install, and anything unreadable, gets "less over time". */
export const DEFAULT_CHATTER_MODE: ChatterMode = "less";

/** "From your 50th take": the take NUMBER (1-based) where "less" turns quiet. */
export const QUIET_FROM_TAKE = 50;

/**
 * The pill's take counter in the shared webview localStorage. YappyPill bumps it
 * once per take; Settings reads the same key to show "Takes so far".
 */
export const TAKES_SEEN_KEY = "yap.live.takesSeen";

/** Settings choices: [stored id, label, short blurb]. */
export const CHATTER_OPTIONS: readonly (readonly [ChatterMode, string, string])[] = [
  ["always", "Always", "Every take, every line"],
  ["less", "Less over time", "Quieter from your 50th take"],
  ["milestones", "Only milestones", "Big moments only"],
];

/** The stored value → a mode. Unknown, empty or not a string → the default. */
export function parseChatterMode(raw: unknown): ChatterMode {
  return typeof raw === "string" && (CHATTER_MODES as readonly string[]).includes(raw)
    ? (raw as ChatterMode)
    : DEFAULT_CHATTER_MODE;
}

/** A take count from storage. Anything unreadable or negative is 0. */
export function parseTakeCount(raw: unknown): number {
  const n = typeof raw === "number" ? raw : typeof raw === "string" && raw.trim() !== "" ? Number(raw) : NaN;
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
}

/**
 * Milestone takes — the only takes "only milestones" speaks on. The 100th and
 * 1000th match the pill's milestone nudges (nudge.ts `MILESTONE_AT`); the rest
 * are the first take and the round numbers in between, then every 1000th.
 */
export const MILESTONE_TAKES: readonly number[] = [1, 10, 50, 100, 250, 500, 1000];
export function isMilestoneTake(takeNumber: number): boolean {
  if (!Number.isFinite(takeNumber) || takeNumber < 1) return false;
  const n = Math.floor(takeNumber);
  return MILESTONE_TAKES.includes(n) || n % 1000 === 0;
}

/** Bubble length once quiet: a line longer than this is cut at a word. */
export const QUIET_BUBBLE_CHARS = 28;
/** Hop height once quiet (1 = the full hop). */
export const QUIET_HOP_SCALE = 0.5;
/** Once quiet, a finished take gets a line on 1 take in this many. */
export const QUIET_DONE_EVERY = 2;
/** Once quiet, the repeating during-take chatter keeps 1 line in this many (the gap doubles). */
export const QUIET_REPEAT_EVERY = 2;
/** Once quiet, how many after-release lines a take may get (full is live.ts AFTER_TAKE_MAX_LINES = 2). */
export const QUIET_AFTER_LINES = 1;
/** Once quiet, the share of friend visits that still happen. */
export const QUIET_VISIT_SHARE = 0.25;

export type ChatterStage = "full" | "quiet" | "milestones";

/** Everything the pill needs to know about how much Yappy may talk on ONE take. */
export interface ChatterPolicy {
  stage: ChatterStage;
  /** Lines said on crossing into notes / desk / essay / saga while you talk. */
  tierLines: boolean;
  /** Keep 1 repeating during-take line in this many; 0 = no repeating chatter. */
  repeatEvery: number;
  /** Max after-release lines while the take decodes. */
  afterLines: number;
  /** A finished take gets a line on 1 take in this many; 0 = never. */
  doneEvery: number;
  /** Longest bubble in characters; null = no cap. */
  maxChars: number | null;
  /** Hop height multiplier. */
  hopScale: number;
  /** Share of friend visits that still happen (0..1). */
  visitShare: number;
  /** First-time tips (the new-user glance, the pill's tip nudges). */
  firstTimeTips: boolean;
}

const FULL: ChatterPolicy = {
  stage: "full", tierLines: true, repeatEvery: 1, afterLines: 2, doneEvery: 1,
  maxChars: null, hopScale: 1, visitShare: 1, firstTimeTips: true,
};
const QUIET: ChatterPolicy = {
  stage: "quiet", tierLines: true, repeatEvery: QUIET_REPEAT_EVERY, afterLines: QUIET_AFTER_LINES,
  doneEvery: QUIET_DONE_EVERY, maxChars: QUIET_BUBBLE_CHARS, hopScale: QUIET_HOP_SCALE,
  visitShare: QUIET_VISIT_SHARE, firstTimeTips: false,
};
const MILESTONES: ChatterPolicy = {
  stage: "milestones", tierLines: false, repeatEvery: 0, afterLines: 0, doneEvery: 0,
  maxChars: QUIET_BUBBLE_CHARS, hopScale: QUIET_HOP_SCALE, visitShare: 0, firstTimeTips: false,
};

/**
 * The policy for take number `takeNumber` (1-based: the take being recorded).
 * "less" is the full show for takes 1..49 and quiet from the 50th on.
 */
export function chatterPolicy(mode: ChatterMode, takeNumber: number): ChatterPolicy {
  const m = parseChatterMode(mode);
  if (m === "always") return FULL;
  if (m === "milestones") return MILESTONES;
  return parseTakeCount(takeNumber) >= QUIET_FROM_TAKE ? QUIET : FULL;
}

/** May a during-take line be said? `crossed` = a tier crossing; `repeatN` = this repeating line's 0-based index. */
export function allowDuringLine(p: ChatterPolicy, crossed: boolean, repeatN: number): boolean {
  if (crossed) return p.tierLines;
  if (p.repeatEvery <= 0) return false;
  return Math.max(0, Math.floor(repeatN)) % p.repeatEvery === 0;
}

/** May the finished take `takeNumber` get a line? Milestone takes always may. */
export function allowDoneLine(p: ChatterPolicy, takeNumber: number): boolean {
  if (isMilestoneTake(takeNumber)) return true;
  if (p.doneEvery <= 0) return false;
  return parseTakeCount(takeNumber) % p.doneEvery === 0;
}

/** May the `saidAfter`-th after-release line be said? */
export const allowAfterLine = (p: ChatterPolicy, saidAfter: number): boolean => saidAfter < p.afterLines;

/** Should a friend visit happen? `roll` in [0, 1) — callers pass their own deterministic roll. */
export const allowVisit = (p: ChatterPolicy, roll: number): boolean => roll < p.visitShare;

/** Shorten a bubble to the policy's cap, at a word, with an ellipsis. Never empty for a non-empty line. */
export function shortenBubble(text: string, maxChars: number | null): string {
  if (maxChars === null || text.length <= maxChars) return text;
  const room = Math.max(1, maxChars - 1);
  const cut = text.slice(0, room + 1);
  const space = cut.lastIndexOf(" ");
  const head = (space > 0 ? cut.slice(0, space) : text.slice(0, room)).replace(/[\s.,;:!?…-]+$/u, "");
  return `${head || text.slice(0, room)}…`;
}

/**
 * The ONE plain sentence Settings shows under "Yappy talks", per mode. The
 * "less" sentence is the approved copy (design page pill.html §04).
 */
export function chatterSentence(mode: ChatterMode): string {
  switch (parseChatterMode(mode)) {
    case "always":
      return "Yappy talks on every take, however many you do.";
    case "milestones":
      return "Yappy stays quiet and only speaks up for milestones, like your 100th take.";
    default:
      return "From your 50th take, bubbles come less often and are shorter, hops get smaller, friends visit less, and first-time tips stop.";
  }
}

/** The take count label shown next to the sentence. */
export function takeCountLabel(takes: number): string {
  const n = parseTakeCount(takes);
  return `Takes so far: ${n.toLocaleString("en-US")}`;
}

/** Where "less" stands for a user with `takes` takes done, in plain words (Settings only). */
export function chatterStatus(mode: ChatterMode, takes: number): string {
  const m = parseChatterMode(mode);
  if (m !== "less") return "";
  const n = parseTakeCount(takes);
  if (n + 1 >= QUIET_FROM_TAKE) return "Yappy is talking less now.";
  const left = QUIET_FROM_TAKE - 1 - n;
  return left === 1 ? "1 take until he talks less." : `${left} takes until he talks less.`;
}

/** The one structured telemetry line for a take's chatter decision. Counts only, never a line's text. */
export function chatterLog(mode: ChatterMode, takeNumber: number, p: ChatterPolicy): string {
  return `pill chatter mode=${parseChatterMode(mode)} take=${parseTakeCount(takeNumber)} stage=${p.stage}`;
}
