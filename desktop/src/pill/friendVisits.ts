/**
 * yap-1106-friend-visits — friends visit the pill, with the approved quiet
 * rules (companions.html sections 01 + 04, pill.html section 04).
 *
 * The friends live in Home's studio; the pill is only where they VISIT, and
 * each one visits only when the app event that friend owns happens:
 *
 *   Tidy    the take had a spoken self-correction ("scratch that")
 *   Gulp    a word went into the Dictionary
 *   Shelly  the battery ran low
 *   Hoot    the meeting's notes are ready (during the meeting itself Hoot is
 *           at the tape machine — meetingScene.ts — which is not a visit)
 *   Cinder  Yappy News is ready, and only while the cloud is connected
 *   Yappy   a milestone — your 100th (and 1000th) take, your longest take
 *           of the week, a new streak record (yap-1106-main-character-switch,
 *           companions.html section 04) — only when someone else sits in the
 *           pill, because "Yappy is never gone": he works the studio
 *           typewriter full-time and visits for milestones and nothing else
 *
 * Ember, Sal and Mochi are room-only: they never visit. Whoever SITS in the
 * pill never visits it.
 *
 * The quiet rules, every one a pure function of a clock the caller passes in
 * (friendVisits.test.ts drives them with a fake clock):
 *
 *   - a visit lasts at most [`VISIT_MAX_MS`] (4 s), shorter for a heavy user;
 *   - each friend visits at most once per local day;
 *   - never while a meeting records or a call is live (`callLive`), the screen
 *     is shared or Do Not Disturb is on (Mochi's rule) — and a visit already on screen leaves the moment one
 *     of those starts;
 *   - one friend at a time;
 *   - from take [`QUIETER_AFTER_TAKES`] on, friends visit less: at most
 *     [`DAILY_VISITS_QUIETER`] visits a day in all, at least
 *     [`VISIT_GAP_QUIETER_MS`] apart; from take [`QUIETEST_AFTER_TAKES`] on,
 *     [`DAILY_VISITS_QUIETEST`] a day. "Takes" is the app's real total
 *     (`get_insights` -> `totalSessions`, via [`takeTracker`]), never a
 *     counter that starts at 0 on an existing install.
 *
 * Pure + deterministic: no timers, no events, no DOM, no Tauri.
 * `FriendVisit.tsx` feeds it what the app already knows and paints the result.
 */
import type { FriendId } from "../sprites/friends";
import { COMPANION_VISIT_CAP_MS } from "./meetingScene";
import type { ChatterMode } from "./chatter";

/** A pill visitor: any friend, or Yappy when someone else sits in the pill. */
export type VisitorId = FriendId | "yappy";

/** The app events a friend can visit for. */
export const FRIEND_EVENTS = [
  "backtrack-caught",
  "word-added",
  "battery-low",
  "meeting-notes-ready",
  "news-ready",
  "milestone",
  "longest-take-week",
  "streak-record",
] as const;
export type FriendEvent = (typeof FRIEND_EVENTS)[number];

/** Who owns each event. A friend appears only for its own. */
export const EVENT_OWNER: Readonly<Record<FriendEvent, VisitorId>> = {
  "backtrack-caught": "tidy",
  "word-added": "gulp",
  "battery-low": "shelly",
  "meeting-notes-ready": "hoot",
  "news-ready": "cinder",
  milestone: "yappy",
  "longest-take-week": "yappy",
  "streak-record": "yappy",
};

/** Yappy's milestones: the only events that bring him to a pill someone else sits in. */
export const YAPPY_MILESTONES: readonly FriendEvent[] = ["milestone", "longest-take-week", "streak-record"];

/** Room-only friends: they live in Home's studio and never visit the pill. */
export const ROOM_ONLY: readonly VisitorId[] = ["ember", "sal", "mochi"];

/** The longest any visit lasts (the companions page: 4 s). */
export const VISIT_MAX_MS = COMPANION_VISIT_CAP_MS;
/** A heavy user's visit is shorter. */
export const VISIT_QUIETER_MS = 3000;
/** From this many takes on, friends visit less. */
export const QUIETER_AFTER_TAKES = 50;
/** From this many takes on, friends visit least. */
export const QUIETEST_AFTER_TAKES = 200;
/** Visits a day, all friends together, from take 50 on. */
export const DAILY_VISITS_QUIETER = 2;
/** Visits a day, all friends together, from take 200 on. */
export const DAILY_VISITS_QUIETEST = 1;
/** From take 50 on, two visits are at least this far apart. */
export const VISIT_GAP_QUIETER_MS = 2 * 60 * 60 * 1000;

