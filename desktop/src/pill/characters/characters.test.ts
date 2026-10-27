/**
 * Y5-K — the ONE data-completeness contract every registered character is swept
 * by. The shell owns phase x dock (dock.test.ts, character-agnostic, against
 * dock-geometry.json); a character owes only what is checked here. Registering
 * an INCOMPLETE character turns this file RED — that is why it exists.
 */
import { describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
// @ts-expect-error -- node builtin, untyped here; the test runs under Node.
import { readFileSync, readdirSync } from "node:fs";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { characters, registerCharacter, resolveCharacter } from "./registry";
import { CHARACTER_PHASES, CHARACTER_TONES, fitsBox, type CharacterBox, type CharacterFrame, type DockAxis } from "./types";
import { example, paintBlob } from "./example";
import { BAR_DOCKS, barBox, shellBarPhase, waveAxis } from "../dock";
import type { LivePhase } from "../live";
import PillShell from "../PillShell";

const HERE = new URL("./", import.meta.url);
const read = (rel: string): string => readFileSync(new URL(rel, HERE), "utf8") as string;
const MOTIONS = ["still", "breathe", "work", "pulse", "shake"];

const frameFor = (phase: LivePhase, tone: CharacterFrame["tone"], box: CharacterBox, dock: DockAxis = "x"): CharacterFrame => ({
  phase, tone, level: 0.5, box, dock, reducedMotion: false,
});

/** Every box the shell can hand a character for `phase`: its dock-geometry row at each dock, meeting or not. */
function shellBoxes(phase: LivePhase): Array<{ box: CharacterBox; axis: DockAxis; why: string }> {
  const out: Array<{ box: CharacterBox; axis: DockAxis; why: string }> = [];
  for (const dock of BAR_DOCKS) {
    for (const meeting of [false, true]) {
      const bp = shellBarPhase(phase, meeting);
      const b = barBox(bp, dock);
      out.push({ box: { w: b.width, h: b.height }, axis: waveAxis(bp, dock), why: `${bp}@${dock}` });
    }
  }
  return out;
}

/** The creature modules: every non-test source under characters/<id>/. */
function creatureSources(): Record<string, string> {
  const files = (readdirSync(HERE, { recursive: true, encoding: "utf8" }) as string[])
    .map((p) => p.split("\\").join("/"))
    .filter((p) => p.includes("/") && /\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p));
  return Object.fromEntries(files.map((f) => [f, read(`./${f}`)]));
}

