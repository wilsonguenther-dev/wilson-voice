/**
 * yap-1106-friends-art-hd — every friend has idle, blink and a two-frame
 * action in the manifest format, every frame obeys the density rules from the
 * companions page (section 2) and lands on whole pixels, and the side-by-side
 * sheet (Yappy beside each friend's four frames) renders crisply.
 *
 * The PR's side-by-side screenshot is this sheet, written headless from the
 * same software raster the snapshot test uses (no browser, no app):
 *
 *   cd desktop && YAP_FRIENDS_SHEET=/abs/path/friends-sheet.png npx vitest run src/sprites/friends.test.ts
 */
import { afterEach, describe, expect, it, vi } from "vitest";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { writeFileSync } from "node:fs";
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { deflateSync } from "node:zlib";
import { FRIENDS, H, type PixelCtx } from "./art";
import { FRIEND_IDS, drawFriendFrame, frameSpec, friendManifest, pickFrame, type FrameKind, type FriendManifest } from "./friends";
import { Raster } from "./raster";

const KINDS: readonly (readonly [FrameKind, number])[] = [["idle", 0], ["blink", 0], ["action", 0], ["action", 1]];

const manifest = (id: string): FriendManifest => {
  const m = friendManifest(id);
  if (!m) throw new Error(`no manifest for ${id}`);
  return m;
};

function render(id: string, kind: FrameKind, index = 0): Raster {
  const m = manifest(id);
  const r = new Raster(m.w, m.h);
  expect(drawFriendFrame(r, id, kind, index)).toBe(true);
  return r;
}

const same = (a: Raster, b: Raster): boolean => a.data.length === b.data.length && a.data.every((v, i) => v === b.data[i]);

interface Fill { col: string; a: number; x: number; y: number; w: number; h: number }

/** A context that only records what was filled, with which colour and alpha. */
function recorder(): PixelCtx & { fills: Fill[] } {
  const fills: Fill[] = [];
  return {
    fills,
    fillStyle: "#000000",
    globalAlpha: 1,
    fillRect(x: number, y: number, w: number, h: number) { fills.push({ col: String(this.fillStyle).toLowerCase(), a: this.globalAlpha, x, y, w, h }); },
  };
}

function countPx(r: Raster, test: (p: { r: number; g: number; b: number; a: number }) => boolean): number {
  let n = 0;
  for (let y = 0; y < r.height; y++) for (let x = 0; x < r.width; x++) if (test(r.px(x, y))) n++;
  return n;
}

afterEach(() => { vi.restoreAllMocks(); });

