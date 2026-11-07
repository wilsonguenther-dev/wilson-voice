#!/usr/bin/env node
/**
 * yap-1107-parity-kit — the ARTIFACT half of the parity kit.
 *
 * Wilson approved design pages on 2026-11-06 (~/Obsidian/Wilson-Brain/Projects/
 * Yap/design-2026-11-06/*.html). "Visually similar" is not acceptance: the app
 * must match them EXACTLY. This script opens one of those static pages (its
 * assets/*.js are classic scripts, so file:// loads them) in the SAME headless
 * Chrome every Yap instrument launches (scripts/headless-chrome.mjs,
 * --headless=new: never a visible window), at deviceScaleFactor 2, freezes its
 * animation at a fixed frame, and for each named selector records:
 *
 *   box       getBoundingClientRect in page CSS px (x, y, w, h)
 *   fontPx    every computed font-size carrying text inside it (sorted, unique)
 *   padding   the container's computed padding (t r b l, px) and its gap
 *   colors    every colour it paints (text colour, opaque backgrounds, drawn
 *             borders, svg fill/stroke) as exact sRGB hex, normalised by
 *             painting it on a 1x1 canvas — so oklch()/var() tokens compare too
 *   sprites   every canvas / img / top-level svg in it: its box, its scale
 *             (CSS px per intrinsic px), whether it is inside the container and
 *             not cut by any overflow-clipping ancestor, and (canvas/img) the
 *             dominant colours of its SAMPLED PIXELS
 *
 * and (with --out) a 2x PNG crop of that box. scripts/parity-sheet.mjs imports
 * the in-page measurer below and runs it on the APP's DOM, so both sides of a
 * parity row are measured by ONE function — a second copy could disagree.
 *
 * FREEZING. A virtual clock is injected before any page script: performance.now
 * and Date.now return it, requestAnimationFrame only queues. After load the
 * clock is stepped (16 ms frames) to --frame ms, every CSS animation is paused
 * at that time, and nothing advances again — so a crop never depends on when
 * the shutter fired.
 *
 * USAGE
 *   node scripts/parity-crops.mjs --page pill.html --selector "#rows" [--selector ...]
 *        [--out <scratch dir>] [--frame 1200] [--width 1280] [--height 900]
 *        [--design-dir <dir>]
 *   Prints ONE JSON document on stdout. With --out it also writes <slug>.png
 *   per selector and crops.json there. --out is PR evidence, never a commit.
 *
 * EXIT  0 every selector found · 1 a selector matched nothing · 2 usage ·
 *       3 environment (no Chrome, no page).
 * ENV   YAP_SMOKE_CHROME (Chromium path, shared with the smoke).
 * NO npm DEPENDENCY, no Playwright.
 */

import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { evaluate, openChrome, sleep } from "./headless-chrome.mjs";

export const DESIGN_DIR = join(homedir(), "Obsidian", "Wilson-Brain", "Projects", "Yap", "design-2026-11-06");
export const CROP_SCALE = 2;
export const DEFAULT_FRAME_MS = 1200;
export const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

/* ── the virtual clock (injected before any page script) ───────────────── */
export const FREEZE_SCRIPT = String.raw`
(() => {
  let now = 0;
  const epoch = 1794000000000;
  const queue = new Map();
  let id = 0;
  try { Object.defineProperty(performance, "now", { value: () => now, configurable: true }); } catch {}
  Date.now = () => epoch + now;
  window.requestAnimationFrame = (cb) => { const i = ++id; queue.set(i, cb); return i; };
  window.cancelAnimationFrame = (i) => { queue.delete(i); };
  window.__parityAdvance = (to) => {
    let frames = 0;
    while (now < to && frames < 10000) {
      now = Math.min(to, now + 16);
      const cbs = [...queue.values()];
      queue.clear();
      for (const cb of cbs) { try { cb(now); } catch (e) {} }
      frames++;
    }
    for (const a of document.getAnimations()) { try { a.pause(); a.currentTime = to; } catch (e) {} }
    return { now, frames };
  };
})();
`;

