/**
 * Y8-D — coaching nudges: the bar teaches the app, in Yappy's voice, without
 * becoming nagware.
 *
 * Wispr's bar has `growthNudgeActive` ("Click a textbox, hold fn to dictate",
 * "Open Cursor or any IDE"). This is Yappy's version, and its whole value is
 * its DISCIPLINE: the teardown's "explicitly not wanted" list names trial-nag
 * surfaces, so the caps below are the feature, not a tuning detail.
 *
 *   - a nudge fires at most once per CONDITION, ever (the ledger survives a
 *     relaunch: `nudge_ledger` / `nudge_mark_shown`, one `settings_kv` row);
 *   - at most [`MAX_NUDGES_PER_SESSION`] per app session;
 *   - none at all after [`NUDGE_WINDOW_DAYS`] days, or once the user has
 *     [`FLUENT_AFTER_TAKES`] successful takes — a fluent user is left alone;
 *   - never while a take is live, and never before this session has had a take
 *     (a nudge is a lesson about the take you just did, not a launch banner);
 *   - never money: commerce is Y2's, and a lesson that sells is a nag.
 *
 * Pure + deterministic: no timers, no events, no DOM. `NudgeRow.tsx` feeds it
 * what the app already knows and renders what it returns.
 */
import { milestoneLine, nudgeLine } from "./tone";
import { PILL_MESSAGES, pillMessageAllowed, type ChatTone, type PillCategory } from "./live";

/** nudge.rs `TAKE_CONTEXT_EVENT`: one per finished dictation. */
export const TAKE_CONTEXT_EVENT = "take_context";

/** At most this many nudges in one app session. */
export const MAX_NUDGES_PER_SESSION = 1;
/** No nudge at all once the install is this many days old. */
export const NUDGE_WINDOW_DAYS = 7;
/** A user with this many successful takes is fluent: no more nudges, ever. */
export const FLUENT_AFTER_TAKES = 20;
/** The meetings lesson waits until dictation itself is familiar. */
export const MEETINGS_AFTER_TAKES = 3;
/** An untouched nudge leaves on its own, handing the hitbox back to the pill. */
export const NUDGE_TTL_MS = 9_000;

/** `dictation::mode_for_app`'s families (nudge.rs `category_name`). */
export type AppCategory = "email" | "document" | "notes" | "code" | "chat" | "plain" | "list";

/** Features the engine can notice have never been used. */
export type Feature = "meetings";

/** Every condition a nudge can fire on, in PRIORITY order. */
export const NUDGE_IDS = ["no-text-field", "code-editor", "meetings"] as const;
export type NudgeId = (typeof NUDGE_IDS)[number];

/** nudge.rs `TakeContext` (serde camelCase). */
export interface TakeContextPayload {
  textFieldFocused: boolean;
  appCategory: AppCategory;
  pasted: boolean;
}

/** Everything the engine reads. All of it is something the app already knows. */
export interface NudgeContext {
  /** A take is recording or processing. Nothing fires during one. */
  takeLive: boolean;
  /** The take this session just finished; `null` until the first one. */
  lastTake: TakeContextPayload | null;
  /** Successful takes ever (history count + this session's). */
  takesSoFar: number;
  /** Days since the ledger was first read on this install. */
  daysSinceInstall: number;
  /** Features this install has never used. */
  unusedFeatures: readonly Feature[];
}

/** What has already been shown. */
export interface NudgeShown {
  /** Nudges shown in THIS app session. */
  thisSession: number;
  /** Every condition that has ever fired on this install. */
  ever: ReadonlySet<string>;
  /**
   * WF-17 — per-nudge showings ([`tipHistoryFromLedger`]). When present it
   * replaces `ever` for nudges: each may show [`TIP_MAX_SHOWS`] times, at most
   * once every [`TIP_MIN_GAP_DAYS`] days. Absent = the old once-ever rule.
   */
  history?: ReadonlyMap<string, TipHistory>;
  /** WF-17 — today's [`dayIndex`]; read with `history`. */
  today?: number;
  /** WF-17 — the categories muted in Settings › Notifications. */
  muted?: ReadonlySet<string>;
}