describe("friend manifests", () => {
  it("every friend has idle, blink and a two-frame action", () => {
    expect([...FRIEND_IDS].sort()).toEqual(["cinder", "ember", "gulp", "hoot", "mochi", "sal", "shelly", "tidy"]);
    for (const id of FRIEND_IDS) {
      const m = manifest(id);
      expect(m.id).toBe(id);
      expect(m.label.length).toBeGreaterThan(0);
      expect({ w: m.w, h: m.h }).toEqual({ w: FRIENDS[id].w, h: FRIENDS[id].h });
      expect(m.idle).toBeTruthy();
      expect(m.blink).toBeTruthy();
      expect(m.action.frames).toHaveLength(2);
      expect(m.action.name).toMatch(/^[a-z]+$/);
      expect(m.action.frameMs).toBeGreaterThan(0);
      expect(m.blinkMs).toBeGreaterThan(0);
      expect(m.blinkEveryMs[1]).toBeGreaterThanOrEqual(m.blinkEveryMs[0]);
      expect(m.blinkEveryMs[0]).toBeGreaterThan(m.blinkMs);
    }
  });

  it("every frame lands on whole art pixels", () => {
    for (const id of FRIEND_IDS) for (const [kind, i] of KINDS) {
      const r = render(id, kind, i);
      expect(r.subpixel, `${id} ${kind}${i}`).toBe(0);
      expect(countPx(r, (p) => p.a > 0), `${id} ${kind}${i} paints`).toBeGreaterThan(50);
    }
  });

  it("blink differs from idle, and the two action frames differ from each other and from idle", () => {
    for (const id of FRIEND_IDS) {
      const idle = render(id, "idle"), blink = render(id, "blink"), a0 = render(id, "action", 0), a1 = render(id, "action", 1);
      expect(same(idle, blink), `${id} blink`).toBe(false);
      expect(same(a0, a1), `${id} action frames`).toBe(false);
      expect(same(idle, a0), `${id} action 0`).toBe(false);
      expect(same(idle, a1), `${id} action 1`).toBe(false);
    }
  });

  it("follows the density rules: ground shadow, blush, eye shine, warm outline, light/base/shade tones", () => {
    for (const id of FRIEND_IDS) {
      const m = manifest(id);
      const rec = recorder();
      drawFriendFrame(rec, id, "idle");
      const cols = new Set(rec.fills.map((f) => f.col));
      // ground shadow: warm brown at 20 percent, and it shows in the pixels
      expect(rec.fills.some((f) => f.col === "#3a2418" && Math.abs(f.a - .2) < 1e-9), `${id} shadow fill`).toBe(true);
      expect(countPx(render(id, "idle"), (p) => p.r === 58 && p.g === 36 && p.b === 24 && p.a === 51), `${id} shadow px`).toBeGreaterThan(0);
      // blush
      expect(cols.has("#ff89a8"), `${id} blush`).toBe(true);
      // warm outline: never pure black
      expect(cols.has("#000") || cols.has("#000000"), `${id} black`).toBe(false);
      // at least a base, a light and a shade tone plus the outline
      expect(cols.size, `${id} tones`).toBeGreaterThanOrEqual(6);
      // two-pixel eye shine (Mochi's eyes rest shut: her peek shows it)
      const eyes = render(id, m.eyesClosedAtRest ? "blink" : "idle");
      expect(countPx(eyes, (p) => p.r === 255 && p.g === 255 && p.b === 255 && p.a === 255), `${id} shine`).toBeGreaterThanOrEqual(2);
    }
  });

  it("Ember, who floats, gets the shadow and blush on every frame", () => {
    for (const [kind, i] of KINDS) {
      const rec = recorder();
      drawFriendFrame(rec, "ember", kind, i);
      expect(rec.fills.some((f) => f.col === "#3a2418"), `ember ${kind}${i}`).toBe(true);
      expect(rec.fills.some((f) => f.col === "#ff89a8"), `ember ${kind}${i}`).toBe(true);
    }
  });

  it("frameSpec picks the frame the kind names", () => {
    const m = manifest("hoot");
    expect(frameSpec(m, "idle")).toBe(m.idle);
    expect(frameSpec(m, "blink")).toBe(m.blink);
    expect(frameSpec(m, "action")).toBe(m.action.frames[0]);
    expect(frameSpec(m, "action", 1)).toBe(m.action.frames[1]);
    expect(frameSpec(m, "action", 7)).toBe(m.action.frames[0]);
  });

  it("unknown ids draw nothing and log one line", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const r = new Raster(8, 8);
    expect(friendManifest("yappy")).toBeNull();
    expect(friendManifest("")).toBeNull();
    expect(drawFriendFrame(r, "nobody", "idle")).toBe(false);
    expect(countPx(r, (p) => p.a > 0)).toBe(0);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0][0])).toMatch(/^sprites friend_frame_unknown id=nobody kind=idle$/);
  });
});

describe("pickFrame", () => {
  it("reduce-motion never animates: idle at rest, the first action frame while acting", () => {
    const m = manifest("sal");
    for (const t of [0, 133, m.action.frameMs, 99999]) {
      expect(pickFrame(m, t, { reducedMotion: true })).toEqual({ kind: "idle", index: 0 });
      expect(pickFrame(m, t, { reducedMotion: true, acting: true })).toEqual({ kind: "action", index: 0 });
    }
  });

  it("a broken clock shows idle", () => {
    const m = manifest("tidy");
    for (const t of [NaN, -1, Infinity]) expect(pickFrame(m, t, { acting: true })).toEqual({ kind: "idle", index: 0 });
  });

  it("acting alternates the two action frames every frameMs", () => {
    const m = manifest("cinder");
    const f = m.action.frameMs;
    expect(pickFrame(m, 0, { acting: true })).toEqual({ kind: "action", index: 0 });
    expect(pickFrame(m, f - 1, { acting: true })).toEqual({ kind: "action", index: 0 });
    expect(pickFrame(m, f, { acting: true })).toEqual({ kind: "action", index: 1 });
    expect(pickFrame(m, 2 * f, { acting: true })).toEqual({ kind: "action", index: 0 });
  });

  it("resting blinks once per period and holds the blink for blinkMs", () => {
    const fixed: FriendManifest = { ...manifest("gulp"), blinkEveryMs: [1000, 1000], blinkMs: 100 };
    expect(pickFrame(fixed, 0).kind).toBe("idle");
    expect(pickFrame(fixed, 899).kind).toBe("idle");
    expect(pickFrame(fixed, 900).kind).toBe("blink");
    expect(pickFrame(fixed, 999).kind).toBe("blink");
    expect(pickFrame(fixed, 1000).kind).toBe("idle");
    expect(pickFrame(fixed, 1950).kind).toBe("blink");
  });

  it("every friend blinks within its range, and they do not all blink together", () => {
    const firstBlink: number[] = [];
    for (const id of FRIEND_IDS) {
      const m = manifest(id);
      let t = 0;
      while (t <= m.blinkEveryMs[1] && pickFrame(m, t).kind !== "blink") t += 10;
      expect(t, id).toBeLessThanOrEqual(m.blinkEveryMs[1]);
      expect(t, id).toBeGreaterThanOrEqual(m.blinkEveryMs[0] - m.blinkMs - 10);
      firstBlink.push(t);
    }
    expect(new Set(firstBlink).size).toBeGreaterThan(1);
  });
});

