/**
 * FB-09 — every approved sprite, rendered from the app's own code, must equal
 * the approved artifact frames: ZERO colour tolerance, ONE pixel of position
 * tolerance. Wilson, 2026-11-06: "make sure none of these ever degrade".
 *
 * The approved frames are the design-review scripts themselves, copied byte
 * for byte to ./approved/ (friends-hd.js, yappy.js) and run here against the
 * same software raster as the app code — so a recoloured, blurred, cropped or
 * drifted sprite fails CI, and so does an edit to the approved copy (sha256).
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { createHash } from "node:crypto";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { CHICK_BOX, FRIENDS, H, SPRITE_FRAMES, SPRITE_NAMES, drawChick, type ChickPose, type Helpers, type PixelCtx, type SpriteName, type SpriteOpt } from "./art";
import { Raster, snapDiff } from "./raster";

const readSibling = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

/** sha256 of the approved artifacts as Wilson signed them off (2026-11-06). */
const APPROVED_SHA256: Record<string, string> = {
  "friends-hd.js": "1021e57eda47225eddd7e473b83168fd181d85a5b65eafbdaa9fcd3ec747fe84",
  "yappy.js": "ef5e77f62fe31c80a165e4a8538be625b8e8baf0d402c8202416fa594b69633c",
};

interface ApprovedSprite { w: number; h: number; draw(o: PixelCtx, h: Helpers, f: number, opt?: SpriteOpt): void }
interface Approved {
  Yappy: { drawChick(o: PixelCtx, cx: number, feetY: number, A: unknown): void; base(A: Partial<ChickPose>): unknown };
  FR: { D: Record<string, ApprovedSprite>; H(o: PixelCtx): Helpers };
}

/** Run the approved scripts in a fake window whose canvases are rasters. */
function loadApproved(): Approved {
  const canvas = () => {
    const c = {
      width: 300, height: 150, r: null as Raster | null, dataset: {}, style: {},
      getAttribute: () => "0",
      getContext(): Raster { let r = c.r; if (!r || r.width !== c.width || r.height !== c.height) { r = new Raster(c.width, c.height); c.r = r; } return r; },
    };
    return c;
  };
  const win: Record<string, unknown> = { devicePixelRatio: 1, matchMedia: () => ({ matches: true }), addEventListener: () => undefined };
  const doc = { createElement: canvas };
  for (const file of ["yappy.js", "friends-hd.js"]) {
    const src = readSibling(`./approved/${file}`);
    new Function("window", "document", "performance", "requestAnimationFrame", src)(win, doc, { now: () => 0 }, () => 0);
  }
  return win as unknown as Approved;
}

const APPROVED = loadApproved();

/** The option sets each sprite's approved draw reads. */
const OPTS: Record<SpriteName, (SpriteOpt | undefined)[]> = {
  yappy: [undefined], shelly: [undefined], tidy: [undefined], gulp: [undefined], ember: [undefined], sal: [undefined], mochi: [undefined],
  hoot: [undefined, { bare: true }],
  cinder: [undefined, { talk: true }, { t: 0.1 }, { t: 0.2, talk: true }],
};

function renderApp(name: SpriteName, f: number, opt?: SpriteOpt): Raster {
  const s = FRIENDS[name], r = new Raster(s.w, s.h);
  s.draw(r, H(r), f, opt);
  return r;
}
function renderApproved(name: SpriteName, f: number, opt?: SpriteOpt): Raster {
  const s = APPROVED.FR.D[name], r = new Raster(s.w, s.h);
  s.draw(r, APPROVED.FR.H(r), f, opt);
  return r;
}

describe("approved artifacts are the ones Wilson signed off", () => {
  it("approved_copies_are_byte_identical", () => {
    for (const [file, sha] of Object.entries(APPROVED_SHA256)) {
      const got = createHash("sha256").update(readFileSync(new URL(`./approved/${file}`, import.meta.url))).digest("hex");
      expect(got, file).toBe(sha);
    }
  });

  it("every acceptance sprite exists in both the app and the approved art", () => {
    expect([...SPRITE_NAMES]).toEqual(["yappy", "hoot", "shelly", "tidy", "gulp", "ember", "sal", "mochi", "cinder"]);
    for (const n of SPRITE_NAMES) {
      expect(APPROVED.FR.D[n], n).toBeTruthy();
      expect([FRIENDS[n].w, FRIENDS[n].h], n).toEqual([APPROVED.FR.D[n].w, APPROVED.FR.D[n].h]);
    }
  });
});

describe("sprite_snapshot_matches_approved — zero colour tolerance, one pixel of position", () => {
  for (const name of SPRITE_NAMES)
    for (const f of SPRITE_FRAMES[name])
      for (const opt of OPTS[name]) {
        it(`${name} f=${f} opt=${JSON.stringify(opt ?? {})}`, () => {
          const got = renderApp(name, f, opt), want = renderApproved(name, f, opt);
          const d = snapDiff(got, want, 1);
          expect(d.painted, "the approved frame paints something").toBeGreaterThan(0);
          expect(d.mismatched, `${name} f=${f}: ${d.mismatched} px differ at best shift (${d.dx},${d.dy})`).toBe(0);
          expect(got.subpixel, "every app draw lands on whole art pixels").toBe(0);
        });
      }
});

