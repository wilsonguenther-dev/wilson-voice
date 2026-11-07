// yap-1106-news-prefs-onboarding — Yappy News interests, the pure half.
//
// One record (`news_prefs.rs` on the backend) is edited from three places: the
// onboarding step (only when the cloud option is connected), the one-time Home
// card (the day the cloud is connected, if onboarding never offered it), and
// Settings → Yappy News. Everything here is pure so it is testable without a
// webview; the three surfaces share `NewsInterests` for the picker itself.
import { invoke } from "@tauri-apps/api/core";

/** Where a save or dismiss came from (the backend logs it, nothing else). */
export type NewsSource = "onboarding" | "home" | "settings";

export type NewsOffer = "unseen" | "declined" | "accepted";

/** The editable part — `news_prefs::NewsPrefs`. */
export interface NewsPrefs {
  enabled: boolean;
  topics: string[];
  tickers: string[];
  custom: string[];
}

/** `news_prefs::NewsView`. */
export interface NewsView extends NewsPrefs {
  cloudConnected: boolean;
  active: boolean;
  offer: NewsOffer;
  onboardingStepDue: boolean;
  homeCardDue: boolean;
  cloudConnectedAtMs: number | null;
  lastRefreshMs: number | null;
}

/** The chips, in the order they show. Ids match `news_prefs::TOPIC_CHIPS`. */
export const TOPIC_CHIPS: readonly { id: string; label: string }[] = [
  { id: "developer", label: "Developer" },
  { id: "stocks", label: "Stocks" },
  { id: "business", label: "Business" },
  { id: "tech", label: "Tech" },
  { id: "science", label: "Science" },
  { id: "world", label: "World" },
  { id: "sports", label: "Sports" },
  { id: "culture", label: "Culture" },
];

/** Bounds, the same as the backend's (it re-checks every save). */
export const MAX_TICKERS = 12;
export const MAX_CUSTOM = 10;
export const MAX_TICKER_LEN = 10;
export const MIN_CUSTOM_LEN = 2;
export const MAX_CUSTOM_LEN = 60;

/** The one privacy sentence every surface shows (design page, section 5). */
export const NEWS_PRIVACY_LINE =
  "Headlines come from public news feeds; your topics are sent to them as searches; the cloud only sees the three headlines it explains. Nothing you dictate is ever used.";

export const EMPTY_PREFS: NewsPrefs = { enabled: true, topics: [], tickers: [], custom: [] };

export function prefsOf(v: NewsView): NewsPrefs {
  return { enabled: v.enabled, topics: [...v.topics], tickers: [...v.tickers], custom: [...v.custom] };
}

export function hasAnyTopic(p: NewsPrefs): boolean {
  return p.topics.length + p.tickers.length + p.custom.length > 0;
}

export function samePrefs(a: NewsPrefs, b: NewsPrefs): boolean {
  const eq = (x: string[], y: string[]) => x.length === y.length && x.every((v, i) => v === y[i]);
  return a.enabled === b.enabled && eq(a.topics, b.topics) && eq(a.tickers, b.tickers) && eq(a.custom, b.custom);
}

export function toggleTopic(p: NewsPrefs, id: string): NewsPrefs {
  const topics = p.topics.includes(id) ? p.topics.filter((t) => t !== id) : [...p.topics, id];
  return { ...p, topics };
}

/** `$nvda ` → `NVDA`; null when it cannot be a ticker. */
export function normalizeTicker(raw: string): string | null {
  const t = raw.trim().replace(/^\$+/, "").toUpperCase();
  return /^[A-Z][A-Z0-9.-]*$/.test(t) && t.length <= MAX_TICKER_LEN ? t : null;
}

export function normalizeCustom(raw: string): string | null {
  const t = raw.trim().split(/\s+/).filter(Boolean).join(" ");
  if (/[\u0000-\u001f\u007f]/.test(t)) return null;
  const n = [...t].length;
  return n >= MIN_CUSTOM_LEN && n <= MAX_CUSTOM_LEN ? t : null;
}

