/**
 * yap-1106-insights-redesign — the pure half of the Insights screen.
 *
 * The screen leads with three headline numbers (words this week, time saved,
 * hours in meetings) and ONE plain sentence, then the charts, then a meetings
 * section. Everything that decides a number or a word lives here so it is
 * tested without a DOM: the view only lays it out.
 */
import type { Insights, MeetingStats } from "../appTypes";

/** Typing speed the "time saved" figure compares against. A typical adult
 *  types around 40 words a minute; stating the assumption keeps the number honest. */
export const TYPING_WPM = 40;

/** How many people the meetings section lists. */
export const TOP_PEOPLE_LIMIT = 5;

/** One person's talk time across meetings. Filled by the voiceprints work
 *  (backlog item 8); absent on a backend that does not name speakers yet. */
export interface PersonTalkTime {
  name: string;
  seconds: number;
  meetings: number;
}

export type ChartState = "loading" | "error" | "empty" | "ready";

/** Per-chart state: an error beats loading beats empty. */
export function chartState(opts: { loading: boolean; error?: string | null; hasData: boolean }): ChartState {
  if (opts.error) return "error";
  if (opts.hasData) return "ready";
  if (opts.loading) return "loading";
  return "empty";
}

/** Copy every empty chart shares (UI spec §3.4, Empty). */
export const CHART_EMPTY_COPY = "Not enough takes yet; this fills in after a few days.";
/** Copy every failed chart shares (UI spec §3.4, Error). */
export const CHART_ERROR_COPY = "Couldn't load this chart.";

/** Sum of the last seven days. */
export function wordsThisWeek(last7: readonly { words: number }[] | undefined): number {
  return (last7 ?? []).reduce((n, d) => n + Math.max(0, d.words || 0), 0);
}

/**
 * Minutes saved by speaking instead of typing: the minutes the same words take
 * at {@link TYPING_WPM}, minus the minutes actually spent speaking. Measured
 * speech time is used when the backend has it; otherwise the average speaking
 * rate stands in. Never negative.
 */
export function timeSavedMinutes(
  totalWords: number,
  speechSecondsTotal: number | undefined,
  avgWpm: number,
): number {
  const words = Math.max(0, totalWords || 0);
  if (words === 0) return 0;
  const typing = words / TYPING_WPM;
  const speaking =
    speechSecondsTotal && speechSecondsTotal > 0
      ? speechSecondsTotal / 60
      : words / Math.max(avgWpm || 0, TYPING_WPM);
  return Math.max(0, typing - speaking);
}

/** Minutes as a person says them: "under a minute", "12 min", "2 h 5 min". */
export function formatMinutes(minutes: number): string {
  const m = Math.round(Math.max(0, minutes));
  if (m < 1) return "under a minute";
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest === 0 ? `${h} h` : `${h} h ${rest} min`;
}

/** Hours in meetings, one decimal under ten hours: "0.5", "3.2", "14". */
export function meetingHours(meetings: MeetingStats | undefined | null): string {
  const h = Math.max(0, meetings?.totalSeconds ?? 0) / 3600;
  if (h === 0) return "0";
  return h < 10 ? h.toFixed(1) : Math.round(h).toLocaleString();
}

export interface Headline {
  key: "week" | "saved" | "meetings";
  value: string;
  label: string;
  /** The assumption behind the number, shown as its tooltip. */
  hint: string;
}

/** The three headline numbers. `null` insights (still loading) shows "—". */
export function headlineNumbers(insights: Insights | null | undefined): Headline[] {
  if (!insights)
    return [
      { key: "week", value: "—", label: "words this week", hint: "Words dictated in the last seven days." },
      { key: "saved", value: "—", label: "time saved", hint: `Compared with typing at ${TYPING_WPM} words a minute.` },
      { key: "meetings", value: "—", label: "hours in meetings", hint: "Time recorded in meetings." },
    ];
  return [
    {
      key: "week",
      value: wordsThisWeek(insights.wordsLast7).toLocaleString(),
      label: "words this week",
      hint: "Words dictated in the last seven days.",
    },
    {
      key: "saved",
      value: formatMinutes(timeSavedMinutes(insights.totalWords, insights.speechSecondsTotal, insights.avgWpm)),
      label: "time saved",
      hint: `Compared with typing the same ${insights.totalWords.toLocaleString()} words at ${TYPING_WPM} words a minute.`,
    },
    {
      key: "meetings",
      value: meetingHours(insights.meetings),
      label: "hours in meetings",
      hint: "Time recorded in meetings, all time.",
    },
  ];
}

