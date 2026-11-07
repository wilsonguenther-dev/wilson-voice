/**
 * yap-1107-sidebar-collapses-to-icons — the pure half of the sidebar's icon
 * rail: where the choice is remembered, when the window is narrow enough that
 * the rail is used on its own, and which keys drive it. App.tsx owns the
 * listeners and the DOM; everything here is plain data so vitest drives it
 * without a window.
 *
 * Persistence: localStorage key `yap.sidebar.collapsed` ("1" / "0"). It is
 * read synchronously in App's useState initializer, so the very first paint
 * already has the right width (no expand-then-collapse flash on relaunch).
 * localStorage, not settings.json: it is a per-window view preference that
 * must be known before the settings bridge answers.
 */
import type { Nav } from "./appTypes";

/** localStorage key for the user's collapse choice. */
export const SIDEBAR_COLLAPSED_KEY = "yap.sidebar.collapsed";

/**
 * YAP-UI-SPEC-2026-11 section 1.1, Main window: "rail collapses to icons under
 * 900 px". A window narrower than this always shows the rail.
 */
export const RAIL_AUTO_BELOW_PX = 900;

/** Expanded sidebar width and the icon rail width, in CSS px (App.css mirrors them). */
export const SIDEBAR_EXPANDED_PX = 220;
export const SIDEBAR_RAIL_PX = 56;

/** The nav in rail/sidebar order; Cmd-1 is index 0. */
export const NAV_ORDER: readonly Nav[] = [
  "home",
  "history",
  "permissions",
  "meetings",
  "insights",
  "dictionary",
  "scratchpad",
  "settings",
];

/** The keyboard hint shown in the toggle's tooltip ("⌃⌘S", the macOS sidebar convention). */
export const SIDEBAR_TOGGLE_HINT = "⌃⌘S";

type ReadStore = { getItem(key: string): string | null } | null | undefined;
type WriteStore = { setItem(key: string, value: string): void } | null | undefined;

/** The remembered choice; false (expanded) when nothing is stored or storage throws. */
export function loadCollapsed(store: ReadStore): boolean {
  if (!store) return false;
  try {
    return store.getItem(SIDEBAR_COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

/** Remembers the choice. Returns false when storage is missing or refuses (quota, private mode). */
export function saveCollapsed(store: WriteStore, collapsed: boolean): boolean {
  if (!store) return false;
  try {
    store.setItem(SIDEBAR_COLLAPSED_KEY, collapsed ? "1" : "0");
    return true;
  } catch {
    return false;
  }
}

/** True when the window is under the spec's 900 px line. */
export function isNarrowWindow(widthPx: number): boolean {
  return Number.isFinite(widthPx) && widthPx < RAIL_AUTO_BELOW_PX;
}

/** The media query App.tsx watches; matches exactly when isNarrowWindow() is true for integer widths. */
export const NARROW_QUERY = `(max-width: ${RAIL_AUTO_BELOW_PX - 1}px)`;

/** What the sidebar renders: the icon rail when the user chose it OR the window is narrow. */
export function railShown(collapsed: boolean, narrow: boolean): boolean {
  return collapsed || narrow;
}

/** Why the rail is showing, for telemetry and the toggle's label. */
export function railReason(collapsed: boolean, narrow: boolean): "expanded" | "user" | "narrow" {
  if (narrow) return "narrow";
  return collapsed ? "user" : "expanded";
}

/** Width the main column gains when the rail replaces the expanded sidebar. */
export function railDelta(): number {
  return SIDEBAR_EXPANDED_PX - SIDEBAR_RAIL_PX;
}

export interface SidebarKey {
  code: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  repeat?: boolean;
  isComposing?: boolean;
  defaultPrevented?: boolean;
}

export type SidebarKeyAction = { kind: "toggle" } | { kind: "nav"; nav: Nav } | null;

/**
 * Ctrl-Cmd-S toggles the rail; Cmd-1..Cmd-8 jump to the nav items in order.
 * Physical key codes, so a non-US layout still works. Anything else, a held
 * (auto-repeating) key, an IME composition or a key another handler already
 * consumed (the shortcut recorders) is null.
 */
export function sidebarKeyAction(e: SidebarKey): SidebarKeyAction {
  if (e.defaultPrevented || e.isComposing || e.repeat) return null;
  if (e.altKey || e.shiftKey) return null;
  if (e.metaKey && e.ctrlKey) return e.code === "KeyS" ? { kind: "toggle" } : null;
  if (e.metaKey && !e.ctrlKey) {
    const m = /^Digit([1-9])$/.exec(e.code);
    if (!m) return null;
    const nav = NAV_ORDER[Number(m[1]) - 1];
    return nav ? { kind: "nav", nav } : null;
  }
  return null;
}

/** "⌘3" for the nav item at index 2, used in the rail tooltip. */
export function navShortcutHint(nav: Nav): string {
  const i = NAV_ORDER.indexOf(nav);
  return i < 0 ? "" : `⌘${i + 1}`;
}