export interface Nudge {
  id: NudgeId | MilestoneId;
  /** WF-17 — its row in `live.ts` `PILL_MESSAGES` (tip / news / milestone). */
  category: PillCategory;
  /** The line in each companion tone. */
  copy: Record<ChatTone, string>;
}

/** Does `id`'s condition hold for this context? Caps are applied separately. */
function holds(id: NudgeId, ctx: NudgeContext): boolean {
  const take = ctx.lastTake;
  if (!take) return false;
  switch (id) {
    case "no-text-field":
      // The take succeeded, the paste had nowhere to go, and the user is about
      // to conclude Yap is broken. The one lesson that matters most.
      return !take.textFieldFocused;
    case "code-editor":
      return take.appCategory === "code" && take.textFieldFocused;
    case "meetings":
      return ctx.unusedFeatures.includes("meetings") && ctx.takesSoFar >= MEETINGS_AFTER_TAKES;
  }
}

/** The whole engine's caps — true when NOTHING may fire. */
export function silenced(ctx: NudgeContext, shown: NudgeShown): boolean {
  return (
    ctx.takeLive ||
    shown.thisSession >= MAX_NUDGES_PER_SESSION ||
    ctx.daysSinceInstall >= NUDGE_WINDOW_DAYS ||
    ctx.takesSoFar >= FLUENT_AFTER_TAKES
  );
}

/** The nudge to show now, or `null`. Highest-priority unfired condition wins. */
export function nextNudge(ctx: NudgeContext, shown: NudgeShown): Nudge | null {
  if (silenced(ctx, shown)) return null;
  const muted = shown.muted ?? NOTHING_MUTED;
  for (const id of NUDGE_IDS) {
    // WF-17 — a muted category never reaches the pill.
    if (!pillMessageAllowed(`nudge:${id}` as const, muted)) continue;
    if (shown.history) {
      if (!tipFrequencyAllows(shown.history.get(id), shown.today ?? 0)) continue;
    } else if (shown.ever.has(id)) continue;
    if (holds(id, ctx)) return nudge(id);
  }
  return null;
}

const NOTHING_MUTED: ReadonlySet<string> = new Set();

// ── WF-17 — each tip: at most once every 3 days, 3 times in total ──────────

/** A tip waits at least this many whole days between showings. */
export const TIP_MIN_GAP_DAYS = 3;
/** A tip shows at most this many times on one install, ever. */
export const TIP_MAX_SHOWS = 3;

/** How often a nudge has shown, and on which day it last did. */
export interface TipHistory {
  count: number;
  /** [`dayIndex`] of the last showing; `null` = a pre-WF-17 entry, day unknown. */
  lastDay: number | null;
}

/** Whole UTC days since the epoch: the unit the ledger slugs carry. */
export const dayIndex = (ms: number): number => Math.floor(ms / 86_400_000);

/** May a nudge with this history show on `today`? */
export function tipFrequencyAllows(h: TipHistory | undefined, today: number): boolean {
  if (!h || h.count <= 0) return true;
  if (h.count >= TIP_MAX_SHOWS) return false;
  if (h.lastDay === null) return true;
  return today - h.lastDay >= TIP_MIN_GAP_DAYS;
}

/**
 * The ledger slug recording one showing of `id` on `day`. The ledger
 * (nudge.rs `mark`) stores each slug once and only accepts `[a-z0-9-]`, so a
 * showing per day is `<id>-<day>` ("no-text-field-20398", well inside
 * `MAX_CONDITION_LEN`). A bare `<id>` is a showing from before WF-17.
 */
export const tipShownSlug = (id: NudgeId, day: number): string => `${id}-${day}`;

