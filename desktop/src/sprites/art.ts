/**
 * FB-09 — the approved companion art, in the app's own code.
 *
 * Wilson, 2026-11-06 ~17:00Z: "all of them look blurred" and "make sure none
 * of these ever degrade". The nine sprites below are the round-two art from
 * the design review (design-2026-11-06/assets/friends-hd.js, copied verbatim
 * to ./approved/ so CI can read it) and the pill chick `drawChick`, which since
 * yap-1106-yappy-detail-pass IS the approved round-two Yappy (`D.yappy`) driven
 * by the pill's pose (./approved/yappy.js keeps the 0.9.3 chick it replaced, as
 * the "before"). `art.snapshot.test.ts` renders every one of them from THIS
 * file and pixel-compares it to the approved frames: zero tolerance for
 * colour, one pixel of tolerance for position.
 *
 * Every draw call lands on whole art pixels (`Math.round` in the helpers,
 * integer widths), so a sprite is a grid of solid cells. Scaling to the screen
 * is `crisp.ts`'s job, at a whole number of DEVICE pixels per art pixel.
 *
 * Rules (same as Yappy): light from top-left, a base tone + a light ellipse +
 * a shade ellipse, a warm dark outline, 1-2 px eye highlights, 0.7 blush, a
 * ground shadow so nobody looks pasted on. Pure canvas, no assets.
 *
 * Do not "tidy" the draw bodies: they are a line-for-line port and the
 * snapshot test is the only judge of a change. New art goes through a new
 * approved artifact first.
 */

/** The three members of CanvasRenderingContext2D the art uses. A real 2D
 * context satisfies it, and so does the test raster (./raster.ts). */
export interface PixelCtx {
  fillStyle: string | CanvasGradient | CanvasPattern;
  globalAlpha: number;
  fillRect(x: number, y: number, w: number, h: number): void;
}

export interface Helpers {
  /** A filled pixel ellipse, one whole-pixel row at a time. */
  ell(cx: number, cy: number, rx: number, ry: number, col: string, a?: number): void;
  /** A whole-pixel rectangle (x and y rounded; w and h are whole numbers). */
  r(x: number, y: number, w: number, h: number, col: string, a?: number): void;
}

export interface SpriteOpt {
  /** Hoot without his headphones. */
  bare?: boolean;
  /** Cinder's mouth open. */
  talk?: boolean;
  /** Seconds, for Cinder's chest flame flicker. */
  t?: number;
}

export interface FriendSprite {
  /** Art width and height in art pixels. */
  w: number;
  h: number;
  /** f: 0 idle, 1 blink / action, 2 talk (Yappy). */
  draw(o: PixelCtx, h: Helpers, f: number, opt?: SpriteOpt): void;
}

/** Every approved sprite, in the order the acceptance names them. */
export const SPRITE_NAMES = ["yappy", "hoot", "shelly", "tidy", "gulp", "ember", "sal", "mochi", "cinder"] as const;
export type SpriteName = (typeof SPRITE_NAMES)[number];

/** The frames the approved art defines per sprite (f values the draw reads). */
export const SPRITE_FRAMES: Record<SpriteName, readonly number[]> = {
  yappy: [0, 1, 2], hoot: [0, 1], shelly: [0, 1], tidy: [0, 1], gulp: [0, 1],
  ember: [0, 1], sal: [0, 1], mochi: [0, 1], cinder: [0, 1],
};

export function H(o: PixelCtx): Helpers {
  return {
    ell(cx, cy, rx, ry, col, a) {
      o.fillStyle = col; if (a != null) o.globalAlpha = a;
      for (let y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) {
        const t = (y + .5 - cy) / ry; if (Math.abs(t) > 1) continue;
        const hw = rx * Math.sqrt(1 - t * t);
        o.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1);
      }
      o.globalAlpha = 1;
    },
    r(x, y, w, h, col, a) { o.fillStyle = col; if (a != null) o.globalAlpha = a; o.fillRect(Math.round(x), Math.round(y), w, h); o.globalAlpha = 1; },
  };
}

/** The ground shadow every friend stands on (warm brown at 20 percent). */
export function shadow(h: Helpers, cx: number, y: number, rx: number): void { h.ell(cx, y, rx, 1.6, "#3a2418", .2); }

/** A friend eye: dark oval, a two-pixel shine top-left, a soft one bottom-right; f === 1 is shut. */
export function eye(h: Helpers, x: number, y: number, f: number, big?: boolean): void {
  if (f === 1) { h.r(x - 2, y, 4, 1, "#2b2118"); return; }
  h.ell(x, y, big ? 2.6 : 2.1, big ? 3 : 2.5, "#2b2118"); h.r(x - 1, y - 2, 2, 1, "#fff"); h.r(x - 1, y - 1, 1, 1, "#fff"); h.r(x + 1, y + 1, 1, 1, "#fff", .7);
}

