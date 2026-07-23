/**
 * foldPlayer — dependency-free Canvas2D playback of a baked origami fold.
 *
 * WHY this design (see docs/research/origami-yapping-pill.md):
 * The pill must unfold like real origami when you dictate and fold back "the
 * exact same way". A live rigid-fold physics sim gets the motion right but the
 * runtime wrong (WebGL + battery/thermal on an always-on-top panel). SVG-morph
 * libs (flubber, Lottie) tween silhouettes, not creases — not faithful folds.
 *
 * So we BAKE ONCE, PLAY BACK CHEAP: offline, Amanda Ghassaei's Origami Simulator
 * (MIT) sweeps a crane crease pattern 0→1 and dumps per-frame vertex positions
 * to a small JSON (a `FoldModel`). At runtime we interpolate ONE eased scalar
 * `progress` across those frames on a 2D canvas. Unfold is progress 0→1, fold is
 * 1→0 — the reverse is bit-exact BY CONSTRUCTION (same frames, reverse order),
 * which is the only way to truly "fold back the exact same way". rAF runs only
 * while animating, so idle CPU ≈ 0.
 *
 * No deps, no WebGL, no network. A `FoldModel` is plain data (FOLD-format-derived).
 */

/** One baked frame: 2D-projected vertex coords, normalized to the unit square. */
export interface FoldFrame {
  /** [x, y] per vertex, x,y in [0,1]. Topology (faces/edges) is shared, below. */
  vertices: Array<[number, number]>;
}

/** A baked fold: ordered frames (index 0 = fully folded, last = fully open). */
export interface FoldModel {
  name: string;
  /** Ordered folded→open. Reverse-play them to fold back, bit-exact. */
  frames: FoldFrame[];
  /** Vertex-index polygons, shared across every frame (the paper's facets). */
  faces: number[][];
  /** Optional crease/boundary segments (vertex-index pairs) drawn as fold lines. */
  edges?: Array<[number, number]>;
}

export interface FoldStyle {
  fill: string;
  stroke: string;
  lineWidth: number;
  /** Uniform inset so the mascot never touches the canvas edge (px). */
  pad: number;
}

const DEFAULT_STYLE: FoldStyle = {
  fill: "rgba(245,245,247,0.92)",
  stroke: "rgba(255,255,255,0.35)",
  lineWidth: 0.75,
  pad: 6,
};

/** Symmetric ease — matches the pill's cubic-bezier(0.22,1,0.36,1) feel. */
export function easeInOut(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

const clamp01 = (v: number): number => (v < 0 ? 0 : v > 1 ? 1 : v);

export class FoldPlayer {
  private model: FoldModel;
  private style: FoldStyle;
  /** Raw playback progress 0..1 (0 = folded, 1 = open). */
  private progress = 0;
  private target = 0;
  private raf = 0;
  private lastTs = 0;
  /** Seconds for a full fold or unfold. */
  private durationMs = 340;
  private onIdle?: () => void;

  constructor(model: FoldModel, style: Partial<FoldStyle> = {}) {
    this.model = model;
    this.style = { ...DEFAULT_STYLE, ...style };
  }

  setModel(model: FoldModel): void {
    this.model = model;
  }

  /** Interpolate the vertex set at eased progress p (0..1). */
  private frameAt(p: number): Array<[number, number]> {
    const frames = this.model.frames;
    if (frames.length === 0) return [];
    if (frames.length === 1) return frames[0].vertices;
    const e = easeInOut(clamp01(p));
    // Map eased progress onto the frame index space, then lerp the bracket.
    const x = e * (frames.length - 1);
    const i = Math.min(Math.floor(x), frames.length - 2);
    const f = x - i;
    const a = frames[i].vertices;
    const b = frames[i + 1].vertices;
    const out: Array<[number, number]> = new Array(a.length);
    for (let v = 0; v < a.length; v++) {
      out[v] = [a[v][0] + (b[v][0] - a[v][0]) * f, a[v][1] + (b[v][1] - a[v][1]) * f];
    }
    return out;
  }

  /** Draw the current progress into a canvas context sized w×h (device px). */
  render(ctx: CanvasRenderingContext2D, w: number, h: number): void {
    const verts = this.frameAt(this.progress);
    ctx.clearRect(0, 0, w, h);
    if (verts.length === 0) return;
    const { pad, fill, stroke, lineWidth } = this.style;
    const sx = (x: number) => pad + x * (w - 2 * pad);
    const sy = (y: number) => pad + y * (h - 2 * pad);

    ctx.fillStyle = fill;
    for (const face of this.model.faces) {
      if (face.length < 3) continue;
      ctx.beginPath();
      const [fx, fy] = verts[face[0]];
      ctx.moveTo(sx(fx), sy(fy));
      for (let k = 1; k < face.length; k++) {
        const [vx, vy] = verts[face[k]];
        ctx.lineTo(sx(vx), sy(vy));
      }
      ctx.closePath();
      ctx.fill();
    }
    if (this.model.edges && lineWidth > 0) {
      ctx.strokeStyle = stroke;
      ctx.lineWidth = lineWidth;
      ctx.beginPath();
      for (const [i, j] of this.model.edges) {
        ctx.moveTo(sx(verts[i][0]), sy(verts[i][1]));
        ctx.lineTo(sx(verts[j][0]), sy(verts[j][1]));
      }
      ctx.stroke();
    }
  }

  /** Animate toward open (1) or folded (0). Reverse traversal = exact refold. */
  animateTo(
    dir: "open" | "fold",
    draw: () => void,
    onDone?: () => void,
  ): void {
    this.target = dir === "open" ? 1 : 0;
    this.onIdle = onDone;
    if (this.raf) return; // already animating toward the latest target
    this.lastTs = 0;
    const step = (ts: number) => {
      if (this.lastTs === 0) this.lastTs = ts;
      const dt = ts - this.lastTs;
      this.lastTs = ts;
      const delta = dt / this.durationMs;
      if (this.progress < this.target) {
        this.progress = Math.min(this.target, this.progress + delta);
      } else {
        this.progress = Math.max(this.target, this.progress - delta);
      }
      draw();
      if (this.progress !== this.target) {
        this.raf = requestAnimationFrame(step);
      } else {
        this.raf = 0;
        this.onIdle?.();
      }
    };
    this.raf = requestAnimationFrame(step);
  }

  /** Jump instantly (no animation) — e.g. restoring state. */
  set(p: number): void {
    this.progress = clamp01(p);
  }

  get value(): number {
    return this.progress;
  }

  stop(): void {
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
  }
}

/**
 * A tiny hand-authored placeholder fold so the player runs BEFORE the real crane
 * bake exists. A square "flap" whose top edge folds down to a flat strip
 * (folded) and opens to a full square (open). Replace with baked `crane.json`.
 */
export function demoFlapModel(): FoldModel {
  const foldedTopY = 0.5; // top edge collapsed onto the mid-line
  return {
    name: "demo-flap",
    faces: [[0, 1, 2, 3]],
    edges: [
      [0, 1],
      [1, 2],
      [2, 3],
      [3, 0],
    ],
    frames: [
      // folded: a flat strip (top edge pulled down to y=0.5)
      { vertices: [[0.15, foldedTopY], [0.85, foldedTopY], [0.85, 0.85], [0.15, 0.85]] },
      // mid
      { vertices: [[0.15, 0.32], [0.85, 0.32], [0.85, 0.85], [0.15, 0.85]] },
      // open: full square
      { vertices: [[0.15, 0.15], [0.85, 0.15], [0.85, 0.85], [0.15, 0.85]] },
    ],
  };
}
