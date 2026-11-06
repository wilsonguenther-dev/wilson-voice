/**
 * yap-1106-one-pill — the reducer tests for the ONE pill layout.
 *
 * Every dictation phase x {no meeting, recording, after-stop} x dock must give
 * one layout where the hint, bubble, clock, progress and character boxes never
 * overlap, the clock touches or sits within 12 pt of the character, and the
 * panel frame equals the union of its content boxes plus the margin. The
 * painted version of the same checks is scripts/pill-matrix.mjs.
 */
import { describe, expect, it } from "vitest";
import { ALL_LIVE_PHASES } from "./live";
import { BAR_DOCKS } from "./dock";
import {
  layoutCharacterFor,
  CHARACTER_OPEN, CHARACTER_REST, CLOCK_REACH_PT, LEGACY_PANEL, PANEL_MARGIN, SHRINK_HOLD_MS,
  boxGap, boxesOverlap, capsuleCentre, currentPillLayout, layoutProblems, layoutVars, meetingMode, pillLayout,
  publishPillLayout, shrinkDelay, unionBox, type MeetingMode, type PillLayout,
} from "./state";

const MEETINGS: readonly MeetingMode[] = ["none", "recording", "after"];

function every(fn: (l: PillLayout) => void) {
  let n = 0;
  for (const phase of ALL_LIVE_PHASES) for (const meeting of MEETINGS) for (const dock of BAR_DOCKS) {
    fn(pillLayout({ phase, meeting, dock }));
    n += 1;
  }
  return n;
}