/** Each nudge's history, read back out of the ledger's `shown` slugs. */
export function tipHistoryFromLedger(shown: readonly string[]): Map<NudgeId, TipHistory> {
  const out = new Map<NudgeId, TipHistory>();
  for (const id of NUDGE_IDS) {
    let count = 0;
    let lastDay: number | null = null;
    for (const slug of shown) {
      if (slug === id) {
        count += 1;
        continue;
      }
      if (!slug.startsWith(`${id}-`)) continue;
      const rest = slug.slice(id.length + 1);
      if (!/^\d+$/.test(rest)) continue;
      const day = Number(rest);
      count += 1;
      lastDay = lastDay === null ? day : Math.max(lastDay, day);
    }
    if (count > 0) out.set(id, { count, lastDay });
  }
  return out;
}

/** `h` after one more showing on `day`. */
export const recordTipShown = (h: TipHistory | undefined, day: number): TipHistory => ({
  count: (h?.count ?? 0) + 1,
  lastDay: day,
});

// ── WF-17 — milestones ──────────────────────────────────────────────────────

/** Counts worth a word, lowest first. Each fires once, ever. */
export const MILESTONE_IDS = ["takes-100", "takes-1000"] as const;
export type MilestoneId = (typeof MILESTONE_IDS)[number];
/** The successful-take count each milestone celebrates. */
export const MILESTONE_AT: Record<MilestoneId, number> = { "takes-100": 100, "takes-1000": 1000 };

/**
 * The milestone to show now, or `null`. Only the HIGHEST one reached is ever
 * a candidate (an install that arrives at 1,000 takes is not told about 100),
 * it fires once ever (`shown.ever`), never during a take, shares the
 * one-note-per-session cap with the nudges, and a muted category never shows.
 * Unlike a nudge it ignores the 7-day window and the fluency cap: milestones
 * are for people who already know the app.
 */
export function nextMilestone(ctx: NudgeContext, shown: NudgeShown): Nudge | null {
  if (ctx.takeLive || shown.thisSession >= MAX_NUDGES_PER_SESSION) return null;
  // Highest first: only the top milestone reached is ever a candidate.
  for (const id of [...MILESTONE_IDS].reverse()) {
    if (ctx.takesSoFar < MILESTONE_AT[id]) continue;
    if (shown.ever.has(id)) return null;
    if (!pillMessageAllowed(`milestone:${id}` as const, shown.muted ?? NOTHING_MUTED)) return null;
    return milestone(id);
  }
  return null;
}

/** A milestone by id, its copy from `tone.ts`. */
export function milestone(id: MilestoneId): Nudge {
  return {
    id,
    category: PILL_MESSAGES[`milestone:${id}` as const],
    copy: {
      rude: milestoneLine(id, "rude"),
      friendly: milestoneLine(id, "friendly"),
      rose: milestoneLine(id, "rose"),
    },
  };
}

/** A nudge by id, its copy straight out of the one voice system (`tone.ts`). */
export function nudge(id: NudgeId): Nudge {
  return {
    id,
    category: PILL_MESSAGES[`nudge:${id}` as const],
    copy: {
      rude: nudgeLine(id, "rude"),
      friendly: nudgeLine(id, "friendly"),
      rose: nudgeLine(id, "rose"),
    },
  };
}

/** Whole days between the install clock and `nowMs` (never negative). */
export function daysSince(firstSeenMs: number, nowMs: number): number {
  if (!Number.isFinite(firstSeenMs) || firstSeenMs <= 0) return 0;
  return Math.max(0, (nowMs - firstSeenMs) / 86_400_000);
}

/** Is `v` a `TakeContextPayload`? The event crosses a language boundary. */
export function isTakeContext(v: unknown): v is TakeContextPayload {
  if (!v || typeof v !== "object") return false;
  const c = v as Record<string, unknown>;
  return typeof c.textFieldFocused === "boolean" && typeof c.appCategory === "string" && typeof c.pasted === "boolean";
}
