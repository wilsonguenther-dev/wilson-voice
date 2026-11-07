/**
 * yap-1106-pill-sky — the pill's sky is the HOME ROOM'S TIME OF DAY (Wilson's
 * pick, option C of companions.html section 9): the same real clock the room's
 * window runs on (`skyPhase`, home/habitat), the same four keyframe colours
 * (home/YappyHouse.tsx `SKY`), drawn the pill's way — six pixel bands from the
 * room's top colour to its bottom colour, the sun (or the moon and a few stars
 * at night) and the grass, all at the pill's 176 x 44 art scale.
 *
 * Three rules this module exists to keep, each a test in sky.test.ts:
 *
 *  1. ONE draw per redraw. The whole backdrop (sky, disc, stars, grass) is
 *     painted ONCE into a cached surface and every redraw is a single
 *     `drawImage` of it (`blitSky`). The surface is repainted only when the
 *     time of day changes — at most four times a day — and the clock itself is
 *     read at most every `SKY_CHECK_MS`, never per frame.
 *  2. Readable at the 22-px nub. The room BLENDS between its keyframes; the
 *     pill does not. Measured: a continuous blend passes through late-dusk
 *     colours where neither Yappy's outline nor his body clears 2.6:1 against
 *     the sky behind his face. The four keyframes all clear 5:1, so the pill
 *     SNAPS to the nearest one (`skyTimeAt`).
 *  3. Nothing drifts. The old sky drifted two clouds on the ambient tick; this
 *     one is static between time-of-day changes, so reduced motion (one calm
 *     frame) and a meeting park (OS-12) show exactly what a running pill shows,
 *     and the idle redraw schedule (`framePlan`) is untouched.
 */
import { skyPhase } from "../home/habitat/habitat";
import { H, type PixelCtx } from "../sprites/art";

export const SKY_TIMES = ["dawn", "day", "dusk", "night"] as const;
export type SkyTime = (typeof SKY_TIMES)[number];

/**
 * The Home room's keyframes, top and bottom of its window, copied from
 * home/YappyHouse.tsx `SKY` (dawn .0, day .25, dusk .5, night .75 of
 * `skyPhase`). "Final colors come from the room's sky code" (section 9).
 */
export const ROOM_SKY: Record<SkyTime, { at: number; top: string; bot: string }> = {
  dawn: { at: 0.0, top: "#f6a6a0", bot: "#ffd9a0" },
  day: { at: 0.25, top: "#7ec8ff", bot: "#cdeeff" },
  dusk: { at: 0.5, top: "#ff9d6b", bot: "#ffd27a" },
  night: { at: 0.75, top: "#171a3d", bot: "#2c3060" },
};

/** Six bands, deep to pale — pixel steps, never a smooth blend. */
export const SKY_BANDS = 6;
/** The clock is re-read at most this often (ms of the redraw clock). */
export const SKY_CHECK_MS = 30_000;

/** The pill's scene: the art buffer YappyPill draws into (PW x PH, grass at GY). */
export const SKY_SCENE = { w: 176, h: 44, gy: 30 } as const;

/** Sun / moon per time of day: where it sits in the scene and its colour. */
const DISC: Record<SkyTime, { x: number; y: number; col: string; moon: boolean }> = {
  dawn: { x: 30, y: 22, col: "#ffcf7a", moon: false },
  day: { x: 40, y: 8, col: "#ffd23f", moon: false },
  dusk: { x: 140, y: 20, col: "#ff8a3c", moon: false },
  night: { x: 130, y: 8, col: "#f6f0cf", moon: true },
};
/** Night only: fixed stars, kept clear of Yappy's face (x 75..101 at the nub). */
const STARS: ReadonlyArray<readonly [number, number]> = [[12, 4], [30, 5], [52, 12], [60, 3], [112, 6], [150, 4], [164, 14]];
const STAR_BRIGHT = "#fffbe0";
const STAR_DIM = "#8a8fb8";

export interface SkyLook {
  time: SkyTime;
  /** Top and bottom of the room's window: the capsule's own fill uses these. */
  top: string;
  bot: string;
  /** `SKY_BANDS` hex colours, top to bottom. */
  bands: string[];
  disc: { x: number; y: number; col: string; moon: boolean };
  stars: Array<{ x: number; y: number; col: string }>;
  grass: string;
  grassD: string;
}

const rgb = (h: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)) as [number, number, number];
const hex2 = (v: number) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");

/** `#rrggbb` a→b at k (0..1), as `#rrggbb` (the pixel raster takes hex only). */
export function mixHex(a: string, b: string, k: number): string {
  const A = rgb(a), B = rgb(b);
  return "#" + A.map((v, i) => hex2(v + (B[i] - v) * k)).join("");
}

/** The room's clock, snapped to its nearest keyframe (rule 2 above). */
export function skyTimeFromPhase(p: number): SkyTime {
  const q = ((p % 1) + 1) % 1;
  return SKY_TIMES[Math.round(q * 4) % 4];
}

/** Real local time -> the pill's time of day: 6am dawn, noon day, 6pm dusk, midnight night. */
export function skyTimeAt(d: Date): SkyTime {
  return skyTimeFromPhase(skyPhase(d));
}

