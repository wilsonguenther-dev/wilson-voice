import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { AX_OFF_LINE, FRONT_TAB_LINE, SITE_UNKNOWN_NAME, browserCallsLines } from "./browserCalls";

const rust = readFileSync(new URL("../../../src-tauri/src/call_titles.rs", import.meta.url), "utf8");

describe("yap-call-2 browser calls copy", () => {
  it("states the front-tab limit always", () => {
    expect(browserCallsLines(true).frontTab).toContain("front tab");
    expect(browserCallsLines(null).frontTab).toBe(FRONT_TAB_LINE);
  });

  it("shows the Accessibility line only when Accessibility is known to be off", () => {
    expect(browserCallsLines(false).axOff).toBe(AX_OFF_LINE);
    expect(browserCallsLines(true).axOff).toBeNull();
    expect(browserCallsLines(null).axOff).toBeNull();
  });

  it("matches the Rust copy in call_titles.rs", () => {
    expect(rust).toContain(`"${FRONT_TAB_LINE}"`);
    expect(rust).toContain(`"${AX_OFF_LINE}"`);
    expect(rust).toContain(`"${SITE_UNKNOWN_NAME}"`);
  });

  it("says Safari and Firefox calls are not offered yet", () => {
    const all = Object.values(browserCallsLines(false)).join(" ");
    expect(all).toMatch(/Safari and Firefox are not offered yet/);
  });
});
