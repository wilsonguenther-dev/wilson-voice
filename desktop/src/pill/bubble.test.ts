/**
 * yap-1106-yappy-states-and-motion — the speech bubble: one at a time, a
 * 2-line clamp, a max width, never over the dock edge, copy from one table.
 */
import { describe, it, expect } from "vitest";
import FLOAT_CSS from "../float.css?raw";
import { DURATION } from "./motion";
import { BUBBLE_H, BUBBLE_W } from "./state";
import {
  BUBBLE_COPY, BUBBLE_DOCK_INSET_PX, BUBBLE_MAX_H_PX, BUBBLE_MAX_LINES, BUBBLE_MAX_WIDTH_PX,
  bubbleCentreX, bubbleCopy, bubbleExitMs, createBubbleQueue, fitsBubble, handsFreeExplainLine,
} from "./bubble";

function harness(reduceMotion = false) {
  const log: string[] = [];
  let text = "", shown = false;
  const timers = new Map<number, { at: number; fn: () => void }>();
  let now = 0, nextId = 1;
  const q = createBubbleQueue(
    { setText: (t) => { text = t; log.push(`text:${t}`); }, setShown: (on) => { shown = on; log.push(on ? "show" : "hide"); } },
    { after: (ms, fn) => { const id = nextId++; timers.set(id, { at: now + ms, fn }); return id; }, cancel: (id) => { timers.delete(id); } },
    { reduceMotion },
  );
  const advance = (ms: number) => {
    now += ms;
    for (const [id, t] of [...timers]) if (t.at <= now) { timers.delete(id); t.fn(); }
  };
  return { q, log, advance, state: () => ({ text, shown }), pending: () => timers.size };
}

describe("bubble queue", () => {
  it("never_shows_two_bubbles_a_new_line_waits_for_the_old_to_leave", () => {
    const h = harness();
    h.q.say("go on…");
    expect(h.state()).toEqual({ text: "go on…", shown: true });
    h.q.say("keep going!");
    // the old line is LEAVING, the new one not yet in
    expect(h.state()).toEqual({ text: "go on…", shown: false });
    h.advance(DURATION.bubbleOut - 1);
    expect(h.state().shown).toBe(false);
    h.advance(1);
    expect(h.state()).toEqual({ text: "keep going!", shown: true });
    expect(h.q.current()).toBe("keep going!");
  });

  it("latest_line_wins_while_leaving_and_empty_hides", () => {
    const h = harness();
    h.q.say("a"); h.q.say("b"); h.q.say("c");
    h.advance(DURATION.bubbleOut);
    expect(h.state()).toEqual({ text: "c", shown: true });
    h.q.say("");
    h.advance(DURATION.bubbleOut);
    expect(h.state()).toEqual({ text: "", shown: false });
    expect(h.q.current()).toBe("");
  });

  it("repeating_the_same_line_is_not_a_restart", () => {
    const h = harness();
    h.q.say("x"); const n = h.log.length;
    h.q.say("x");
    expect(h.log.length).toBe(n);
  });

  it("reduce_motion_uses_the_short_crossfade_and_dispose_cancels", () => {
    expect(bubbleExitMs(true)).toBe(DURATION.reducedCrossfade);
    expect(bubbleExitMs(false)).toBe(DURATION.bubbleOut);
    const h = harness(true);
    h.q.say("x"); h.q.say("y");
    expect(h.pending()).toBe(1);
    h.q.dispose();
    expect(h.pending()).toBe(0);
  });
});

describe("bubble geometry", () => {
  it("max_width_and_two_line_clamp_are_reserved_by_the_layout", () => {
    expect(BUBBLE_MAX_LINES).toBe(2);
    expect(BUBBLE_W).toBe(BUBBLE_MAX_WIDTH_PX);
    expect(BUBBLE_H).toBe(BUBBLE_MAX_H_PX);
    expect(FLOAT_CSS).toMatch(/\.kami-bubble-text\s*\{[^}]*-webkit-line-clamp:\s*2/);
    expect(FLOAT_CSS).toMatch(new RegExp(`\\.kami-bubble\\s*\\{[^}]*max-width:\\s*min\\(${BUBBLE_MAX_WIDTH_PX}px`));
  });

  it("has_its_own_enter_and_exit_animation_and_honours_reduced_motion", () => {
    expect(FLOAT_CSS).toMatch(/\.kami-bubble\.show\s*\{[^}]*var\(--dur-bubble-in/);
    expect(FLOAT_CSS).toMatch(/\.kami-bubble\s*\{[^}]*var\(--dur-bubble-out/);
    expect(FLOAT_CSS).toMatch(/prefers-reduced-motion: reduce\)\s*\{[^@]*\.kami-bubble,\s*\.kami-bubble\.show/);
  });

  it("never_overlaps_the_dock_edge", () => {
    const box = { x: 0, w: 220 };
    const bw = 120;
    for (const cx of [0, 10, 60, 110, 200, 220]) {
      const left = bubbleCentreX(cx, bw, box, "left") - bw / 2;
      expect(left, `left dock cx=${cx}`).toBeGreaterThanOrEqual(box.x + BUBBLE_DOCK_INSET_PX);
      const right = bubbleCentreX(cx, bw, box, "right") + bw / 2;
      expect(right, `right dock cx=${cx}`).toBeLessThanOrEqual(box.x + box.w - BUBBLE_DOCK_INSET_PX);
      const b = bubbleCentreX(cx, bw, box, "bottom");
      expect(b - bw / 2).toBeGreaterThanOrEqual(box.x);
      expect(b + bw / 2).toBeLessThanOrEqual(box.x + box.w);
    }
    // hidden bubble: centred on the capsule, no clamp to apply
    expect(bubbleCentreX(42, 0, box, "left")).toBe(42);
  });
});

describe("bubble copy", () => {
  it("copy_comes_from_one_table_total_over_every_tone", () => {
    for (const id of Object.keys(BUBBLE_COPY) as (keyof typeof BUBBLE_COPY)[]) {
      for (const tone of ["friendly", "rude", "rose"] as const) {
        expect(BUBBLE_COPY[id][tone], `${id}/${tone}`).toContain("{key}");
        expect(bubbleCopy(id, tone, "fn")).not.toContain("{key}");
      }
    }
  });

  it("hands_free_explanation_line_names_the_live_key_and_fits", () => {
    expect(handsFreeExplainLine("friendly", "fn")).toBe("Hands-free: I keep listening until you press fn");
    for (const tone of ["friendly", "rude", "rose"] as const) {
      for (const key of ["fn", "⌥Space", "⌃⌥⇧⌘Backspace"]) {
        const line = handsFreeExplainLine(tone, key);
        expect(line).toContain(key);
        expect(fitsBubble(line), `${tone}/${key}: ${line}`).toBe(true);
      }
    }
  });

  it("fits_bubble_rejects_a_three_line_sentence", () => {
    expect(fitsBubble("ok")).toBe(true);
    expect(fitsBubble("this sentence is far too long to ever fit inside two short lines of a tiny speech bubble")).toBe(false);
  });
});