/** Rust-facing event names (the pill listens; any backend may raise them). */
export const FRIEND_EVENT = "friend_event";
/** `{ screenSharing?: boolean, doNotDisturb?: boolean }` — the quiet state. */
export const PILL_QUIET_EVENT = "pill_quiet";
/** The command a pill that mounted late asks once for the quiet state (pill_quiet.rs `current`). */
export const PILL_QUIET_STATE = "pill_quiet_state";

/** Everything the rules read. All of it is something the app already knows. */
export interface VisitContext {
  /** A meeting is recording, or a call is live. */
  meetingRecording: boolean;
  /** The screen is being shared or presented. */
  screenSharing: boolean;
  /** Do Not Disturb / a Focus is on. */
  doNotDisturb: boolean;
  /** Takes ever, for the 50 / 200 ladder. */
  takesSoFar: number;
  /** Who sits in the pill (the `pill_style` character id). */
  occupant: string;
  /** The cloud connection is on (Cinder's precondition). */
  cloudConnected: boolean;
  /**
   * yap-1106-chatter-setting — "Yappy talks" (Settings › Companion). Omitted
   * or "less": the 50 / 200 ladder above. "always": the ladder is off, every
   * friend keeps the full first-day rate. "milestones": only the milestone
   * visit (Yappy's 100th / 1000th take); every other friend stays home.
   */
  chatter?: ChatterMode;
}

/** What has already been shown. Survives a relaunch (one localStorage row). */
export interface VisitLedger {
  /** The local day `count` is for (YYYY-MM-DD). */
  day: string;
  /** Visits shown on `day`, all friends together. */
  count: number;
  /** The local day each visitor last visited. */
  lastDay: Partial<Record<VisitorId, string>>;
  /** When the last visit started (epoch ms), or null. */
  lastAtMs: number | null;
}

export const EMPTY_LEDGER: VisitLedger = { day: "", count: 0, lastDay: {}, lastAtMs: null };

export interface Visit {
  friend: VisitorId;
  event: FriendEvent;
  startMs: number;
  untilMs: number;
}

export type VisitSkip =
  | "unknown-event"
  | "not-own-event"
  | "occupant"
  | "cloud-off"
  | "meeting-recording"
  | "screen-sharing"
  | "do-not-disturb"
  | "visit-showing"
  | "already-today"
  | "daily-cap"
  | "too-soon"
  | "chatter-milestones";

/** The three quiet states (a recording meeting or live call, a shared screen, Do Not Disturb). */
export type QuietState = Pick<VisitContext, "meetingRecording" | "screenSharing" | "doNotDisturb">;

export type VisitDecision = { show: true; visit: Visit } | { show: false; reason: VisitSkip };

export const isFriendEvent = (v: unknown): v is FriendEvent =>
  typeof v === "string" && (FRIEND_EVENTS as readonly string[]).includes(v);

