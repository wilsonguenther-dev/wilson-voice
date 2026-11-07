/** FB-09 — the software raster the sprite snapshot test stands on. */
import { describe, expect, it } from "vitest";
import { Raster, parseHex, snapDiff } from "./raster";

describe("raster", () => {
  it("parses #rgb and #rrggbb, and refuses anything else", () => {
    expect(parseHex("#fff")).toEqual({ r: 255, g: 255, b: 255, a: 255 });
    expect(parseHex("#3a2418")).toEqual({ r: 0x3a, g: 0x24, b: 0x18, a: 255 });
    expect(() => parseHex("rgba(0,0,0,.5)")).toThrow(/unsupported colour/);
    const r = new Raster(2, 2);
    r.fillStyle = {} as CanvasPattern;
    expect(() => r.fillRect(0, 0, 1, 1)).toThrow(/only hex/);
  });

  it("fills whole pixels, blends globalAlpha source-over, clears", () => {
    const r = new Raster(4, 4);
    r.fillStyle = "#ff0000"; r.fillRect(0, 0, 2, 2);
    expect(r.px(1, 1)).toEqual({ r: 255, g: 0, b: 0, a: 255 });
    expect(r.px(2, 2).a).toBe(0);
    r.globalAlpha = 0.5; r.fillStyle = "#0000ff"; r.fillRect(1, 1, 1, 1);
    expect(r.px(1, 1)).toEqual({ r: 128, g: 0, b: 128, a: 255 });
    r.globalAlpha = 0; r.fillRect(0, 0, 1, 1);
    expect(r.px(0, 0)).toEqual({ r: 255, g: 0, b: 0, a: 255 });
    r.clearRect(0, 0, 4, 4);
    expect(r.px(0, 0).a).toBe(0);
    expect(r.px(-1, 9)).toEqual({ r: 0, g: 0, b: 0, a: 0 });
    expect(r.subpixel).toBe(0);
  });

  it("counts off-grid edges and fractional blits as subpixel; save/restore/translate/scale compose", () => {
    const r = new Raster(8, 8);
    r.fillStyle = "#00ff00"; r.fillRect(0.5, 0, 1, 1);
    expect(r.subpixel).toBe(1);
    r.save(); r.translate(2, 2); r.scale(2, 2); r.fillRect(0, 0, 1, 1); r.restore();
    expect(r.px(3, 3)).toEqual({ r: 0, g: 255, b: 0, a: 255 });
    r.restore();   // an unbalanced restore is a no-op
    r.setTransform(1, 0, 0, 1, 0, 0);
    const src = new Raster(2, 2); src.fillStyle = "#123456"; src.fillRect(0, 0, 2, 2);
    r.drawImage(src, 0, 0, 2, 2, 0, 0, 3, 3);
    expect(r.subpixel).toBe(2);
    expect(r.smoothedBlits).toBe(1);
  });

  it("snapDiff: identical = 0, transparent pixels compare equal whatever their RGB", () => {
    const a = new Raster(3, 3), b = new Raster(3, 3);
    a.fillStyle = "#abcdef"; a.fillRect(1, 1, 1, 1); b.fillStyle = "#abcdef"; b.fillRect(1, 1, 1, 1);
    b.data[0] = 99;   // RGB under alpha 0
    expect(snapDiff(b, a)).toEqual({ dx: 0, dy: 0, mismatched: 0, painted: 1 });
  });
});