/* ── the ONE measurer, serialized into the page (artifact AND app side) ── */
export const MEASURE = String.raw`
function __parityMeasure(sel, opts) {
  opts = opts || {};
  const minShare = opts.minShare || 0.02;
  const el = document.querySelector(sel);
  if (!el) return { selector: sel, found: false };
  const px = (v) => Math.round(parseFloat(v || "0") * 100) / 100;
  const dot = document.createElement("canvas"); dot.width = 1; dot.height = 1;
  const dctx = dot.getContext("2d", { willReadFrequently: true });
  const hex = (css) => {
    if (!css || css === "none" || css === "transparent") return null;
    dctx.clearRect(0, 0, 1, 1); dctx.fillStyle = "#000"; dctx.fillStyle = css; dctx.fillRect(0, 0, 1, 1);
    const [r, g, b, a] = dctx.getImageData(0, 0, 1, 1).data;
    if (a === 0) return null;
    const h = (n) => n.toString(16).padStart(2, "0");
    return "#" + h(r) + h(g) + h(b) + (a === 255 ? "" : h(a));
  };
  const shown = (e) => {
    const r = e.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    const cs = getComputedStyle(e);
    return cs.visibility !== "hidden" && cs.display !== "none" && parseFloat(cs.opacity || "1") > 0;
  };
  const box = (r) => ({ x: px(r.left + scrollX), y: px(r.top + scrollY), w: px(r.width), h: px(r.height) });
  const within = (a, b, tol) => a.left >= b.left - tol && a.top >= b.top - tol && a.right <= b.right + tol && a.bottom <= b.bottom + tol;
  const R = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  const fonts = new Set(), colors = new Set();
  const all = [el, ...el.querySelectorAll("*")];
  for (const e of all) {
    if (!shown(e)) continue;
    const s = getComputedStyle(e);
    const ownText = [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (ownText) { fonts.add(px(s.fontSize)); const c = hex(s.color); if (c) colors.add(c); }
    const bg = hex(s.backgroundColor); if (bg) colors.add(bg);
    for (const side of ["Top", "Right", "Bottom", "Left"]) {
      if (parseFloat(s["border" + side + "Width"]) > 0 && s["border" + side + "Style"] !== "none") {
        const c = hex(s["border" + side + "Color"]); if (c) colors.add(c);
      }
    }
    if (e instanceof SVGElement && e.tagName.toLowerCase() !== "svg") {
      const f = hex(s.fill); if (f) colors.add(f);
      if (s.stroke && s.stroke !== "none" && parseFloat(s.strokeWidth) > 0) { const k = hex(s.stroke); if (k) colors.add(k); }
    }
  }
  const sample = (e) => {
    try {
      const w = e.width || e.naturalWidth, h = e.height || e.naturalHeight;
      if (!w || !h) return { sampled: false, why: "no intrinsic size" };
      const c = document.createElement("canvas"); c.width = w; c.height = h;
      const x = c.getContext("2d", { willReadFrequently: true });
      x.drawImage(e, 0, 0);
      const d = x.getImageData(0, 0, w, h).data;
      const count = new Map(); let opaque = 0;
      for (let i = 0; i < d.length; i += 4) {
        if (d[i + 3] < 250) continue;
        opaque++;
        const k = "#" + [d[i], d[i + 1], d[i + 2]].map((n) => n.toString(16).padStart(2, "0")).join("");
        count.set(k, (count.get(k) || 0) + 1);
      }
      const palette = [...count.entries()].filter(([, n]) => opaque && n / opaque >= minShare)
        .sort((a, b) => b[1] - a[1]).slice(0, 16).map(([k, n]) => ({ hex: k, share: Math.round((n / opaque) * 1000) / 1000 }));
      return { sampled: true, opaquePx: opaque, palette };
    } catch (err) {
      return { sampled: false, why: String(err && err.message || err).slice(0, 120) };
    }
  };
  const spriteEls = new Set([...el.querySelectorAll("canvas, img, svg")].filter((e) => !(e.parentElement && e.parentElement.closest("svg"))));
  for (const extra of opts.sprites || []) for (const e of el.querySelectorAll(extra)) spriteEls.add(e);
  const sprites = [];
  for (const e of spriteEls) {
    if (!shown(e)) continue;
    const r = e.getBoundingClientRect();
    const tag = e.tagName.toLowerCase();
    const iw = tag === "canvas" ? e.width : tag === "img" ? e.naturalWidth : 0;
    const inside = within(r, R, 0.5);
    let clippedBy = null;
    for (let a = e.parentElement; a; a = a.parentElement) {
      const as = getComputedStyle(a);
      if (as.overflowX === "visible" && as.overflowY === "visible") continue;
      if (!within(r, a.getBoundingClientRect(), 0.5)) { clippedBy = a.tagName.toLowerCase() + (a.id ? "#" + a.id : "") + (a.classList[0] ? "." + a.classList[0] : ""); break; }
    }
    const rec = { tag, cls: e.getAttribute("class") || "", box: box(r), intrinsicW: iw || null, scale: iw ? Math.round((r.width / iw) * 1000) / 1000 : null, inside, clippedBy };
    if (tag === "canvas" || tag === "img") rec.pixels = sample(e);
    sprites.push(rec);
  }
  return {
    selector: sel, found: true, box: box(R),
    viewport: { w: innerWidth, h: innerHeight, dpr: devicePixelRatio },
    fontPxRoot: px(cs.fontSize),
    fontPx: [...fonts].sort((a, b) => a - b),
    padding: { t: px(cs.paddingTop), r: px(cs.paddingRight), b: px(cs.paddingBottom), l: px(cs.paddingLeft) },
    gap: { row: px(cs.rowGap), col: px(cs.columnGap) },
    colors: [...colors].sort(),
    sprites,
  };
}
`;

