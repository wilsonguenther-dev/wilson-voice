import { describe, expect, it, vi } from "vitest";
import {
  chipLabel, chipTitle, createLanguageStore, currentPick, isLanguagePrefs, LANGUAGES_CHANGED_EVENT,
  nextPick, pickCycle, pickPrefs, SETTINGS_EVENT, showChip, toggleLanguage, withPrefs,
  type LanguageBackend, type LanguageOptions, type LanguagePrefs,
} from "./languagePicker";

const offered = [
  { code: "en", name: "English" },
  { code: "es", name: "Spanish" },
  { code: "fr", name: "French" },
];

const opts = (over: Partial<LanguageOptions> = {}): LanguageOptions => ({
  prefs: { primary: "en", set: ["en", "es"], auto: true },
  offered,
  supportsDetect: true,
  mode: "autoWithinSet",
  notice: null,
  maxSet: 4,
  ...over,
});

describe("language picker (Y10-A)", () => {
  it("hides the chip for a one-language set (the default install)", () => {
    expect(showChip(opts({ prefs: { primary: "en", set: ["en"], auto: true }, mode: "single" }))).toBe(false);
    expect(showChip(null)).toBe(false);
    expect(showChip(opts())).toBe(true);
  });

  it("cycles auto -> each language -> auto", () => {
    expect(chipLabel(opts())).toBe("Auto EN/ES");
    const a = nextPick(opts());
    expect(a).toEqual({ primary: "en", set: ["en", "es"], auto: false });
    const b = nextPick(opts({ prefs: a, mode: "single" }));
    expect(b).toEqual({ primary: "es", set: ["en", "es"], auto: false });
    const c = nextPick(opts({ prefs: { ...b, set: ["es", "en"] }, mode: "single" }));
    expect(c.auto).toBe(true);
  });

  it("never offers Auto on a model that cannot detect, and says why", () => {
    const o = opts({
      supportsDetect: false,
      mode: "fallbackNoDetect",
      notice: "This speech model can't detect languages, so Yap uses English only.",
    });
    expect(pickCycle(o)).toEqual(["en", "es"]);
    expect(chipLabel(o)).toBe("EN");
    expect(chipTitle(o)).toContain("can't detect");
    expect(nextPick(o)).toEqual({ primary: "es", set: ["en", "es"], auto: false });
  });

  it("caps the set and never empties it", () => {
    const p = { primary: "en", set: ["en"], auto: true };
    expect(toggleLanguage(p, "en", 4)).toBe(p);
    expect(toggleLanguage(p, "es", 4).set).toEqual(["en", "es"]);
    const full = { primary: "en", set: ["en", "es", "fr", "de"], auto: true };
    expect(toggleLanguage(full, "it", 4)).toBe(full);
    expect(toggleLanguage(full, "en", 4).primary).toBe("es");
  });
});

/* ── WF-11 — the tray's Languages submenu and the chip share ONE store ───── */

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

/**
 * The backend as every window sees it: one settings truth, the one writer
 * (`write_dictation_languages`) that broadcasts LANGUAGES_CHANGED_EVENT to
 * every window, and `trayPick` — what a click on a tray row does (the row's
 * prefs are `pickPrefs`, mirrored by `tray_menu.rs` `prefs_for_pick`).
 */
function fakeBackend(initial: LanguageOptions) {
  let current = initial;
  let hold = false;
  let failNext: string | null = null;
  const held: (() => void)[] = [];
  const handlers = new Map<string, Set<(p: unknown) => void>>();
  const emit = (event: string, payload: unknown) => {
    handlers.get(event)?.forEach((cb) => cb(payload));
  };
  const writeOne = (prefs: LanguagePrefs) => {
    current = withPrefs(current, prefs);
    emit(SETTINGS_EVENT, {});
    emit(LANGUAGES_CHANGED_EVENT, prefs);
  };
  const backend: LanguageBackend = {
    options: vi.fn(() => {
      const snap = current;
      if (!hold) return Promise.resolve(snap);
      return new Promise<LanguageOptions>((r) => held.push(() => r(snap)));
    }),
    write: vi.fn(async (prefs: LanguagePrefs) => {
      if (failNext) {
        const msg = failNext;
        failNext = null;
        throw new Error(msg);
      }
      writeOne(prefs);
    }),
    listen: vi.fn(async (event: string, cb: (p: unknown) => void) => {
      if (!handlers.has(event)) handlers.set(event, new Set());
      handlers.get(event)!.add(cb);
      return () => {
        handlers.get(event)!.delete(cb);
      };
    }),
  };
  return {
    backend,
    current: () => current,
    trayPick: (pick: string) => writeOne(pickPrefs(current, pick)),
    hold: (on: boolean) => {
      hold = on;
    },
    release: () => held.splice(0).forEach((r) => r()),
    failNextWrite: (msg: string) => {
      failNext = msg;
    },
    listeners: (event: string) => handlers.get(event)?.size ?? 0,
  };
}

