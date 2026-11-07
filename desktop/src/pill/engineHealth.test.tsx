/**
 * engineHealth.test — the pill renders each engine health state
 * (yap-1106-engine-owner). Server render, as license.render.test.tsx does.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { EngineHealthChip } from "./EngineHealthChip";
import {
  ENGINE_HEALTH_LABEL,
  ENGINE_HEALTH_PHASES,
  ENGINE_HEALTH_STATES,
  engineHealthTakesStrip,
  engineHealthVisible,
  parseEngineHealth,
} from "./engineHealth";
import { PHASE_ID } from "./live";

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

describe("engine health in the pill strip", () => {
  it("takes the strip only while a decode is on screen and the state needs saying", () => {
    expect(engineHealthTakesStrip("stuck", "transcribing")).toBe(true);
    expect(engineHealthTakesStrip("restarting", "transcribing")).toBe(true);
    expect(engineHealthTakesStrip("loading", "model_loading")).toBe(true);
    expect(engineHealthTakesStrip("ready", "transcribing")).toBe(false);
    expect(engineHealthTakesStrip("busy", "transcribing")).toBe(false);
    expect(engineHealthTakesStrip("stuck", "listening")).toBe(false);
    expect(engineHealthTakesStrip("stuck", "error")).toBe(false);
  });

  it("every phase the chip may take is a real pill phase id", () => {
    const ids = Object.values(PHASE_ID);
    for (const id of ENGINE_HEALTH_PHASES) expect(ids).toContain(id);
  });
});

/**
 * yap-1106-engine-watchdog-restart — the event stream a stuck decode produces
 * (busy → stuck → restarting → ready), rendered the way float-main.tsx
 * renders the strip, next to a stand-in for the rest of the pill. The chip is
 * a plain status line: it never disables, covers or takes focus from anything.
 */
describe("engine health through a watchdog restart", () => {
  const MAIN_VIEW = '<button type="button" class="main-view">Dictate</button>';

  function pillFrame(payload: unknown) {
    const health = parseEngineHealth(payload);
    return renderToStaticMarkup(
      <div className="pill">
        <button type="button" className="main-view">
          Dictate
        </button>
        <div className="phase-strip">
          {engineHealthTakesStrip(health, "transcribing") ? (
            <EngineHealthChip state={health} />
          ) : (
            <span className="phase-line">Transcribing…</span>
          )}
        </div>
      </div>,
    );
  }

  it("moves stuck -> restarting -> ready while the main view stays interactive", () => {
    const frames = ["busy", "stuck", "restarting", "ready"].map(pillFrame);
    expect(frames[0]).toContain("phase-line");
    expect(frames[1]).toContain('data-state="stuck"');
    expect(frames[1]).toContain("Catching up…");
    expect(frames[2]).toContain('data-state="restarting"');
    expect(frames[2]).toContain("Restarting…");
    // Ready again: the chip leaves and the phase line is back.
    expect(frames[3]).not.toContain("engine-health");
    expect(frames[3]).toContain("phase-line");
    for (const html of frames) {
      // Every frame still renders the main view, enabled and reachable.
      expect(html).toContain(MAIN_VIEW);
      expect(html).not.toMatch(/\b(inert|disabled|aria-modal|aria-hidden|tabindex)\b/);
    }
  });

  it("keeps rendering when an unknown payload arrives mid-restart", () => {
    const html = pillFrame({ not: "a state" });
    expect(html).toContain(MAIN_VIEW);
    expect(html).toContain("phase-line");
  });
});
