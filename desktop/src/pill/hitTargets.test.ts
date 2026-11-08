/**
 * yap-1106-fb-02-pill-first-click — every pill button works on the first click.
 * Pins the webview half: the click-through region is the LIST of rects the
 * controls draw in (each at least 24 x 24 pt), the centre and four inner
 * corners of every control are inside it in every dock, a press holds the panel,
 * and every click logs `pill.click {control, handled}`. The browser half (real
 * clicks on real elements) is scripts/pill-matrix.mjs --clicks; the AppKit half
 * is float_pill.rs `every_control_centre_and_inner_corner_is_a_hit`.
 */
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Rect4 } from "./drag";

const invoke = vi.fn((..._args: unknown[]) => Promise.resolve());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

type Drag = typeof import("./drag");
type Hit = typeof import("./hitTargets");
let drag: Drag;
let hit: Hit;

function lastList(): Rect4[] {
  const calls = invoke.mock.calls.filter((c) => c[0] === "pill_set_hitboxes");
  if (!calls.length) return [];
  const { rects } = calls[calls.length - 1][1] as { rects: { x: number; y: number; w: number; h: number }[] };
  return rects.map((r) => [r.x, r.y, r.w, r.h] as Rect4);
}

beforeEach(async () => {
  vi.resetModules();
  invoke.mockClear();
  drag = await import("./drag");
  hit = await import("./hitTargets");
});
afterEach(() => vi.unstubAllGlobals());

// One geometry per dock: the capsule, the meeting stop in the clock slot, the
// Trial-ended chip and a card's button — where yap-1106-one-pill lays them out.
const DOCKS: Record<string, { capsule: Rect4; controls: Record<string, Rect4> }> = {
  bottom: {
    capsule: [114, 67, 72, 22],
    controls: { "meeting-stop": [196, 68, 20, 20], "trial-ended": [100, 40, 100, 26], "copy-card-close": [262, 8, 18, 18] },
  },
  left: {
    capsule: [10, 59, 72, 22],
    controls: { "meeting-stop": [30, 90, 20, 20], "trial-ended": [6, 20, 96, 26], "copy-card-close": [80, 8, 18, 18] },
  },
  right: {
    capsule: [218, 59, 72, 22],
    controls: { "meeting-stop": [250, 90, 20, 20], "trial-ended": [198, 20, 96, 26], "copy-card-close": [270, 8, 18, 18] },
  },
};

describe("padToMin / probePoints", () => {
  it("grows a control under 24 x 24 pt about its centre and leaves a big one alone", () => {
    expect(hit.padToMin([196, 68, 20, 20])).toEqual([194, 66, 24, 24]);
    expect(hit.padToMin([0, 0, 100, 26])).toEqual([0, 0, 100, 26]);
    expect(hit.MIN_HIT_PT).toBe(24);
  });
  it("is the centre plus the four inner corners, every one inside the control", () => {
    const pts = hit.probePoints([10, 20, 30, 24]);
    expect(pts).toEqual([
      [25, 32],
      [11, 21],
      [39, 21],
      [11, 43],
      [39, 43],
    ]);
    for (const [x, y] of pts) expect(hit.pointInRects(x, y, [[10, 20, 30, 24]])).toBe(true);
  });
});