describe("pill/state — one pill, one layout", () => {
  it("every_phase_x_meeting_x_dock_is_clean", () => {
    const bad: string[] = [];
    const n = every((l) => {
      for (const p of layoutProblems(l)) bad.push(`${l.phase}/${l.meeting}/${l.dock}: ${p}`);
    });
    expect(n).toBe(ALL_LIVE_PHASES.length * MEETINGS.length * BAR_DOCKS.length);
    expect(n).toBeGreaterThan(30); // the matrix really ran
    expect(bad).toEqual([]);
  });

  it("no_slot_overlaps_the_character_or_another_slot", () => {
    every((l) => {
      const slots = [l.bubble, l.hint, l.clock, l.progress].filter((b): b is NonNullable<typeof b> => b !== null);
      for (const s of slots) expect(boxesOverlap(s, l.char)).toBe(false);
      for (let i = 0; i < slots.length; i++) for (let j = i + 1; j < slots.length; j++) {
        expect(boxesOverlap(slots[i], slots[j])).toBe(false);
      }
    });
  });

  it("the_clock_sits_within_12pt_of_the_character_at_every_dock", () => {
    every((l) => {
      if (l.meeting === "none") {
        expect(l.clock).toBeNull();
        return;
      }
      expect(l.clock).not.toBeNull();
      expect(boxGap(l.clock!, l.char)).toBeLessThanOrEqual(CLOCK_REACH_PT);
    });
  });

  it("the_panel_frame_equals_the_union_of_its_content_boxes_plus_margin", () => {
    every((l) => {
      const boxes = [l.char, l.bubble, l.hint, ...(l.clock ? [l.clock] : []), ...(l.progress ? [l.progress] : [])];
      expect(l.card).toBeNull();
      const u = unionBox(boxes);
      expect(u.x).toBeCloseTo(PANEL_MARGIN, 5);
      expect(u.y).toBeCloseTo(PANEL_MARGIN, 5);
      expect(l.panel.w).toBe(Math.ceil(u.w + 2 * PANEL_MARGIN));
      expect(l.panel.h).toBe(Math.ceil(u.h + 2 * PANEL_MARGIN));
    });
  });

  it("bottom_dock_puts_the_clock_in_yappys_trailing_slot_not_under_him", () => {
    const l = pillLayout({ phase: "idle", meeting: "recording", dock: "bottom" });
    expect(l.clock!.x).toBeGreaterThanOrEqual(l.char.x + l.char.w);
    // vertically centred on him: a slot of the pill, never a second capsule below it
    expect(l.clock!.y + l.clock!.h / 2).toBeCloseTo(l.char.y + l.char.h / 2, 5);
    expect(l.clock!.y).toBeLessThan(l.char.y + l.char.h);
  });

  it("side_docks_keep_the_clock_and_hint_flush_to_yappys_edge", () => {
    for (const phase of ["idle", "listening-locked"] as const) {
      const left = pillLayout({ phase, meeting: "recording", dock: "left" });
      expect(left.char.x).toBe(PANEL_MARGIN);
      expect(left.clock!.x).toBe(left.char.x);
      expect(left.hint.x).toBe(left.char.x);
      const right = pillLayout({ phase, meeting: "recording", dock: "right" });
      const edge = right.panel.w - PANEL_MARGIN;
      expect(right.char.x + right.char.w).toBeCloseTo(edge, 5);
      expect(right.clock!.x + right.clock!.w).toBeCloseTo(edge, 5);
      expect(right.hint.x + right.hint.w).toBeCloseTo(edge, 5);
    }
  });

  it("the_hands_free_hint_is_never_drawn_over_yappys_face", () => {
    for (const dock of BAR_DOCKS) {
      const l = pillLayout({ phase: "listening-locked", meeting: "recording", dock });
      expect(l.hint.y + l.hint.h).toBeLessThanOrEqual(l.char.y);
      expect(l.bubble.y + l.bubble.h).toBeLessThanOrEqual(l.char.y);
    }
  });

  it("the_character_box_is_the_capsule_envelope_for_its_phase_class", () => {
    const rest = pillLayout({ phase: "idle", meeting: "none", dock: "bottom" });
    expect([rest.char.w, rest.char.h]).toEqual([CHARACTER_REST.w, CHARACTER_REST.h]);
    const open = pillLayout({ phase: "listening", meeting: "none", dock: "bottom" });
    expect([open.char.w, open.char.h]).toEqual([CHARACTER_OPEN.w, CHARACTER_OPEN.h]);
    // the widest open capsule YappyPill draws (236 x 60 at the 1.16 desk tier) fits
    expect(236 * 1.16).toBeLessThanOrEqual(CHARACTER_OPEN.w);
    expect(60 * 1.16).toBeLessThanOrEqual(CHARACTER_OPEN.h);
  });

  it("transcribing_reserves_the_progress_row_under_yappy", () => {
    for (const dock of BAR_DOCKS) {
      const l = pillLayout({ phase: "transcribing", meeting: "recording", dock });
      expect(l.progress).not.toBeNull();
      expect(l.progress!.y).toBeGreaterThanOrEqual(l.char.y + l.char.h);
      expect(pillLayout({ phase: "listening", meeting: "none", dock }).progress).toBeNull();
    }
  });

  it("capsule_centre_hugs_the_dock_edge_and_centres_at_the_bottom", () => {
    const left = pillLayout({ phase: "listening", meeting: "none", dock: "left" });
    expect(capsuleCentre(left, 72).x - 36).toBe(left.char.x);
    const right = pillLayout({ phase: "listening", meeting: "none", dock: "right" });
    expect(capsuleCentre(right, 72).x + 36).toBeCloseTo(right.char.x + right.char.w, 5);
    const bottom = pillLayout({ phase: "listening", meeting: "none", dock: "bottom" });
    expect(capsuleCentre(bottom, 72)).toEqual(bottom.anchor);
  });

  it("the_anchor_is_the_character_centre", () => {
    every((l) => {
      expect(l.anchor.x).toBeCloseTo(l.char.x + l.char.w / 2, 5);
      expect(l.anchor.y).toBeCloseTo(l.char.y + l.char.h / 2, 5);
    });
  });

  it("a_legacy_character_keeps_the_fixed_panel", () => {
    const l = pillLayout({ phase: "listening", meeting: "recording", dock: "left", character: "legacy" });
    expect(l.legacy).toBe(true);
    expect(l.panel).toEqual({ w: LEGACY_PANEL.w, h: LEGACY_PANEL.h });
    expect(l.anchor).toEqual({ x: 150, y: 70 });
    expect(layoutProblems(l)).toEqual([]);
  });

  it("meeting_mode_ranks_the_post_stop_badge_over_recording", () => {
    expect(meetingMode({ recording: false }, null)).toBe("none");
    expect(meetingMode({ recording: true }, null)).toBe("recording");
    const after = { kind: "processing", id: "m", label: "Transcribing 4%", title: "t", percent: 4, opens: false } as unknown as Parameters<typeof meetingMode>[1];
    expect(meetingMode({ recording: false }, after)).toBe("after");
    expect(meetingMode(null, undefined)).toBe("none");
  });

  it("the_layout_key_changes_only_with_its_geometry", () => {
    const a = pillLayout({ phase: "listening", meeting: "none", dock: "bottom" });
    const b = pillLayout({ phase: "polishing", meeting: "none", dock: "bottom" });
    expect(a.key).toBe(b.key);
    expect(a.panel).toEqual(b.panel);
    expect(pillLayout({ phase: "transcribing", meeting: "none", dock: "bottom" }).key).not.toBe(a.key);
    expect(pillLayout({ phase: "listening", meeting: "recording", dock: "bottom" }).key).not.toBe(a.key);
  });

  it("the_panel_grows_at_once_and_shrinks_late", () => {
    const rest = pillLayout({ phase: "idle", meeting: "none", dock: "bottom" });
    const open = pillLayout({ phase: "listening", meeting: "none", dock: "bottom" });
    expect(shrinkDelay(rest, open)).toBe(0);
    expect(shrinkDelay(open, rest)).toBe(SHRINK_HOLD_MS);
    expect(shrinkDelay(null, rest)).toBe(0);
    // a dock change is a move, never held
    expect(shrinkDelay(open, pillLayout({ phase: "idle", meeting: "none", dock: "left" }))).toBe(0);
  });

  it("layout_vars_carry_every_slot", () => {
    const v = layoutVars(pillLayout({ phase: "transcribing", meeting: "recording", dock: "right" }));
    for (const k of ["--panel-w", "--char-x", "--char-y", "--char-w", "--char-h", "--bubble-y", "--hint-x", "--hint-y", "--hint-w", "--clock-x", "--clock-y", "--clock-w", "--progress-y"]) {
      expect(v[k]).toMatch(/^-?\d+(\.\d+)?px$/);
    }
  });

  it("a_card_on_screen_gets_its_own_row_and_grows_the_panel", () => {
    for (const dock of BAR_DOCKS) {
      const bare = pillLayout({ phase: "idle", meeting: "recording", dock });
      const l = pillLayout({ phase: "idle", meeting: "recording", dock, card: { h: 96 } });
      expect(l.card).not.toBeNull();
      expect(l.card!.h).toBe(96);
      expect(l.card!.y + l.card!.h).toBeLessThanOrEqual(l.hint.y);
      expect(l.panel.h).toBeGreaterThan(bare.panel.h);
      expect(l.key).not.toBe(bare.key);
      expect(layoutProblems(l)).toEqual([]);
    }
  });

  it("layout_problems_is_not_blind", () => {
    const good = pillLayout({ phase: "idle", meeting: "recording", dock: "left" });
    expect(layoutProblems(good)).toEqual([]);
    // the clock dropped onto the character, the old free-floating badge far away, and a wrong frame
    const onFace: PillLayout = { ...good, clock: { ...good.clock!, x: good.char.x, y: good.char.y } };
    expect(layoutProblems(onFace).join(";")).toMatch(/overlap character\/clock/);
    const stranded: PillLayout = { ...good, clock: { ...good.clock!, x: good.char.x + 150, y: good.char.y + 60 } };
    expect(layoutProblems(stranded).join(";")).toMatch(/from the character/);
    const fixed: PillLayout = { ...good, panel: { w: 300, h: 140 } };
    expect(layoutProblems(fixed).join(";")).toMatch(/union\+margin/);
  });

  it("the_published_layout_is_what_yappys_draw_loop_reads", () => {
    const l = pillLayout({ phase: "listening", meeting: "recording", dock: "right" });
    publishPillLayout(l);
    expect(currentPillLayout()).toBe(l);
    publishPillLayout(null);
    expect(currentPillLayout()).toBeNull();
  });

  it("a_character_change_is_never_held", () => {
    const yappy = pillLayout({ phase: "listening", meeting: "none", dock: "bottom" });
    const legacy = pillLayout({ phase: "idle", meeting: "none", dock: "bottom", character: "legacy" });
    expect(shrinkDelay(yappy, legacy)).toBe(0);
    expect(shrinkDelay(legacy, yappy)).toBe(0);
  });

  it("a_painted_slot_outside_the_panel_is_reported", () => {
    const good = pillLayout({ phase: "idle", meeting: "recording", dock: "bottom" });
    const out: PillLayout = { ...good, hint: { ...good.hint, x: -40 } };
    expect(layoutProblems(out).join(";")).toMatch(/hint outside the panel/);
  });
});

describe("layoutCharacterFor", () => {
  it("a character with its own stage lays out as yappy, one without as legacy", () => {
    expect(layoutCharacterFor(true)).toBe("yappy");
    expect(layoutCharacterFor(false)).toBe("legacy");
  });
});