const D = {} as Record<SpriteName, FriendSprite>;
/* ── verbatim port of friends-hd.js `D` (only `o` renamed `_o`: unused) ── */
  D.yappy = { w: 40, h: 36, draw: function (_o, h, f) { var cx = 20, cy = 20; shadow(h, cx, 33, 10);
      h.ell(cx + 10, cy + 2, 4, 3, "#5a2e24"); h.ell(cx + 10, cy + 2, 3, 2, "#e09a1c");
      h.ell(cx, cy, 12, 12, "#5a2e24"); h.ell(cx, cy - .6, 11.6, 11.6, "#9a6248"); h.ell(cx, cy, 11, 11, "#ffd24a");
      h.ell(cx + 3, cy + 4, 8, 7, "#f2b52e"); h.ell(cx - 1, cy - 1, 9, 9, "#ffd24a"); h.ell(cx - 3, cy - 4, 7, 6, "#ffe9a0"); h.r(cx - 7, cy - 8, 3, 1, "#fffbe6"); h.r(cx - 8, cy - 7, 1, 1, "#fffbe6");
      h.ell(cx, cy + 5, 6, 5.5, "#fff2c4"); h.r(cx - 4, cy + 9, 8, 1, "#f3dca0");
      h.r(cx - 1, cy - 13, 1, 3, "#c98a14"); h.r(cx - 3, cy - 14, 2, 2, "#e8a41f"); h.r(cx, cy - 15, 3, 2, "#e8a41f"); h.r(cx + 1, cy - 15, 1, 1, "#ffd27a");
      h.ell(cx - 10, cy + 1, 3, 4.5, "#5a2e24"); h.ell(cx - 10, cy + 1, 2, 3.5, "#ffd24a"); h.r(cx - 11, cy + 3, 2, 1, "#e09a1c"); h.r(cx - 11, cy + 1, 1, 1, "#e09a1c");
      eye(h, cx - 5, cy + 1, f, true); eye(h, cx + 5, cy + 1, f, true);
      h.r(cx - 1, cy + 5, 3, 1, "#e07a12"); h.r(cx - 2, cy + 6, 5, 1, "#ff9f1c"); h.r(cx - 1, cy + 7, 3, 1, f === 2 ? "#7a2e14" : "#ff9f1c"); if (f === 2) h.r(cx, cy + 8, 1, 1, "#ff5d7a");
      h.r(cx - 9, cy + 4, 3, 1, "#ff89a8", .75); h.r(cx + 7, cy + 4, 3, 1, "#ff89a8", .75); h.r(cx - 8, cy + 5, 1, 1, "#ff89a8", .4);
      [-4, 3].forEach(function (x) { h.r(cx + x, cy + 11, 1, 2, "#f2861a"); h.r(cx + x - 1, cy + 13, 3, 1, "#d06a10"); }); } };
  D.hoot = { w: 40, h: 40, draw: function (_o, h, f, opt) { var cx = 20, cy = 23; shadow(h, cx, 37, 11);
      h.ell(cx, cy, 13, 13.5, "#4a2c1e"); h.ell(cx, cy, 12, 12.5, "#a8774f"); h.ell(cx + 4, cy + 5, 8, 8, "#8a5d3a"); h.ell(cx - 1, cy - 1, 10, 10, "#a8774f"); h.ell(cx - 4, cy - 5, 6, 5, "#c8956a");
      h.ell(cx, cy + 6, 7.5, 6.5, "#f0dcb4"); [[-4, 4], [0, 5], [4, 4], [-2, 8], [2, 8]].forEach(function (p) { h.r(cx + p[0], cy + p[1], 1, 1, "#c9a878"); h.r(cx + p[0] + 1, cy + p[1] + 1, 1, 1, "#c9a878"); });
      h.r(cx - 10, cy - 14, 3, 5, "#4a2c1e"); h.r(cx - 9, cy - 13, 2, 4, "#a8774f"); h.r(cx + 7, cy - 14, 3, 5, "#4a2c1e"); h.r(cx + 7, cy - 13, 2, 4, "#a8774f");
      h.ell(cx - 5, cy - 3, 5.5, 5, "#4a2c1e"); h.ell(cx + 5, cy - 3, 5.5, 5, "#4a2c1e"); h.ell(cx - 5, cy - 3, 4.6, 4.2, "#fbf1dc"); h.ell(cx + 5, cy - 3, 4.6, 4.2, "#fbf1dc");
      eye(h, cx - 5, cy - 3, f, true); eye(h, cx + 5, cy - 3, f, true);
      h.r(cx - 1, cy, 3, 2, "#ff9f1c"); h.r(cx, cy + 2, 1, 1, "#e07a12");
      h.ell(cx - 12, cy + 3, 3, 7, "#4a2c1e"); h.ell(cx - 12, cy + 3, 2, 6, "#8a5d3a"); h.ell(cx + 12, cy + 3, 3, 7, "#4a2c1e"); h.ell(cx + 12, cy + 3, 2, 6, "#8a5d3a"); h.r(cx - 13, cy + 5, 2, 1, "#6e4428"); h.r(cx + 12, cy + 5, 2, 1, "#6e4428");
      h.r(cx - 10, cy - 4, 1, 1, "#ff89a8", .6); h.r(cx + 9, cy - 4, 1, 1, "#ff89a8", .6);
      [-4, 3].forEach(function (x) { h.r(cx + x - 1, cy + 13, 3, 1, "#f2861a"); h.r(cx + x, cy + 12, 1, 1, "#f2861a"); });
      if (!(opt && opt.bare)) { h.r(cx - 9, cy - 16, 18, 2, "#2a2d35"); h.r(cx - 9, cy - 16, 18, 1, "#5a606e"); h.r(cx - 12, cy - 14, 2, 8, "#2a2d35"); h.r(cx + 10, cy - 14, 2, 8, "#2a2d35"); h.r(cx - 14, cy - 8, 4, 7, "#c8443a"); h.r(cx - 14, cy - 8, 2, 2, "#ff7a6a"); h.r(cx + 10, cy - 8, 4, 7, "#c8443a"); h.r(cx + 12, cy - 8, 2, 2, "#ff7a6a"); } } };
  D.shelly = { w: 46, h: 36, draw: function (_o, h, f) { shadow(h, 23, 33, 18);
      h.r(4, 25, 36, 7, "#5a4632"); h.r(5, 25, 35, 6, "#e8d9b5"); h.r(5, 29, 35, 2, "#cdbb92"); h.r(6, 25, 30, 1, "#fff4d8");
      h.ell(37, 20, 5, 7, "#5a4632"); h.ell(37, 20, 4, 6, "#e8d9b5"); h.ell(38, 22, 3, 4, "#cdbb92"); h.ell(36, 18, 2, 3, "#fff4d8");
      h.r(35, 8 - f, 1, 7 + f, "#5a4632"); h.r(40, 8 - f, 1, 7 + f, "#5a4632");
      h.ell(35.5, 7 - f, 2.2, 2.2, "#5a4632"); h.ell(35.5, 7 - f, 1.6, 1.6, "#fff"); h.r(35, 7 - f, 1, 1, "#2b2118"); h.ell(40.5, 7 - f, 2.2, 2.2, "#5a4632"); h.ell(40.5, 7 - f, 1.6, 1.6, "#fff"); h.r(40, 7 - f, 1, 1, "#2b2118");
      h.r(36, 22, 3, 1, "#5a4632"); h.r(35, 21, 1, 1, "#5a4632"); h.r(34, 20, 2, 1, "#ff89a8", .7);
      h.ell(18, 16, 12, 12, "#2f5a2c"); h.ell(18, 16, 11, 11, "#7fbf6a"); h.ell(21, 19, 8, 8, "#5f9f4e"); h.ell(16, 13, 7, 6, "#a7dc90");
      h.ell(18, 16, 7, 7, "#2f5a2c"); h.ell(18, 16, 6, 6, "#7fbf6a"); h.ell(19, 17, 3.5, 3.5, "#2f5a2c"); h.ell(19, 17, 2.5, 2.5, "#a7dc90"); h.r(19, 17, 1, 1, "#2f5a2c");
      h.r(9, 7, 3, 1, "#e8ffd8"); h.r(8, 8, 1, 2, "#e8ffd8");
      h.r(24, 2, 7, 10, "#2a2d35"); h.r(25, 1, 5, 1, "#2a2d35"); h.r(25, 3, 5, 8, "#3b3f4a"); var lv = f === 1 ? 3 : 2; for (var i = 0; i < lv; i++) h.r(26, 9 - i * 3, 3, 2, i === lv - 1 ? "#ffd23f" : "#7fdc7a"); } };
  D.tidy = { w: 42, h: 36, draw: function (_o, h, f) { shadow(h, 20, 33, 13);
      h.r(31 - f, 6, 1, 22, "#6b4428"); h.r(32 - f, 6, 1, 22, "#a5704a"); h.r(27 - f, 27, 9, 5, "#9a7a3a"); h.r(28 - f, 27, 7, 4, "#e8c86a"); for (var k = 0; k < 4; k++) h.r(28 - f + k * 2, 31, 1, 2, "#c9a45c");
      h.ell(20, 21, 12, 10, "#5a1a14"); h.ell(20, 21, 11, 9, "#e0453a"); h.ell(23, 24, 8, 6, "#b8332a"); h.ell(17, 17, 6, 4, "#ff7a6a"); h.r(14, 15, 3, 1, "#ffd0c8");
      h.r(20, 12, 1, 18, "#5a1a14"); [[14, 20, 2.4], [26, 19, 2.4], [15, 26, 1.8], [25, 26, 1.8], [20, 23, 1.6]].forEach(function (s) { if (s[0] === 20) return; h.ell(s[0], s[1], s[2], s[2], "#2b2118"); h.r(s[0] - 1, s[1] - 1, 1, 1, "#5a4a4a"); });
      h.ell(20, 11, 7, 5.5, "#2b2118"); h.ell(18, 10, 3, 2, "#4a3a3a"); h.ell(16, 11, 2.6, 2.8, "#fff"); h.ell(24, 11, 2.6, 2.8, "#fff");
      if (f === 1) { h.r(15, 11, 3, 1, "#2b2118"); h.r(23, 11, 3, 1, "#2b2118"); } else { h.r(16, 11, 2, 2, "#2b2118"); h.r(24, 11, 2, 2, "#2b2118"); h.r(16, 11, 1, 1, "#fff"); h.r(24, 11, 1, 1, "#fff"); }
      h.r(18, 14, 4, 1, "#ff89a8", .6); h.r(15, 4, 1, 4, "#2b2118"); h.r(14, 3, 1, 1, "#2b2118"); h.r(25, 4, 1, 4, "#2b2118"); h.r(26, 3, 1, 1, "#2b2118");
      [12, 16, 24, 28].forEach(function (x) { h.r(x, 30, 1, 2, "#2b2118"); }); } };
  D.gulp = { w: 46, h: 38, draw: function (_o, h, f) { shadow(h, 21, 35, 14);
      h.ell(21, 24, 14, 10, "#2f5a2c"); h.ell(21, 24, 13, 9, "#6cc35a"); h.ell(24, 27, 10, 6, "#4f9f42"); h.ell(18, 20, 9, 5, "#94dc7c"); h.ell(21, 28, 8, 5, "#e1f4c4"); h.r(15, 32, 12, 1, "#c4dca0");
      [[14, 13], [28, 13]].forEach(function (p) { h.ell(p[0], p[1], 5, 5, "#2f5a2c"); h.ell(p[0], p[1], 4, 4, "#6cc35a"); h.ell(p[0] - 1, p[1] - 1, 2, 2, "#94dc7c"); h.ell(p[0], p[1] + 1, 2.6, 2.6, "#fff"); });
      eye(h, 14, 14, f === 1 ? 0 : 0); eye(h, 28, 14, 0);
      if (f === 1) { h.r(12, 21, 18, 3, "#7a2e14"); h.r(13, 22, 16, 1, "#ff5d7a"); h.r(29, 22, 9, 1, "#ff5d7a"); h.r(37, 17, 9, 8, "#c9c3ad"); h.r(38, 18, 7, 6, "#fffdf5"); h.r(39, 20, 5, 1, "#8b877f"); h.r(39, 22, 4, 1, "#8b877f"); }
      else { h.r(12, 21, 18, 1, "#2f5a2c"); h.r(11, 20, 1, 1, "#2f5a2c"); h.r(30, 20, 1, 1, "#2f5a2c"); }
      h.r(9, 23, 3, 1, "#ff89a8", .7); h.r(31, 23, 3, 1, "#ff89a8", .7);
      [[9, 33], [29, 33]].forEach(function (p) { h.r(p[0], p[1], 6, 2, "#4f9f42"); h.r(p[0], p[1] + 1, 1, 1, "#2f5a2c"); h.r(p[0] + 2, p[1] + 1, 1, 1, "#2f5a2c"); h.r(p[0] + 4, p[1] + 1, 1, 1, "#2f5a2c"); }); } };
  D.ember = { w: 32, h: 28, draw: function (_o, h, f) {
      h.ell(13, 8, 6, 4, "#9fb8d8"); h.ell(13, 8, 5, 3, "#eef6ff", .75); h.ell(19, 9, 4, 3, "#9fb8d8"); h.ell(19, 9, 3, 2, "#eef6ff", .75);
      h.ell(14, 15, 6, 4.5, "#2e2440"); h.ell(14, 15, 5, 3.5, "#5a4a6a"); h.ell(13, 14, 3, 2, "#7a6a8a");
      h.ell(8, 14, 4, 4, "#2e2440"); h.ell(8, 14, 3.2, 3.2, "#3e3450"); h.ell(7, 14, 1.6, 1.8, "#fff"); h.r(7, 14, 1, 1, "#2b2118"); h.r(4, 9, 1, 3, "#2e2440"); h.r(3, 8, 1, 1, "#2e2440"); h.r(6, 9, 1, 2, "#2e2440");
      h.ell(21, 18, 5, 4, "#8a6a10"); h.ell(21, 18, 4, 3, f === 1 ? "#fff6a0" : "#ffd84a"); h.ell(20, 17, 2, 1.5, "#fffbe0");
      if (f === 1) { [[27, 16], [26, 21], [17, 23], [24, 23], [27, 19]].forEach(function (p) { h.r(p[0], p[1], 1, 1, "#fff6a0", .55); }); }
      h.r(11, 19, 1, 2, "#2e2440"); h.r(15, 19, 1, 2, "#2e2440"); } };
  D.sal = { w: 46, h: 30, draw: function (_o, h, f) { shadow(h, 22, 27, 18);
      h.r(30, 18 + f, 12, 3, "#7a2e14"); h.r(31, 18 + f, 11, 2, "#ff8a3c"); h.r(40, 17 + f, 4, 2, "#ff8a3c");
      h.ell(22, 18, 11, 6, "#7a2e14"); h.ell(22, 18, 10, 5, "#ff7a3c"); h.ell(24, 20, 8, 3, "#e05a24"); h.ell(20, 16, 7, 2.5, "#ffa86a");
      [16, 21, 26].forEach(function (x) { h.r(x, 15, 2, 2, "#ffd23f"); h.r(x, 15, 1, 1, "#fff2b0"); });
      h.ell(9, 16, 7, 5.5, "#7a2e14"); h.ell(9, 16, 6, 4.5, "#ff7a3c"); h.ell(8, 14, 4, 2.5, "#ffa86a"); eye(h, 7, 15, f); eye(h, 12, 15, f); h.r(5, 19, 6, 1, "#7a2e14"); h.r(4, 18, 1, 1, "#7a2e14"); h.r(4, 17, 2, 1, "#ff89a8", .7); h.r(13, 17, 2, 1, "#ff89a8", .7);
      [[13, 22], [17, 22], [27, 22], [31, 22]].forEach(function (p) { h.r(p[0], p[1], 2, 3, "#e05a24"); h.r(p[0] - 1, p[1] + 3, 4, 1, "#7a2e14"); }); } };
  D.mochi = { w: 40, h: 40, draw: function (_o, h, f) { shadow(h, 20, 37, 11);
      h.ell(20, 27, 11, 10, "#3e3e48"); h.ell(20, 27, 10, 9, "#9a9aa6"); h.ell(23, 30, 7, 6, "#7c7c88"); h.ell(20, 30, 6, 5, "#dcdce4");
      h.ell(8, 12, 6, 6, "#3e3e48"); h.ell(8, 12, 5, 5, "#9a9aa6"); h.ell(8, 12, 3, 3, "#f0c8d0"); h.ell(32, 12, 6, 6, "#3e3e48"); h.ell(32, 12, 5, 5, "#9a9aa6"); h.ell(32, 12, 3, 3, "#f0c8d0");
      h.ell(20, 15, 11, 9, "#3e3e48"); h.ell(20, 15, 10, 8, "#9a9aa6"); h.ell(17, 12, 6, 4, "#b4b4c0");
      eye(h, 15, 15, f ? 1 : 1); eye(h, 25, 15, 1);
      h.ell(20, 18, 3, 3.5, "#2b2b33"); h.r(19, 16, 1, 1, "#6a6a78"); h.r(12, 19, 3, 1, "#ff89a8", .7); h.r(25, 19, 3, 1, "#ff89a8", .7);
      h.r(14, 24, 4, 3, "#7c7c88"); h.r(22, 24, 4, 3, "#7c7c88"); h.r(17, 23, 1, 1, "#3e3e48"); h.r(22, 23, 1, 1, "#3e3e48");
      if (f === 1) { h.r(30, 3, 2, 1, "#5a6280"); h.r(31, 4, 1, 1, "#5a6280"); h.r(30, 5, 2, 1, "#5a6280"); } } };
  /* Cinder, redrawn from Wilson's 2026-11-06 reference: a round red chibi
   * dragon, cream belly and horns, small pink-membrane wings, big eyes, a
   * little flame on the chest. f: 0 idle, 1 blink. opt.talk opens the mouth. */
  D.cinder = { w: 46, h: 42, draw: function (_o, h, f, opt) { var cx = 23, talk = opt && opt.talk, fl = Math.floor((opt && opt.t || 0) * 6) % 2;
      shadow(h, cx, 39, 12);
      // tail with a spade tip
      h.r(31, 31, 6, 3, "#6a1420"); h.r(32, 31, 5, 2, "#d8343a"); h.r(36, 28, 3, 4, "#6a1420"); h.r(37, 29, 2, 2, "#d8343a"); h.r(38, 26, 4, 4, "#6a1420"); h.r(39, 27, 2, 2, "#ff6b6b");
      // wings, behind
      h.r(5, 15, 3, 3, "#6a1420"); h.r(38, 15, 3, 3, "#6a1420");
      h.ell(9, 20, 5, 4.5, "#6a1420"); h.ell(9, 20, 4, 3.5, "#f28aa0"); h.r(7, 18, 1, 5, "#c85a72"); h.r(10, 18, 1, 5, "#c85a72"); h.r(6, 16, 2, 2, "#d8343a");
      h.ell(37, 20, 5, 4.5, "#6a1420"); h.ell(37, 20, 4, 3.5, "#f28aa0"); h.r(35, 18, 1, 5, "#c85a72"); h.r(38, 18, 1, 5, "#c85a72"); h.r(38, 16, 2, 2, "#d8343a");
      // body
      h.ell(cx, 29, 10.5, 9, "#6a1420"); h.ell(cx, 29, 9.5, 8, "#e0393e"); h.ell(cx + 3, 32, 7, 5, "#b82a33"); h.ell(cx - 3, 26, 5, 3, "#ff6b6b");
      h.ell(cx, 31, 6, 6, "#e8cfa0"); h.ell(cx, 30.5, 5.4, 5.4, "#fbe8c4"); [29, 32, 35].forEach(function (y) { h.r(cx - 4, y, 8, 1, "#ead2a2"); });
      // chest flame
      h.r(cx - 1, 25 - fl, 2, 1, "#ffd23f"); h.r(cx - 2, 26, 4, 2, "#ff8a3c"); h.r(cx - 1, 26, 2, 2, "#ffd23f"); h.r(cx - 1, 28, 2, 1, "#ff8a3c"); h.r(cx, 27, 1, 1, "#fff6c0");
      // arms and feet
      h.ell(13, 29, 2.5, 2, "#6a1420"); h.ell(13, 29, 1.6, 1.2, "#e0393e"); h.ell(33, 29, 2.5, 2, "#6a1420"); h.ell(33, 29, 1.6, 1.2, "#e0393e");
      [[17, 37], [29, 37]].forEach(function (p) { h.ell(p[0], p[1], 3.5, 2, "#6a1420"); h.ell(p[0], p[1], 2.6, 1.3, "#d8343a"); h.r(p[0] - 2, p[1] + 1, 1, 1, "#fbe8c4"); h.r(p[0], p[1] + 1, 1, 1, "#fbe8c4"); h.r(p[0] + 2, p[1] + 1, 1, 1, "#fbe8c4"); });
      // horns and head spikes
      h.r(12, 5, 3, 5, "#7a5a3a"); h.r(13, 5, 2, 4, "#f6e3b0"); h.r(11, 3, 2, 3, "#7a5a3a"); h.r(12, 3, 1, 2, "#f6e3b0");
      h.r(31, 5, 3, 5, "#7a5a3a"); h.r(31, 5, 2, 4, "#f6e3b0"); h.r(33, 3, 2, 3, "#7a5a3a"); h.r(33, 3, 1, 2, "#f6e3b0");
      // head
      h.ell(cx, 14, 12, 10, "#6a1420"); h.ell(cx, 14, 11, 9, "#e0393e"); h.ell(cx + 4, 17, 8, 5, "#c82e36"); h.ell(cx - 2, 12, 8, 6, "#e0393e"); h.ell(cx - 5, 9, 4, 2.5, "#ff6b6b"); h.r(cx - 8, 7, 2, 1, "#ffd0d0");
      [[cx - 3, 4], [cx, 3], [cx + 3, 4]].forEach(function (p) { h.r(p[0] - 1, p[1], 3, 2, "#6a1420"); h.r(p[0], p[1], 1, 1, "#f6e3b0"); });
      // eyes: big, dark, double shine
      if (f === 1) { h.r(cx - 8, 15, 5, 1, "#2b2118"); h.r(cx + 3, 15, 5, 1, "#2b2118"); }
      else { [cx - 5.5, cx + 5.5].forEach(function (x) { h.ell(x, 14, 3.2, 3.8, "#2b2118"); h.ell(x, 15, 2.2, 2, "#3a3a6a"); h.r(x - 2, 12, 2, 2, "#fff"); h.r(x + 1, 16, 1, 1, "#fff"); }); }
      // snout, nostrils, mouth
      h.ell(cx, 19, 4.5, 2.5, "#ff7b7b"); h.r(cx - 2, 18, 1, 1, "#8a1a24"); h.r(cx + 1, 18, 1, 1, "#8a1a24");
      if (talk) { h.r(cx - 2, 20, 4, 3, "#6a1420"); h.r(cx - 1, 22, 2, 1, "#ff5d7a"); } else { h.r(cx - 2, 21, 1, 1, "#6a1420"); h.r(cx - 1, 22, 2, 1, "#6a1420"); h.r(cx + 1, 21, 1, 1, "#6a1420"); }
      h.r(cx - 11, 18, 3, 1, "#ff89a8"); h.r(cx + 8, 18, 3, 1, "#ff89a8");
    } };