describe("the click-through region is the controls' own rects", () => {
  for (const [dock, g] of Object.entries(DOCKS)) {
    it(`(${dock} dock) centre + 4 inner corners of every control are hits; empty glass is not`, () => {
      drag.reportPillHitbox(...g.capsule);
      const padded = Object.values(g.controls).map((r) => hit.padToMin(r));
      drag.extendPillHitboxRects(padded, drag.CONTROLS_OWNER);
      const list = lastList();
      expect(list).toHaveLength(1 + padded.length);
      for (const r of list.slice(1)) {
        expect(r[2]).toBeGreaterThanOrEqual(24);
        expect(r[3]).toBeGreaterThanOrEqual(24);
      }
      for (const [name, r] of Object.entries(g.controls)) {
        for (const [x, y] of hit.probePoints(r)) {
          expect(hit.pointInRects(x, y, list), `${dock} ${name} @${x},${y}`).toBe(true);
        }
      }
      for (const [x, y] of hit.probePoints(g.capsule)) expect(hit.pointInRects(x, y, list)).toBe(true);
      // A corner of the panel no control covers stays click-through glass.
      expect(hit.pointInRects(1, 139, list)).toBe(false);
    });
  }

  it("keeps the pill and its controls reachable while a card holds the hitbox, and drops them on null", () => {
    drag.reportPillHitbox(114, 67, 72, 22);
    drag.extendPillHitboxRects([[194, 66, 24, 24]], drag.CONTROLS_OWNER);
    drag.extendPillHitbox({ x: 90, y: 113, w: 120, h: 21 }, "meeting-badge");
    drag.holdPillHitbox({ x: 10, y: 6, w: 280, h: 60 }, "copy-card");
    let list = lastList();
    expect(list).toContainEqual([10, 6, 280, 60]);
    expect(list).toContainEqual([194, 66, 24, 24]);
    // Loop-three review (batch 4, note 4): while the reveal/a card held the
    // hitbox the capsule itself was click-through, so the first click on the
    // pill landed on the app underneath. The capsule and every extra stay in.
    expect(list).toContainEqual([114, 67, 72, 22]);
    expect(list).toContainEqual([90, 113, 120, 21]);
    // The capsule moving under a hold re-publishes the list with the new rect.
    drag.reportPillHitbox(120, 67, 72, 22);
    list = lastList();
    expect(list).toContainEqual([120, 67, 72, 22]);
    expect(list).not.toContainEqual([114, 67, 72, 22]);
    drag.extendPillHitbox(null, "meeting-badge");
    drag.holdPillHitbox(null, "copy-card");
    drag.extendPillHitboxRects(null, drag.CONTROLS_OWNER);
    list = lastList();
    expect(list).toEqual([[114, 67, 72, 22]]);
  });

  it("still sends the legacy bounding union for older listeners", () => {
    drag.reportPillHitbox(114, 67, 72, 22);
    drag.extendPillHitboxRects([[194, 66, 24, 24]], drag.CONTROLS_OWNER);
    const legacy = invoke.mock.calls.filter((c) => c[0] === "pill_set_hitbox");
    expect(legacy[legacy.length - 1][1]).toEqual({ x: 114, y: 66, w: 104, h: 24 });
  });
});

describe("measureControls", () => {
  it("measures every visible control, padded, and skips hidden ones", () => {
    vi.stubGlobal("getComputedStyle", (el: { hidden?: boolean }) => ({
      visibility: el.hidden ? "hidden" : "visible",
      pointerEvents: "auto",
    }));
    const el = (r: Rect4, hidden = false) => ({
      hidden,
      getClientRects: () => [1],
      getBoundingClientRect: () => ({ left: r[0], top: r[1], width: r[2], height: r[3] }),
    });
    const root = {
      querySelectorAll: (sel: string) => (sel === hit.CONTROL_SELECTOR ? [el([196, 68, 20, 20]), el([0, 0, 50, 30], true), el([5, 5, 0, 0])] : []),
    } as unknown as ParentNode;
    expect(hit.measureControls(root)).toEqual([[194, 66, 24, 24]]);
  });
});

