/**
 * yap-1107-parity-companions — measured parity with the approved Companions
 * page (~/Obsidian/Wilson-Brain/Projects/Yap/design-2026-11-06/companions.html).
 * The numbers below were measured off that page with scripts/parity-crops.mjs
 * (2x, frame 1200 ms) and are recorded in
 * docs/loop/evidence/parity-companions.manifest.json. The pixel sheet itself is
 * the integrator's (scripts/parity-sheet.mjs --check); these pin the code half.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { SCENE_H, SCENE_W, YAPPY_X, forcedLaunchWalk } from "./launchWalk";
import { matrixLaunchWalkScene } from "./forced";
import { PHASE_ID } from "./live";
import { FRIENDS, SPRITE_NAMES } from "../sprites/art";
import { STUDIO_H, STUDIO_W } from "../home/studio/studio";

const read = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

/* Art size (art px) of every friend on the approved page (assets/friends-hd.js D.*). */
const APPROVED_ART: Record<string, [number, number]> = {
  yappy: [40, 36], hoot: [40, 40], shelly: [46, 36], tidy: [42, 36], gulp: [46, 38],
  ember: [32, 28], sal: [46, 30], mochi: [40, 40], cinder: [46, 42],
};

describe("parity-companions: the launch walk window (s06)", () => {
  it("is the storyboard's 196 x 44 window, Yappy centred on a whole pixel", () => {
    expect(SCENE_W).toBe(196);
    expect(SCENE_H).toBe(44);
    expect(YAPPY_X).toBe(98);
    expect(Number.isInteger(YAPPY_X)).toBe(true);
  });

  it("tokens.css carries the same box and float.css reads it (no stale 176-scene literals)", () => {
    const tokens = read("../tokens.css");
    expect(tokens).toMatch(/--launch-walk-w:\s*196px;/);
    expect(tokens).toMatch(/--launch-walk-h:\s*44px;/);
    const css = read("../float.css");
    const start = css.indexOf(".launch-walk-line {");
    const end = css.indexOf(".launch-walk-retry:disabled");
    expect(start).toBeGreaterThan(-1);
    const block = css.slice(start, end);
    expect(block).toContain("var(--launch-walk-w)");
    expect(block).toContain("var(--launch-walk-h)");
    expect(block).not.toMatch(/\b(88|160|22)px\b/);
  });
});

describe("parity-companions: art at Yappy's level (s02, s08)", () => {
  it("every approved friend is drawn at the approved art size", () => {
    for (const [id, [w, h]] of Object.entries(APPROVED_ART)) {
      expect(SPRITE_NAMES as readonly string[]).toContain(id);
      const f = FRIENDS[id as (typeof SPRITE_NAMES)[number]];
      expect([id, f.w, f.h]).toEqual([id, w, h]);
    }
  });
});

describe("parity-companions: the studio (s03, option C)", () => {
  it("is the 320 x 160 side view, exactly 960 x 480 at 3x", () => {
    expect(STUDIO_W * 3).toBe(960);
    expect(STUDIO_H * 3).toBe(480);
  });
});

