/**
 * Y8-B — the Flow-Bar affordance slots.
 *
 * The bar geometry is Y5-I's table, `dock-geometry.json`, read RAW here: the
 * thickness bound below is derived from the committed boxes directly (the
 * shorter side of each box), not from `barTokens`, so a slot that only fits
 * because the helper agrees with itself still fails. There is no second table.
 */
import { describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(() => Promise.resolve()) }));
vi.mock("@tauri-apps/api/menu", () => ({ Menu: { new: vi.fn() } }));

import GEOMETRY from "./dock-geometry.json";
import {
  FLOW_BAR_SLOTS, FlowBarSlots, layoutSlots, pressSlot, SCRATCHPAD_WINDOW_LANDED, SLOT_GAP_PX,
  slotById, slotLength, tooltipSide, wordReadout, type SlotCtx,
} from "./slots";
import type { BarDock } from "./dock";
import type { LivePhase } from "./live";

type Box = { width: number; height: number };
type Row = { orientation: string } & Record<BarDock, Box>;
const ROWS = GEOMETRY.phases as unknown as Record<string, Row>;
const DOCKS = GEOMETRY.docks as BarDock[];
/** Every TAKE phase the table carries (the meeting row is the shell's, not a LivePhase). */
const PHASES = Object.keys(ROWS).filter((p) => p !== "meeting-on-screen") as LivePhase[];

const ctxOf = (over: Partial<SlotCtx> = {}): SlotCtx => ({
  phase: "idle",
  dock: "bottom",
  meetingOnScreen: false,
  progress: null,
  listen: { estWords: 0, sec: 0 },
  scratchpadOnBar: true,
  ...over,
});
const noop = { toggleDictation: () => {} };
const render = (ctx: SlotCtx) =>
  ["lead", "trail"].map((group) =>
    renderToStaticMarkup(
      createElement(FlowBarSlots, { ctx, group: group as "lead" | "trail", actions: noop, dragged: () => false }),
    )).join("");