/** The local calendar day of `ms`, as YYYY-MM-DD. */
export function dayKey(ms: number): string {
  const d = new Date(Number.isFinite(ms) ? ms : 0);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

const takes = (n: number) => (Number.isFinite(n) && n > 0 ? Math.floor(n) : 0);

/**
 * yap-1106-chatter-setting — the take count the quieting ladder reads. With
 * "Yappy talks: always" the ladder never starts (0); otherwise the real count.
 */
export function quietingTakes(ctx: Pick<VisitContext, "takesSoFar" | "chatter">): number {
  return ctx.chatter === "always" ? 0 : takes(ctx.takesSoFar);
}

/** The reason everyone is silent right now, or null. Meeting outranks the rest. */
export function quietReason(ctx: QuietState): VisitSkip | null {
  if (ctx.meetingRecording) return "meeting-recording";
  if (ctx.screenSharing) return "screen-sharing";
  if (ctx.doNotDisturb) return "do-not-disturb";
  return null;
}

/** Visits a day, all friends together, for this many takes (Infinity = per-friend rule only). */
export function dailyVisitCap(takesSoFar: number): number {
  const t = takes(takesSoFar);
  if (t >= QUIETEST_AFTER_TAKES) return DAILY_VISITS_QUIETEST;
  if (t >= QUIETER_AFTER_TAKES) return DAILY_VISITS_QUIETER;
  return Infinity;
}

/** How long a visit lasts for this many takes. Never more than 4 s. */
export function visitDurationMs(takesSoFar: number): number {
  return takes(takesSoFar) >= QUIETER_AFTER_TAKES ? VISIT_QUIETER_MS : VISIT_MAX_MS;
}

/** Is `visit` on screen at `nowMs`? A quiet state ends it at once. */
export function visitShowing(visit: Visit | null, ctx: QuietState, nowMs: number): boolean {
  if (!visit) return false;
  if (quietReason(ctx)) return false;
  return nowMs >= visit.startMs && nowMs < visit.untilMs;
}

/** Milliseconds left on screen (0 once over). */
export function visitRemainingMs(visit: Visit | null, nowMs: number): number {
  if (!visit) return 0;
  return Math.max(0, Math.min(visit.untilMs - nowMs, visit.untilMs - visit.startMs));
}

/**
 * Should `event` bring its friend into the pill at `nowMs`? `requested` is the
 * friend a caller asked for, if any: anyone but the event's owner is refused,
 * so a friend can never appear for someone else's event.
 */
export function decideVisit(
  event: string,
  ctx: VisitContext,
  ledger: VisitLedger,
  nowMs: number,
  current: Visit | null = null,
  requested?: string,
): VisitDecision {
  if (!isFriendEvent(event)) return { show: false, reason: "unknown-event" };
  const friend = EVENT_OWNER[event];
  if (requested !== undefined && requested !== friend) return { show: false, reason: "not-own-event" };
  // Room-only friends own no event (EVENT_OWNER), so they can never get here.
  // yap-1106-chatter-setting — "only milestones": Yappy's milestone visits are the only ones
  // (checked before the occupant so the skip reason names the setting).
  if (ctx.chatter === "milestones" && !YAPPY_MILESTONES.includes(event)) return { show: false, reason: "chatter-milestones" };
  if (ctx.occupant === friend) return { show: false, reason: "occupant" };
  if (friend === "cinder" && !ctx.cloudConnected) return { show: false, reason: "cloud-off" };
  const quiet = quietReason(ctx);
  if (quiet) return { show: false, reason: quiet };
  if (visitShowing(current, ctx, nowMs)) return { show: false, reason: "visit-showing" };
  const today = dayKey(nowMs);
  if (ledger.lastDay[friend] === today) return { show: false, reason: "already-today" };
  // The gap is checked before the day's cap, so a heavy user's second event
  // inside the gap says "too-soon" and a later one that day says "daily-cap".
  // Either way it is skipped; only the reason in the log differs.
  const quietTakes = quietingTakes(ctx);
  if (
    quietTakes >= QUIETER_AFTER_TAKES &&
    ledger.lastAtMs !== null &&
    nowMs >= ledger.lastAtMs &&
    nowMs - ledger.lastAtMs < VISIT_GAP_QUIETER_MS
  ) {
    return { show: false, reason: "too-soon" };
  }
  const countToday = ledger.day === today ? ledger.count : 0;
  if (countToday >= dailyVisitCap(quietTakes)) return { show: false, reason: "daily-cap" };
  const ms = visitDurationMs(quietTakes);
  return { show: true, visit: { friend, event, startMs: nowMs, untilMs: nowMs + ms } };
}

/** The ledger after `visit` was shown. Rolls the day over. */
export function recordVisit(ledger: VisitLedger, visit: Visit): VisitLedger {
  const day = dayKey(visit.startMs);
  return {
    day,
    count: (ledger.day === day ? ledger.count : 0) + 1,
    lastDay: { ...ledger.lastDay, [visit.friend]: day },
    lastAtMs: visit.startMs,
  };
}

const DAY_RE = /^\d{4}-\d{2}-\d{2}$/;
const VISITORS: readonly string[] = [...new Set(Object.values(EVENT_OWNER))];

/** Parse the stored ledger; anything malformed is an empty ledger, never a throw. */
export function parseLedger(raw: unknown): VisitLedger {
  if (typeof raw !== "string" || !raw) return EMPTY_LEDGER;
  let v: unknown;
  try { v = JSON.parse(raw); } catch { return EMPTY_LEDGER; }
  if (!v || typeof v !== "object") return EMPTY_LEDGER;
  const o = v as Record<string, unknown>;
  const day = typeof o.day === "string" && DAY_RE.test(o.day) ? o.day : "";
  const count = typeof o.count === "number" && Number.isFinite(o.count) && o.count > 0 ? Math.floor(o.count) : 0;
  const lastAtMs = typeof o.lastAtMs === "number" && Number.isFinite(o.lastAtMs) ? o.lastAtMs : null;
  const lastDay: Partial<Record<VisitorId, string>> = {};
  if (o.lastDay && typeof o.lastDay === "object") {
    for (const [k, d] of Object.entries(o.lastDay as Record<string, unknown>)) {
      if (VISITORS.includes(k) && typeof d === "string" && DAY_RE.test(d)) lastDay[k as VisitorId] = d;
    }
  }
  return { day, count: day ? count : 0, lastDay, lastAtMs };
}

export const serializeLedger = (l: VisitLedger): string => JSON.stringify(l);

/** The quiet state from a `pill_quiet` payload; unknown fields read as off. */
export function parseQuietState(raw: unknown): { screenSharing: boolean; doNotDisturb: boolean } {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  return { screenSharing: o.screenSharing === true, doNotDisturb: o.doNotDisturb === true };
}

/**
 * The call-offer phases (`call_offer.rs`, mirrored in callOfferModel.ts) in
 * which a call app is live: a call is where screens get shared, so a live
 * call is as quiet as a recording meeting even before anyone presses Record.
 * `idle` and the after-call `end_prompt` are not live.
 */
export const CALL_LIVE_PHASES: readonly string[] = ["offering", "chip", "recording"];

/** Is a call live, from a `call_offer` view payload? Anything malformed reads as not live. */
export function callLive(view: unknown): boolean {
  const o = view && typeof view === "object" ? (view as Record<string, unknown>) : null;
  return typeof o?.phase === "string" && CALL_LIVE_PHASES.includes(o.phase);
}

/** The milestone a take count reaches exactly, or null (Yappy's visit). */
export const MILESTONE_TAKES: readonly number[] = [100, 1000];
export const milestoneReached = (takesSoFar: number): boolean => MILESTONE_TAKES.includes(takes(takesSoFar));

/**
 * An EDGE, not a level: did the take count go from `prev` to `next` across a
 * milestone (prev < m <= next)? A re-read that lands on the same total never
 * repeats Yappy's visit, and a jump (two takes landing between reads) still
 * fires once.
 */
export function milestoneCrossed(prev: number, next: number): boolean {
  const a = takes(prev);
  const b = takes(next);
  return MILESTONE_TAKES.some((m) => a < m && m <= b);
}

/** Where the take count came from: the app's real total, or the pill's own guess. */
export type TakesSource = "insights" | "fallback";

/**
 * The real total of takes in a `get_insights` payload (`totalSessions` — the
 * number Home's stats line and the sidebar badge show), or null when the
 * payload carries none.
 */
export function insightsTotal(raw: unknown): number | null {
  if (!raw || typeof raw !== "object") return null;
  const n = (raw as Record<string, unknown>).totalSessions;
  return typeof n === "number" && Number.isFinite(n) && n >= 0 ? Math.floor(n) : null;
}

/** The pill's running take count, seeded from the app's real total. */
export interface TakeTracker {
  /** Read the real total once (on mount). Resolves to where the count came from. */
  seed(): Promise<TakesSource>;
  /** A `transcript` landed. Resolves true only when the count crossed 100 or 1000 just now. */
  transcript(): Promise<boolean>;
  /** Takes so far, for the 50 / 200 ladder. */
  takes(): number;
  source(): TakesSource;
}

/**
 * `readTotal` is `invoke("get_insights")`; `fallbackTakes` is the pill's old
 * local counter, used only when the real total cannot be read (with one warn
 * line saying so). Calls are serialised, so a `transcript` that lands before
 * the seed resolves counts on top of the real total, never on top of 0.
 *
 * On each `transcript` the real total is read again and the count becomes
 * max(real, previous + 1): the take that just landed counts even if the read
 * raced its save, and the count never goes backwards inside one pill session.
 * Switching from the fallback to the real total re-seeds silently (no
 * milestone for a jump from a guess).
 */
export function takeTracker(
  readTotal: () => Promise<unknown>,
  fallbackTakes: () => number,
  warn: (line: string) => void = (line) => console.warn(line),
): TakeTracker {
  let count = 0;
  /** The highest total seen; the milestone edge is measured against it. */
  let peak = 0;
  let src: TakesSource = "fallback";
  let queue: Promise<unknown> = Promise.resolve();
  const read = async (): Promise<number | null> => {
    try {
      const n = insightsTotal(await readTotal());
      if (n === null) warn("pill friend_visit takes_read_failed reason=no-total");
      return n;
    } catch (e) {
      warn(`pill friend_visit takes_read_failed reason=${e instanceof Error ? e.name : "unknown"}`);
      return null;
    }
  };
  const run = <T>(job: () => Promise<T>): Promise<T> => {
    const next = queue.then(job, job);
    queue = next.catch(() => undefined);
    return next;
  };
  return {
    seed: () =>
      run(async () => {
        const n = await read();
        if (n !== null) {
          count = n;
          peak = n;
          src = "insights";
        } else {
          count = takes(fallbackTakes());
          peak = count;
          src = "fallback";
          warn("pill friend_visit takes_fallback reason=insights-unavailable");
        }
        return src;
      }),
    transcript: () =>
      run(async () => {
        const n = await read();
        if (n !== null && src === "fallback") {
          // The first real read after a fallback replaces the guess; a guess
          // never decides a milestone.
          count = n;
          peak = n;
          src = "insights";
          return false;
        }
        // The app saves the take before it emits `transcript`, so the re-read
        // is the real total (a retry that adds no take adds nothing here).
        // Only when the read fails does the pill guess one more.
        count = n ?? count + 1;
        // An edge against the highest total seen, so a re-read (or a total
        // that dips after a delete and climbs back) never repeats it.
        const crossed = milestoneCrossed(peak, count);
        peak = Math.max(peak, count);
        return crossed;
      }),
    takes: () => count,
    source: () => src,
  };
}

/** One structured log line per decision (no transcript text, no paths). */
export function visitLogLine(event: string, d: VisitDecision): string {
  const ev = String(event).replace(/[^a-z0-9-]/gi, "").slice(0, 32) || "none";
  return d.show
    ? `pill friend_visit shown friend=${d.visit.friend} event=${ev} ms=${d.visit.untilMs - d.visit.startMs}`
    : `pill friend_visit skipped event=${ev} reason=${d.reason}`;
}

/**
 * DEV / matrix only: `float.html?friend=<id>` forces a visit on screen so
 * `scripts/pill-matrix.mjs` can photograph it. Anything that is not a pill
 * visitor (room-only friends included) forces nothing.
 */
export function forcedFriendVisit(search: string): VisitorId | null {
  const id = new URLSearchParams(search).get("friend");
  if (!id || !VISITORS.includes(id)) return null;
  return id as VisitorId;
}

/* ── yap-1106-main-character-switch — Yappy's milestones ───────────────── */

/**
 * What Yappy keeps count of to know a milestone when it happens. Survives a
 * relaunch (one localStorage row next to the visit ledger). Counts and
 * seconds only: never a transcript, never a path.
 */
export interface TakeStats {
  /** The local Monday (YYYY-MM-DD) `weekTakes` / `weekLongestS` are for. */
  week: string;
  /** Takes so far this week. */
  weekTakes: number;
  /** The longest take this week, in seconds of speech. */
  weekLongestS: number;
  /** Days in a row with at least one take, ending on `lastDay`. */
  streak: number;
  /** The longest streak ever. */
  bestStreak: number;
  /** The local day of the last take (YYYY-MM-DD), or "". */
  lastDay: string;
}

export const EMPTY_TAKE_STATS: TakeStats = { week: "", weekTakes: 0, weekLongestS: 0, streak: 0, bestStreak: 0, lastDay: "" };

/** A "longest take of the week" counts only after this many takes that week (so Monday's second take is not one). */
export const LONGEST_WEEK_MIN_TAKES = 5;
/** …and only for a take of at least this many seconds of speech. */
export const LONGEST_TAKE_MIN_S = 10;
/** A streak record counts from this many days in a row (a first two-day run is not one). */
export const STREAK_RECORD_MIN_DAYS = 3;

/** The local Monday of the week `ms` falls in, as YYYY-MM-DD. */
export function weekKey(ms: number): string {
  const d = new Date(Number.isFinite(ms) ? ms : 0);
  const back = (d.getDay() + 6) % 7; // Monday = 0
  return dayKey(new Date(d.getFullYear(), d.getMonth(), d.getDate() - back, 12).getTime());
}

/** The local day before the day of `ms` (calendar arithmetic, safe across a DST change). */
function dayBefore(ms: number): string {
  const d = new Date(Number.isFinite(ms) ? ms : 0);
  return dayKey(new Date(d.getFullYear(), d.getMonth(), d.getDate() - 1, 12).getTime());
}

export interface TakeNote {
  stats: TakeStats;
  /** The milestone this take reached (Yappy's event), or null. */
  milestone: FriendEvent | null;
}

/**
 * Count one finished take and say which of Yappy's milestones it reached, if
 * any: the 100th / 1000th take outranks a new streak record, which outranks
 * the longest take of the week. `speechS` is the take's seconds of speech
 * (null when the backend did not say). Pure: the caller passes the clock.
 */
export function noteTake(
  stats: TakeStats,
  takesSoFar: number,
  speechS: number | null,
  nowMs: number,
  /**
   * The take tracker's edge (`TakeTracker.transcript()`): true only when the
   * real total crossed 100 or 1000 just now. When given it decides the count
   * milestone, so a re-read that lands on the same total never repeats it.
   */
  countCrossed?: boolean,
): TakeNote {
  const today = dayKey(nowMs);
  const week = weekKey(nowMs);
  const sameWeek = stats.week === week;
  const weekTakesBefore = sameWeek ? stats.weekTakes : 0;
  const weekLongestBefore = sameWeek ? stats.weekLongestS : 0;
  const s = typeof speechS === "number" && Number.isFinite(speechS) && speechS > 0 ? speechS : 0;
  const streak = stats.lastDay === today ? Math.max(1, stats.streak) : stats.lastDay === dayBefore(nowMs) ? stats.streak + 1 : 1;
  const next: TakeStats = {
    week,
    weekTakes: weekTakesBefore + 1,
    weekLongestS: Math.max(weekLongestBefore, s),
    streak,
    bestStreak: Math.max(stats.bestStreak, streak),
    lastDay: today,
  };
  let milestone: FriendEvent | null = null;
  if (countCrossed ?? milestoneReached(takesSoFar)) milestone = "milestone";
  else if (streak > stats.bestStreak && streak >= STREAK_RECORD_MIN_DAYS) milestone = "streak-record";
  else if (weekTakesBefore >= LONGEST_WEEK_MIN_TAKES && s >= LONGEST_TAKE_MIN_S && s > weekLongestBefore) milestone = "longest-take-week";
  return { stats: next, milestone };
}

const whole = (v: unknown): number => (typeof v === "number" && Number.isFinite(v) && v > 0 ? Math.floor(v) : 0);

/** Parse the stored take stats; anything malformed is empty, never a throw. */
export function parseTakeStats(raw: unknown): TakeStats {
  if (typeof raw !== "string" || !raw) return EMPTY_TAKE_STATS;
  let v: unknown;
  try { v = JSON.parse(raw); } catch { return EMPTY_TAKE_STATS; }
  if (!v || typeof v !== "object") return EMPTY_TAKE_STATS;
  const o = v as Record<string, unknown>;
  const week = typeof o.week === "string" && DAY_RE.test(o.week) ? o.week : "";
  const lastDay = typeof o.lastDay === "string" && DAY_RE.test(o.lastDay) ? o.lastDay : "";
  const longest = typeof o.weekLongestS === "number" && Number.isFinite(o.weekLongestS) && o.weekLongestS > 0 ? o.weekLongestS : 0;
  const streak = lastDay ? whole(o.streak) : 0;
  return {
    week,
    weekTakes: week ? whole(o.weekTakes) : 0,
    weekLongestS: week ? longest : 0,
    streak,
    bestStreak: Math.max(whole(o.bestStreak), streak),
    lastDay,
  };
}

export const serializeTakeStats = (t: TakeStats): string => JSON.stringify(t);

/** The seconds of speech in a `transcript` payload (camelCase or snake_case), or null. */
export function takeSpeechSeconds(payload: unknown): number | null {
  const o = payload && typeof payload === "object" ? (payload as Record<string, unknown>) : null;
  const v = o?.speechSeconds ?? o?.speech_seconds;
  return typeof v === "number" && Number.isFinite(v) && v > 0 ? v : null;
}

/** What Yappy says when he visits for a milestone (companions.html: "that's a hundred! — Yappy"). */
export function milestoneLine(event: FriendEvent, takesSoFar: number): string | null {
  if (event === "milestone") return takes(takesSoFar) >= 1000 ? "a thousand takes! — Yappy" : "that's a hundred! — Yappy";
  if (event === "longest-take-week") return "longest take this week! — Yappy";
  if (event === "streak-record") return "new streak record! — Yappy";
  return null;
}