/**
 * The one insight sentence under the numbers. Picks the most useful true
 * thing, in order: a real week-over-week change, a running streak, where the
 * words go, the running total. Never a claim the data does not support.
 */
export function insightSentence(
  insights: Insights | null | undefined,
  daily: readonly { date: string; words: number }[],
): string | null {
  if (!insights) return null;
  const week = wordsThisWeek(insights.wordsLast7);
  const prior = daily.length >= 14 ? wordsThisWeek(daily.slice(-14, -7)) : 0;
  if (week > 0 && prior > 0) {
    const change = Math.round(((week - prior) / prior) * 100);
    if (change >= 10) return `You said ${change}% more this week than the week before.`;
    if (change <= -10) return `You said ${Math.abs(change)}% less this week than the week before.`;
  }
  if (insights.streakDays >= 3) {
    return insights.streakDays >= insights.longestStreak
      ? `You have dictated ${insights.streakDays} days in a row, your longest run yet.`
      : `You have dictated ${insights.streakDays} days in a row; your best is ${insights.longestStreak}.`;
  }
  const top = insights.topApps[0];
  if (top && insights.totalWords > 0 && top.words / insights.totalWords >= 0.4) {
    return `Most of your words go to ${top.app}: ${Math.round((top.words / insights.totalWords) * 100)}% of everything you have said.`;
  }
  if (insights.totalWords > 0) {
    return `You have dictated ${insights.totalWords.toLocaleString()} words with Yap so far.`;
  }
  const m = insights.meetings;
  if (m && m.totalMeetings > 0) {
    return `You have recorded ${m.totalMeetings} ${m.totalMeetings === 1 ? "meeting" : "meetings"} with Yap so far.`;
  }
  return null;
}

/** The people who talked most, longest first, at most {@link TOP_PEOPLE_LIMIT}.
 *  `null` means the backend does not name speakers yet (not the same as nobody). */
export function topPeople(meetings: MeetingStats | undefined | null): PersonTalkTime[] | null {
  const people = meetings?.topPeople;
  if (!Array.isArray(people)) return null;
  return people
    .filter((p) => p && typeof p.name === "string" && p.name.trim() !== "" && p.seconds > 0)
    .slice()
    .sort((a, b) => b.seconds - a.seconds || a.name.localeCompare(b.name))
    .slice(0, TOP_PEOPLE_LIMIT);
}

/** Arrow-key movement through a chart's points. Returns the new index, or
 *  `null` when the key is not a chart key (let the browser have it). */
export function nextPointIndex(current: number, key: string, length: number): number | null {
  if (length <= 0) return null;
  const at = current < 0 ? -1 : Math.min(current, length - 1);
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return at < 0 ? 0 : Math.min(length - 1, at + 1);
    case "ArrowLeft":
    case "ArrowUp":
      return at < 0 ? length - 1 : Math.max(0, at - 1);
    case "Home":
      return 0;
    case "End":
      return length - 1;
    default:
      return null;
  }
}

/** What a screen reader hears on a point: "Tue, Nov 4, 1,204 words". */
export function pointAnnouncement(label: string, words: number, unit = "words"): string {
  return `${label}, ${words.toLocaleString()} ${words === 1 && unit === "words" ? "word" : unit}`;
}

/**
 * One structured telemetry line, `ui <event> key=value` (UI spec, Telemetry).
 * Values are short machine strings or numbers, never dictated text.
 */
export function uiLogLine(event: string, fields: Record<string, string | number | boolean> = {}): string {
  const kv = Object.entries(fields)
    .map(([k, v]) => `${k}=${String(v).replace(/\s+/g, "_")}`)
    .join(" ");
  return kv ? `ui ${event} ${kv}` : `ui ${event}`;
}

export function uiLog(event: string, fields?: Record<string, string | number | boolean>): void {
  try {
    console.info(uiLogLine(event, fields));
  } catch {
    /* logging never breaks a screen */
  }
}
