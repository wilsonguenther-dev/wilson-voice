/**
 * engineHealth.test — the pill renders each engine health state
 * (yap-1106-engine-owner). Server render, as license.render.test.tsx does.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { EngineHealthChip } from "./EngineHealthChip";
import {
  ENGINE_HEALTH_LABEL,
  ENGINE_HEALTH_STATES,
  engineHealthVisible,
  parseEngineHealth,
} from "./engineHealth";

describe("engine health chip", () => {
  it.each(ENGINE_HEALTH_STATES)("renders the %s state in plain words", (state) => {
    const html = renderToStaticMarkup(<EngineHealthChip state={state} />);
    expect(html).toContain(`data-state="${state}"`);
    expect(html).toContain(ENGINE_HEALTH_LABEL[state]);
    expect(html).toContain('role="status"');
  });

  it("says Catching up… while the engine is stuck", () => {
    expect(renderToStaticMarkup(<EngineHealthChip state="stuck" />)).toContain("Catching up…");
  });

  it("has one distinct label per state", () => {
    const labels = ENGINE_HEALTH_STATES.map((s) => ENGINE_HEALTH_LABEL[s]);
    expect(new Set(labels).size).toBe(ENGINE_HEALTH_STATES.length);
  });

  it("parses event payloads and falls back to idle", () => {
    expect(parseEngineHealth("stuck")).toBe("stuck");
    expect(parseEngineHealth("bogus")).toBe("idle");
    expect(parseEngineHealth(3)).toBe("idle");
  });

  it("shows only the states a person needs to see", () => {
    expect(ENGINE_HEALTH_STATES.filter(engineHealthVisible)).toEqual([
      "loading",
      "stuck",
      "restarting",
    ]);
  });
});
