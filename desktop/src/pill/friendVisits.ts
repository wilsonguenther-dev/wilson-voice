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
 *   Yappy   a milestone (100th / 1000th take) — only when someone else sits
 *           in the pill, because "Yappy is never gone"
 *
 * Ember, Sal and Mochi are room-only: they never visit. Whoever SITS in the
 * pill never visits it.
 *
 * The quiet rules, every one a pure function of a clock the caller passes in
 * (friendVisits.test.ts drives them with a fake clock):
 *
 *   - a visit lasts at most [`VISIT_MAX_MS`] (4 s), shorter for a heavy user;
 *   - each friend visits at most once per local day;
 *   - never while a meeting records, the screen is shared or Do Not Disturb is
 *     on (Mochi's rule) — and a visit already on screen leaves the moment one
 *     of those starts;
 *   - one friend at a time;
 *   - from take [`QUIETER_AFTER_TAKES`] on, friends visit less: at most
 *     [`DAILY_VISITS_QUIETER`] visits a day in all, at least
 *     [`VISIT_GAP_QUIETER_MS`] apart; from take [`QUIETEST_AFTER_TAKES`] on,
 *     [`DAILY_VISITS_QUIETEST`] a day.
 *
 * Pure + deterministic: no timers, no events, no DOM, no Tauri.
 * `FriendVisit.tsx` feeds it what the app already knows and paints the result.
 */
import type { FriendId } from "../sprites/friends";
import { COMPANION_VISIT_CAP_MS } from "./meetingScene";

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
};

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

/** Everything the rules read. All of it is something the app already knows. */
export interface VisitContext {
  /** A meeting is recording. */
  meeting: boolean;
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
  | "room-only"
  | "occupant"
  | "cloud-off"
  | "meeting"
  | "screen-sharing"
  | "do-not-disturb"
  | "visit-showing"
  | "already-today"
  | "daily-cap"
  | "too-soon";

export type VisitDecision = { show: true; visit: Visit } | { show: false; reason: VisitSkip };

export const isFriendEvent = (v: unknown): v is FriendEvent =>
  typeof v === "string" && (FRIEND_EVENTS as readonly string[]).includes(v);

/** The local calendar day of `ms`, as YYYY-MM-DD. */
export function dayKey(ms: number): string {
  const d = new Date(Number.isFinite(ms) ? ms : 0);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

const takes = (n: number) => (Number.isFinite(n) && n > 0 ? Math.floor(n) : 0);

/** The reason everyone is silent right now, or null. Meeting outranks the rest. */
export function quietReason(ctx: Pick<VisitContext, "meeting" | "screenSharing" | "doNotDisturb">): VisitSkip | null {
  if (ctx.meeting) return "meeting";
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
export function visitShowing(visit: Visit | null, ctx: Pick<VisitContext, "meeting" | "screenSharing" | "doNotDisturb">, nowMs: number): boolean {
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
  if (ROOM_ONLY.includes(friend)) return { show: false, reason: "room-only" };
  if (ctx.occupant === friend) return { show: false, reason: "occupant" };
  if (friend === "cinder" && !ctx.cloudConnected) return { show: false, reason: "cloud-off" };
  const quiet = quietReason(ctx);
  if (quiet) return { show: false, reason: quiet };
  if (visitShowing(current, ctx, nowMs)) return { show: false, reason: "visit-showing" };
  const today = dayKey(nowMs);
  if (ledger.lastDay[friend] === today) return { show: false, reason: "already-today" };
  const countToday = ledger.day === today ? ledger.count : 0;
  if (countToday >= dailyVisitCap(ctx.takesSoFar)) return { show: false, reason: "daily-cap" };
  if (
    takes(ctx.takesSoFar) >= QUIETER_AFTER_TAKES &&
    ledger.lastAtMs !== null &&
    nowMs >= ledger.lastAtMs &&
    nowMs - ledger.lastAtMs < VISIT_GAP_QUIETER_MS
  ) {
    return { show: false, reason: "too-soon" };
  }
  const ms = visitDurationMs(ctx.takesSoFar);
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

/** The milestone a take count reaches exactly, or null (Yappy's visit). */
export const MILESTONE_TAKES: readonly number[] = [100, 1000];
export const milestoneReached = (takesSoFar: number): boolean => MILESTONE_TAKES.includes(takes(takesSoFar));

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
