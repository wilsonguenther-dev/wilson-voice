// yap-1106-news-home-briefing — the Home briefing's pure half: which stories
// make the card (three at most), when a refresh replaces the briefing (only
// when something new ranks higher), how each story names its source, how a
// quote line reads (always "delayed"), and the day-scoped store. No I/O here
// except the Storage passed in, so the tests drive it directly.
import type { NewsCadence } from "./newsPrefs";

/** The card shows three stories at most (news design page, section 2). */
export const BRIEFING_MAX = 3;
/** While Home is open in market hours, tickers are checked this often. */
export const QUOTE_CHECK_MS = 30 * 60_000;
export const BRIEFING_STORE_KEY = "yap.news.briefing.v1";
export const BRIEFING_HIDDEN_KEY = "yap.news.briefingHiddenDay";

/** `news::rank::RankedStory` as the backend serialises it (Story flattened). */
export interface CachedStory {
  source: string;
  topic: string;
  title: string;
  url: string;
  outlet?: string | null;
  outlets?: number;
  points?: number | null;
  score: number;
  urlKey: string;
}

/** `news::explain::ExplainedLine`. */
export interface ExplainedLine {
  urlKey: string;
  url: string;
  headline: string;
  line: string;
  explained: boolean;
}

/** `news::feeds::Quote` (only the movers reach Home). */
export interface QuoteMover {
  symbol: string;
  price: number;
  changePct: number;
  delayed: boolean;
}

/** `news::briefing::QuotesOutcome`. */
export interface QuotesOutcome {
  ran: boolean;
  skipped: string | null;
  movers: QuoteMover[];
  checked: number;
}

export interface BriefingStory {
  urlKey: string;
  url: string;
  headline: string;
  /** Yappy's one line, or "" when the headline stands alone. */
  line: string;
  source: string;
  topic: string;
  outlet: string | null;
  outlets: number;
  points: number | null;
  score: number;
}

export interface Briefing {
  id: string;
  deliveredMs: number;
  /** The local day it was delivered (YYYY-MM-DD): the cache lasts the day. */
  day: string;
  stories: BriefingStory[];
}