describe("parity-companions: the manifest", () => {
  type State = { id: string; artifact: { page: string; selector: string }; app: { page: string; selector: string; query?: string; capture: string }; expect: { w: number; h: number } };
  const m = JSON.parse(read("../../../docs/loop/evidence/parity-companions.manifest.json")) as {
    designDir?: string; states: State[]; excluded: { id: string; selector: string; why: string }[];
    conflicts: { id: string; selector: string; companions: { w: number; h: number }; pill: { w: number; h: number }; deltaPct: number; decision: string }[];
    openDefects: { id: string; state: string; blockedOn: string }[];
    fixedDefects: { id: string; state: string; fix: string; tests: string }[];
  };

  it("lists every drawn app state of the approved page, each with an artifact selector, an app target and a measured box", () => {
    // batch-1 bounce: designDir was a literal non-path string — the default design dir is used instead.
    expect(m.designDir).toBeUndefined();
    const ids = m.states.map((s) => s.id);
    expect(ids).toEqual(["s03-studio-side-view", "s06-launch-walk-frame", "s09-sky-c-day", "s09-friend-visit"]);
    for (const s of m.states) {
      expect(s.artifact.page).toBe("companions.html");
      expect(s.artifact.selector.length).toBeGreaterThan(0);
      expect(s.app.selector.length).toBeGreaterThan(0);
      expect(s.app.capture.length).toBeGreaterThan(0);
      expect(s.expect.w).toBeGreaterThan(0);
      expect(s.expect.h).toBeGreaterThan(0);
      // batch-1 bounce: a selector no app source carries, and a dock the matrix never shoots.
      expect(s.app.selector).not.toContain("yappy-capsule-box");
      if (s.app.query) expect(new URLSearchParams(s.app.query).get("dock")).toBe("bottom");
      // pill rows are shot by the item's integrator step: pill-matrix --out <captures>/app
      if (s.app.page === "float.html") expect(s.app.capture.startsWith("app/"), s.id).toBe(true);
    }
    // loop3 retry: .kami-canvas is the whole 300 x 140 panel, never the 196 x 44 window the page draws.
    for (const id of ["s09-sky-c-day", "s09-friend-visit"]) {
      const row = m.states.find((s) => s.id === id)!;
      expect(row.app.selector, id).toBe(".yappy-win");
    }
    const sky = m.states.find((s) => s.id === "s09-sky-c-day")!;
    // the 196 x 44 window with the day sky is an open state (pill.html), never the 64 x 20 resting one
    expect(new URLSearchParams(sky.app.query).get("phase")).toBe("listening-locked");
    expect([sky.expect.w, sky.expect.h]).toEqual([196, 44]);
    const walk = m.states.find((s) => s.id === "s06-launch-walk-frame")!;
    expect([walk.expect.w, walk.expect.h]).toEqual([SCENE_W, SCENE_H]);
    expect(new URLSearchParams(walk.app.query).get("walk")).toBe("0");
    const studio = m.states.find((s) => s.id === "s03-studio-side-view")!;
    expect([studio.expect.w, studio.expect.h]).toEqual([STUDIO_W * 3, STUDIO_H * 3]);
    // the page's own example (companions.html line 297): Yappy visits Hoot for the
    // 100th take, in the open done window (segment "✓ 100th take"), shot by the
    // narrow friend-visit-window matrix row.
    const visit = m.states.find((s) => s.id === "s09-friend-visit")!;
    const vq = new URLSearchParams(visit.app.query);
    expect(vq.get("friend")).toBe("yappy");
    expect(vq.get("pill")).toBe("hoot");
    expect(vq.get("phase")).toBe("done");
    expect(visit.app.capture).toBe("app/friend-visit-window__hoot__bottom__none.png");
    // pill.html's open-with-segment window (196) is within 3 percent of the page's 200
    expect(Math.abs(visit.expect.w - 196) / 196).toBeLessThanOrEqual(0.03);
  });

  it("names every design-page exhibit it does not measure, with the reason", () => {
    const shown = [...m.states.map((s) => s.id), ...m.excluded.map((e) => e.id), ...m.conflicts.map((c) => c.id)];
    for (const want of [
      "s02-detail-art", "s03-studio-side-view", "s04-who-sits-in-pill", "s05-three-voices", "s05-reaction-words",
      "s06-launch-walk-frame", "s06-launch-walk-2x", "s07-reactions-while-talking", "s08-cinder-sal-mochi", "s08-cast",
      "s09-sky-a-today", "s09-sky-c-day", "s09-hoot-in-pill", "s09-hoot-meeting", "s09-friend-visit",
    ]) expect(shown).toContain(want);
    for (const e of m.excluded) expect(e.why.length).toBeGreaterThan(20);
  });

  it("lists every page-against-page box conflict with both numbers and the decision, never silently", () => {
    const ids = m.conflicts.map((c) => c.id);
    expect(ids).toEqual(["s09-hoot-in-pill", "s09-hoot-meeting"]);
    for (const c of m.conflicts) {
      // a conflict is only a conflict when the two pages are more than the 3 percent apart
      const d = Math.max(Math.abs(c.companions.w - c.pill.w) / c.pill.w, Math.abs(c.companions.h - c.pill.h) / c.pill.h) * 100;
      expect(d, c.id).toBeGreaterThan(3);
      expect(Math.round(d), c.id).toBe(c.deltaPct);
      expect(c.decision, c.id).toMatch(/pill\.html wins the box/);
      expect(m.states.some((s) => s.id === c.id), c.id).toBe(false);
    }
    // the visitor-inside-the-window defect is fixed in code and recorded as fixed
    expect(m.openDefects.map((d) => d.state)).not.toContain("s09-friend-visit");
    expect(m.fixedDefects.map((d) => d.state)).toContain("s09-friend-visit");
  });
});

