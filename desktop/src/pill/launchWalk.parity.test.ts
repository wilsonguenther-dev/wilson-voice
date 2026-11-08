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
  };

  it("lists every drawn app state of the approved page, each with an artifact selector, an app target and a measured box", () => {
    // batch-1 bounce: designDir was a literal non-path string — the default design dir is used instead.
    expect(m.designDir).toBeUndefined();
    const ids = m.states.map((s) => s.id);
    expect(ids).toEqual(["s03-studio-side-view", "s06-launch-walk-frame", "s09-sky-c-day", "s09-hoot-in-pill", "s09-hoot-meeting", "s09-friend-visit"]);
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
    }
    const walk = m.states.find((s) => s.id === "s06-launch-walk-frame")!;
    expect([walk.expect.w, walk.expect.h]).toEqual([SCENE_W, SCENE_H]);
    expect(new URLSearchParams(walk.app.query).get("walk")).toBe("0");
    const studio = m.states.find((s) => s.id === "s03-studio-side-view")!;
    expect([studio.expect.w, studio.expect.h]).toEqual([STUDIO_W * 3, STUDIO_H * 3]);
    const visit = m.states.find((s) => s.id === "s09-friend-visit")!;
    expect(new URLSearchParams(visit.app.query).get("friend")).toBe("tidy");
  });

  it("names every design-page exhibit it does not measure, with the reason", () => {
    const shown = [...m.states.map((s) => s.id), ...m.excluded.map((e) => e.id)];
    for (const want of [
      "s02-detail-art", "s03-studio-side-view", "s04-who-sits-in-pill", "s05-three-voices", "s05-reaction-words",
      "s06-launch-walk-frame", "s06-launch-walk-2x", "s07-reactions-while-talking", "s08-cinder-sal-mochi", "s08-cast",
      "s09-sky-a-today", "s09-sky-c-day", "s09-hoot-in-pill", "s09-hoot-meeting", "s09-friend-visit",
    ]) expect(shown).toContain(want);
    for (const e of m.excluded) expect(e.why.length).toBeGreaterThan(20);
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
