/**
 * yap-1107-one-pill-during-meeting — Wilson, 2026-11-07 04:56Z, on a recording
 * meeting: "why is the recording and the actual pill next to each other? That
 * was never part of the plan." A meeting is ONE pill: the clock is a segment of
 * one shell flush to Yappy, and at rest the meeting scene IS the pill.
 */
import { describe, expect, it } from "vitest";
import { ALL_LIVE_PHASES } from "./live";
import { BAR_DOCKS } from "./dock";
import { stackClockLabel } from "./meeting";
import {
  CLOCK_H, CLOCK_STACK_W, CLOCK_W, RESTING_PHASES,
  boxGap, clockWidth, layoutProblems, layoutVars, meetingShellBox, meetingSurfaceProblems, pillLayout, unionBox,
  type MeetingMode,
} from "./state";

const MEETINGS: readonly MeetingMode[] = ["none", "recording", "after"];

describe("one pill during a meeting", () => {
  it("every_meeting_state_is_one_surface", () => {
    const bad: string[] = [];
    for (const phase of ALL_LIVE_PHASES) for (const meeting of MEETINGS) for (const dock of BAR_DOCKS) {
      const l = pillLayout({ phase, meeting, dock });
      for (const p of meetingSurfaceProblems(l)) bad.push(`${phase}/${meeting}/${dock}: ${p}`);
      for (const p of layoutProblems(l)) bad.push(`${phase}/${meeting}/${dock}: ${p}`);
    }
    expect(bad).toEqual([]);
  });

  it("the_clock_is_flush_to_yappy_never_a_capsule_a_gap_away", () => {
    for (const phase of ALL_LIVE_PHASES) for (const meeting of ["recording", "after"] as const) for (const dock of BAR_DOCKS) {
      const l = pillLayout({ phase, meeting, dock });
      expect(l.clock).not.toBeNull();
      expect(boxGap(l.clock!, l.char)).toBe(0);
      const side = dock === "left" || dock === "right";
      expect(l.shell).toEqual(meetingShellBox(l.char, l.clock!, side));
      if (!side) expect(l.shell).toEqual(unionBox([l.char, l.clock!]));
    }
  });

  it("a_resting_recording_meeting_draws_the_scene_as_the_pill", () => {
    for (const phase of Array.from(RESTING_PHASES)) {
      const l = pillLayout({ phase, meeting: "recording", dock: "bottom" });
      expect(l.sceneCharacter).toBe(true);
      expect(l.clock).toEqual(l.char);
      expect(l.shell).toEqual(l.char);
      expect([l.char.w, l.char.h]).toEqual([CLOCK_W.recording, CLOCK_H]);
    }
  });

  it("a_side_dock_meeting_keeps_the_narrow_tab_and_joins_the_clock_flush_to_it", () => {
    // yap-1106-pill-vertical-dock-world keeps the side shell <= 64 px wide; the
    // meeting clock is the inner-side segment of the same one shell, level with him.
    for (const phase of ["idle", "sleepy", "listening"] as const) for (const dock of ["left", "right"] as const) {
      const l = pillLayout({ phase, meeting: "recording", dock });
      expect(l.sceneCharacter).toBe(false);
      expect(l.char.w).toBeLessThanOrEqual(64);
      expect(boxGap(l.clock!, l.char)).toBe(0);
      if (dock === "left") expect(l.clock!.x).toBe(l.char.x + l.char.w);
      else expect(l.clock!.x + l.clock!.w).toBe(l.char.x);
      // level with him: the clock's rows lie inside his, so the shell is one band
      expect(l.clock!.y).toBeGreaterThanOrEqual(l.char.y);
      expect(l.clock!.y + l.clock!.h).toBeLessThanOrEqual(l.char.y + l.char.h);
      expect(l.shell!.h).toBeCloseTo(CLOCK_H, 6);
      expect(l.shell!.w).toBeCloseTo(l.char.w + l.clock!.w, 6);
      // the shell never sits behind the bubble or the hint
      expect(boxGap(l.shell!, l.bubble)).toBeGreaterThan(0);
      expect(boxGap(l.shell!, l.hint)).toBeGreaterThan(0);
      expect(meetingSurfaceProblems(l)).toEqual([]);
    }
  });

  it("dictating_mid_meeting_shrinks_the_clock_to_the_stack_beside_stop", () => {
    for (const dock of BAR_DOCKS) {
      const l = pillLayout({ phase: "listening", meeting: "recording", dock });
      expect(l.sceneCharacter).toBe(false);
      expect(l.clock!.w).toBe(CLOCK_STACK_W);
    }
    expect(clockWidth("recording", false)).toBe(CLOCK_STACK_W);
    expect(clockWidth("recording", true)).toBe(CLOCK_W.recording);
    expect(clockWidth("after", false)).toBe(CLOCK_W.after);
  });

  it("no_meeting_means_no_shell_and_yappy_draws_his_own_capsule", () => {
    for (const phase of ALL_LIVE_PHASES) for (const dock of BAR_DOCKS) {
      const l = pillLayout({ phase, meeting: "none", dock });
      expect(l.shell).toBeNull();
      expect(l.sceneCharacter).toBe(false);
    }
  });

  it("the_after_badge_shares_the_shell_with_the_resting_capsule", () => {
    const l = pillLayout({ phase: "idle", meeting: "after", dock: "bottom" });
    expect(l.sceneCharacter).toBe(false);
    expect(l.clock!.x).toBeCloseTo(l.char.x + l.char.w, 5);
  });

  it("layout_vars_publish_the_shell_box", () => {
    const l = pillLayout({ phase: "idle", meeting: "recording", dock: "bottom" });
    const v = layoutVars(l);
    expect(v["--shell-w"]).toBe(`${Math.round(l.shell!.w * 100) / 100}px`);
    expect(v["--shell-h"]).toBe(`${Math.round(l.shell!.h * 100) / 100}px`);
    expect(layoutVars(pillLayout({ phase: "idle", meeting: "none", dock: "bottom" }))["--shell-w"]).toBe("0px");
  });

  it("the_stack_clock_reads_mm_ss_under_an_hour", () => {
    expect(stackClockLabel("00:03:37")).toBe("03:37");
    expect(stackClockLabel("01:03:37")).toBe("01:03:37");
    expect(stackClockLabel("")).toBe("");
  });
});
