import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import {
  forcedSilentMic, nextMicrophone, parseSilentMicWarning, SAMPLE_SILENT_MIC_WARNING,
  silentMicAccessibleName, silentMicShows, switchedLine, type SilentMicList,
} from "./silentMic";

const here = (p: string) => readFileSync(fileURLToPath(new URL(p, import.meta.url)), "utf8");

const LIST: SilentMicList = {
  devices: [
    { id: "builtin", name: "MacBook Pro Microphone", isSystemDefault: true },
    { id: "loopback", name: "BlackHole 2ch", isVirtual: true },
    { id: "shure", name: "Shure MV7" },
  ],
  chosenId: null,
  inUseId: "builtin",
};

describe("WF-13 silent-mic warning — pill model", () => {
  it("reads the status field, trimmed; anything else is no warning", () => {
    expect(parseSilentMicWarning("  I can't hear Shure MV7 ")).toBe("I can't hear Shure MV7");
    expect(parseSilentMicWarning("")).toBeNull();
    expect(parseSilentMicWarning(null)).toBeNull();
    expect(parseSilentMicWarning(42)).toBeNull();
  });

  it("shows only during a live take", () => {
    expect(silentMicShows("I can't hear X", "listening")).toBe(true);
    expect(silentMicShows("I can't hear X", "listening-locked")).toBe(true);
    expect(silentMicShows("I can't hear X", "transcribing")).toBe(false);
    expect(silentMicShows("I can't hear X", "idle")).toBe(false);
    expect(silentMicShows(null, "listening")).toBe(false);
  });

  it("Switch mic moves to the next REAL microphone, skipping virtual inputs", () => {
    expect(nextMicrophone(LIST)?.id).toBe("shure");
    expect(nextMicrophone({ ...LIST, inUseId: "shure" })?.id).toBe("builtin");
  });

  it("falls back to a virtual input only when nothing physical is left", () => {
    const only = { devices: [LIST.devices[0], LIST.devices[1]], chosenId: null, inUseId: "builtin" };
    expect(nextMicrophone(only)?.id).toBe("loopback");
  });

  it("has nothing to switch to with one mic (the row says so instead)", () => {
    expect(nextMicrophone({ devices: [LIST.devices[0]], chosenId: null, inUseId: "builtin" })).toBeNull();
    expect(nextMicrophone({ devices: [], chosenId: null, inUseId: null })).toBeNull();
    expect(nextMicrophone(null)).toBeNull();
  });

  it("uses the saved pick when nothing is in use yet", () => {
    expect(nextMicrophone({ ...LIST, inUseId: null, chosenId: "shure" })?.id).toBe("builtin");
  });

  it("is honest that a pick applies from the next take", () => {
    expect(switchedLine("Shure MV7")).toBe("Next take uses Shure MV7");
    expect(switchedLine(" ")).toBe("Next take uses the new microphone");
  });

  it("the accessible name says the take is still recording", () => {
    expect(silentMicAccessibleName("I can't hear X")).toMatch(/still recording/);
  });

  it("the matrix can force the row", () => {
    expect(forcedSilentMic("?phase=listening&silentMic=1")).toBe(SAMPLE_SILENT_MIC_WARNING);
    expect(forcedSilentMic("?phase=listening")).toBeNull();
  });

  it("is mounted by the float window and styled from tokens", () => {
    const main = here("../float-main.tsx");
    expect(main).toContain("<SilentMicRow");
    expect(main).toContain("matrixSilentMicScene()");
    expect(main).toContain("parseSilentMicWarning(st.silentMicWarning)");
    const css = here("../float.css");
    expect(css).toContain(".silent-mic-row");
    expect(css).toContain("var(--pill-silent-mic-text)");
    expect(here("../tokens.css")).toContain("--pill-silent-mic-text");
  });
});