describe("one shared language store (WF-11)", () => {
  it("choosing a language in the tray changes the pill's chip at once", async () => {
    const be = fakeBackend(opts());
    const chip = createLanguageStore(be.backend);
    const stop = chip.start();
    await flush();
    expect(chipLabel(chip.get().opts!)).toBe("Auto EN/ES");
    // The re-read that follows the broadcast is slow (it may probe the
    // model) — the chip must not wait for it.
    be.hold(true);
    be.trayPick("es");
    expect(chipLabel(chip.get().opts!)).toBe("ES");
    expect(currentPick(chip.get().opts!)).toBe("es");
    be.release();
    await flush();
    expect(chipLabel(chip.get().opts!)).toBe("ES");
    // And back to Auto-detect from the tray.
    be.hold(false);
    be.trayPick("auto");
    expect(chipLabel(chip.get().opts!)).toBe("Auto EN/ES");
    stop();
  });

  it("and the reverse: a chip click writes what the tray row writes, and every surface follows", async () => {
    const be = fakeBackend(opts());
    const chip = createLanguageStore(be.backend);
    const settings = createLanguageStore(be.backend);
    const stops = [chip.start(), settings.start()];
    await flush();
    const seen: string[] = [];
    const unsub = chip.subscribe(() => seen.push(chipLabel(chip.get().opts!)));
    const writing = chip.write(nextPick(chip.get().opts!));
    // The chip changed before the write even returned.
    expect(chipLabel(chip.get().opts!)).toBe("EN");
    expect(await writing).toBe(true);
    // The exact prefs the tray's "English" row would have written.
    expect(be.backend.write).toHaveBeenCalledWith(pickPrefs(opts(), "en"));
    expect(be.backend.write).toHaveBeenCalledWith({ primary: "en", set: ["en", "es"], auto: false });
    // The backend truth the tray's check is drawn from, and the other window.
    expect(currentPick(be.current())).toBe("en");
    expect(chipLabel(settings.get().opts!)).toBe("EN");
    expect(seen[0]).toBe("EN");
    // A pick by name, as the tray's rows are listed.
    expect(await settings.pick("es")).toBe(true);
    expect(chipLabel(chip.get().opts!)).toBe("ES");
    unsub();
    stops.forEach((s) => s());
  });

  it("a re-read that started before a pick never paints the old language back", async () => {
    const be = fakeBackend(opts());
    const chip = createLanguageStore(be.backend);
    const stop = chip.start();
    await flush();
    be.hold(true);
    const stale = chip.refresh();
    be.hold(false);
    be.trayPick("es");
    await flush();
    be.release();
    await stale;
    expect(chipLabel(chip.get().opts!)).toBe("ES");
    stop();
  });

  it("a failed write rolls the chip back and says why", async () => {
    const be = fakeBackend(opts());
    const chip = createLanguageStore(be.backend);
    const stop = chip.start();
    await flush();
    be.failNextWrite("Yap could not save your settings");
    expect(await chip.pick("es")).toBe(false);
    expect(chipLabel(chip.get().opts!)).toBe("Auto EN/ES");
    expect(chip.get().error).toBe("Yap could not save your settings");
    stop();
  });

  it("stops listening when the last surface goes, ignores junk, and reports a failed read", async () => {
    const be = fakeBackend(opts());
    const store = createLanguageStore(be.backend);
    expect(await store.pick("es")).toBe(false); // nothing loaded yet
    const a = store.start();
    const b = store.start();
    await flush();
    expect(be.listeners(LANGUAGES_CHANGED_EVENT)).toBe(1);
    store.apply({ primary: 3 });
    store.apply(null);
    expect(chipLabel(store.get().opts!)).toBe("Auto EN/ES");
    a();
    a(); // a second stop of the same surface is a no-op
    expect(be.listeners(LANGUAGES_CHANGED_EVENT)).toBe(1);
    b();
    expect(be.listeners(LANGUAGES_CHANGED_EVENT)).toBe(0);
    expect(be.listeners(SETTINGS_EVENT)).toBe(0);

    const broken = createLanguageStore({
      options: () => Promise.reject("language probe failed"),
      write: () => Promise.resolve(),
      listen: () => Promise.reject(new Error("no runtime")),
    });
    const stop = broken.start();
    await flush();
    expect(broken.get()).toEqual({ opts: null, error: "language probe failed" });
    broken.apply({ primary: "es", set: ["es"], auto: false });
    expect(broken.get().opts).toBeNull();
    stop();
  });

  it("decides the mode locally from the prefs and the last probe", () => {
    const o = opts();
    expect(withPrefs(o, { primary: "es", set: ["en", "es"], auto: false }).mode).toBe("single");
    expect(withPrefs(o, { primary: "en", set: ["en", "es"], auto: true }).mode).toBe("autoWithinSet");
    const noDetect = opts({ supportsDetect: false, mode: "fallbackNoDetect", notice: "n" });
    const kept = withPrefs(noDetect, { primary: "es", set: ["en", "es"], auto: true });
    expect(kept.mode).toBe("fallbackNoDetect");
    expect(kept.notice).toBe("n");
    const unknown = opts({ supportsDetect: null, offered: [] });
    expect(withPrefs(unknown, { primary: "en", set: ["en", "es"], auto: true }).mode).toBe("fallbackNoDetect");
    // A set the model can only transcribe one of decodes that one.
    expect(withPrefs(o, { primary: "en", set: ["en", "de"], auto: true })).toMatchObject({
      mode: "single",
      notice: null,
    });
  });

  it("knows a prefs payload when it sees one", () => {
    expect(isLanguagePrefs({ primary: "en", set: ["en"], auto: true })).toBe(true);
    expect(isLanguagePrefs({ primary: "en", set: [], auto: true })).toBe(false);
    expect(isLanguagePrefs({ primary: "en", set: [1], auto: true })).toBe(false);
    expect(isLanguagePrefs({ primary: "en", set: ["en"] })).toBe(false);
    expect(isLanguagePrefs("en")).toBe(false);
    expect(LANGUAGES_CHANGED_EVENT).toBe("dictation-languages");
  });
});
