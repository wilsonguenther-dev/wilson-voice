import { describe, expect, it } from "vitest";
import {
  canCopy,
  COPY_COMMAND,
  copyNote,
  cycleFontSize,
  DEFAULT_PREFS,
  EXPAND_COMMAND,
  FONT_LABEL,
  FONT_PX,
  FONT_SIZES,
  loadPrefs,
  parsePrefs,
  PREFS_KEY,
  savePrefs,
  serializePrefs,
  setExpanded,
  stepFontSize,
  windowShortcut,
  type PrefsStore,
  type WindowPrefs,
} from "./windowPrefs";

function memoryStore(): PrefsStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
  };
}

const throwing: PrefsStore = {
  getItem: () => {
    throw new Error("storage disabled");
  },
  setItem: () => {
    throw new Error("quota");
  },
};

describe("persisted window settings", () => {
  it("round-trips size, expand and rail state through storage", () => {
    const store = memoryStore();
    const p: WindowPrefs = { fontSize: "large", expanded: true, railHidden: true };
    expect(savePrefs(store, p)).toBe(true);
    expect(store.data.has(PREFS_KEY)).toBe(true);
    expect(loadPrefs(store)).toEqual(p);
  });

  it("a fresh install gets the defaults", () => {
    expect(loadPrefs(memoryStore())).toEqual(DEFAULT_PREFS);
    expect(DEFAULT_PREFS).toEqual({ fontSize: "medium", expanded: false, railHidden: false });
  });

  it("missing or throwing storage never breaks the window", () => {
    expect(loadPrefs(null)).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(undefined)).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(throwing)).toEqual(DEFAULT_PREFS);
    expect(savePrefs(null, DEFAULT_PREFS)).toBe(false);
    expect(savePrefs(throwing, DEFAULT_PREFS)).toBe(false);
  });

  it("garbage falls back to the defaults", () => {
    for (const raw of [null, "", "not json", "[]", "null", "42", '"large"']) {
      expect(parsePrefs(raw), String(raw)).toEqual(DEFAULT_PREFS);
    }
  });

  it("each broken field falls back on its own, the good ones are kept", () => {
    expect(parsePrefs('{"fontSize":"huge","expanded":true,"railHidden":"yes"}')).toEqual({
      fontSize: "medium",
      expanded: true,
      railHidden: false,
    });
    expect(parsePrefs('{"fontSize":"small"}')).toEqual({ ...DEFAULT_PREFS, fontSize: "small" });
  });

  it("serializes only the known fields", () => {
    const extra = { ...DEFAULT_PREFS, stray: 1 } as WindowPrefs;
    expect(JSON.parse(serializePrefs(extra))).toEqual(DEFAULT_PREFS);
  });

  it("returned defaults are copies, so a caller cannot mutate the shared default", () => {
    const p = loadPrefs(null);
    p.expanded = true;
    expect(DEFAULT_PREFS.expanded).toBe(false);
  });
});

describe("text size", () => {
  it("has exactly three sizes, each with a px value and a label", () => {
    expect(FONT_SIZES).toEqual(["small", "medium", "large"]);
    for (const s of FONT_SIZES) {
      expect(FONT_PX[s]).toBeGreaterThan(0);
      expect(FONT_LABEL[s]).toMatch(/text$/);
    }
    expect(FONT_PX.small).toBeLessThan(FONT_PX.medium);
    expect(FONT_PX.medium).toBeLessThan(FONT_PX.large);
  });

  it("⌘+ / ⌘− step and stop at the ends", () => {
    expect(stepFontSize("small", 1)).toBe("medium");
    expect(stepFontSize("medium", 1)).toBe("large");
    expect(stepFontSize("large", 1)).toBe("large");
    expect(stepFontSize("large", -1)).toBe("medium");
    expect(stepFontSize("small", -1)).toBe("small");
    expect(stepFontSize("bogus" as never, 1)).toBe("large");
  });

  it("the Aa button cycles and wraps", () => {
    expect(cycleFontSize("small")).toBe("medium");
    expect(cycleFontSize("medium")).toBe("large");
    expect(cycleFontSize("large")).toBe("small");
  });
});

describe("window shortcuts", () => {
  const k = (key: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) =>
    windowShortcut({ key, metaKey: true, ctrlKey: false, ...mods });

  it("maps ⌘+, ⌘=, ⌘−, ⌘E", () => {
    expect(k("=")).toBe("bigger");
    expect(k("+")).toBe("bigger");
    expect(k("-")).toBe("smaller");
    expect(k("_")).toBe("smaller");
    expect(k("−")).toBe("smaller");
    expect(k("e")).toBe("toggleRail");
    expect(k("E")).toBe("toggleRail");
  });

  it("needs ⌘ (or Ctrl), ignores ⌥ and other keys", () => {
    expect(k("e", { metaKey: false })).toBeNull();
    expect(k("e", { metaKey: false, ctrlKey: true })).toBe("toggleRail");
    expect(k("e", { altKey: true })).toBeNull();
    expect(k("s")).toBeNull();
  });
});

describe("floating Copy", () => {
  it("puts the FULL note on the clipboard through the transient-marked command", async () => {
    const calls: [string, Record<string, unknown>][] = [];
    const run = async (cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      return 42;
    };
    const note = "# Groceries\n\n- eggs\n- milk  \n\n  trailing indent and space ";
    await copyNote(run, note);
    expect(COPY_COMMAND).toBe("copy_note_transient");
    expect(calls).toEqual([["copy_note_transient", { text: note }]]);
  });

  it("surfaces a failed copy to the caller", async () => {
    const run = async () => {
      throw new Error("Couldn't put the note on the clipboard.");
    };
    await expect(copyNote(run, "hi")).rejects.toThrow(/clipboard/);
  });

  it("offers itself only when the note has text", () => {
    expect(canCopy("")).toBe(false);
    expect(canCopy("  \n\t")).toBe(false);
    expect(canCopy("x")).toBe(true);
  });
});

describe("expand", () => {
  it("asks the backend and returns the state it reports", async () => {
    const calls: [string, Record<string, unknown>][] = [];
    const run = async (cmd: string, args: Record<string, unknown>) => {
      calls.push([cmd, args]);
      return args.expanded;
    };
    expect(await setExpanded(run, true)).toBe(true);
    expect(await setExpanded(run, false)).toBe(false);
    expect(calls).toEqual([
      [EXPAND_COMMAND, { expanded: true }],
      [EXPAND_COMMAND, { expanded: false }],
    ]);
  });

  it("a non-boolean reply keeps the asked-for state", async () => {
    expect(await setExpanded(async () => undefined, true)).toBe(true);
  });
});
