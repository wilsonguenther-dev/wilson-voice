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
import { invoke } from "@tauri-apps/api/core";
import {
  FLOW_BAR_SLOTS, FlowBarSlots, layoutSlots, meetingNoteTitle, pressSlot, railFailed, railKeysFrom,
  railLogLine, railShown, SCRATCHPAD_WINDOW_LANDED, SLOT_GAP_PX, slotAxis, slotById, slotLength,
  subscribeRailFailure, tooltipSide, wordReadout, type RailBindingRow, type SlotCtx,
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
  it("is the five take-time slots plus item 18's rail, Dictate in the centre, and no trial numeral", () => {
    expect(FLOW_BAR_SLOTS.map((s) => s.id).sort()).toEqual([
      "cancel", "dictate", "hands-free-stop", "meeting", "meeting-stop", "menu", "note", "scratchpad", "wpm",
    ]);
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

// ── yap-1106-pill-notes-and-meeting / WF-03 — the resting rail ──────────────

const rows = (scratch: string, meeting: string, hold = "fn"): RailBindingRow[] => [
  { action: "dictate_hold", key: hold, liveKey: hold },
  { action: "open_scratchpad", key: scratch, liveKey: scratch },
  { action: "record_meeting", key: meeting, liveKey: null },
];
const railCtx = (over: Partial<SlotCtx> = {}): SlotCtx =>
  ctxOf({ phase: "idle", railOpen: true, keys: railKeysFrom(rows("⌃⌘J", "⌃⌘M")), ...over });
const ids = (ctx: SlotCtx) => layoutSlots(ctx).placements.map((p) => p.id);
const flush = () => new Promise((r) => setTimeout(r, 0));

describe("item 18 / WF-03 pill rail", () => {
  it("rail_shows_dictate_scratchpad_meeting_at_rest_and_note_stop_while_recording", () => {
    // Closed (no hover, no focus): the resting pill is the seed alone, as before.
    expect(ids(ctxOf({ phase: "idle" }))).toEqual(["dictate"]);
    expect(railShown(ctxOf({ phase: "idle" }))).toBe(false);
    // Hover/focus at rest: ONE rail — Scratchpad · Dictate · Meeting.
    expect(ids(railCtx())).toEqual(["scratchpad", "dictate", "meeting"]);
    // A meeting recording: Start gives way to Note and Stop meeting.
    expect(ids(railCtx({ meetingRecording: true, meetingOnScreen: true }))).toEqual(
      ["scratchpad", "dictate", "note", "meeting-stop"],
    );
    // Mid-take the rail never opens: the take owns the bar.
    expect(railShown(railCtx({ phase: "listening" }))).toBe(false);
    expect(ids(railCtx({ phase: "listening" }))).not.toContain("meeting");
  });

  it("tooltip_keycap_follows_a_rebind_without_restart", () => {
    const before = render(railCtx());
    expect(before).toContain('<kbd class="keycap">⌃⌘J</kbd>');
    expect(before).toContain('<kbd class="keycap">⌃⌘M</kbd>');
    // The user rebinds the Scratchpad: the next `hotkey_bindings` read is the
    // next render's keys. Same mount, same module, no restart.
    const after = render(railCtx({ keys: railKeysFrom(rows("⌥⌘N", "⌃⌘M")) }));
    expect(after).toContain('<kbd class="keycap">⌥⌘N</kbd>');
    expect(after).not.toContain("⌃⌘J");
    // Dictate's keycap is the live hold key.
    expect(slotById("dictate").keycap?.(railCtx({ keys: railKeysFrom(rows("x", "y", "fn⌃")) }))).toBe("fn⌃");
  });

  it("rail_keys_prefer_the_live_key_and_never_invent_one", () => {
    expect(railKeysFrom([{ action: "open_scratchpad", key: "⌃⌘J", liveKey: "⌃⌥J" }]).scratchpad).toBe("⌃⌥J");
    expect(railKeysFrom([{ action: "record_meeting", key: "⌃⌘M", liveKey: null }]).meeting).toBe("⌃⌘M");
    expect(railKeysFrom([{ action: "record_meeting", key: "  ", liveKey: null }]).meeting).toBeNull();
    expect(railKeysFrom(null)).toEqual({ dictate: null, scratchpad: null, meeting: null });
    // No key: no keycap drawn at all.
    expect(render(railCtx({ keys: railKeysFrom([]) }))).not.toContain("keycap");
  });

  it("rail_buttons_are_keyboard_reachable", () => {
    for (const ctx of [railCtx(), railCtx({ meetingRecording: true, meetingOnScreen: true })]) {
      const html = render(ctx);
      const buttons = [...html.matchAll(/<button[^>]*>/g)].map((m) => m[0]);
      expect(buttons.length).toBeGreaterThan(0);
      for (const b of buttons) {
        expect(b).toContain('type="button"');
        expect(b).not.toContain('tabindex="-1"');
        expect(b).toMatch(/aria-label="[^"]+"/);
      }
    }
    expect(render(railCtx())).toContain('aria-keyshortcuts="⌃⌘J"');
    // Enter/Space on a focused rail button is handled by the button itself
    // (FlowBarSlots onKeyDown -> pressSlot), the same path as a click.
    expect(pressSlot(slotById("meeting"), railCtx(), noop, false)).toBe(true);
  });

  it("rail_rotates_on_left_and_right_docks", () => {
    expect(slotAxis(railCtx({ dock: "bottom" }))).toBe("row");
    expect(slotAxis(railCtx({ dock: "left" }))).toBe("column");
    expect(slotAxis(railCtx({ dock: "right" }))).toBe("column");
    // The meeting banner stays a row at every dock (dock-geometry.json).
    expect(slotAxis(railCtx({ dock: "left", meetingRecording: true, meetingOnScreen: true }))).toBe("row");
    for (const dock of DOCKS) {
      const html = render(railCtx({ dock }));
      const sides = new Set([...html.matchAll(/data-tip-side="(\w+)"/g)].map((m) => m[1]));
      expect(sides).toEqual(new Set([tooltipSide(dock)]));
      expect(html).toContain('data-rail="open"');
      expect(html).toContain(`data-bar-axis="${dock === "bottom" ? "row" : "column"}"`);
    }
  });

  it("rail_actions_reach_the_backend_commands", async () => {
    const inv = vi.mocked(invoke);
    inv.mockClear();
    pressSlot(slotById("scratchpad"), railCtx(), noop, false);
    pressSlot(slotById("meeting-stop"), railCtx({ meetingRecording: true }), noop, false);
    pressSlot(slotById("note"), railCtx({ meetingRecording: true, meetingClock: "12:34", meetingTitle: "Standup" }), noop, false);
    await flush();
    await flush();
    const names = inv.mock.calls.map((c) => c[0]);
    expect(names).toContain("open_scratchpad");
    expect(names).toContain("stop_meeting");
    expect(inv).toHaveBeenCalledWith("save_note_draft", {
      id: null,
      title: "Meeting note · Standup · 12:34",
      content: "[12:34] ",
    });
    // Start meeting asks the backend's own kind picker first.
    inv.mockClear();
    pressSlot(slotById("meeting"), railCtx(), noop, false);
    await flush();
    expect(inv.mock.calls.map((c) => c[0])).toContain("meeting_kind_choices");
  });

  it("hands_free_has_a_clickable_stop_with_its_key", () => {
    const ctx = ctxOf({ phase: "listening-locked", stopKey: "⌃⌥Space" });
    const stop = slotById("hands-free-stop");
    expect(stop.visible(ctx)).toBe(true);
    expect(stop.visible(ctxOf({ phase: "listening" }))).toBe(false);
    expect(render(ctx)).toContain('<kbd class="keycap">⌃⌥Space</kbd>');
    const toggle = vi.fn();
    expect(pressSlot(stop, ctx, { toggleDictation: toggle }, false)).toBe(true);
    expect(toggle).toHaveBeenCalledTimes(1);
    // It fits the 30 px side-dock strip like every other take-time slot.
    expect(stop.acrossPx(ctxOf({ phase: "listening-locked", dock: "left" }))).toBeLessThanOrEqual(GEOMETRY.strip_px);
  });

  it("note_title_and_telemetry_carry_no_text", () => {
    expect(meetingNoteTitle("3:05")).toBe("Meeting note · 3:05");
    expect(meetingNoteTitle(null, "  ")).toBe("Meeting note · 0:00");
    expect(railLogLine("meeting_start", { kind: "in_person", dock: "left", ms: 12 })).toBe(
      "pill_rail meeting_start kind=in_person dock=left ms=12",
    );
    const seen: string[] = [];
    const un = subscribeRailFailure((s) => seen.push(s));
    railFailed("meeting_stop", "boom", "Meeting didn't stop — use Stop in the menu bar.");
    un();
    railFailed("meeting_stop", "boom", "ignored after unsubscribe");
    expect(seen).toEqual(["Meeting didn't stop — use Stop in the menu bar."]);
  });
});
