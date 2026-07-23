/**
 * fold — the origami companion ("Kami") geometry + renderer.
 *
 * A hinge-tree fold over a real crease pattern: each facet rotates about its
 * crease by a dihedral angle scaled by one scalar `t` (0 = folded/peek, 1 =
 * open). Because the whole pose is that single scalar, folding back is the exact
 * reverse. Rendered to Canvas2D — painter-sorted, shaded like paper, creases
 * drawn (mountain solid / valley dashed). No WebGL (battery/thermal on an
 * always-on panel). This is the runtime the prototype proved; the shipping path
 * swaps `PETS` verts for frames baked from Origami Simulator when ready.
 */

export interface Facet {
  poly: number[];
  parent: number;
  crease: [number, number] | null;
  dih: number;
}
export interface Pet {
  tint: [number, number, number];
  edge: [number, number, number];
  /** facet indices whose dihedral is modulated by the mouth-open amount */
  mouth: number[];
  verts: Array<[number, number]>;
  facets: Facet[];
}

const V = (x: number, y: number): [number, number] => [x, y];

export const PETS: Record<string, Pet> = {
  crane: {
    tint: [247, 241, 230], edge: [205, 191, 159], mouth: [5, 6],
    verts: [V(-0.15,0.16),V(0.9,0.11),V(0.9,-0.11),V(-0.15,-0.16),V(0.42,1.18),V(0.42,-1.18),V(-1.05,0.30),V(1.62,0.34),V(-1.02,0.14),V(-1.02,0.46),V(-1.62,0.40),V(-1.62,0.20)],
    facets: [
      {poly:[0,1,2,3],parent:-1,crease:null,dih:0},
      {poly:[0,1,4],parent:0,crease:[0,1],dih:1.15},
      {poly:[3,2,5],parent:0,crease:[3,2],dih:-1.15},
      {poly:[0,3,6],parent:0,crease:[0,3],dih:0.95},
      {poly:[1,2,7],parent:0,crease:[1,2],dih:0.8},
      {poly:[8,9,10],parent:3,crease:[8,9],dih:0.55},
      {poly:[8,9,11],parent:3,crease:[8,9],dih:-0.55},
    ],
  },
  fox: {
    tint: [240, 150, 70], edge: [180, 95, 40], mouth: [5, 6],
    verts: [V(-0.2,0.2),V(0.85,0.14),V(0.85,-0.14),V(-0.2,-0.2),V(0.55,0.95),V(0.55,-0.95),V(-1.1,0),V(0.95,0),V(-1.0,0.12),V(-1.0,-0.12),V(-1.55,0.22),V(-1.55,-0.02)],
    facets: [
      {poly:[0,1,2,3],parent:-1,crease:null,dih:0},
      {poly:[0,1,4],parent:0,crease:[0,1],dih:1.35},
      {poly:[3,2,5],parent:0,crease:[3,2],dih:-1.35},
      {poly:[0,3,6],parent:0,crease:[0,3],dih:0.7},
      {poly:[1,2,7],parent:0,crease:[1,2],dih:0.5},
      {poly:[8,9,10],parent:3,crease:[8,9],dih:0.5},
      {poly:[8,9,11],parent:3,crease:[8,9],dih:-0.5},
    ],
  },
  frog: {
    tint: [110, 200, 130], edge: [60, 150, 90], mouth: [4, 5],
    verts: [V(-0.3,0.35),V(0.7,0.25),V(0.7,-0.25),V(-0.3,-0.35),V(0.2,1.0),V(0.2,-1.0),V(-1.0,0.28),V(-1.0,-0.28),V(-0.55,0.1),V(-0.55,-0.1),V(-1.25,0.16),V(-1.25,-0.16)],
    facets: [
      {poly:[0,1,2,3],parent:-1,crease:null,dih:0},
      {poly:[0,1,4],parent:0,crease:[0,1],dih:1.5},
      {poly:[3,2,5],parent:0,crease:[3,2],dih:-1.5},
      {poly:[0,3,6,7],parent:0,crease:[0,3],dih:0.65},
      {poly:[8,9,10],parent:0,crease:[8,9],dih:0.6},
      {poly:[8,9,11],parent:0,crease:[8,9],dih:-0.6},
    ],
  },
};

type V3 = [number, number, number];
const sub = (a: V3, b: V3): V3 => [a[0]-b[0], a[1]-b[1], a[2]-b[2]];
const add = (a: V3, b: V3): V3 => [a[0]+b[0], a[1]+b[1], a[2]+b[2]];
const cross = (a: V3, b: V3): V3 => [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]];
const dot = (a: V3, b: V3): number => a[0]*b[0]+a[1]*b[1]+a[2]*b[2];
const norm = (a: V3): V3 => { const l = Math.hypot(a[0],a[1],a[2]) || 1; return [a[0]/l,a[1]/l,a[2]/l]; };
function rotAbout(p: V3, A: V3, k: V3, th: number): V3 {
  const v = sub(p, A), c = Math.cos(th), s = Math.sin(th), kv = cross(k, v), kk = dot(k, v);
  return add([v[0]*c+kv[0]*s+k[0]*kk*(1-c), v[1]*c+kv[1]*s+k[1]*kk*(1-c), v[2]*c+kv[2]*s+k[2]*kk*(1-c)], A);
}