/** The approved friends, keyed by name. `FRIENDS.yappy` is the round-two
 * detail pass of Yappy; the pill chick `drawChick` below draws the same bird
 * (its rest, blink and talk poses are `FRIENDS.yappy` f=0, 1, 2 exactly). */
export const FRIENDS: Readonly<Record<SpriteName, FriendSprite>> = D;

/* ── the pill chick ──────────────────────────────────────────────────────── */

/** The chick palette, straight from YappyPill's `C` (one bird, one palette). */
export const CHICK_COLORS = {
  body: "#ffd24a", light: "#ffe9a0", dark: "#eba21f", outline: "#6b3a2e", cavity: "#7a2e14", belly: "#fff2c4",
  beak: "#ff9f1c", beakD: "#e07a12", eye: "#3a2f36", white: "#ffffff", blush: "#ff89a8", foot: "#f2861a", tongue: "#ff5d7a", sprout: "#e8a41f",
  glass: "#2c2a36", pencil: "#ffcf3a", pencilT: "#ff8fb0",
} as const;

/** The pose fields the chick reads (YappyPill's `SP` is a superset). */
export interface ChickPose {
  hop: number; sx: number; sy: number; blink: number; mood: number; beakF: number; sway: number; flap: number;
  look: "down" | "fwd"; glasses: boolean; pencil: boolean;
  /** Y2-B — tufts folded flat once the trial is out (app-only pose). */
  droop: boolean;
  /**
   * yap-1106-yappy-states-and-motion — a HANDS-FREE take: wings folded across
   * his belly (no hands needed). A POSE, not an
   * animation, so it holds under reduce motion too. Optional: every existing
   * caller and snapshot draws exactly as before.
   */
  handsFree?: boolean;
}