/** Adding a ticker or topic: the new prefs, or one sentence saying why not. */
export type AddResult = { ok: true; prefs: NewsPrefs } | { ok: false; error: string };

export function addTicker(p: NewsPrefs, raw: string): AddResult {
  const t = normalizeTicker(raw);
  if (!t) return { ok: false, error: `"${raw.trim()}" doesn't look like a ticker. Use letters like NVDA or BRK.B.` };
  if (p.tickers.includes(t)) return { ok: true, prefs: p };
  if (p.tickers.length >= MAX_TICKERS)
    return { ok: false, error: `Yappy can follow up to ${MAX_TICKERS} tickers. Remove one to add another.` };
  return { ok: true, prefs: { ...p, tickers: [...p.tickers, t] } };
}

export function addCustom(p: NewsPrefs, raw: string): AddResult {
  const t = normalizeCustom(raw);
  if (!t)
    return {
      ok: false,
      error: `Topics need ${MIN_CUSTOM_LEN} to ${MAX_CUSTOM_LEN} characters, like "Austin real estate".`,
    };
  if (p.custom.some((c) => c.toLowerCase() === t.toLowerCase())) return { ok: true, prefs: p };
  if (p.custom.length >= MAX_CUSTOM)
    return { ok: false, error: `Yappy can follow up to ${MAX_CUSTOM} of your own topics. Remove one to add another.` };
  return { ok: true, prefs: { ...p, custom: [...p.custom, t] } };
}

export function removeTicker(p: NewsPrefs, t: string): NewsPrefs {
  return { ...p, tickers: p.tickers.filter((x) => x !== t) };
}

export function removeCustom(p: NewsPrefs, t: string): NewsPrefs {
  return { ...p, custom: p.custom.filter((x) => x !== t) };
}

/** Human labels for the saved topics, chips first, then tickers, then custom. */
export function topicLabels(p: NewsPrefs): string[] {
  const chips = TOPIC_CHIPS.filter((c) => p.topics.includes(c.id)).map((c) => c.label);
  return [...chips, ...p.tickers, ...p.custom];
}

/** The on/off line Settings shows. */
export function statusLine(v: NewsView): string {
  if (!v.cloudConnected) return "Off. Yappy News needs the cloud option, so nothing is fetched.";
  if (!v.enabled) return "Off. Your topics are kept for when you turn it back on.";
  if (!hasAnyTopic(v)) return "Off until you pick at least one topic.";
  return "On.";
}

/** "Last refresh: …" — `now` is injectable for tests. */
export function lastRefreshLine(ms: number | null, now: number = Date.now()): string {
  if (ms == null || ms <= 0) return "Not refreshed yet.";
  const mins = Math.max(0, Math.round((now - ms) / 60_000));
  if (mins < 1) return "Last refreshed just now.";
  if (mins < 60) return `Last refreshed ${mins} min ago.`;
  const d = new Date(ms);
  const sameDay = new Date(now).toDateString() === d.toDateString();
  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return sameDay ? `Last refreshed today at ${time}.` : `Last refreshed ${d.toLocaleDateString()} at ${time}.`;
}

/** Whether onboarding shows the news step: the cloud is on and it was never answered. */
export function newsStepDue(v: NewsView | null): boolean {
  return !!v && v.cloudConnected && v.onboardingStepDue;
}

/** Whether Home shows the one-time card. */
export function homeCardDue(v: NewsView | null): boolean {
  return !!v && v.cloudConnected && v.homeCardDue;
}

export function loadNews(): Promise<NewsView> {
  return invoke<NewsView>("get_news_prefs");
}

export function saveNews(prefs: NewsPrefs, source: NewsSource): Promise<NewsView> {
  return invoke<NewsView>("set_news_prefs", { prefs, source });
}

export function dismissNews(source: NewsSource): Promise<NewsView> {
  return invoke<NewsView>("dismiss_news_offer", { source });
}