describe("chick_snapshot_matches_approved — the pill's Yappy vs approved yappy.js", () => {
  const BASE: ChickPose = { hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0, look: "fwd", glasses: false, pencil: false, droop: false };
  const POSES: Partial<ChickPose>[] = [
    {}, { blink: 1 }, { mood: 0.8 }, { mood: -0.8 }, { look: "down" }, { glasses: true }, { pencil: true },
    { beakF: 1 }, { beakF: 2 }, { hop: 2 }, { sx: 1, sy: -1 }, { sway: 1.4 }, { flap: 1 }, { glasses: true, pencil: true, beakF: 2, look: "down" },
  ];
  for (const p of POSES) {
    it(`pose ${JSON.stringify(p)}`, () => {
      const got = new Raster(CHICK_BOX.w, CHICK_BOX.h), want = new Raster(CHICK_BOX.w, CHICK_BOX.h);
      drawChick(got, CHICK_BOX.cx, CHICK_BOX.feetY, { ...BASE, ...p });
      APPROVED.Yappy.drawChick(want, CHICK_BOX.cx, CHICK_BOX.feetY, APPROVED.Yappy.base({ ...BASE, ...p }));
      const d = snapDiff(got, want, 1);
      expect(d.painted).toBeGreaterThan(0);
      expect(d.mismatched, `pose ${JSON.stringify(p)}: ${d.mismatched} px differ`).toBe(0);
      expect(got.subpixel).toBe(0);
    });
  }

  it("the pill draws THIS chick (no second copy in YappyPill)", () => {
    const pill = readSibling("../pill/YappyPill.tsx");
    expect(pill).toMatch(/import \{ drawChick as drawChickArt \} from "\.\.\/sprites\/art";/);
    expect(pill).toMatch(/drawChickArt\(octx, cx, feetY, A\)/);
    expect(pill).not.toMatch(/const cy = feetY - 11 - A\.hop/);
  });
});

/* ── the comparator really catches degradation (negative controls) ──────── */

function shifted(src: Raster, dx: number, dy: number): Raster {
  // 3 px of margin, so a shifted sprite that touches its edge is never cropped
  const r = new Raster(src.width + 3, src.height + 3);
  r.imageSmoothingEnabled = false;
  r.drawImage(src, 0, 0, src.width, src.height, dx, dy, src.width, src.height);
  return r;
}
function boxBlur(src: Raster): Raster {
  const r = new Raster(src.width, src.height);
  for (let y = 0; y < src.height; y++) for (let x = 0; x < src.width; x++) {
    const acc = [0, 0, 0, 0]; let n = 0;
    for (let j = -1; j <= 1; j++) for (let i = -1; i <= 1; i++) { const p = src.px(x + i, y + j); acc[0] += p.r; acc[1] += p.g; acc[2] += p.b; acc[3] += p.a; n++; }
    const k = (y * src.width + x) * 4;
    for (let c = 0; c < 4; c++) r.data[k + c] = Math.round(acc[c] / n);
  }
  return r;
}

describe("a blurred or drifted sprite fails", () => {
  for (const name of SPRITE_NAMES) {
    it(`${name}: 1 px drift passes, 2 px drift fails, blur fails, a recolour fails`, () => {
      const want = renderApproved(name, 0);
      expect(snapDiff(shifted(want, 1, 0), want, 1).mismatched, "1 px drift is inside tolerance").toBe(0);
      expect(snapDiff(shifted(want, 1, 1), want, 1).mismatched, "1 px drift is inside tolerance").toBe(0);
      expect(snapDiff(shifted(want, 2, 0), want, 1).mismatched, "2 px drift").toBeGreaterThan(0);
      expect(snapDiff(boxBlur(want), want, 1).mismatched, "blur").toBeGreaterThan(0);
      const recol = shifted(want, 0, 0);
      const i = recol.data.findIndex((_, k) => k % 4 === 3 && recol.data[k] > 0) - 3;
      recol.data[i] = (recol.data[i] + 1) % 256;   // ONE channel of ONE pixel, off by one
      expect(snapDiff(recol, want, 1).mismatched, "a one-step colour change").toBeGreaterThan(0);
    });
  }

  it("a fractional blit is counted as subpixel (what a real canvas would smooth)", () => {
    const want = renderApproved("hoot", 0);
    expect(want.subpixel, "the approved art itself is whole-pixel").toBe(0);
    const r = new Raster(100, 100);
    r.imageSmoothingEnabled = false;
    r.drawImage(want, 0, 0, want.width, want.height, 0, 0, want.width * 1.5, want.height * 1.5);
    expect(r.subpixel).toBeGreaterThan(0);
  });
});