describe("press hold + pill.click telemetry", () => {
  class FakeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  function fakeDoc(control: { name: string; disabled: boolean }) {
    const handlers: Record<string, ((e: unknown) => void)[]> = {};
    const on = (t: string, fn: (e: unknown) => void) => (handlers[t] ||= []).push(fn);
    const target = {
      closest: (sel: string) =>
        sel === hit.CONTROL_SELECTOR
          ? { disabled: control.disabled, getAttribute: (a: string) => (a === hit.CONTROL_ATTR ? control.name : null) }
          : null,
    };
    const win = { addEventListener: on, removeEventListener() {} };
    const doc = {
      defaultView: win,
      documentElement: {},
      querySelectorAll: () => [],
      addEventListener: on,
      removeEventListener() {},
    };
    const fire = (t: string, e: Record<string, unknown>) => (handlers[t] || []).forEach((fn) => fn({ target, button: 0, ...e }));
    return { doc: doc as unknown as Document, fire };
  }

  it("holds the panel from pointer-down to pointer-up and logs one pill.click per click", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.stubGlobal("MutationObserver", FakeObserver);
    const { doc, fire } = fakeDoc({ name: "meeting-stop", disabled: false });
    const stop = hit.startPillControlTargets(doc);
    fire("pointerdown", {});
    fire("pointerup", {});
    fire("click", {});
    const calls = invoke.mock.calls.filter((c) => c[0] !== "pill_set_hitbox" && c[0] !== "pill_set_hitboxes");
    expect(calls).toEqual([
      ["pill_press_hold", { held: true }],
      ["pill_press_hold", { held: false }],
      ["pill_click_log", { control: "meeting-stop", handled: true }],
    ]);
    stop();
  });

  it("logs handled=false for a disabled control", () => {
    vi.stubGlobal("ResizeObserver", FakeObserver);
    vi.stubGlobal("MutationObserver", FakeObserver);
    const { doc, fire } = fakeDoc({ name: "language-offer-add", disabled: true });
    const stop = hit.startPillControlTargets(doc);
    fire("click", {});
    expect(invoke.mock.calls.filter((c) => c[0] === "pill_click_log")).toEqual([
      ["pill_click_log", { control: "language-offer-add", handled: false }],
    ]);
    stop();
  });
});

/* ── batch-6 bounce: what the browser's own hit test does to a press ──────────
 * `pill-matrix --clicks` found every control missing its corner presses:
 * WebKit/Chrome hit-test a box against its ROUNDED border, the refused-press
 * chip sat on the meeting clock row, and the grown stop was pushed into the
 * clock's rounded clip. These pin the CSS that fixes each, read from float.css
 * rather than assumed. */
