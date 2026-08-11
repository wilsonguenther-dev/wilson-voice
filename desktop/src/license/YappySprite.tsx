/**
 * YP3 — Yappy, standing still.
 *
 * The companion already exists twice, and neither fits here: `home/YappyHouse`
 * and `pill/YappyPill` are animated `<canvas>` scenes driven by a
 * requestAnimationFrame loop, a tone, a mood and a clock. A purchase prompt
 * wants the *character*, not a running loop — and a licensing screen is the
 * last place in the app that should own a rAF timer (YV81 spent a whole pass
 * parking idle animations).
 *
 * So this is the same bird, same palette, drawn once as SVG rects on the same
 * one-unit-per-pixel grid the canvas versions use: the front-facing body rows
 * (`BODY` in YappyHouse's `drawYappy`) and the same yolk/carrot/eye colours.
 * No state, no timer, no canvas — it costs nothing when it is on screen and
 * nothing when it is not.
 */

/** Straight from YappyHouse's `C` — one bird, one palette. */
const C = {
  yolkHi: "#ffe483",
  yolk: "#ffcf3f",
  yolkSh: "#efa72c",
  yolkDk: "#d3871d",
  carrot: "#ff7a3c",
  carrotDk: "#e85f24",
  eye: "#2b2118",
  shine: "#ffffff",
  blush: "#ff89a8",
};

/** Half-widths of the front-facing body, top row first (YappyHouse `BODY`). */
const BODY = [3, 5, 6, 7, 7, 7, 7, 7, 7, 7, 6, 6, 5, 4, 3];

const CX = 10; // centre column of the 20-wide grid
const TOP = 4; // first body row

type Px = [x: number, y: number, w: number, h: number, fill: string];

/** The whole bird, as a flat list of pixel rects, built once at module load. */
const PIXELS: Px[] = [
  // tuft
  [CX - 1, TOP - 3, 2, 3, C.yolk],
  [CX - 1, TOP - 4, 1, 1, C.yolkHi],
  // body, with a dark edge pixel each side so the silhouette reads at 1×
  ...BODY.flatMap((hw, r): Px[] => [
    [CX - hw, TOP + r, hw * 2, 1, C.yolk],
    [CX - hw, TOP + r, 1, 1, C.yolkDk],
    [CX + hw - 1, TOP + r, 1, 1, C.yolkDk],
  ]),
  // light from the upper left, shade to the lower right
  [CX - 6, TOP + 1, 3, 4, C.yolkHi],
  [CX - 7, TOP + 5, 2, 4, C.yolkHi],
  [CX + 3, TOP + 8, 4, 5, C.yolkSh],
  [CX - 6, TOP + 12, 12, 2, C.yolkSh],
  // wings
  [CX - 8, TOP + 5, 2, 5, C.yolkSh],
  [CX + 6, TOP + 5, 2, 5, C.yolkSh],
  // face
  [CX - 4, TOP + 5, 2, 2, C.eye],
  [CX + 2, TOP + 5, 2, 2, C.eye],
  [CX - 4, TOP + 5, 1, 1, C.shine],
  [CX + 2, TOP + 5, 1, 1, C.shine],
  [CX - 1, TOP + 8, 2, 2, C.carrot],
  [CX - 1, TOP + 9, 2, 1, C.carrotDk],
  [CX - 6, TOP + 8, 2, 1, C.blush],
  [CX + 4, TOP + 8, 2, 1, C.blush],
  // feet
  [CX - 4, TOP + 15, 3, 2, C.carrot],
  [CX - 4, TOP + 16, 3, 1, C.carrotDk],
  [CX + 1, TOP + 15, 3, 2, C.carrot],
  [CX + 1, TOP + 16, 3, 1, C.carrotDk],
];

export default function YappySprite({
  size = 96,
  className,
}: {
  size?: number;
  className?: string;
}) {
  return (
    <svg
      className={className}
      width={size}
      height={(size * 22) / 20}
      viewBox="0 0 20 22"
      shapeRendering="crispEdges"
      role="img"
      aria-label="Yappy, Yap's companion"
      focusable="false"
    >
      {PIXELS.map(([x, y, w, h, fill], i) => (
        <rect key={i} x={x} y={y} width={w} height={h} fill={fill} />
      ))}
    </svg>
  );
}
