#!/usr/bin/env node
/**
 * yap-1106-yappy-detail-pass — the BEFORE / AFTER strip for the PR.
 *
 * Paints the pill chick in every pose the pill's phases drive, twice: the
 * 0.9.3 chick it replaces (desktop/src/sprites/approved/yappy.js `drawChick`)
 * on the top row and the detail-pass chick the app now runs
 * (desktop/src/sprites/art.ts `drawChick`) on the bottom row, plus the
 * approved round-two `D.yappy` frames f=0/1/2 for reference. Pure software
 * raster: no browser, no app, no build, no network. Writes ONE PNG and a JSON
 * legend (column -> pose) into --out.
 *
 *   node scripts/yappy-detail-strip.mjs --out <scratch dir> [--scale 6]
 *
 * Needs Node >= 22.18 / 23.6 (type stripping, to import art.ts directly).
 * The full-pill photos of all phases are scripts/pill-matrix.mjs, run once on
 * main and once on this branch.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SPRITES = join(ROOT, "desktop", "src", "sprites");
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : dflt; };
const OUT = arg("--out", null);
const SCALE = Math.max(1, Math.floor(Number(arg("--scale", "6")) || 6));
if (!OUT) { console.error("yappy-strip usage: --out <scratch dir> [--scale n]"); process.exit(2); }

const art = await import(join(SPRITES, "art.ts"));

/** The smallest PixelCtx the art needs: fillStyle (#rgb/#rrggbb), globalAlpha, fillRect, source-over. */
class Px {
  constructor(w, h) { this.width = w; this.height = h; this.data = new Uint8ClampedArray(w * h * 4); this.fillStyle = "#000000"; this.globalAlpha = 1; }
  getContext() { return this; }
  setTransform() {} clearRect() { this.data.fill(0); }
  fillRect(x, y, w, h) {
    const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(String(this.fillStyle).trim()); if (!m) return;
    const hx = m[1].length === 3 ? m[1].split("").map((c) => c + c).join("") : m[1];
    const c = [0, 2, 4].map((k) => parseInt(hx.slice(k, k + 2), 16)), a = this.globalAlpha;
    for (let py = Math.max(0, Math.round(y)); py < Math.min(this.height, Math.round(y + h)); py++)
      for (let px = Math.max(0, Math.round(x)); px < Math.min(this.width, Math.round(x + w)); px++) {
        const i = (py * this.width + px) * 4, da = this.data[i + 3] / 255, oa = a + da * (1 - a);
        for (let k = 0; k < 3; k++) this.data[i + k] = oa ? Math.round((c[k] * a + this.data[i + k] * da * (1 - a)) / oa) : 0;
        this.data[i + 3] = Math.round(oa * 255);
      }
  }
}

/** Run an approved design-review script in a fake window (as art.snapshot.test.ts does). */
function loadApproved() {
  const win = { devicePixelRatio: 1, matchMedia: () => ({ matches: true }), addEventListener: () => undefined };
  const doc = { createElement: () => new Px(300, 150) };
  for (const file of ["yappy.js", "friends-hd.js"])
    new Function("window", "document", "performance", "requestAnimationFrame", readFileSync(join(SPRITES, "approved", file), "utf8"))(win, doc, { now: () => 0 }, () => 0);
  return win;
}
const APPROVED = loadApproved();

const BASE = { hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0, look: "fwd", glasses: false, pencil: false, droop: false };
/** Every pose field YappyPill's phases drive (drawScene), alone and in the props' combinations. */
const POSES = [
  ["rest", {}],
  ["blink", { blink: 1 }],
  ["happy", { mood: 0.8 }],
  ["sad", { mood: -0.8 }],
  ["look down", { look: "down" }],
  ["beak half open", { beakF: 1 }],
  ["beak open", { beakF: 2 }],
  ["hop + squash", { hop: 3, sx: 1, sy: -1 }],
  ["wing flap", { flap: 2 }],
  ["sprout sway", { sway: 2 }],
  ["pencil (pad prop)", { pencil: true, look: "down" }],
  ["glasses (desk prop)", { glasses: true, beakF: 1 }],
  ["droop (trial over)", { droop: true }],
];
const CELL = { w: 44, h: 40, cx: 22, feetY: 32 };