/* ── the side-by-side sheet ───────────────────────────────────────────── */

const CELL = { w: 48, h: 44 } as const;
const SCALE = 4;
const COLS = 5; // Yappy, idle, blink, action 0, action 1

/** Yappy (the density reference) beside every friend's four frames, one friend per row. */
function friendsSheet(scale = SCALE): Raster {
  const W = COLS * CELL.w, Hh = FRIEND_IDS.length * CELL.h;
  const r = new Raster(W * scale, Hh * scale);
  r.imageSmoothingEnabled = false;
  r.setTransform(1, 0, 0, 1, 0, 0);
  r.fillStyle = "#f6efe1"; r.fillRect(0, 0, W * scale, Hh * scale);
  r.fillStyle = "#eee3cb"; r.fillRect(0, 0, CELL.w * scale, Hh * scale);
  FRIEND_IDS.forEach((id, row) => {
    const place = (w: number, h: number, col: number) => {
      const ox = col * CELL.w + Math.floor((CELL.w - w) / 2), oy = row * CELL.h + (CELL.h - h);
      r.setTransform(scale, 0, 0, scale, ox * scale, oy * scale);
    };
    place(FRIENDS.yappy.w, FRIENDS.yappy.h, 0);
    FRIENDS.yappy.draw(r, H(r), 0);
    const m = manifest(id);
    KINDS.forEach(([kind, i], k) => { place(m.w, m.h, k + 1); drawFriendFrame(r, id, kind, i); });
  });
  r.setTransform(1, 0, 0, 1, 0, 0);
  return r;
}

const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; }
  return t;
})();

function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (let i = 0; i < bytes.length; i++) c = CRC[(c ^ bytes[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

/** A minimal RGBA PNG (8-bit, filter 0 on every row). */
function encodePng(r: Raster): Uint8Array {
  const raw = new Uint8Array(r.height * (r.width * 4 + 1));
  for (let y = 0; y < r.height; y++) {
    raw[y * (r.width * 4 + 1)] = 0;
    raw.set(r.data.subarray(y * r.width * 4, (y + 1) * r.width * 4), y * (r.width * 4 + 1) + 1);
  }
  const idat = new Uint8Array(deflateSync(raw) as Uint8Array);
  const ihdr = new Uint8Array(13);
  const dv = new DataView(ihdr.buffer);
  dv.setUint32(0, r.width); dv.setUint32(4, r.height); ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  const chunk = (type: string, data: Uint8Array): Uint8Array => {
    const out = new Uint8Array(12 + data.length);
    const v = new DataView(out.buffer);
    v.setUint32(0, data.length);
    for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
    out.set(data, 8);
    v.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
    return out;
  };
  const parts = [new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", new Uint8Array(0))];
  const png = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) { png.set(p, o); o += p.length; }
  return png;
}

describe("side-by-side sheet", () => {
  it("renders Yappy beside every friend's four frames, crisp at a whole-number scale", () => {
    const sheet = friendsSheet();
    expect(sheet.width).toBe(COLS * CELL.w * SCALE);
    expect(sheet.height).toBe(FRIEND_IDS.length * CELL.h * SCALE);
    expect(sheet.subpixel).toBe(0);
    expect(sheet.smoothedBlits).toBe(0);
    const png = encodePng(sheet);
    expect([...png.subarray(0, 8)]).toEqual([137, 80, 78, 71, 13, 10, 26, 10]);
    expect(crc32(new TextEncoder().encode("123456789"))).toBe(0xcbf43926);
    const out = (globalThis as unknown as { process?: { env?: Record<string, string | undefined> } }).process?.env?.YAP_FRIENDS_SHEET;
    if (out) {
      writeFileSync(out, png);
      console.info(`sprites friends_sheet_written bytes=${png.length} w=${sheet.width} h=${sheet.height}`);
    }
  });
});
