/**
 * WF-15 — the Scratchpad's window controls, as pure data.
 *
 * Four controls: text size (Aa, ⌘+ / ⌘−, three steps), Expand (fill the
 * current screen and back), a floating Copy (the whole note, plain text, on
 * the clipboard with the transient marker so clipboard managers skip it) and
 * ⌘E to show or hide the notes rail. Size, expand and rail state persist.
 *
 * Nothing here touches a window or `@tauri-apps/*`: storage and the copy
 * command come in as arguments, so the whole contract is tested without one.
 * `localStorage` is the right home (as `appTypes.ts`'s trial flag): losing it
 * costs one reset to the defaults, not worth a settings migration.
 */

export type FontSize = "small" | "medium" | "large";

/** Smallest to largest; ⌘+ / ⌘− walk this list and stop at the ends. */
export const FONT_SIZES: readonly FontSize[] = ["small", "medium", "large"];

/** The editor's text size for each step, in CSS pixels. */
export const FONT_PX: Record<FontSize, number> = { small: 12, medium: 13.5, large: 16 };

/** What the Aa button says about the current step (its accessible name too). */
export const FONT_LABEL: Record<FontSize, string> = {
  small: "Small text",
  medium: "Medium text",
  large: "Large text",
};

export interface WindowPrefs {
  fontSize: FontSize;
  /** The panel fills the screen it is on. */
  expanded: boolean;
  /** ⌘E — the notes rail is hidden and the editor takes the full width. */
  railHidden: boolean;
}

export const DEFAULT_PREFS: WindowPrefs = { fontSize: "medium", expanded: false, railHidden: false };

export const PREFS_KEY = "yap.scratchpad.windowPrefs";

/** The slice of `Storage` this module needs. */
export interface PrefsStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function isFontSize(v: unknown): v is FontSize {
  return typeof v === "string" && (FONT_SIZES as readonly string[]).includes(v);
}

/** Any stored string (or none) to valid prefs: unknown or broken fields fall back one by one. */
export function parsePrefs(raw: string | null): WindowPrefs {
  if (raw == null) return { ...DEFAULT_PREFS };
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return { ...DEFAULT_PREFS };
  }
  if (typeof v !== "object" || v === null || Array.isArray(v)) return { ...DEFAULT_PREFS };
  const o = v as Record<string, unknown>;
  return {
    fontSize: isFontSize(o.fontSize) ? o.fontSize : DEFAULT_PREFS.fontSize,
    expanded: typeof o.expanded === "boolean" ? o.expanded : DEFAULT_PREFS.expanded,
    railHidden: typeof o.railHidden === "boolean" ? o.railHidden : DEFAULT_PREFS.railHidden,
  };
}

export function serializePrefs(p: WindowPrefs): string {
  return JSON.stringify({ fontSize: p.fontSize, expanded: p.expanded, railHidden: p.railHidden });
}

/** Read the saved prefs; storage that is missing or throws (private mode) gives the defaults. */
export function loadPrefs(store: PrefsStore | null | undefined): WindowPrefs {
  if (!store) return { ...DEFAULT_PREFS };
  try {
    return parsePrefs(store.getItem(PREFS_KEY));
  } catch {
    return { ...DEFAULT_PREFS };
  }
}

/** Save; `false` when storage is missing or refuses (the prefs then last this session only). */
export function savePrefs(store: PrefsStore | null | undefined, p: WindowPrefs): boolean {
  if (!store) return false;
  try {
    store.setItem(PREFS_KEY, serializePrefs(p));
    return true;
  } catch {
    return false;
  }
}

/** ⌘+ (`1`) / ⌘− (`-1`): one step, clamped at the ends. */
export function stepFontSize(size: FontSize, dir: 1 | -1): FontSize {
  const i = FONT_SIZES.indexOf(size);
  const next = Math.min(FONT_SIZES.length - 1, Math.max(0, (i < 0 ? 1 : i) + dir));
  return FONT_SIZES[next];
}

/** The Aa button: the next size, wrapping from large back to small. */
export function cycleFontSize(size: FontSize): FontSize {
  const i = FONT_SIZES.indexOf(size);
  return FONT_SIZES[(i + 1) % FONT_SIZES.length];
}

export type WindowShortcut = "bigger" | "smaller" | "toggleRail";

/** The window-control chords: ⌘+ (also ⌘= and ⇧⌘=), ⌘−, ⌘E. Ctrl stands in for ⌘ off macOS. */
export function windowShortcut(e: {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey?: boolean;
}): WindowShortcut | null {
  if (!(e.metaKey || e.ctrlKey) || e.altKey) return null;
  const k = e.key.toLowerCase();
  if (k === "=" || k === "+") return "bigger";
  if (k === "-" || k === "_" || k === "−") return "smaller";
  if (k === "e") return "toggleRail";
  return null;
}

/** The backend command behind the floating Copy (lib.rs `copy_note_transient`). */
export const COPY_COMMAND = "copy_note_transient";
/** The backend command behind Expand (lib.rs `scratchpad_set_expanded`). */
export const EXPAND_COMMAND = "scratchpad_set_expanded";

export type Invoker = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

/** Copy offers itself only when there is something to copy. */
export function canCopy(content: string): boolean {
  return content.trim().length > 0;
}

/**
 * The floating Copy: the WHOLE note body exactly as typed (never just the
 * selection, never trimmed), through the transient-marked copy command.
 */
export async function copyNote(run: Invoker, content: string): Promise<void> {
  await run(COPY_COMMAND, { text: content });
}

/** Expand / collapse; resolves to the state the backend reports. */
export async function setExpanded(run: Invoker, expanded: boolean): Promise<boolean> {
  const now = await run(EXPAND_COMMAND, { expanded });
  return typeof now === "boolean" ? now : expanded;
}
