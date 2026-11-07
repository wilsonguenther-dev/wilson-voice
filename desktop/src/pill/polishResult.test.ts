import { describe, expect, it } from "vitest";
import {
  POLISH_RESULT_EVENT,
  REVEAL_EDIT_TTL_MS,
  hasRevealableEdit,
  isPolishResult,
  revealEditEffect,
  revealEditLine,
  type PolishResult,
} from "./polishResult";

const cloud: PolishResult = {
  raw: "um so lets meet on monday no tuesday",
  polished: "Let's meet on Tuesday.",
  route: "cloud",
  reason: "online",
};

describe("polish result (yap-1106 → reveal-edit card)", () => {
  it("mirrors the Rust event name and the ~4 s pill line", () => {
    expect(POLISH_RESULT_EVENT).toBe("polish_result");
    expect(REVEAL_EDIT_TTL_MS).toBe(4_000);
  });

  it("accepts the {raw, polished, route, reason} payload and rejects anything else", () => {
    expect(isPolishResult(cloud)).toBe(true);
    expect(isPolishResult({ ...cloud, route: "local", reason: "timeout" })).toBe(true);
    expect(isPolishResult({ ...cloud, route: "elsewhere" })).toBe(false);
    expect(isPolishResult({ raw: "a", polished: "b" })).toBe(false);
    expect(isPolishResult(null)).toBe(false);
    expect(isPolishResult("polish_result")).toBe(false);
  });

  it("offers Reveal edit only when the polish changed the words", () => {
    expect(hasRevealableEdit(cloud)).toBe(true);
    expect(revealEditLine(cloud)).toBe("Polished · Reveal edit");
    const same = { ...cloud, polished: `  ${cloud.raw} ` };
    expect(hasRevealableEdit(same)).toBe(false);
    expect(revealEditLine(same)).toBeNull();
    expect(revealEditLine({ ...cloud, raw: "  " })).toBeNull();
  });

  it("Keep leaves the paste alone", () => {
    expect(revealEditEffect(cloud, "keep", true)).toEqual({ kind: "none" });
    expect(revealEditEffect(cloud, "keep", false)).toEqual({ kind: "none" });
  });

  it("Use original re-pastes the raw take, or copies it when the field changed", () => {
    expect(revealEditEffect(cloud, "use_original", true)).toEqual({ kind: "paste", text: cloud.raw });
    const copied = revealEditEffect(cloud, "use_original", false);
    expect(copied.kind).toBe("copy");
    if (copied.kind === "copy") {
      expect(copied.text).toBe(cloud.raw);
      expect(copied.notice).toMatch(/clipboard/);
    }
    expect(revealEditEffect({ ...cloud, polished: cloud.raw }, "use_original", true)).toEqual({ kind: "none" });
  });

  it("is the same contract for a local polish (the card covers both routes)", () => {
    const local = { ...cloud, route: "local" as const, reason: "offline" };
    expect(revealEditEffect(local, "use_original", true)).toEqual({ kind: "paste", text: local.raw });
  });
});