describe("parity-companions: the launch walk is a matrix row", () => {
  it("matrixLaunchWalkScene forces the walk's first frame for Yappy at the bottom dock, no meeting", () => {
    const row = matrixLaunchWalkScene();
    expect(row.id).toBe("launch-walk");
    expect(row.phase).toBe("model-loading");
    expect(row.phaseId).toBe(PHASE_ID["model-loading"]);
    expect(row.query).toBe("walk=0");
    expect(row.characters).toEqual(["yappy"]);
    expect(row.docks).toEqual(["bottom"]);
    expect(row.meetings).toEqual(["none"]);
    // the forced pill mounts the walk only when walk= is in the query
    expect(forcedLaunchWalk("?" + row.query)).not.toBeNull();
    expect(forcedLaunchWalk("?phase=model_loading")).toBeNull();
  });
});

describe("parity-companions: batch-8 bounce, the captures the sheet needs", () => {
  it("shoots the two narrow parity rows first, before the full matrix a timebox can cut short", () => {
    const src = read("../float-main.tsx");
    const start = src.indexOf("__YAP_PILL_PHASES__ = [");
    expect(start).toBeGreaterThan(-1);
    const list = src.slice(start, src.indexOf("];", start));
    const walk = list.indexOf("matrixLaunchWalkScene()");
    const visit = list.indexOf("matrixFriendVisitWindowScene()");
    const phases = list.indexOf("...matrixPhases()");
    expect(walk).toBeGreaterThan(-1);
    expect(visit).toBeGreaterThan(-1);
    expect(phases).toBeGreaterThan(-1);
    expect(walk).toBeLessThan(phases);
    expect(visit).toBeLessThan(phases);
    // each listed exactly once
    expect(list.split("matrixLaunchWalkScene()").length).toBe(2);
    expect(list.split("matrixFriendVisitWindowScene()").length).toBe(2);
  });

  it("the manifest's capture recipe writes every capture name its rows read", () => {
    const m = JSON.parse(read("../../../docs/loop/evidence/parity-companions.manifest.json")) as {
      captures: string; states: { id: string; app: { capture: string; width?: number; height?: number } }[];
      openDefects: { state: string; blockedOn: string }[];
    };
    for (const s of m.states) {
      const cap = s.app.capture;
      if (cap.startsWith("app/")) {
        // app/<rowId>__<character>__<dock>__<meeting>.png, and the recipe shoots that row with --only
        const rowId = cap.slice(4).split("__")[0];
        expect(m.captures, s.id).toContain("--only " + rowId);
      } else {
        // custom-home-<W>x<H>.png comes from smoke-windowed with that W1/H1
        const hit = /^custom-home-(\d+)x(\d+)\.png$/.exec(cap);
        expect(hit, s.id).not.toBeNull();
        expect(m.captures, s.id).toContain("YAP_SMOKE_W1=" + hit![1] + " YAP_SMOKE_H1=" + hit![2]);
        expect([s.app.width, s.app.height], s.id).toEqual([Number(hit![1]), Number(hit![2])]);
      }
    }
    expect(m.captures).toContain("--build --check");
    // the reverted pill window is recorded as a blocker, never hidden
    expect(m.openDefects.find((d) => d.state === "s09-sky-c-day")?.blockedOn).toMatch(/yap-1107-parity-pill/);
  });
});
