/** Y5-K — the registry: one id -> one creature, and an unknown or retired id is yappy. */
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn(() => Promise.resolve(() => {})) }));

import { FALLBACK_CHARACTER, RETIRED_CHARACTERS, characters, registerCharacter, resolveCharacter, shellBody } from "./registry";
import { classic } from "./classic";
import { yappy } from "./yappy";

describe("character registry", () => {
  it("ships yappy first; the retired classic is not registered", () => {
    expect(characters()[0]).toBe(yappy);
    expect(characters()).not.toContain(classic);
    expect(characters().map((c) => c.id)).not.toContain("classic");
    expect(FALLBACK_CHARACTER).toBe("yappy");
  });

  it("resolves pill-style setting once, and an unknown, empty, missing or retired id falls back to yappy", () => {
    expect(resolveCharacter("yappy")).toBe(yappy);
    for (const id of ["unicorn", "", null, undefined, "Yappy", ...RETIRED_CHARACTERS]) expect(resolveCharacter(id)).toBe(yappy);
  });

  it("classic is retired: a saved \"classic\" resolves to yappy, never to the old capsule", () => {
    expect(RETIRED_CHARACTERS).toContain("classic");
    expect(resolveCharacter("classic")).toBe(yappy);
    expect(resolveCharacter("classic")).not.toBe(classic);
  });

  it("the shell's neutral body is the old capsule's painter, kept out of the registry", () => {
    expect(shellBody()).toBe(classic);
    expect(characters()).not.toContain(shellBody());
  });

  it("re-registering the same creature is a no-op; a different one under a taken id throws", () => {
    const before = characters().length;
    expect(registerCharacter(yappy)).toBe(yappy);
    expect(characters().length).toBe(before);
    expect(() => registerCharacter({ ...yappy })).toThrow(/already registered/);
    expect(() => registerCharacter({ id: " ", label: "x", render: () => null })).toThrow(/needs an id/);
  });

  it("only Yappy declares a whole-window stage; the shell body paints in the shell's body slot", () => {
    expect(typeof yappy.stage).toBe("object");
    expect(classic.stage).toBeUndefined();
  });
});