export function localDay(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function isStory(v: unknown): v is CachedStory {
  if (!v || typeof v !== "object") return false;
  const s = v as Record<string, unknown>;
  return (
    typeof s.title === "string" &&
    typeof s.url === "string" &&
    typeof s.urlKey === "string" &&
    typeof s.score === "number" &&
    Number.isFinite(s.score)
  );
}

/** The best stories, best first, three at most; anything malformed is dropped. */
export function topStories(raw: unknown): CachedStory[] {
  if (!Array.isArray(raw)) return [];
  return raw
    .filter(isStory)
    .slice()
    .sort((a, b) => b.score - a.score)
    .slice(0, BRIEFING_MAX);
}

/**
 * Does a fresh refresh earn a new briefing? Yes when there is none yet, or
 * when a story not already on the card ranks above the lowest one on it (the
 * afternoon briefing comes "only if something new ranks above the morning
 * stories"). A refresh with nothing new keeps the current card.
 */
export function ranksHigher(current: Briefing | null, fresh: readonly CachedStory[]): boolean {
  const top = fresh.slice(0, BRIEFING_MAX);
  if (top.length === 0) return false;
  if (!current || current.stories.length === 0) return true;
  const shown = new Set(current.stories.map((s) => s.urlKey));
  const fresher = top.filter((s) => !shown.has(s.urlKey));
  if (fresher.length === 0) return false;
  if (current.stories.length < BRIEFING_MAX) return true;
  const floor = Math.min(...current.stories.map((s) => s.score));
  return fresher.some((s) => s.score > floor);
}

/** The card's stories: Yappy's line where the explain step gave one. */
export function buildBriefing(stories: readonly CachedStory[], lines: readonly ExplainedLine[] | null, now: number): Briefing {
  const byKey = new Map((lines ?? []).map((l) => [l.urlKey, l]));
  const picked = stories.slice(0, BRIEFING_MAX).map((s): BriefingStory => {
    const l = byKey.get(s.urlKey);
    return {
      urlKey: s.urlKey,
      url: s.url,
      headline: s.title,
      line: l && l.explained ? l.line : "",
      source: s.source,
      topic: s.topic,
      outlet: s.outlet ?? null,
      outlets: typeof s.outlets === "number" && s.outlets > 0 ? s.outlets : 1,
      points: typeof s.points === "number" ? s.points : null,
      score: s.score,
    };
  });
  return { id: `b${now}`, deliveredMs: now, day: localDay(now), stories: picked };
}

/** Is this story about the markets (a followed ticker or the Stocks chip)? */
export function isMarketStory(s: Pick<BriefingStory, "topic" | "source">, tickers: readonly string[]): boolean {
  return s.source === "quotes" || s.topic === "stocks" || tickers.includes(s.topic.toUpperCase());
}

/** The short topic tag on each story, as drawn on the design page. */
export function topicTag(s: Pick<BriefingStory, "topic" | "source">, tickers: readonly string[]): string {
  if (isMarketStory(s, tickers)) return "STOCK";
  const known: Record<string, string> = {
    developer: "DEV", business: "BIZ", tech: "TECH", science: "SCI", world: "WORLD", sports: "SPORTS", culture: "CULTURE",
  };
  return known[s.topic] ?? "MORE";
}

/** Where the story came from; market stories say their quotes are delayed. */
export function sourceLabel(s: BriefingStory, tickers: readonly string[]): string {
  let base: string;
  if (s.source === "hacker_news") base = s.points != null ? `Hacker News · ${s.points} points` : "Hacker News";
  else if (s.source === "quotes") base = "Quote feed";
  else if (s.outlets > 1) base = `Google News · ${s.outlets} outlets`;
  else base = s.outlet ? s.outlet : "Google News";
  return isMarketStory(s, tickers) ? `${base} · quotes delayed` : base;
}

/** One mover: "NVDA up 3.1% · delayed". Every quote is marked delayed. */
export function quoteLine(q: QuoteMover): string {
  const pct = Math.abs(q.changePct).toFixed(1);
  return `${q.symbol} ${q.changePct >= 0 ? "up" : "down"} ${pct}% · delayed`;
}

const COUNT_WORDS = ["none", "one", "two", "three"];

/** The pair's lines in the card header: Cinder names it, Yappy explains. */
export function pairLines(b: Briefing, tickers: readonly string[]): { cinder: string; yappy: string } {
  const n = b.stories.length;
  const market = b.stories.some((s) => isMarketStory(s, tickers));
  const count = COUNT_WORDS[n] ?? String(n);
  const cinder = `${count} today${market ? ", one about your stocks" : ""}!`;
  const yappy = b.stories.some((s) => s.line) ? "on it. short version below." : "headlines below, links to every story.";
  return { cinder, yappy };
}

export function cadenceLine(c: NewsCadence): string {
  switch (c) {
    case "every_few_hours":
      return "Updated every few hours";
    case "on_home_open":
      return "Updated when you open Home";
    case "on_tap":
      return "Updated when you tap refresh";
    default:
      return "Updated twice a day";
  }
}

/** Is a fresh briefing a NEW one (Cinder flies), or the one already shown? */
export function isNewBriefing(prev: Briefing | null, next: Briefing | null): boolean {
  return !!next && next.stories.length > 0 && (!prev || prev.id !== next.id);
}

/** The front-end half of the quiet rules (the backend checks them again). */
export function quietNow(q: { meetingRecording: boolean; screenSharing: boolean; doNotDisturb: boolean }): boolean {
  return q.meetingRecording || q.screenSharing || q.doNotDisturb;
}

type KV = Pick<Storage, "getItem" | "setItem">;

/** Today's briefing, or null (none saved, unreadable, or from another day). */
export function loadStoredBriefing(store: KV | null, now: number): Briefing | null {
  try {
    const raw = store?.getItem(BRIEFING_STORE_KEY);
    if (!raw) return null;
    const b = JSON.parse(raw) as Briefing;
    if (!b || typeof b.id !== "string" || !Array.isArray(b.stories) || b.day !== localDay(now)) return null;
    return { ...b, stories: b.stories.slice(0, BRIEFING_MAX) };
  } catch {
    return null;
  }
}

export function saveStoredBriefing(store: KV | null, b: Briefing): void {
  try {
    store?.setItem(BRIEFING_STORE_KEY, JSON.stringify(b));
  } catch {
    console.warn("news briefing store_failed");
  }
}

/** "Hide for today": hidden until the next local day. */
export function hiddenToday(store: KV | null, now: number): boolean {
  try {
    return store?.getItem(BRIEFING_HIDDEN_KEY) === localDay(now);
  } catch {
    return false;
  }
}

export function hideToday(store: KV | null, now: number): void {
  try {
    store?.setItem(BRIEFING_HIDDEN_KEY, localDay(now));
  } catch {
    console.warn("news briefing hide_store_failed");
  }
}
