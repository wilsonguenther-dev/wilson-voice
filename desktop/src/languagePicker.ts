/**
 * Y10-A — the language picker's pure model, shared by the bar's chip
 * (src/pill/LanguageChip.tsx) and the Settings control
 * (src/views/settings/LanguageSettings.tsx).
 *
 * The backend owns the truth: `language_options` reports the user's set, the
 * languages the INSTALLED model can transcribe, and whether it can detect at
 * all; `set_dictation_languages` is the one writer. This module only decides
 * what a click means and what the chip says, so it is unit-tested without a
 * Tauri runtime.
 */

export interface LanguageChoice {
  code: string;
  name: string;
}

export interface LanguagePrefs {
  primary: string;
  set: string[];
  auto: boolean;
}

/** Mirrors `languages::DecodeMode` (serde camelCase). */
export type DecodeMode = "single" | "autoWithinSet" | "fallbackNoDetect";

/** Mirrors `languages::LanguageOptions`. */
export interface LanguageOptions {
  prefs: LanguagePrefs;
  offered: LanguageChoice[];
  /** `null` = no model downloaded yet, so the probe has not run. */
  supportsDetect: boolean | null;
  mode: DecodeMode;
  notice: string | null;
  maxSet: number;
}

export const AUTO = "auto";

/** Upper-case ISO code, the chip's compact form. */
export function shortCode(code: string): string {
  return code.toUpperCase();
}

export function languageName(o: LanguageOptions, code: string): string {
  return o.offered.find((c) => c.code === code)?.name ?? shortCode(code);
}

/** The chip shows only when there is a choice to make. */
export function showChip(o: LanguageOptions | null): o is LanguageOptions {
  return !!o && o.prefs.set.length > 1;
}

/** What the take will actually use right now: "auto" or one code. */
export function currentPick(o: LanguageOptions): string {
  return o.mode === "autoWithinSet" ? AUTO : o.prefs.primary;
}

/**
 * The click cycle: Auto (only when the model can detect) then each language
 * of the set, in the picker's own order so the cycle is stable no matter
 * which language is currently primary.
 */
export function pickCycle(o: LanguageOptions): string[] {
  const order = o.offered.map((c) => c.code);
  const rank = (c: string) => {
    const i = order.indexOf(c);
    return i < 0 ? order.length : i;
  };
  const langs = [...o.prefs.set].sort((a, b) => rank(a) - rank(b));
  return o.supportsDetect === false ? langs : [AUTO, ...langs];
}

/** The prefs one click on the chip writes. */
export function nextPick(o: LanguageOptions): LanguagePrefs {
  const cycle = pickCycle(o);
  const at = cycle.indexOf(currentPick(o));
  const next = cycle[(at + 1) % cycle.length];
  if (next === AUTO) return { ...o.prefs, auto: true };
  return { primary: next, set: o.prefs.set, auto: false };
}

export function chipLabel(o: LanguageOptions): string {
  if (o.mode === "autoWithinSet") return `Auto ${o.prefs.set.map(shortCode).join("/")}`;
  return shortCode(o.prefs.primary);
}

/** The chip's tooltip: what it is doing, and the notice when there is one. */
export function chipTitle(o: LanguageOptions): string {
  const doing =
    o.mode === "autoWithinSet"
      ? `Detecting ${o.prefs.set.map((c) => languageName(o, c)).join(" or ")}`
      : `Dictating in ${languageName(o, o.prefs.primary)}`;
  return o.notice ? `${doing}. ${o.notice}` : `${doing}. Click to switch.`;
}

/** Add or remove a language from the set (Settings checkboxes). */
export function toggleLanguage(prefs: LanguagePrefs, code: string, maxSet: number): LanguagePrefs {
  if (prefs.set.includes(code)) {
    if (prefs.set.length <= 1) return prefs; // never an empty set
    const set = prefs.set.filter((c) => c !== code);
    const primary = prefs.primary === code ? set[0] : prefs.primary;
    return { ...prefs, primary, set };
  }
  if (prefs.set.length >= maxSet) return prefs;
  return { ...prefs, set: [...prefs.set, code] };
}