describe("Y8-B flow-bar slots", () => {
  it("is exactly the five slots the spec names, Dictate in the centre, and no trial numeral", () => {
    expect(FLOW_BAR_SLOTS.map((s) => s.id).sort()).toEqual(["cancel", "dictate", "menu", "scratchpad", "wpm"]);
    expect(FLOW_BAR_SLOTS.filter((s) => s.group === "centre").map((s) => s.id)).toEqual(["dictate"]);
    // Y2-B's numeral rides the capsule, never the slot row.
    expect(FLOW_BAR_SLOTS.some((s) => /trial|license|numeral/i.test(s.id))).toBe(false);
    const listening = layoutSlots(ctxOf({ phase: "listening" })).placements.map((p) => p.id);
    const d = listening.indexOf("dictate");
    expect(listening.slice(0, d).every((id) => slotById(id).group === "lead")).toBe(true);
    expect(listening.slice(d + 1).every((id) => slotById(id).group === "trail")).toBe(true);
  });

  it("every_slot_renders_within_the_thickness_bound_in_all_three_docks", () => {
    let checked = 0;
    for (const phase of PHASES) {
      for (const dock of DOCKS) {
        const box = ROWS[phase][dock];
        const thickness = Math.min(box.width, box.height);
        const ctx = ctxOf({ phase, dock, progress: phase === "transcribing" ? { chunk: 2, of: 4, wordsSoFar: 40 } : null, listen: { estWords: 20, sec: 10 } });
        const { placements, axis } = layoutSlots(ctx);
        for (const p of placements) {
          expect(p.across, `${p.id} @ ${phase}/${dock}`).toBeLessThanOrEqual(thickness);
          if (axis === "column") expect(p.across).toBeLessThanOrEqual(GEOMETRY.strip_px);
          checked++;
        }
        // The Scratchpad is gated off until DB-D lands; its box must still fit
        // every phase it is designed to appear in, so flipping the flag is safe.
        if (phase === "listening") expect(slotById("scratchpad").acrossPx(ctx)).toBeLessThanOrEqual(thickness);
      }
    }
    // Not vacuous: the four take-time slots were really placed in a side-dock column.
    const col = layoutSlots(ctxOf({ phase: "listening", dock: "left", listen: { estWords: 20, sec: 10 } }));
    expect(col.axis).toBe("column");
    expect(col.placements.map((p) => p.id)).toEqual(["cancel", "dictate", "wpm", "menu"]);
    expect(checked).toBeGreaterThan(PHASES.length * DOCKS.length);
  });

  it("tooltip_side_flips_per_dock_edge", () => {
    expect(tooltipSide("bottom")).toBe("top");
    expect(tooltipSide("left")).toBe("right");
    expect(tooltipSide("right")).toBe("left");
    // Never toward the edge the bar is docked to — that is the tooltip that opens off-screen.
    for (const dock of DOCKS) expect(tooltipSide(dock)).not.toBe(dock);
    for (const dock of DOCKS) {
      const html = render(ctxOf({ phase: "listening", dock }));
      const sides = [...html.matchAll(/data-tip-side="(\w+)"/g)].map((m) => m[1]);
      expect(sides.length).toBeGreaterThan(0);
      expect(new Set(sides)).toEqual(new Set([tooltipSide(dock)]));
    }
  });

  it("an_invisible_slot_occupies_zero_length", () => {
    const idle = ctxOf({ phase: "idle" });
    const cancel = slotById("cancel");
    expect(cancel.visible(idle)).toBe(false);
    expect(slotLength(cancel, idle)).toBe(0);
    expect(layoutSlots(idle).placements.map((p) => p.id)).toEqual(["dictate"]);
    expect(layoutSlots(idle).length).toBe(slotById("dictate").lengthPx);
    // Not rendered at all: no element, no gap.
    expect(render(idle)).toBe("");
    // A visible set pays a gap only BETWEEN visible slots.
    const live = layoutSlots(ctxOf({ phase: "listening" }));
    const sum = live.placements.reduce((n, p) => n + p.length, 0);
    expect(live.length).toBe(sum + SLOT_GAP_PX * (live.placements.length - 1));
    // The Scratchpad toggle is ON in ctxOf, but the window (DB-D) has not landed:
    // a slot whose feature has not landed takes no length and draws nothing.
    expect(SCRATCHPAD_WINDOW_LANDED).toBe(false);
    expect(slotLength(slotById("scratchpad"), ctxOf({ phase: "listening" }))).toBe(0);
    expect(render(ctxOf({ phase: "listening" }))).not.toContain('data-slot="scratchpad"');
    expect(render(ctxOf({ phase: "listening" }))).toContain('data-slot="cancel"');
  });

  it("wpm_uses_real_words_while_transcribing", () => {
    // 60 s listened; the estimate says 300 words, the decoder has really produced 120.
    const listen = { estWords: 300, sec: 60 };
    const xs = wordReadout(ctxOf({ phase: "transcribing", progress: { chunk: 3, of: 12, wordsSoFar: 120 }, listen }));
    expect(xs).toEqual({ words: 120, wpm: 120, estimate: false });
    // While listening there is no text yet: the estimate, and it says so.
    const ls = wordReadout(ctxOf({ phase: "listening", listen }));
    expect(ls).toEqual({ words: 300, wpm: 300, estimate: true });
    expect(render(ctxOf({ phase: "listening", listen }))).toContain("~300");
    // Any other phase: no number at all, and the slot is gone.
    expect(wordReadout(ctxOf({ phase: "polishing", listen }))).toBeNull();
    expect(slotById("wpm").visible(ctxOf({ phase: "done", listen }))).toBe(false);
    // Too short to state a rate honestly.
    expect(wordReadout(ctxOf({ phase: "listening", listen: { estWords: 2, sec: 1 } }))?.wpm).toBeNull();
  });

  it("a press that became a drag never presses the slot under it (YV65 owns the gesture)", () => {
    const toggle = vi.fn();
    const ctx = ctxOf({ phase: "listening" });
    expect(pressSlot(slotById("dictate"), ctx, { toggleDictation: toggle }, true)).toBe(false);
    expect(toggle).not.toHaveBeenCalled();
    expect(pressSlot(slotById("dictate"), ctx, { toggleDictation: toggle }, false)).toBe(true);
    expect(toggle).toHaveBeenCalledTimes(1);
    // A readout is not a control, and an invisible slot cannot be pressed.
    expect(pressSlot(slotById("wpm"), ctx, noop, false)).toBe(false);
    expect(pressSlot(slotById("cancel"), ctxOf({ phase: "idle" }), noop, false)).toBe(false);
    // No slot handles pointerdown: the capsule's drag handlers receive it.
    expect(render(ctx)).not.toMatch(/pointerdown/i);
  });
});
