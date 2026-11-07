// yap-1106-news-duo-delivery — Cinder and Yappy deliver the news together.
// The pure half: the turn script (Cinder names the topic in one short line,
// Yappy explains in the chosen voice) and ONE clock that both the studio and
// the briefing card read, so the flight, the hand-over and the turns stay in
// step. Quiet rules (meeting, screen share, Do Not Disturb) are applied by the
// caller: while it is quiet the caller passes no arrival time, and this module
// answers "done" (nothing moves, nobody talks).
import { CINDER_FLIGHT_MS } from "../home/studio/studio";
import { voiceFor, type VoiceId } from "../pill/voices";
import { isMarketStory, type Briefing, type BriefingStory } from "./briefing";

/** Cinder lands at Yappy's desk and hands the paper over in this long. */
export const DUO_HANDOVER_MS = 900;
/** Cinder's turn: one short line naming the topic. */
export const DUO_CINDER_TURN_MS = 1800;
/** Yappy's turn: the one-line explanation. */
export const DUO_YAPPY_TURN_MS = 4200;
/** Cinder's topic line never runs longer than this (one short line). */
export const CINDER_LINE_MAX = 32;

export type DuoSpeaker = "cinder" | "yappy";

export interface DuoTurn {
  who: DuoSpeaker;
  text: string;
  /** The story this turn is about. */
  urlKey: string;
}

export type DuoStage = "flying" | "handover" | "turns" | "done";

export interface DuoFrame {
  stage: DuoStage;
  /** Cinder's flight, 0..1 (1 = at the desk). */
  flight: number;
  /** The paper's trip from Cinder to Yappy's desk, 0..1 (1 = on the desk). */
  handover: number;
  /** The turn being spoken, or -1 (none). */
  turn: number;
  speaker: DuoSpeaker | null;
  /** Milliseconds until the frame changes stage or turn; null = never. */
  nextChangeMs: number | null;
}

const TOPIC_WORDS: Record<string, string> = {
  developer: "Dev news",
  business: "Business news",
  tech: "Tech news",
  science: "Science news",
  world: "World news",
  sports: "Sports news",
  culture: "Culture news",
  ai: "AI news",
  design: "Design news",
  startups: "Startup news",
  crypto: "Crypto news",
  local: "Local news",
};

function clip(s: string, max: number): string {
  const one = s.replace(/\s+/g, " ").trim();
  return one.length <= max ? one : `${one.slice(0, max - 1).trimEnd()}…`;
}

/** Cinder names the topic in one short line ("Tech news!", "Your stocks: NVDA!"). */
export function cinderTopicLine(s: Pick<BriefingStory, "topic" | "source">, tickers: readonly string[]): string {
  if (isMarketStory(s, tickers)) {
    const sym = tickers.includes(s.topic.toUpperCase()) ? s.topic.toUpperCase() : "";
    return clip(sym ? `Your stocks: ${sym}!` : "Your stocks!", CINDER_LINE_MAX);
  }
  const known = TOPIC_WORDS[s.topic];
  if (known) return `${known}!`;
  const custom = s.topic.trim();
  return custom ? clip(`About ${custom}!`, CINDER_LINE_MAX) : "Next one!";
}

const LEADS: Record<VoiceId, string> = {
  bestfriend: "okay so: ",
  badass: "real talk: ",
  professional: "In brief: ",
  rose: "for you, love: ",
};

/**
 * Yappy explains in the chosen voice: the explain step's line (already written
 * in that voice), or, when the headline stands alone, the headline with the
 * voice's own lead-in. Never more than the headline says.
 */
export function yappyTurnLine(s: Pick<BriefingStory, "headline" | "line">, tone: string | null | undefined): string {
  if (s.line.trim()) return s.line.trim();
  return `${LEADS[voiceFor(tone)]}${s.headline.trim()}`;
}

/** The whole script: per story, Cinder's topic line then Yappy's explanation. */
export function duoTurns(b: Briefing | null, tickers: readonly string[], tone: string | null | undefined): DuoTurn[] {
  if (!b) return [];
  const out: DuoTurn[] = [];
  for (const s of b.stories) {
    out.push({ who: "cinder", text: cinderTopicLine(s, tickers), urlKey: s.urlKey });
    out.push({ who: "yappy", text: yappyTurnLine(s, tone), urlKey: s.urlKey });
  }
  return out;
}

const turnMs = (i: number) => (i % 2 === 0 ? DUO_CINDER_TURN_MS : DUO_YAPPY_TURN_MS);

/** How long the whole delivery runs (flight + hand-over + turns); reduce motion drops the flight and hand-over. */
export function duoLengthMs(turns: number, reduceMotion: boolean): number {
  let ms = reduceMotion ? 0 : CINDER_FLIGHT_MS + DUO_HANDOVER_MS;
  for (let i = 0; i < Math.max(0, Math.floor(turns)); i++) ms += turnMs(i);
  return ms;
}

const DONE: DuoFrame = { stage: "done", flight: 1, handover: 1, turn: -1, speaker: null, nextChangeMs: null };

/**
 * Where the delivery is at `nowMs`. No arrival (none yet, or it is quiet), a
 * clock that runs backwards, or a finished script = done. Reduce motion shows
 * the pair side by side at once (no flight, no hand-over) and only the words
 * take turns.
 */
export function duoAt(arrivedAtMs: number | null | undefined, nowMs: number, reduceMotion: boolean, turns: number): DuoFrame {
  if (arrivedAtMs == null || !Number.isFinite(arrivedAtMs) || !Number.isFinite(nowMs)) return { ...DONE };
  let e = nowMs - arrivedAtMs;
  if (e < 0) return { ...DONE };
  if (!reduceMotion) {
    if (e < CINDER_FLIGHT_MS)
      return { stage: "flying", flight: e / CINDER_FLIGHT_MS, handover: 0, turn: -1, speaker: null, nextChangeMs: CINDER_FLIGHT_MS - e };
    e -= CINDER_FLIGHT_MS;
    if (e < DUO_HANDOVER_MS)
      return { stage: "handover", flight: 1, handover: e / DUO_HANDOVER_MS, turn: -1, speaker: null, nextChangeMs: DUO_HANDOVER_MS - e };
    e -= DUO_HANDOVER_MS;
  }
  const n = Math.max(0, Math.floor(turns));
  for (let i = 0; i < n; i++) {
    const len = turnMs(i);
    if (e < len) return { stage: "turns", flight: 1, handover: 1, turn: i, speaker: i % 2 === 0 ? "cinder" : "yappy", nextChangeMs: len - e };
    e -= len;
  }
  return { ...DONE };
}

/** The arrival time that makes the delivery read as finished now (Skip). */
export function skippedArrival(nowMs: number, turns: number, reduceMotion: boolean): number {
  return nowMs - duoLengthMs(turns, reduceMotion) - 1;
}