/** The 0.9.3 art box (`Yappy.sprite()` in ./approved/yappy.js): feet at (20, 30)
 * on 40x34. The detail-pass bird (toes and shadow to feetY + 3) still fits it. */
export const CHICK_BOX = { w: 40, h: 34, cx: 20, feetY: 30 } as const;

/**
 * The rest pose's position inside the approved `FRIENDS.yappy` art box
 * (40 x 36, body centre (20, 20)): `drawChick(o, 20, 31, REST)` paints the
 * approved frame pixel for pixel (art.snapshot.test.ts proves it).
 */
export const CHICK_DETAIL_BOX = { w: 40, h: 36, cx: 20, feetY: 31 } as const;

/** yap-1106-yappy-detail-pass — the detail-pass tones (friends-hd.js `D.yappy`). */
export const CHICK_DETAIL = {
  outline: "#5a2e24", rim: "#9a6248", shade: "#f2b52e", spark: "#fffbe6", bellyLine: "#f3dca0", tail: "#e09a1c",
  stem: "#c98a14", leafHi: "#ffd27a", toe: "#d06a10", eye: "#2b2118",
} as const;

/**
 * The pill chick — yap-1106-yappy-detail-pass (Wilson approved, design review
 * 2026-11-06): the round-two Yappy from friends-hd.js `D.yappy` (rim-lit warm
 * outline, a shade crescent, a leaf sprout, wing feathers, toes, the big
 * two-pixel eye shine, ground shadow), driven by the pill's pose so every
 * phase the character contract lists draws the SAME bird.
 *
 * The rest pose (no hop/squash/sway/flap, eyes open, beak shut) is the approved
 * frame f=0 exactly; `blink` is f=1 and `beakF >= 2` is f=2. The pose fields
 * the approved art has no frame for (mood, look down, glasses, pencil, droop,
 * beak half-open) are drawn in the same tones on the same whole-pixel grid.
 * The ground shadow stays on the ground (`feetY + 2`) when he hops.
 */