/** Everything the backdrop paints for one time of day. Pure. */
export function skyLook(time: SkyTime): SkyLook {
  const k = ROOM_SKY[time];
  const bands = Array.from({ length: SKY_BANDS }, (_, i) => mixHex(k.top, k.bot, i / (SKY_BANDS - 1)));
  const night = time === "night";
  return {
    time,
    top: k.top,
    bot: k.bot,
    bands,
    disc: { ...DISC[time] },
    stars: night ? STARS.map(([x, y], i) => ({ x, y, col: i % 3 === 2 ? STAR_DIM : STAR_BRIGHT })) : [],
    grass: night ? "#4f7f45" : time === "dusk" ? "#7aa65a" : "#8fd06a",
    grassD: night ? "#3e6a37" : "#6fb84e",
  };
}

/** The ONE place a dev/matrix build may pin the sky (`float.html?sky=night`). */
export function skyForced(raw: string | null | undefined): SkyTime | null {
  return (SKY_TIMES as readonly string[]).includes(raw ?? "") ? (raw as SkyTime) : null;
}

/** A surface that can be cleared and painted: a canvas 2D context or the test raster. */
export interface SkyPaintCtx extends PixelCtx {
  clearRect(x: number, y: number, w: number, h: number): void;
}

/**
 * Paint the whole backdrop for `look` into `o` (w x h, grass from `gy`). Runs
 * only when the cached surface is (re)built, never per redraw.
 */
export function paintSky(o: SkyPaintCtx, look: SkyLook, w: number, h: number, gy: number): void {
  const hp = H(o);
  o.globalAlpha = 1;
  o.clearRect(0, 0, w, h);
  const bh = Math.ceil(gy / SKY_BANDS);
  for (let i = 0; i < SKY_BANDS; i++) {
    const y = i * bh;
    if (y >= gy) break;
    hp.r(0, y, w, Math.min(bh, gy - y), look.bands[i]);
  }
  for (const s of look.stars) hp.r(s.x, s.y, 1, 1, s.col);
  const d = look.disc;
  hp.ell(d.x, d.y, 5, 5, d.col);
  if (d.moon) {
    // the crescent: bite the disc with the band colour behind it
    const behind = look.bands[Math.max(0, Math.min(SKY_BANDS - 1, Math.floor((d.y - 1) / bh)))];
    hp.ell(d.x + 2, d.y - 1, 4, 4, behind);
  }
  hp.r(0, gy, w, h - gy, look.grass);
  hp.r(0, gy, w, 2, look.grassD);
  for (let x = 3; x < w; x += 9) hp.r(x, gy - 2, 1, 2, look.grassD);
  o.globalAlpha = 1;
}

/** The cached surface: what gets blitted (`image`) and how to paint it (`ctx`). */
export interface SkySurface<S> {
  image: S;
  ctx: SkyPaintCtx;
}

/**
 * The pill's backdrop, cached. `current(nowMs)` returns the surface to blit;
 * it reads the clock at most once per `SKY_CHECK_MS` and repaints the surface
 * only when the time of day actually changed (`builds` counts repaints, for
 * the budget test). A forced time (matrix/dev) never re-reads the clock.
 */
export class SkyBackdrop<S> {
  builds = 0;
  private time: SkyTime | null = null;
  private look_: SkyLook | null = null;
  private nextCheck = -Infinity;

  constructor(
    private readonly surface: SkySurface<S>,
    readonly w: number = SKY_SCENE.w,
    readonly h: number = SKY_SCENE.h,
    readonly gy: number = SKY_SCENE.gy,
    private readonly clock: () => Date = () => new Date(),
    private readonly forced: SkyTime | null = null,
    private readonly onChange: (look: SkyLook, builds: number) => void = () => {},
  ) {}

  current(nowMs: number): S {
    if (nowMs >= this.nextCheck) {
      this.nextCheck = nowMs + SKY_CHECK_MS;
      const time = this.forced ?? skyTimeAt(this.clock());
      if (time !== this.time) {
        this.time = time;
        this.look_ = skyLook(time);
        paintSky(this.surface.ctx, this.look_, this.w, this.h, this.gy);
        this.builds++;
        this.onChange(this.look_, this.builds);
      }
    }
    return this.surface.image;
  }

  /** The look last painted, or null before the first `current`. */
  get look(): SkyLook | null {
    return this.look_;
  }
}

/** The 9-argument drawImage both a canvas context and the test raster have. */
export interface SkyBlitCtx<S> {
  globalAlpha: number;
  drawImage(img: S, sx: number, sy: number, sw: number, sh: number, dx: number, dy: number, dw: number, dh: number): void;
}

/**
 * THE per-redraw cost of the sky: one `drawImage` of the cached backdrop at
 * `alpha` (the world's fade), and nothing else.
 */
export function blitSky<S>(o: SkyBlitCtx<S>, bg: SkyBackdrop<S>, nowMs: number, alpha: number): void {
  const img = bg.current(nowMs);
  o.globalAlpha = alpha;
  o.drawImage(img, 0, 0, bg.w, bg.h, 0, 0, bg.w, bg.h);
  o.globalAlpha = 1;
}
