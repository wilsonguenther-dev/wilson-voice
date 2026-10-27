import { describe, expect, it } from "vitest";
import {
  chipLabel, chipTitle, nextPick, pickCycle, showChip, toggleLanguage, type LanguageOptions,
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