export async function measureSelector(cdp, s, selector, opts = {}) {
  await evaluate(cdp, s, MEASURE + ";true");
  return evaluate(cdp, s, "__parityMeasure(" + JSON.stringify(selector) + "," + JSON.stringify(opts) + ")");
}

/* A PNG of one page-coordinate box at the page's deviceScaleFactor. */
export async function shootBox(cdp, s, b) {
  const shot = await cdp.send(
    "Page.captureScreenshot",
    { format: "png", captureBeyondViewport: true, clip: { x: b.x, y: b.y, width: Math.max(1, b.w), height: Math.max(1, b.h), scale: 1 } },
    s
  );
  return Buffer.from(shot.data, "base64");
}

export const slug = (sel) => sel.replace(/[^a-z0-9]+/gi, "-").replace(/^-|-$/g, "").slice(0, 60) || "root";

export function resolvePage(page, designDir = DESIGN_DIR) {
  return isAbsolute(page) ? page : join(designDir, page);
}

/*
 * Opens `page` once and measures + crops every selector. `cdp` is an open
 * headless-chrome session; the caller owns the browser. Returns the record
 * parity-sheet.mjs reads (pngs as Buffers under `png`).
 */
export async function cropDesign(cdp, { page, selectors, frameMs = DEFAULT_FRAME_MS, width = 1280, height = 900, sprites = [], designDir = DESIGN_DIR }) {
  const file = resolvePage(page, designDir);
  if (!existsSync(file)) throw Object.assign(new Error("design page not found: " + file), { env: true });
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  try {
    await cdp.send("Page.enable", {}, s);
    await cdp.send("Runtime.enable", {}, s);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: CROP_SCALE, mobile: false }, s);
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: FREEZE_SCRIPT }, s);
    await cdp.send("Page.navigate", { url: pathToFileURL(file).href }, s);
    for (let i = 0; i < 100; i++) {
      if ((await evaluate(cdp, s, "document.readyState").catch(() => "")) === "complete") break;
      await sleep(100);
    }
    await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
    const clock = await evaluate(cdp, s, "window.__parityAdvance ? window.__parityAdvance(" + Number(frameMs) + ") : null");
    await sleep(250); // one real paint of the frozen frame
    const crops = [];
    for (const selector of selectors) {
      const m = await measureSelector(cdp, s, selector, { sprites });
      if (m.found) {
        const png = await shootBox(cdp, s, m.box);
        crops.push({ ...m, png });
      } else crops.push(m);
    }
    return { page: file, scale: CROP_SCALE, frameMs: Number(frameMs), clock, viewport: { w: width, h: height }, pageErrors: (cdp.errors.get(s) || []).slice(0, 6), crops };
  } finally {
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  }
}

