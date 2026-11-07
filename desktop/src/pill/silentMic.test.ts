import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import {
  fitMicName, fitsSilentMicRow, forcedSilentMic, nextMicrophone, NO_OTHER_MIC_LINE, parseSilentMicWarning,
  SAMPLE_SILENT_MIC_WARNING, SILENT_MIC_CHAR_PX, SILENT_MIC_HINT_LINES, SILENT_MIC_LINE_PX, SILENT_MIC_ROW_H,
  SILENT_MIC_ROW_W, silentMicAccessibleName, silentMicLine, silentMicLineWidthPx, silentMicShows,
  SWITCH_FAILED_LINE, switchedLine, type SilentMicList,
} from "./silentMic";
import { STRIP_CHAR_PX } from "./live";
import { HINT_LINE_H, HINT_W, pillLayout } from "./state";

const here = (p: string): string => readFileSync(new URL(p, import.meta.url), "utf8") as string;

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

  it("starts from the top of the list when the mic in use is not listed", () => {
    expect(nextMicrophone({ ...LIST, inUseId: "unplugged" })?.id).toBe("builtin");
    expect(nextMicrophone({ ...LIST, inUseId: null, chosenId: null })?.id).toBe("builtin");
  });

  it("uses the saved pick when nothing is in use yet", () => {
    expect(nextMicrophone({ ...LIST, inUseId: null, chosenId: "shure" })?.id).toBe("builtin");
  });

  it("is honest that a pick applies from the next take", () => {
    expect(switchedLine("Shure MV7")).toBe("Next take uses Shure MV7");
    expect(switchedLine(" ")).toBe("Next take uses the new mic");
  });

  // loop3 bounce: "I can't hear MacBook Pro Microphone" scrollWidth=200 >
  // clientWidth=105 in all six pill-matrix captures. The row is two rows now
  // and every line it can draw fits the row's full width.
  it("the row's constants mirror the hint slot and the phase strip's estimate", () => {
    expect(SILENT_MIC_ROW_W).toBe(HINT_W);
    expect(SILENT_MIC_CHAR_PX).toBe(STRIP_CHAR_PX);
    expect(SILENT_MIC_HINT_LINES * HINT_LINE_H).toBeGreaterThanOrEqual(SILENT_MIC_ROW_H);
    const css = here("../float.css");
    expect(css).toMatch(/\.silent-mic-row \{[^}]*height: 36px;/);
    expect(css).toMatch(/\.silent-mic-row \{[^}]*flex-direction: column;/);
    expect(css).toMatch(/\.silent-mic-row \{[^}]*padding: 3px 8px;/);
  });

  it("the matrix sample line is not cut off (the bounce)", () => {
    const line = silentMicLine(SAMPLE_SILENT_MIC_WARNING);
    expect(line).toBe("I can't hear MacBook Pro");
    expect(silentMicLineWidthPx(line)).toBeLessThanOrEqual(SILENT_MIC_LINE_PX);
  });

  it("keeps a line that already fits, word for word", () => {
    expect(silentMicLine("I can't hear Shure MV7")).toBe("I can't hear Shure MV7");
    expect(silentMicLine("I can't hear your microphone")).toBe("I can't hear your microphone");
  });

  it("cuts a long mic name with an ellipsis, never the prefix", () => {
    for (const name of ["External USB Audio Interface Microphone", "Supercalifragilisticexpialidocious", "A".repeat(80)]) {
      const line = fitMicName("I can't hear ", name);
      expect(line.startsWith("I can't hear ")).toBe(true);
      expect(line.endsWith("\u2026")).toBe(true);
      expect(fitsSilentMicRow(line), `${line} is ${silentMicLineWidthPx(line)} px`).toBe(true);
    }
    expect(silentMicLine("x".repeat(60)).endsWith("\u2026")).toBe(true);
    expect(fitsSilentMicRow(silentMicLine("x".repeat(60)))).toBe(true);
  });

  it("every line the row can draw fits it", () => {
    const names = LIST.devices.map((d) => d.name).concat(["External USB Audio Interface Microphone", " "]);
    const lines = [NO_OTHER_MIC_LINE, SWITCH_FAILED_LINE, ...names.map(switchedLine), ...names.map((n) => silentMicLine(`I can't hear ${n}`))];
    for (const line of lines) expect(fitsSilentMicRow(line), `${line} is ${silentMicLineWidthPx(line)} px`).toBe(true);
    expect(switchedLine("MacBook Pro Microphone")).toBe("Next take uses MacBook Pro");
  });

  it("the layout reserves the row's height while it is up (a new layout key)", () => {
    const bare = pillLayout({ phase: "listening", meeting: "none", dock: "bottom" });
    const up = pillLayout({ phase: "listening", meeting: "none", dock: "bottom", hintLines: SILENT_MIC_HINT_LINES });
    expect(up.key).not.toBe(bare.key);
    expect(up.hint.h).toBeGreaterThanOrEqual(SILENT_MIC_ROW_H);
    expect(up.panel.h).toBeGreaterThan(bare.panel.h);
    const main = here("../float-main.tsx");
    expect(main).toContain("silentMicUp ? SILENT_MIC_HINT_LINES : undefined");
    expect(main).toContain("hintLines });");
  });

  it("the accessible name says the take is still recording", () => {
    expect(silentMicAccessibleName("I can't hear X")).toMatch(/still recording/);
  });

  it("the matrix can force the row", () => {
    expect(forcedSilentMic("?phase=listening&silentMic=1")).toBe(SAMPLE_SILENT_MIC_WARNING);
    expect(forcedSilentMic("?phase=listening")).toBeNull();
  });

  it("the matrix row is a listening pill forced into the silent-mic state", async () => {
    const { matrixSilentMicScene } = await import("./forced");
    const row = matrixSilentMicScene();
    expect(row.id).toBe("silent-mic");
    expect(row.phase).toBe("listening");
    expect(row.query).toBe("silentMic=1");
    expect(forcedSilentMic(`?phase=${row.phase}&${row.query}`)).toBe(SAMPLE_SILENT_MIC_WARNING);
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