const cells = [];
for (const [, p] of POSES) {
  const before = new Px(CELL.w, CELL.h), after = new Px(CELL.w, CELL.h);
  APPROVED.Yappy.drawChick(before, CELL.cx, CELL.feetY, APPROVED.Yappy.base({ ...BASE, ...p }));
  art.drawChick(after, CELL.cx, CELL.feetY, { ...BASE, ...p });
  cells.push([before, after]);
}
// reference column: the approved round-two Yappy frames
const ref = [0, 1, 2].map((f) => { const s = APPROVED.FR.D.yappy, o = new Px(s.w, s.h); s.draw(o, APPROVED.FR.H(o), f); const r = new Px(CELL.w, CELL.h); blit(o, r, 2, 0); return r; });

function blit(src, dst, ox, oy) {
  for (let y = 0; y < src.height; y++) for (let x = 0; x < src.width; x++) {
    const tx = x + ox, ty = y + oy; if (tx < 0 || ty < 0 || tx >= dst.width || ty >= dst.height) continue;
    const i = (y * src.width + x) * 4, j = (ty * dst.width + tx) * 4; for (let k = 0; k < 4; k++) dst.data[j + k] = src.data[i + k];
  }
}

// layout: row 0 = before, row 1 = after, row 2 = approved f=0/1/2; 2 px gutters; sky-blue ground for contrast
const cols = POSES.length, G = 2, W = (cols * (CELL.w + G) + G) * SCALE, H = (3 * (CELL.h + G) + G) * SCALE;
const img = new Uint8Array(W * H * 4);
const bg = (row) => (row === 0 ? [214, 220, 228] : row === 1 ? [198, 236, 255] : [236, 230, 214]);
for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) { const i = (y * W + x) * 4; img.set([40, 40, 46, 255], i); }
function place(px, col, row) {
  const ox = (G + col * (CELL.w + G)) * SCALE, oy = (G + row * (CELL.h + G)) * SCALE, b = bg(row);
  for (let y = 0; y < CELL.h; y++) for (let x = 0; x < CELL.w; x++) {
    const i = (y * CELL.w + x) * 4, a = px ? px.data[i + 3] / 255 : 0;
    const c = [0, 1, 2].map((k) => Math.round((px ? px.data[i + k] : 0) * a + b[k] * (1 - a)));
    for (let sy = 0; sy < SCALE; sy++) for (let sx = 0; sx < SCALE; sx++) img.set([...c, 255], ((oy + y * SCALE + sy) * W + ox + x * SCALE + sx) * 4);
  }
}
cells.forEach(([b, a], col) => { place(b, col, 0); place(a, col, 1); });
ref.forEach((r, col) => place(r, col, 2));

// PNG (RGBA, no filter)
const crcT = new Uint32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcT[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
const chunk = (type, data) => { const len = Buffer.alloc(4); len.writeUInt32BE(data.length); const td = Buffer.concat([Buffer.from(type, "ascii"), data]); const c = Buffer.alloc(4); c.writeUInt32BE(crc(td)); return Buffer.concat([len, td, c]); };
const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(W, 0); ihdr.writeUInt32BE(H, 4); ihdr[8] = 8; ihdr[9] = 6;
const raw = Buffer.alloc((W * 4 + 1) * H); for (let y = 0; y < H; y++) { raw[y * (W * 4 + 1)] = 0; Buffer.from(img.buffer, y * W * 4, W * 4).copy(raw, y * (W * 4 + 1) + 1); }
const png = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);

mkdirSync(OUT, { recursive: true });
const file = join(OUT, "yappy-detail-before-after.png");
writeFileSync(file, png);
writeFileSync(join(OUT, "yappy-detail-before-after.json"), JSON.stringify({
  rows: ["before: 0.9.3 chick (approved/yappy.js)", "after: detail pass (sprites/art.ts drawChick)", "reference: approved D.yappy f=0, 1, 2"],
  columns: POSES.map(([label, pose]) => ({ label, pose })), scale: SCALE,
}, null, 2));
console.log(`yappy-strip wrote file=${file} cols=${cols} scale=${SCALE}`);