/* ── CLI ─────────────────────────────────────────────────────────────────── */

function parseArgs(argv) {
  const a = { page: null, selectors: [], out: null, frame: DEFAULT_FRAME_MS, width: 1280, height: 900, designDir: DESIGN_DIR };
  const need = (k, v) => { if (v === undefined || v.startsWith("--")) usage(k + " needs a value"); return v; };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--page") a.page = need(k, argv[++i]);
    else if (k === "--selector") a.selectors.push(need(k, argv[++i]));
    else if (k === "--out") a.out = resolve(need(k, argv[++i]));
    else if (k === "--frame") a.frame = Number(need(k, argv[++i]));
    else if (k === "--width") a.width = Number(need(k, argv[++i]));
    else if (k === "--height") a.height = Number(need(k, argv[++i]));
    else if (k === "--design-dir") a.designDir = resolve(need(k, argv[++i]));
    else usage("unknown argument " + k);
  }
  if (!a.page) usage("--page is required");
  if (!a.selectors.length) usage("at least one --selector is required");
  for (const n of [a.frame, a.width, a.height]) if (!Number.isFinite(n) || n < 0) usage("--frame/--width/--height must be numbers");
  return a;
}
function usage(msg) {
  console.error("parity-crops: " + msg + "\nusage: node scripts/parity-crops.mjs --page <design page> --selector <css> [--selector ...] [--out <dir>] [--frame <ms>] [--width <px>] [--height <px>] [--design-dir <dir>]");
  process.exit(2);
}

async function main() {
  const A = parseArgs(process.argv.slice(2));
  const t0 = Date.now();
  let chrome;
  try {
    chrome = await openChrome(CHROME, "yap-parity-crops");
  } catch (e) {
    console.error("parity-crops: " + e.message);
    return 3;
  }
  const { proc, cdp, ws, profile } = chrome;
  try {
    let rec;
    try {
      rec = await cropDesign(cdp, { page: A.page, selectors: A.selectors, frameMs: A.frame, width: A.width, height: A.height, designDir: A.designDir });
    } catch (e) {
      console.error("parity-crops: " + e.message);
      return e.env ? 3 : 1;
    }
    if (A.out) mkdirSync(A.out, { recursive: true });
    const crops = rec.crops.map((c) => {
      const { png, ...m } = c;
      if (png && A.out) {
        const f = join(A.out, slug(c.selector) + ".png");
        writeFileSync(f, png);
        return { ...m, png: f };
      }
      return { ...m, png: null, pngBytes: png ? png.length : 0 };
    });
    const out = { ...rec, crops };
    if (A.out) writeFileSync(join(A.out, "crops.json"), JSON.stringify(out, null, 2));
    console.log(JSON.stringify(out, null, 2));
    const missing = crops.filter((c) => !c.found);
    console.error("parity-crops event=done page=" + A.page + " selectors=" + crops.length + " missing=" + missing.length + " ms=" + (Date.now() - t0));
    return missing.length ? 1 : 0;
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
    await sleep(200);
    rmSync(profile, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exit(await main());
}