const FLOAT_CSS = (readFileSync(new URL("../float.css", import.meta.url), "utf8") as string).replace(/\/\*[\s\S]*?\*\//g, "");

/**
 * The declarations of EVERY rule whose selector is exactly `selector`, merged in
 * file order (a later declaration wins, as the cascade would). The gated chip
 * is styled in two rules (fb-02's edge, trial-clickable's measured padding).
 */
function ruleBody(selector: string): Record<string, string> {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const all = [...FLOAT_CSS.matchAll(new RegExp(`(?:^|[}\\n])\\s*${esc}\\s*\\{([^}]*)\\}`, "g"))];
  expect(all.length, `float.css has no rule "${selector}"`).toBeGreaterThan(0);
  const out: Record<string, string> = {};
  for (const m of all) {
    for (const decl of m[1].split(";")) {
      const i = decl.indexOf(":");
      if (i > 0) out[decl.slice(0, i).trim()] = decl.slice(i + 1).trim();
    }
  }
  return out;
}

const px = (v: string | undefined) => parseFloat(String(v));

describe("fb-02 — the browser hit test agrees with the panel's rects", () => {
  it("control_hit_shape_is_its_square_rect — a transparent square layer covers every control", () => {
    const layer = ruleBody("[data-pill-control]:not(.slot, .ctrl)::after");
    expect(layer.content).toBe('""');
    expect(layer.position).toBe("absolute");
    expect(layer.inset).toBe("0");
    expect(layer["border-radius"]).toBe("0");
    // The control is the layer's containing block, at zero specificity so a
    // rule that already positions it (absolute chip, fixed card) wins.
    expect(ruleBody(":where([data-pill-control]:not(.slot, .ctrl))").position).toBe("relative");
    // The after-badge is itself a control: its own clip would round the layer off.
    expect(ruleBody(".kami-stage .meeting-badge.meeting-after").overflow).toBe("visible");
  });

  it("control_drawn_at_least_24pt — the drawn floor matches MIN_HIT_PT", () => {
    const floor = ruleBody("[data-pill-control]:not(.slot)");
    expect(px(floor["min-width"])).toBe(hit.MIN_HIT_PT);
    expect(px(floor["min-height"])).toBe(hit.MIN_HIT_PT);
  });

  it("meeting_stop_inner_corners_inside_the_clock_clip — the 24pt stop is not cut by the 14px arc", async () => {
    const st = await import("./state");
    const row = ruleBody(".kami-stage .meeting-badge:not(.meeting-after)");
    const [pt, pr] = row.padding.split(/\s+/).map(px);
    const radius = px(ruleBody(".kami-stage .meeting-badge")["border-radius"]);
    const H = st.CLOCK_H, W = st.CLOCK_W.recording, S = hit.MIN_HIT_PT;
    expect(H - 2 * pt).toBeGreaterThanOrEqual(S); // the stop fits the row's content height
    // The stop, flush to the trailing padding and centred vertically.
    const stop: Rect4 = [W - pr - S, (H - S) / 2, S, S];
    // CSS scales radii that overrun a side: 14px on a 26pt row draws as 13.
    const rr = Math.min(radius, H / 2, W / 2);
    for (const [x, y] of hit.probePoints(stop)) {
      // Inside the rounded clip: distance to the nearest point of the inner
      // (rr-inset) rectangle is at most rr.
      const cx = Math.min(Math.max(x, rr), W - rr);
      const cy = Math.min(Math.max(y, rr), H - rr);
      expect(Math.hypot(x - cx, y - cy), `probe (${x},${y}) is outside the clock's rounded clip`).toBeLessThanOrEqual(rr);
    }
  });

  it("blocked_chip_corners_inside_its_hit_layer — the chip's edge is an inset shadow, so the square layer covers the drawn box", () => {
    // Batch-8 bounce: with a 1pt border the layer (inset 0 = the PADDING box)
    // ended exactly on the probe's right/bottom inner corners, which hit-test
    // half-open, so top-right, bottom-left and bottom-right fell through the
    // rounded border to Yappy's canvas. No border: padding box == drawn box.
    const base = ruleBody(".kami-blocked");
    expect(base.border).toBe("0");
    expect(base["box-shadow"]).toMatch(/^inset 0 0 0 1px /);
    expect(base.overflow).toBeUndefined(); // a clip on the 999px stadium would round the layer off
    const gated = ruleBody(".kami-blocked.gated");
    expect(gated["border-color"]).toBeUndefined();
    expect(gated["box-shadow"]).toMatch(/^inset 0 0 0 1px /);
    // Same 26pt chip as before: the padding took back the 1pt the border gave up.
    const [pv, ph] = base.padding.split(/\s+/).map(px);
    expect(14 + 2 * pv).toBe(26);
    expect(ph).toBe(11);
    // Every probe point of a chip, at its narrowest legal size, is strictly
    // inside the layer (a half-open hit test can never put it on the edge).
    const layer = ruleBody("[data-pill-control]:not(.slot, .ctrl)::after");
    expect(layer.inset).toBe("0");
    const chip: Rect4 = [10.5, 20.25, hit.MIN_HIT_PT + 0.5, 26];
    for (const [x, y] of hit.probePoints(chip)) {
      expect(x).toBeGreaterThan(chip[0]);
      expect(x).toBeLessThan(chip[0] + chip[2]);
      expect(y).toBeGreaterThan(chip[1]);
      expect(y).toBeLessThan(chip[1] + chip[3]);
    }
    // The label gives way inside the chip instead of spilling past its edge.
    const label = ruleBody(".kami-blocked > span");
    expect(label["min-width"]).toBe("0");
    expect(label.overflow).toBe("hidden");
    expect(label["white-space"]).toBe("nowrap");
  });

  it("blocked_chip_never_covers_another_slot — mic/Trial chip sits inside Yappy's box in every dock x meeting", async () => {
    const st = await import("./state");
    const { BAR_DOCKS } = await import("./dock");
    const chip = ruleBody('html[data-one-pill="true"] .kami-blocked');
    expect(chip.left).toBe("calc(var(--char-x) + var(--char-w) / 2)");
    expect(chip.top).toBe("calc(var(--char-y) + var(--char-h) - 6px)");
    expect(chip.translate).toBe("-50% -100%");
    expect(chip["max-width"]).toBe("var(--char-w)");
    // .kami-blocked: 14pt glyph + 2 x 6pt padding (its 1pt edge is an inset
    // shadow); .kami-blocked.gated: the 24pt "Get Yap" button + 2 x 3pt (pill.html §13).
    const CHIP_HS = { blocked: 26, gated: 30 } as const;
    let n = 0;
    for (const phase of ["blocked", "gated"] as const) {
      const CHIP_H = CHIP_HS[phase];
      for (const meeting of ["none", "recording", "after"] as const) {
        for (const dock of BAR_DOCKS) {
          const l = st.pillLayout({ phase, meeting, dock });
          const c = l.char;
          const box = { x: c.x, y: c.y + c.h - 6 - CHIP_H, w: c.w, h: CHIP_H }; // its widest
          expect(box.y).toBeGreaterThanOrEqual(c.y);
          for (const [name, other] of [["clock", l.clock], ["bubble", l.bubble], ["hint", l.hint], ["progress", l.progress], ["card", l.card]] as const) {
            if (other) expect(st.boxesOverlap(box, other), `${phase}/${meeting}/${dock}: chip over the ${name}`).toBe(false);
          }
          n += 1;
        }
      }
    }
    expect(n).toBeGreaterThanOrEqual(6);
  });
});

/**
 * fb-02 (loop three) — every clickable button on the pill carries
 * `data-pill-control`, so it is measured into the panel's hit rects, drawn at
 * least 24 x 24 pt, given the square hit layer and logged as `pill.click`. Read
 * from the component sources: a button added later without the tag fails here
 * before it can become the next "first click does nothing".
 */
const CONTROL_SOURCES = [
  "BacktrackCaughtCard.tsx",
  "CallOffer.tsx",
  "CopyCard.tsx",
  "GovernorChip.tsx",
  "LanguageChip.tsx",
  "LanguageOfferCard.tsx",
  "LaunchWalkScene.tsx",
  "MeetingBadge.tsx",
  "MeetingDictationCard.tsx",
  "MeetingOfferCard.tsx",
  "NudgeRow.tsx",
  "PolishRevealCard.tsx",
  "SilentMicRow.tsx",
  "StyleChipButton.tsx",
  "WarmupEngine.tsx",
  "WhoTalkingCard.tsx",
  "YappyPill.tsx",
] as const;

describe("fb-02 — every pill button is a tagged control", () => {
  it("every_pill_button_carries_data_pill_control — each <button> with an onClick names itself before the handler", () => {
    const untagged: string[] = [];
    let seen = 0;
    for (const file of CONTROL_SOURCES) {
      const src = readFileSync(new URL(`./${file}`, import.meta.url), "utf8") as string;
      const starts = [...src.matchAll(/<button\b/g)].map((m) => m.index ?? 0);
      for (const at of starts) {
        const rest = src.slice(at + 1);
        const next = rest.search(/<button\b|<\/button>/);
        const head = next < 0 ? rest : rest.slice(0, next);
        const click = head.indexOf("onClick=");
        if (click < 0) continue;
        seen += 1;
        if (!head.slice(0, click).includes("data-pill-control")) {
          untagged.push(`${file}:${src.slice(0, at).split("\n").length}`);
        }
      }
    }
    expect(seen).toBeGreaterThanOrEqual(CONTROL_SOURCES.length);
    expect(untagged, `buttons with no data-pill-control: ${untagged.join(", ")}`).toEqual([]);
  });

  it("retry_buttons_share_one_control_name — the warm-up and launch-walk Retry log as pill.click control=retry", () => {
    for (const file of ["LaunchWalkScene.tsx", "WarmupEngine.tsx"]) {
      const src = readFileSync(new URL(`./${file}`, import.meta.url), "utf8") as string;
      expect(src).toContain('data-pill-control="retry"');
    }
  });
});