interface Folded { polys: Array<{ verts: V3[] }>; creases: Array<{ a: V3; b: V3; valley: boolean }>; }
export function foldPet(pet: Pet, t: number, mouthOpen: number): Folded {
  const flat: V3[] = pet.verts.map((v) => [v[0], v[1], 0]);
  const xf: Array<(p: V3) => V3> = new Array(pet.facets.length);
  xf[0] = (p) => p;
  for (let i = 1; i < pet.facets.length; i++) {
    const f = pet.facets[i], par = xf[f.parent];
    const cr = f.crease as [number, number];
    const A = par(flat[cr[0]]), B = par(flat[cr[1]]), k = norm(sub(B, A));
    let dih = f.dih * t;
    if (pet.mouth.includes(i)) dih = f.dih * t + Math.sign(f.dih) * mouthOpen * 0.5 * t;
    xf[i] = (p) => rotAbout(par(p), A, k, dih);
  }
  const polys = pet.facets.map((f, i) => ({ verts: f.poly.map((vi) => xf[i](flat[vi])) }));
  const creases: Folded["creases"] = [];
  for (let i = 1; i < pet.facets.length; i++) {
    const f = pet.facets[i], cr = f.crease as [number, number];
    creases.push({ a: xf[f.parent](flat[cr[0]]), b: xf[f.parent](flat[cr[1]]), valley: f.dih < 0 });
  }
  return { polys, creases };
}

export interface RenderOpts {
  tilt?: number;
  /** hsl triplets (e.g. "351 80% 62%") for mountain / valley crease lines */
  mountain?: string;
  valley?: string;
  scale?: number; // fraction of min(W,H)
}
/** Draw the folded pet centered in the ctx (already sized W×H, DPR-scaled). */
export function renderPet(
  ctx: CanvasRenderingContext2D,
  W: number, H: number, pet: Pet, t: number, mouthOpen: number, o: RenderOpts = {},
): void {
  const tilt = o.tilt ?? 0.6;
  const light = norm([0.3, 0.5, 0.8]);
  const { polys, creases } = foldPet(pet, Math.max(0, t), mouthOpen);
  const scale = Math.min(W, H) * (o.scale ?? 0.30);
  const cx = W / 2, cyc = H * 0.46 + (1 - Math.max(0, t)) * 18; // tuck up when folded
  const cyT = Math.cos(tilt), syT = Math.sin(tilt);
  const toS = (p: V3): [number, number, number] => {
    const y = p[1] * cyT - p[2] * syT, depth = p[1] * syT + p[2] * cyT;
    return [cx + p[0] * scale, cyc - y * scale, depth];
  };
  const items = polys.map((pl) => {
    const s = pl.verts.map(toS);
    const d = s.reduce((a, p) => a + p[2], 0) / s.length;
    return { s, d, world: pl.verts };
  });
  items.sort((a, b) => a.d - b.d);
  for (const it of items) {
    const [A, B, C] = it.world;
    const n = norm(cross(sub(B, A), sub(C, A)));
    const bright = 0.55 + 0.45 * Math.abs(dot(n, light));
    const [r, g, b] = pet.tint, alpha = 0.14 + 0.8 * Math.max(0, t);
    ctx.save();
    ctx.shadowColor = `rgba(0,0,0,${0.25 * t})`; ctx.shadowBlur = 10; ctx.shadowOffsetY = 5;
    ctx.beginPath();
    it.s.forEach((p, i) => (i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1])));
    ctx.closePath();
    ctx.fillStyle = `rgba(${Math.round(r*bright)},${Math.round(g*bright)},${Math.round(b*bright)},${alpha})`;
    ctx.fill();
    ctx.restore();
    ctx.lineWidth = 0.6; ctx.strokeStyle = `rgba(${pet.edge.join(",")},${0.5 * t})`; ctx.stroke();
  }
  const ca = 0.22 + 0.5 * (1 - t) + 0.1 * t;
  const mtn = o.mountain ?? "351 80% 62%", vly = o.valley ?? "210 78% 60%";
  for (const cr of creases) {
    const a = toS(cr.a), b = toS(cr.b);
    ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]);
    ctx.lineWidth = 1.1; ctx.setLineDash(cr.valley ? [4, 3] : []);
    ctx.strokeStyle = `hsla(${cr.valley ? vly : mtn}, ${ca})`;
    ctx.stroke();
  }
  ctx.setLineDash([]);
}