export function drawChick(o: PixelCtx, cx: number, feetY: number, A: ChickPose): void {
  const C = CHICK_COLORS, K = CHICK_DETAIL, h = H(o), ell = h.ell, r = h.r;
  const cy = feetY - 11 - A.hop; const rx = 11 + A.sx, ry = 11 + A.sy;
  shadow(h, cx, feetY + 2, 10);
  ell(cx + rx - 1, cy + 2, 4, 3, K.outline); ell(cx + rx - 1, cy + 2, 3, 2, K.tail);                     // tail
  // body: warm outline, a rim of light on the top edge, base, shade crescent bottom-right, light top-left
  ell(cx, cy, rx + 1, ry + 1, K.outline); ell(cx, cy - .6, rx + .6, ry + .6, K.rim); ell(cx, cy, rx, ry, C.body);
  ell(cx + 3, cy + 4, rx - 3, ry - 4, K.shade); ell(cx - 1, cy - 1, rx - 2, ry - 2, C.body); ell(cx - 3, cy - 4, rx - 4, ry - 5, C.light);
  r(cx - rx + 4, cy - ry + 3, 3, 1, K.spark); r(cx - rx + 3, cy - ry + 4, 1, 1, K.spark);
  ell(cx, cy + 5, rx - 5, ry - 5.5, C.belly); r(cx - 4, cy + ry - 2, 8, 1, K.bellyLine);
  // the leaf sprout is his ears (Y2-B): upright at rest, wilted once the trial is
  // out — a POSE (different pixels, same frame), never an animation.
  const sw = Math.round(A.sway);
  if (A.droop) {
    r(cx - 1 + sw, cy - ry - 2, 1, 2, K.stem);
    r(cx - 3 + sw, cy - ry - 2, 2, 1, C.sprout); r(cx - 4 + sw, cy - ry - 1, 1, 1, C.sprout);
    r(cx + sw, cy - ry - 2, 2, 1, C.sprout); r(cx + 2 + sw, cy - ry - 1, 1, 1, C.sprout);
  } else {
    r(cx - 1 + sw, cy - ry - 2, 1, 3, K.stem); r(cx - 3 + sw, cy - ry - 3, 2, 2, C.sprout);
    r(cx + sw, cy - ry - 4, 3, 2, C.sprout); r(cx + 1 + sw, cy - ry - 4, 1, 1, K.leafHi);
  }
  if (A.handsFree) {
    // yap-1106-yappy-states-and-motion — a HANDS-FREE take: both wings folded
    // over the belly, tips meeting under the beak, each with one feather mark
    // (detail-pass tones). No pencil: no hands needed.
    for (const s of [-1, 1]) {
      ell(cx + s * 4, cy + 8, 4, 2, K.outline); ell(cx + s * 4, cy + 8, 3, 1, C.body);
      r(cx + s * 4 - (s < 0 ? 1 : 0), cy + 8, 1, 1, K.tail);
    }
  } else {
    // front wing with two feather marks; it lifts with `flap` and holds the pencil
    const wx = cx - rx + 1, wy = cy + 1 - A.flap;
    ell(wx, wy, 3, 4.5, K.outline); ell(wx, wy, 2, 3.5, C.body); r(wx - 1, wy + 2, 2, 1, K.tail); r(wx - 1, wy, 1, 1, K.tail);
    if (A.pencil) { const px = cx - rx - 2, py = cy + 3; r(px - 3, py - 3, 1, 1, C.pencilT); r(px - 2, py - 2, 1, 1, C.pencil); r(px - 1, py - 1, 1, 1, C.pencil); r(px, py, 1, 1, C.pencil); r(px + 1, py + 1, 1, 1, C.dark); }
  }
  const ex = 5, ey = cy + 1;
  for (const s of [-1, 1]) { const eXc = cx + s * ex;
    if (A.mood > .55) { r(eXc - 2, ey, 1, 1, K.eye); r(eXc - 1, ey - 1, 1, 1, K.eye); r(eXc, ey - 1, 1, 1, K.eye); r(eXc + 1, ey, 1, 1, K.eye); }
    else if (A.blink > .5 || A.mood < -.5) { r(eXc - 2, ey + (A.mood < -.5 ? 1 : 0), 4, 1, K.eye); }
    else if (A.look === "down") { ell(eXc, ey + 1, 2.6, 2.6, K.eye); r(eXc - 1, ey, 2, 1, C.white); r(eXc + 1, ey + 2, 1, 1, C.white, .7); }
    else { eye(h, eXc, ey, 0, true); }
  }
  // reading glasses for the receptionist / essay personas, sized to the big eyes
  if (A.glasses) {
    for (const s of [-1, 1]) { const lx = cx + s * ex - 3; r(lx, ey - 2, 7, 1, C.glass); r(lx, ey + 2, 7, 1, C.glass); r(lx, ey - 2, 1, 5, C.glass); r(lx + 6, ey - 2, 1, 5, C.glass); }
    r(cx - 1, ey - 1, 3, 1, C.glass);
  }
  // beak: three rows; 1 = a sliver open, >= 2 = open with the tongue out (approved f=2)
  const f = A.beakF;
  r(cx - 1, cy + 5, 3, 1, C.beakD); r(cx - 2, cy + 6, 5, 1, C.beak);
  if (f >= 2) { r(cx - 1, cy + 7, 3, 1, C.cavity); r(cx, cy + 8, 1, 1, C.tongue); }
  else { r(cx - 1, cy + 7, 3, 1, C.beak); if (f === 1) r(cx, cy + 7, 1, 1, C.cavity); }
  r(cx - rx + 2, cy + 4, 3, 1, C.blush, .75); r(cx + rx - 4, cy + 4, 3, 1, C.blush, .75); r(cx - rx + 3, cy + 5, 1, 1, C.blush, .4);
  // toes: a leg under each side and a three-pixel foot
  for (const x of [-4, 3]) { r(cx + x, cy + ry, 1, 2, C.foot); r(cx + x - 1, cy + ry + 2, 3, 1, K.toe); }
}
