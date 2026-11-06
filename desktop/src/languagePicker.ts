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

/**
 * WF-11 — the prefs choosing `pick` ("auto" or a code) writes. The chip's
 * click and the tray's Languages submenu (`tray_menu.rs` `prefs_for_pick`, its
 * Rust mirror) write exactly this, so the two can never disagree.
 */
export function pickPrefs(o: LanguageOptions, pick: string): LanguagePrefs {
  if (pick === AUTO) return { ...o.prefs, auto: true };
  return { primary: pick, set: o.prefs.set, auto: false };
}

/** The prefs one click on the chip writes. */
export function nextPick(o: LanguageOptions): LanguagePrefs {
  const cycle = pickCycle(o);
  const at = cycle.indexOf(currentPick(o));
  return pickPrefs(o, cycle[(at + 1) % cycle.length]);
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

/* ── WF-11 — one shared language store ─────────────────────────────────────
 *
 * The backend's settings are the ONE truth; every surface (the bar's chip,
 * Settings, the tray's Languages submenu, the offer card) writes through the
 * one writer (`set_dictation_languages` → `write_dictation_languages`), which
 * broadcasts LANGUAGES_CHANGED_EVENT with the new prefs. Each window holds one
 * store that applies that payload AT ONCE (no model probe in the way), then
 * re-reads `language_options` for the authoritative notice. The store takes
 * its backend as an argument, so it is tested here without a Tauri runtime;
 * src/languageStore.ts binds it to invoke/listen.
 */

/** Mirrors `languages::LANGUAGES_CHANGED_EVENT` (payload: LanguagePrefs). */
export const LANGUAGES_CHANGED_EVENT = "dictation-languages";

/** Every settings write broadcasts this; the store re-reads on it. */
export const SETTINGS_EVENT = "settings";

export function isLanguagePrefs(v: unknown): v is LanguagePrefs {
  if (!v || typeof v !== "object") return false;
  const p = v as Record<string, unknown>;
  return (
    typeof p.primary === "string" &&
    typeof p.auto === "boolean" &&
    Array.isArray(p.set) &&
    p.set.length > 0 &&
    p.set.every((c) => typeof c === "string")
  );
}

/**
 * The view after `prefs` were written, decided locally so the chip and the
 * Settings control change at once: the decode mode follows the prefs and what
 * the last probe said the model can do (the `languages::plan_decode` rules).
 * A notice survives only while mode and set are unchanged — the re-read that
 * follows brings the authoritative one back.
 */
export function withPrefs(o: LanguageOptions, prefs: LanguagePrefs): LanguageOptions {
  const offered = new Set(o.offered.map((c) => c.code));
  const usable = offered.size ? prefs.set.filter((c) => offered.has(c)) : prefs.set;
  const mode: DecodeMode =
    !prefs.auto || usable.length <= 1
      ? "single"
      : o.supportsDetect === true
        ? "autoWithinSet"
        : "fallbackNoDetect";
  const sameSet =
    prefs.set.length === o.prefs.set.length && prefs.set.every((c, i) => c === o.prefs.set[i]);
  const notice = mode === o.mode && sameSet ? o.notice : null;
  return { ...o, prefs, mode, notice };
}

/** What the store needs from the backend (Tauri in the app, a fake in tests). */
export interface LanguageBackend {
  options(): Promise<LanguageOptions>;
  write(prefs: LanguagePrefs): Promise<unknown>;
  listen(event: string, cb: (payload: unknown) => void): Promise<() => void>;
}

export interface LanguageStoreState {
  opts: LanguageOptions | null;
  error: string | null;
}

export interface LanguageStore {
  get(): LanguageStoreState;
  subscribe(fn: () => void): () => void;
  /** Start listening (ref-counted); returns the matching stop. */
  start(): () => void;
  /** Re-read `language_options`. */
  refresh(): Promise<void>;
  /** Another surface wrote `prefs` (the broadcast): apply at once. */
  apply(prefs: unknown): void;
  /** This surface writes: applied at once, rolled back if the write fails. */
  write(prefs: LanguagePrefs): Promise<boolean>;
  /** Choose "auto" or a code — what a tray row or a chip click means. */
  pick(pick: string): Promise<boolean>;
}

export function createLanguageStore(backend: LanguageBackend): LanguageStore {
  let state: LanguageStoreState = { opts: null, error: null };
  // Bumped by every local change, so a re-read that started before a pick
  // landed can never paint the old language back over it.
  let gen = 0;
  let users = 0;
  let stops: (() => void)[] = [];
  const subs = new Set<() => void>();
  const set = (next: LanguageStoreState) => {
    state = next;
    subs.forEach((fn) => fn());
  };
  const errorText = (e: unknown) => (e instanceof Error ? e.message : String(e));

  const store: LanguageStore = {
    get: () => state,
    subscribe(fn) {
      subs.add(fn);
      return () => {
        subs.delete(fn);
      };
    },
    async refresh() {
      const at = gen;
      try {
        const opts = await backend.options();
        if (at === gen) set({ opts, error: null });
      } catch (e) {
        if (at === gen) set({ ...state, error: errorText(e) });
      }
    },
    apply(prefs) {
      if (!isLanguagePrefs(prefs)) return;
      gen += 1;
      if (state.opts) set({ opts: withPrefs(state.opts, prefs), error: null });
      void store.refresh();
    },
    async write(prefs) {
      const before = state.opts;
      gen += 1;
      if (before) set({ opts: withPrefs(before, prefs), error: null });
      try {
        await backend.write(prefs);
      } catch (e) {
        gen += 1;
        set({ opts: before, error: errorText(e) });
        return false;
      }
      await store.refresh();
      return true;
    },
    pick(pick) {
      const o = state.opts;
      if (!o) return Promise.resolve(false);
      return store.write(pickPrefs(o, pick));
    },
    start() {
      users += 1;
      if (users === 1) {
        let dead = false;
        const keep = (p: Promise<() => void>) => {
          p.then((u) => (dead ? u() : stops.push(u))).catch(() => {});
        };
        keep(backend.listen(LANGUAGES_CHANGED_EVENT, (p) => store.apply(p)));
        keep(backend.listen(SETTINGS_EVENT, () => void store.refresh()));
        stops.push(() => {
          dead = true;
        });
        void store.refresh();
      }
      let stopped = false;
      return () => {
        if (stopped) return;
        stopped = true;
        users -= 1;
        if (users === 0) {
          const all = stops;
          stops = [];
          all.forEach((u) => u());
        }
      };
    },
  };
  return store;
}
