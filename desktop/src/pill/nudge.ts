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
import { nudgeLine } from "./tone";
import type { ChatTone } from "./live";

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
}

export interface Nudge {
  id: NudgeId;
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
  for (const id of NUDGE_IDS) {
    if (shown.ever.has(id)) continue;
    if (holds(id, ctx)) return nudge(id);
  }
  return null;
}

/** A nudge by id, its copy straight out of the one voice system (`tone.ts`). */
export function nudge(id: NudgeId): Nudge {
  return {
    id,
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
