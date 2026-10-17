/**
 * Y5-B — what state is this view actually in?
 *
 * Before this file every view answered that question inline, differently, and
 * two of them answered it with `return null`: `Home.tsx` and `Insights.tsx`
 * both opened with `if (!insights) return null`, so a fresh install — no
 * history, no insights row yet — rendered a heading and then nothing at all.
 * A literal blank page in the shipped app. That is the "looks broken"
 * complaint this item exists to close.
 *
 * The decision is extracted here, as a pure function, for one reason: App.tsx
 * and its seven view modules are thousands of lines of JSX, and asserting
 * "Insights with zero takes shows an empty state, not a blank screen" should
 * not require rendering any of it. `viewState.test.ts` walks the whole matrix
 * against this function instead.
 *
 * PANEL 2026-09-12 correction — not every view has all three states. A
 * permissions screen with nothing in it is a bug, not an empty state, and
 * settings has nothing it could be empty OF. Those two enumerate
 * loading + error + ready, and their "ready" reads as "nothing to fix here".
 * `VIEW_STATES` is that enumeration, and it is what the tests assert against,
 * so a marker can never exist only to satisfy a count.
 */

/** The four states a view can be in. There is no fifth, and there is no null. */
export type ViewStatus = "loading" | "error" | "empty" | "ready";

/** The seven views in the sidebar, in nav order. */
export type ViewId =
  | "home"
  | "permissions"
  | "meetings"
  | "insights"
  | "dictionary"
  | "scratchpad"
  | "settings";

export const VIEW_IDS: readonly ViewId[] = [
  "home",
  "permissions",
  "meetings",
  "insights",
  "dictionary",
  "scratchpad",
  "settings",
];

/**
 * Which states each view can legitimately reach.
 *
 * The five list views can all be genuinely empty on a fresh install. The two
 * that cannot are spelled out rather than padded: `permissions` always has
 * seven rows to show (an empty one means the report failed to load, which is
 * the `error` state), and `settings` always has its panels.
 */
export const VIEW_STATES: Record<ViewId, readonly ViewStatus[]> = {
  home: ["loading", "error", "empty", "ready"],
  permissions: ["loading", "error", "ready"],
  meetings: ["loading", "error", "empty", "ready"],
  insights: ["loading", "error", "empty", "ready"],
  dictionary: ["loading", "error", "empty", "ready"],
  scratchpad: ["loading", "error", "empty", "ready"],
  settings: ["loading", "error", "ready"],
};

/** True when this view can legitimately render an empty state. */
export function hasEmptyState(view: ViewId): boolean {
  return VIEW_STATES[view].includes("empty");
}

/**
 * Is this payload "nothing"?
 *
 * Accepts the three shapes the views actually hold: a list (`history`,
 * `meetings`, `dictionary`, `scratch`), a count (Insights passes
 * `insights.totalSessions`, because "zero takes" is the empty case the item
 * names by hand), and a string. `null`/`undefined` are NOT decided here —
 * they mean "has not arrived yet", which is `loading`, and `viewState`
 * settles that before it ever calls this.
 */
export function isEmptyData(data: unknown): boolean {
  if (Array.isArray(data)) return data.length === 0;
  if (typeof data === "number") return !Number.isFinite(data) || data <= 0;
  if (typeof data === "string") return data.trim().length === 0;
  if (data instanceof Map || data instanceof Set) return data.size === 0;
  return false;
}

/**
 * The state decision, in precedence order.
 *
 *   error   — something failed, and we know what. Beats everything: a stale
 *             empty list under a failed refetch must not read as "you have
 *             nothing", which is the lie `loadMeetings`' `console.error` was
 *             telling before this item.
 *   loading — explicitly in flight, OR the payload is still `null`. A view
 *             whose data has never arrived is loading, not empty. This is the
 *             `return null` blank page, now a state with a name.
 *   empty   — it arrived, and there is nothing in it.
 *   ready   — render the thing.
 *
 * `error` is deliberately loose about its argument: callers pass whatever
 * `invoke` rejected with, which may be a string, a `CommandError` object, or
 * `null`. An empty string is not an error.
 */
export function viewState(
  data: unknown,
  loading?: boolean,
  error?: unknown,
): ViewStatus {
  if (error !== null && error !== undefined && error !== "") return "error";
  if (loading === true) return "loading";
  if (data === null || data === undefined) return "loading";
  return isEmptyData(data) ? "empty" : "ready";
}

/**
 * What failed, in the user's terms.
 *
 * The item's rule: "A DB error is 'Yap could not open its history file', not
 * an SQLite code." Every one of these names the file or the system Yap was
 * reaching for, so the sentence is true without being a stack trace. The raw
 * rejection is still rendered underneath, small, for the support bundle — it
 * is just no longer the headline.
 */
const VIEW_ERROR_TEXT: Record<ViewId, string> = {
  home: "Yap could not open its history file.",
  permissions: "Yap could not read its permission status from macOS.",
  meetings: "Yap could not open your meeting records.",
  insights: "Yap could not read your dictation stats.",
  dictionary: "Yap could not open your dictionary.",
  scratchpad: "Yap could not open your notes.",
  settings: "Yap could not read its settings.",
};

export function viewErrorText(view: ViewId): string {
  return VIEW_ERROR_TEXT[view];
}

/**
 * The label for a determinate load ("Reading 128 takes…") when the count is
 * knowable, and a calm indeterminate one when it is not.
 *
 * The item forbids both a full-page spinner and a skeleton that pulses
 * forever — the Drivia audit found nineteen pages "still loading at 15s"
 * behind exactly that. `expected` is the count when we have it; `null` means
 * we genuinely do not know, and the caller renders the calm variant.
 */
export function loadingLabel(noun: string, expected?: number | null): string {
  if (typeof expected === "number" && Number.isFinite(expected) && expected > 0) {
    return `Reading ${expected.toLocaleString()} ${noun}…`;
  }
  return `Opening your ${noun}…`;
}