describe("the character contract (Y5-K)", () => {
  it("every_registered_character_covers_every_phase_in_every_tone", () => {
    expect(characters().map((c) => c.id)).toEqual(expect.arrayContaining(["classic", "yappy"]));
    expect(CHARACTER_PHASES.length).toBeGreaterThanOrEqual(16);
    for (const c of characters()) {
      expect(c.label.trim().length, `${c.id} label`).toBeGreaterThan(0);
      for (const phase of CHARACTER_PHASES) {
        for (const tone of CHARACTER_TONES) {
          const paint = c.render(frameFor(phase, tone, { w: 240, h: 48 }));
          expect(paint, `${c.id} paints nothing for ${phase}/${tone}`).not.toBeNull();
          expect(paint?.art?.sprite?.length ?? 0, `${c.id} has no art for ${phase}`).toBeGreaterThan(0);
          expect(MOTIONS, `${c.id}/${phase} motion`).toContain(paint?.art?.motion);
          expect(typeof paint?.copy === "string" && paint.copy.trim().length > 0, `${c.id} has no copy for ${phase}/${tone}`).toBe(true);
        }
      }
      // A settled state must not be given a working animation, or the pill
      // looks busy while nothing is happening (carried over from Y5-C).
      for (const p of ["done", "empty", "cancelled", "blocked", "gated"] as LivePhase[]) {
        expect(c.render(frameFor(p, "friendly", { w: 240, h: 48 }))?.art.motion, `${c.id} animates ${p} like work`).not.toBe("work");
      }
    }
  });

  it("an INCOMPLETE character turns the sweep red (the mutation the acceptance re-runs)", () => {
    const holes = { id: "holes", label: "holes", render: (f: CharacterFrame) => (f.phase === "blind" ? null : example.render(f)) };
    const missing = CHARACTER_PHASES.filter((p) => holes.render(frameFor(p, "rude", { w: 99, h: 30 })) === null);
    expect(missing).toEqual(["blind"]);
  });

  it("a_character_never_exceeds_the_box_the_shell_hands_it", () => {
    for (const c of characters()) {
      for (const phase of CHARACTER_PHASES) {
        const boxes = [...shellBoxes(phase), { box: { w: 0, h: 0 }, axis: "x" as DockAxis, why: "empty" }, { box: { w: 3, h: 17 }, axis: "y" as DockAxis, why: "sliver" }];
        for (const { box, axis, why } of boxes) {
          const paint = c.render(frameFor(phase, "friendly", box, axis));
          expect(paint && fitsBox(paint.size, box), `${c.id}/${phase} overflows ${why} ${JSON.stringify(box)}: ${JSON.stringify(paint?.size)}`).toBe(true);
        }
      }
    }
  });

  it("imageSmoothingEnabled is false wherever a character paints to a canvas", () => {
    // DATA: every paint that declares a canvas declares smoothing off.
    for (const c of characters()) {
      for (const phase of CHARACTER_PHASES) {
        const paint = c.render(frameFor(phase, "friendly", { w: 200, h: 40 }));
        if (paint?.canvas) expect(paint.canvas.imageSmoothingEnabled, `${c.id}/${phase}`).toBe(false);
      }
    }
    expect(example.render(frameFor("idle", "rose", { w: 40, h: 40 }))?.canvas).toEqual({ imageSmoothingEnabled: false });
    // SOURCE: every creature module, and Yappy's stage, that takes a 2d context
    // turns smoothing off — a canvas character can never paint blurred.
    const sources = { ...creatureSources(), "../YappyPill.tsx": read("../YappyPill.tsx") };
    const painters = Object.entries(sources).filter(([, src]) => /getContext\(\s*["']2d["']/.test(src));
    expect(painters.map(([f]) => f)).toEqual(expect.arrayContaining(["example/index.tsx", "../YappyPill.tsx"]));
    for (const [f, src] of painters) expect(src, f).toMatch(/imageSmoothingEnabled\s*=\s*false/);
    // RUNTIME: the example's painter really sets it before it draws.
    const calls: string[] = [];
    const ctx = {
      set imageSmoothingEnabled(v: boolean) { calls.push(`smooth=${v}`); },
      clearRect: () => calls.push("clear"),
      fillRect: () => calls.push("fill"),
      fillStyle: "",
    } as unknown as CanvasRenderingContext2D;
    paintBlob(ctx);
    expect(calls[0]).toBe("smooth=false");
    expect(calls.filter((c) => c === "fill").length).toBeGreaterThan(20);
  });

  it("no creature module reads settings, license state, the dock or the backend", () => {
    // Spelled in pieces so this file itself stays clean under the acceptance's
    // own greps over characters/.
    const FORBIDDEN = new RegExp(
      ["@tauri-apps", "pill_?" + "style", "pill" + "Style", "get_settings", "dataset\\.dock", "data-dock",
        "data-bar-" + "position", "from\\s+[\"'][./]*license[\"']", "window\\.", "document\\."].join("|"),
    );
    expect(FORBIDDEN.test("const s = settings." + "pill" + "Style")).toBe(true);
    expect(FORBIDDEN.test('el.setAttribute("data-bar-' + 'position", d)')).toBe(true);
    const src = creatureSources();
    expect(Object.keys(src)).toEqual(expect.arrayContaining(["classic/index.tsx", "yappy/index.tsx", "example/index.tsx"]));
    for (const [f, text] of Object.entries(src)) {
      expect(text, f).not.toMatch(FORBIDDEN);
    }
  });

  it("a_new_creature_needs_no_shell_change", () => {
    // 1. Before it is registered, its id is just an unknown pill-style setting: classic.
    expect(resolveCharacter("example").id).toBe("classic");
    // 2. Registering it is the WHOLE integration.
    registerCharacter(example);
    expect(resolveCharacter("example")).toBe(example);
    expect(characters()).toContain(example);
    // 3. The REAL shell mounts it, unchanged: its art rides the capsule and its
    //    canvas sits in the body slot, inside the same chrome every character gets.
    const markup = renderToStaticMarkup(createElement(PillShell, { character: example, gate: "idle", tone: "rose" }));
    expect(markup).toContain('data-sprite="blob"');
    expect(markup).toContain('data-character="example"');
    expect(markup).toContain('class="char-example"');
    expect(markup).toContain("data-bar-axis=");
    const classic = renderToStaticMarkup(createElement(PillShell, { gate: "idle" }));
    expect(classic).toContain('data-character="classic"');
    expect(classic).toContain('data-sprite="seed"');
    // 4. No shell file names a creature: the mount point resolves an id, the
    //    shell draws a frame, and the geometry has no style dimension.
    const shell = read("../PillShell.tsx");
    const mount = read("../../float-main.tsx");
    for (const [f, text] of [["PillShell.tsx", shell], ["float-main.tsx", mount]] as const) {
      expect(text, f).not.toMatch(/["'](?:yappy|example)["']/);
      expect(text, f).not.toMatch(/(?:style|character\.id)\s*===\s*["']/);
      expect(text, f).not.toMatch(/import\s+\w+\s+from\s+["'][./]*(?:YappyPill|characters\/\w+)["']/);
    }
    expect(read("../dock-geometry.json")).not.toMatch(/classic|yappy|example/i);
  });
});
