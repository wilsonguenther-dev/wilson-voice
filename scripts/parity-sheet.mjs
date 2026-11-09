#!/usr/bin/env node
/**
 * yap-1107-parity-kit — the measured side-by-side PARITY SHEET.
 *
 * Wilson, 2026-11-07: the built app must look EXACTLY like the design pages he
 * approved on 2026-11-06, "no degrading, never degrading". He trusts evidence,
 * not assurances. This is the evidence: ONE self-contained HTML (every image
 * inlined as base64) with one row per approved state —
 *
 *   artifact crop | app capture | diff notes (w/h px, sprite scale, font px,
 *                                  spacing px, hex colours, percent delta,
 *                                  PASS/FAIL)
 *
 * — and with --check it exits 1 on any FAIL. The rules are not tunable:
 *
 *   dim           box w/h, font px, padding/gap px, sprite w/h: more than
 *                 3 percent from the artifact (or from the manifest's expect)
 *   color         the set of hex colours the app paints is not EXACTLY the
 *                 artifact's set (computed styles, normalised to sRGB hex)
 *   sprite-color  a dominant sampled colour of an artifact sprite is missing
 *                 from the app sprite's sampled pixels
 *   sprite-pixels a dominant artifact sprite colour is absent from the app
 *                 CAPTURE's pixels inside that sprite's box
 *   sprite-cropped  any app sprite outside its container or cut by an
 *                 overflow-clipping ancestor (the window edge included)
 *   missing-capture  a state with no capture PNG
 *   capture-not-2x / capture-cropped  the capture is not Retina, or the state's
 *                 box runs off the capture
 *   not-found     the artifact or app selector matched nothing, or the app
 *                 side could not be measured at all
 *
 * BOTH SIDES ARE MEASURED BY ONE FUNCTION: parity-crops.mjs MEASURE, run in the
 * design page (file://, frozen at a fixed frame) and in the app's own DOM. The
 * app DOM is the SAME bundle the instruments drive: the pill through the
 * forced-phase float.html?phase=&pill=&dock= of a YAP_PILL_MATRIX=1 build, the
 * main window through index.html with smoke-windowed's fixture shim — both via
 * the shared helpers in scripts/headless-chrome.mjs (one copy of the shim, the
 * preview server and the forced-phase fixtures). The capture PNGs come from
 * scripts/pill-matrix.mjs (2x) or scripts/smoke-windowed.mjs run with
 * YAP_SMOKE_SCALE=2.
 *
 * MANIFEST (JSON)
 *   { "designDir": "<optional, default the 2026-11-06 design dir>", "frameMs": 1200,
 *     "states": [ {
 *       "id": "idle-bottom",
 *       "artifact": { "page": "pill.html", "selector": "#rows .pill", "sprites": [] },
 *       "app": { "page": "float.html", "query": "phase=idle&pill=yappy&dock=bottom&meeting=none&sky=day",
 *                "selector": ".yappy-shell", "capture": "idle__yappy__bottom__none.png" },
 *          or { "page": "index.html", "view": "home", "selector": ".home", "width": 980, "height": 700,
 *               "fixtures": "scripts/smoke-fixtures.json", "capture": "home-980x700.png",
 *               "clicks": ["<optional selectors clicked in order after the view>"] },
 *       "expect": { "w": 120, "h": 40, "fontPx": [13], "padding": {"t":6,"r":12,"b":6,"l":12}, "gap": {"row":0,"col":8} } } ] }
 *
 * USAGE
 *   node scripts/parity-sheet.mjs --manifest <m.json> --captures <dir> --out <sheet.html>
 *        [--check] [--app-url <http base of a running matrix-mode preview> | --build]
 *   node scripts/parity-sheet.mjs --self-test-must-fail
 *
 *   --build    builds desktop/ with YAP_PILL_MATRIX=1 into a scratch dir and
 *              serves it (shared helpers; needs desktop/node_modules — an
 *              integrator step, never a coder's).
 *
 * EXIT  0 sheet written (and, with --check, every state PASS) · 1 --check saw a
 *       FAIL · 2 usage · 3 environment (no Chrome, no bundle, no page).
 *       --self-test-must-fail: 1 = DETECTORS NOT BLIND (every planted defect was
 *       caught and the clean control passed — the expected result) · 9 = a
 *       detector is blind or over-fires.
 * NO npm DEPENDENCY, no Playwright. PNGs are decoded/encoded with node:zlib.
 */

import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { deflateSync, inflateSync } from "node:zlib";
import {
  TAURI_SHIM,
  buildMatrixBundle,
  evaluate,
  fitPillPanel,
  openChrome,
  pillFixtureScript,
  pinColorScheme,
  pinnedClockScript,
  servePreview,
  settlePhase,
  sleep,
} from "./headless-chrome.mjs";
import { CHROME, CROP_SCALE, DESIGN_DIR, DEFAULT_FRAME_MS, cropDesign, measureSelector } from "./parity-crops.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
export const TOLERANCE_PCT = 3;
const PILL_W = 300; // float_pill.rs PILL_W
const PILL_H = 140; // float_pill.rs PILL_H
const SETTLE_MS = Number(process.env.YAP_PILL_SETTLE_MS || 900);
/* A sprite colour is "dominant" at >= 10 percent of its opaque pixels, and is
   "present" in a capture crop at >= 0.5 percent of that crop's pixels. */
const DOMINANT_SHARE = 0.1;
const PRESENT_SHARE = 0.005;

/* ── PNG (8-bit, non-interlaced; what Chrome writes) ───────────────────── */

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

export function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error("not a PNG");
  let off = 8, w = 0, h = 0, depth = 0, ctype = 0, interlace = 0;
  const idat = [];
  while (off < buf.length) {
    const len = buf.readUInt32BE(off);
    const type = buf.toString("ascii", off + 4, off + 8);
    const body = buf.subarray(off + 8, off + 8 + len);
    if (type === "IHDR") { w = body.readUInt32BE(0); h = body.readUInt32BE(4); depth = body[8]; ctype = body[9]; interlace = body[12]; }
    else if (type === "IDAT") idat.push(body);
    else if (type === "IEND") break;
    off += 12 + len;
  }
  const channels = { 0: 1, 2: 3, 4: 2, 6: 4 }[ctype];
  if (depth !== 8 || !channels || interlace) throw new Error("unsupported PNG (depth " + depth + ", type " + ctype + ", interlace " + interlace + ")");
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * channels;
  const px = Buffer.alloc(stride * h);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    const src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = px.subarray(y * stride, (y + 1) * stride);
    const prev = y ? px.subarray((y - 1) * stride, y * stride) : null;
    for (let i = 0; i < stride; i++) {
      const a = i >= channels ? cur[i - channels] : 0;
      const b = prev ? prev[i] : 0;
      const c = prev && i >= channels ? prev[i - channels] : 0;
      let v = src[i];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      cur[i] = v & 0xff;
    }
  }
  const data = Buffer.alloc(w * h * 4);
  for (let i = 0, j = 0; i < w * h; i++, j += channels) {
    const g = px[j];
    if (channels === 4) { data[i * 4] = px[j]; data[i * 4 + 1] = px[j + 1]; data[i * 4 + 2] = px[j + 2]; data[i * 4 + 3] = px[j + 3]; }
    else if (channels === 3) { data[i * 4] = px[j]; data[i * 4 + 1] = px[j + 1]; data[i * 4 + 2] = px[j + 2]; data[i * 4 + 3] = 255; }
    else if (channels === 2) { data[i * 4] = data[i * 4 + 1] = data[i * 4 + 2] = g; data[i * 4 + 3] = px[j + 1]; }
    else { data[i * 4] = data[i * 4 + 1] = data[i * 4 + 2] = g; data[i * 4 + 3] = 255; }
  }
  return { w, h, data };
}

export function encodePng({ w, h, data }) {
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) data.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4);
  const chunk = (type, body) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(body.length);
    const tb = Buffer.concat([Buffer.from(type, "ascii"), body]);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(tb));
    return Buffer.concat([len, tb, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

/* Crops in pixel space; reports whether the rect ran off the image. */
export function cropImage(img, x, y, w, h) {
  x = Math.round(x); y = Math.round(y); w = Math.max(1, Math.round(w)); h = Math.max(1, Math.round(h));
  const off = x < 0 || y < 0 || x + w > img.w || y + h > img.h;
  const out = { w, h, data: Buffer.alloc(w * h * 4) };
  for (let yy = 0; yy < h; yy++) for (let xx = 0; xx < w; xx++) {
    const sx = x + xx, sy = y + yy;
    if (sx < 0 || sy < 0 || sx >= img.w || sy >= img.h) continue;
    img.data.copy(out.data, (yy * w + xx) * 4, (sy * img.w + sx) * 4, (sy * img.w + sx) * 4 + 4);
  }
  return { img: out, off };
}

function pixelShare(img, hex) {
  const r = parseInt(hex.slice(1, 3), 16), g = parseInt(hex.slice(3, 5), 16), b = parseInt(hex.slice(5, 7), 16);
  let n = 0;
  for (let i = 0; i < img.data.length; i += 4) if (img.data[i] === r && img.data[i + 1] === g && img.data[i + 2] === b) n++;
  return n / (img.w * img.h);
}

/* ── the judge (pure: the real run and the self-test share it) ─────────── */

const pct = (want, got) => (want === 0 ? (got === 0 ? 0 : 100) : (Math.abs(got - want) / Math.abs(want)) * 100);
const r1 = (n) => Math.round(n * 10) / 10;

/*
 * state: a manifest state. art / app: MEASURE records (or null). cap: the
 * decoded capture PNG (or null). artPng: the decoded artifact crop (or null).
 * Returns { id, pass, findings: [{id, detail}], notes: [{what, artifact, app, delta, ok}], maxDeltaPct, appCrop }.
 */
export function judgeState(state, art, app, cap, artPng) {
  const findings = [], notes = [];
  let maxDelta = 0;
  const fail = (id, detail) => findings.push({ id, detail });
  const dim = (what, a, b, findId = "dim") => {
    if (typeof a !== "number" || typeof b !== "number") return;
    const d = pct(a, b);
    // spacing that is 0 on the artifact may be at most 0.5 px in the app
    const ok = a === 0 ? Math.abs(b) <= 0.5 : d <= TOLERANCE_PCT;
    maxDelta = Math.max(maxDelta, d);
    notes.push({ what, artifact: a, app: b, delta: r1(d) + "%", ok });
    if (!ok) fail(findId, what + " " + a + " px on the artifact, " + b + " px in the app (" + r1(d) + "% off, limit " + TOLERANCE_PCT + "%)");
  };
  if (!art || !art.found) fail("not-found", "artifact selector " + JSON.stringify(state.artifact && state.artifact.selector) + " matched nothing");
  if (!app || !app.found) fail("not-found", app && app.error ? "app side not measured: " + app.error : "app selector " + JSON.stringify(state.app && state.app.selector) + " matched nothing");
  if (!cap) fail("missing-capture", "no capture " + JSON.stringify(state.app && state.app.capture) + " in the captures dir");
  let appCrop = null;
  if (art && art.found && app && app.found) {
    dim("width", art.box.w, app.box.w);
    dim("height", art.box.h, app.box.h);
    const ex = state.expect || {};
    for (const k of ["w", "h"]) if (typeof ex[k] === "number") { dim("expect " + k + " (artifact)", ex[k], art.box[k]); dim("expect " + k + " (app)", ex[k], app.box[k]); }
    const fa = art.fontPx || [], fb = app.fontPx || [];
    if (fa.length !== fb.length) { fail("dim", "font sizes differ: artifact [" + fa.join(", ") + "] px, app [" + fb.join(", ") + "] px"); notes.push({ what: "font px", artifact: fa.join(" "), app: fb.join(" "), delta: "-", ok: false }); }
    else fa.forEach((v, i) => dim("font px #" + (i + 1), v, fb[i]));
    if (Array.isArray(ex.fontPx)) {
      const same = ex.fontPx.length === fb.length && ex.fontPx.every((v, i) => pct(v, fb[i]) <= TOLERANCE_PCT);
      notes.push({ what: "expect font px (app)", artifact: ex.fontPx.join(" "), app: fb.join(" "), delta: "-", ok: same });
      if (!same) fail("dim", "app font sizes [" + fb.join(", ") + "] are not the expected [" + ex.fontPx.join(", ") + "]");
    }
    for (const k of ["t", "r", "b", "l"]) dim("padding-" + k, art.padding[k], app.padding[k]);
    for (const k of ["row", "col"]) dim("gap-" + k, art.gap[k], app.gap[k]);
    for (const k of ["t", "r", "b", "l"]) if (ex.padding && typeof ex.padding[k] === "number") dim("expect padding-" + k + " (app)", ex.padding[k], app.padding[k]);
    for (const k of ["row", "col"]) if (ex.gap && typeof ex.gap[k] === "number") dim("expect gap-" + k + " (app)", ex.gap[k], app.gap[k]);

    const ca = new Set(art.colors || []), cb = new Set(app.colors || []);
    const missing = [...ca].filter((c) => !cb.has(c)), extra = [...cb].filter((c) => !ca.has(c));
    notes.push({ what: "hex colours", artifact: [...ca].join(" "), app: [...cb].join(" "), delta: missing.length + extra.length ? "missing " + (missing.join(" ") || "-") + " / extra " + (extra.join(" ") || "-") : "exact", ok: !missing.length && !extra.length });
    if (missing.length || extra.length) fail("color", "colours are not an exact hex match — missing in app: " + (missing.join(", ") || "none") + "; not in artifact: " + (extra.join(", ") || "none"));
    if (Array.isArray(ex.colors)) {
      const lost = ex.colors.map((c) => c.toLowerCase()).filter((c) => !cb.has(c));
      if (lost.length) fail("color", "expected colours absent from the app: " + lost.join(", "));
    }

    const sa = art.sprites || [], sb = app.sprites || [];
    for (const sp of sb) if (!sp.inside || sp.clippedBy) fail("sprite-cropped", "app sprite <" + sp.tag + (sp.cls ? " ." + sp.cls.split(" ")[0] : "") + "> " + (sp.clippedBy ? "is cut by " + sp.clippedBy : "leaves its container"));
    if (sa.length !== sb.length) fail("dim", "sprite count: artifact " + sa.length + ", app " + sb.length);
    const scale = cap && app.viewport && app.viewport.w ? cap.w / app.viewport.w : null;
    sa.forEach((s, i) => {
      const t = sb[i];
      if (!t) return;
      dim("sprite " + (i + 1) + " width", s.box.w, t.box.w);
      dim("sprite " + (i + 1) + " height", s.box.h, t.box.h);
      if (s.scale != null || t.scale != null) notes.push({ what: "sprite " + (i + 1) + " scale", artifact: s.scale, app: t.scale, delta: "-", ok: true });
      const dominant = (s.pixels && s.pixels.sampled ? s.pixels.palette : []).filter((p) => p.share >= DOMINANT_SHARE).map((p) => p.hex);
      if (dominant.length && t.pixels && t.pixels.sampled) {
        const have = new Set(t.pixels.palette.map((p) => p.hex));
        const lost = dominant.filter((h) => !have.has(h));
        notes.push({ what: "sprite " + (i + 1) + " sampled colours", artifact: dominant.join(" "), app: [...have].join(" "), delta: lost.length ? "missing " + lost.join(" ") : "exact", ok: !lost.length });
        if (lost.length) fail("sprite-color", "sprite " + (i + 1) + " lost its sampled colours " + lost.join(", "));
      }
      if (dominant.length && cap && scale) {
        const c = cropImage(cap, t.box.x * scale, t.box.y * scale, t.box.w * scale, t.box.h * scale).img;
        const lost = dominant.filter((h) => pixelShare(c, h) < PRESENT_SHARE);
        notes.push({ what: "sprite " + (i + 1) + " capture pixels", artifact: dominant.join(" "), app: lost.length ? "absent " + lost.join(" ") : "all present", delta: "-", ok: !lost.length });
        if (lost.length) fail("sprite-pixels", "sprite " + (i + 1) + " colours " + lost.join(", ") + " are not in the capture's pixels");
      }
    });

    if (cap && scale) {
      notes.push({ what: "capture scale", artifact: CROP_SCALE + "x", app: r1(scale) + "x", delta: "-", ok: Math.abs(scale - CROP_SCALE) < 0.01 });
      if (Math.abs(scale - CROP_SCALE) >= 0.01) fail("capture-not-2x", "the capture is " + r1(scale) + "x, the artifact crop is " + CROP_SCALE + "x (run smoke-windowed with YAP_SMOKE_SCALE=2)");
      const c = cropImage(cap, app.box.x * scale, app.box.y * scale, app.box.w * scale, app.box.h * scale);
      appCrop = c.img;
      if (c.off) fail("capture-cropped", "the state's box runs off the capture (" + cap.w + "x" + cap.h + " px)");
      if (artPng) dim("crop px width", artPng.w, appCrop.w);
      if (artPng) dim("crop px height", artPng.h, appCrop.h);
    }
  }
  return { id: state.id, pass: findings.length === 0, findings, notes, maxDeltaPct: r1(maxDelta), appCrop };
}

/* ── the sheet ─────────────────────────────────────────────────────────── */

const esc = (s) => String(s ?? "").replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const dataUri = (img) => (img ? "data:image/png;base64," + encodePng(img).toString("base64") : null);

export function renderSheet(rows, meta) {
  const fails = rows.filter((r) => !r.pass).length;
  const cell = (img, what) => {
    const u = dataUri(img);
    return u ? `<img alt="${esc(what)}" src="${u}" style="width:${Math.round(img.w / CROP_SCALE)}px">` : `<div class="none">${esc(what)} missing</div>`;
  };
  const body = rows.map((r) => `
<tr class="${r.pass ? "pass" : "fail"}">
  <td class="id"><b>${esc(r.id)}</b><br><span class="tag">${r.pass ? "PASS" : "FAIL"}</span><br><small>max delta ${esc(r.maxDeltaPct)}%</small></td>
  <td>${cell(r.artPng, "artifact crop")}</td>
  <td>${cell(r.appCrop, "app capture")}</td>
  <td><table class="notes"><tr><th>measure</th><th>artifact</th><th>app</th><th>delta</th><th></th></tr>
    ${r.notes.map((n) => `<tr class="${n.ok ? "" : "bad"}"><td>${esc(n.what)}</td><td>${esc(n.artifact)}</td><td>${esc(n.app)}</td><td>${esc(n.delta)}</td><td>${n.ok ? "PASS" : "FAIL"}</td></tr>`).join("")}
  </table>${r.findings.length ? `<ul>${r.findings.map((f) => `<li><code>${esc(f.id)}</code> ${esc(f.detail)}</li>`).join("")}</ul>` : ""}</td>
</tr>`).join("");
  return `<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>Yap parity sheet ${esc(meta.date)}</title>
<style>
body{font:13px/1.45 -apple-system,system-ui,sans-serif;margin:24px;color:#1d1d1f;background:#fafafa}
h1{font-size:18px;margin:0 0 4px} p{margin:0 0 16px;color:#555}
table.sheet{border-collapse:collapse;width:100%} table.sheet>tbody>tr>td{border-top:1px solid #ddd;padding:12px 10px;vertical-align:top}
tr.fail .tag{background:#b3261e;color:#fff} tr.pass .tag{background:#1f6e43;color:#fff}
.tag{display:inline-block;border-radius:4px;padding:1px 6px;font-weight:600;margin:4px 0}
.none{width:160px;height:60px;border:1px dashed #b3261e;color:#b3261e;display:flex;align-items:center;justify-content:center}
table.notes{border-collapse:collapse;font:12px/1.4 ui-monospace,Menlo,monospace} table.notes td,table.notes th{padding:2px 8px;text-align:left;border-bottom:1px solid #eee}
tr.bad td{color:#b3261e;font-weight:600} img{background:repeating-conic-gradient(#eee 0 25%,#fff 0 50%) 0 0/12px 12px;border:1px solid #ddd}
</style></head><body>
<h1>Parity sheet: ${rows.length} state(s), ${fails} failing</h1>
<p>${esc(meta.date)} · manifest ${esc(meta.manifest)} · rule: dimensions within ${TOLERANCE_PCT} percent, colours exact hex, no cropped sprite, every state captured. Both sides measured by the same function at ${CROP_SCALE}x.</p>
<table class="sheet"><thead><tr><th>state</th><th>approved artifact</th><th>app</th><th>measured difference</th></tr></thead><tbody>${body}</tbody></table>
</body></html>
`;
}

/* yap-1107-parity-news — where a manifest capture name lives on disk.
   smoke-windowed.sh prefixes every file with its fixture state ("custom-" when
   YAP_SMOKE_FIXTURES is set) and a second fixture set is a second run dir
   (parity-news-onb-<date> next to parity-news-<date>). Order: the exact name
   in --captures, then "<state>-<name>" there, then the same two in each
   sibling run "<stem>-<anything>-<date>" of --captures. Pure over the disk;
   null when nothing matches (the state then fails missing-capture). */
export function findCapture(dir, name) {
  if (!dir || !name) return null;
  const inDir = (d) => {
    if (existsSync(join(d, name))) return join(d, name);
    let files = [];
    try { files = readdirSync(d).filter((f) => f.endsWith("-" + name)).sort(); } catch { return null; }
    return files.length ? join(d, files[0]) : null;
  };
  const own = inDir(dir);
  if (own) return own;
  const m = /^(.*?)-(\d{4}-\d{2}-\d{2})$/.exec(basename(dir));
  if (!m) return null;
  let sibs = [];
  try { sibs = readdirSync(dirname(dir)).filter((d) => d !== basename(dir) && d.startsWith(m[1] + "-") && d.endsWith("-" + m[2])).sort(); } catch { return null; }
  for (const d of sibs) {
    const hit = inDir(join(dirname(dir), d));
    if (hit) return hit;
  }
  return null;
}

/* ── the app side: the SAME DOM the instruments capture ────────────────── */

async function newSession(cdp, w, h, scale, source) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: scale, mobile: false }, s);
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source }, s);
  return { s, targetId };
}

async function pillTones(cdp, base) {
  const { s, targetId } = await newSession(cdp, PILL_W, PILL_H, CROP_SCALE, pillFixtureScript({}) + "\n" + TAURI_SHIM);
  try {
    await cdp.send("Page.navigate", { url: base + "/float.html" }, s);
    await settlePhase(cdp, s, null, 200);
    const phases = await evaluate(cdp, s, "window.__YAP_PILL_PHASES__ || null");
    if (!Array.isArray(phases)) return null;
    return Object.fromEntries(phases.flatMap((p) => [[p.phase, p.tone], [p.id, p.tone]]));
  } finally {
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  }
}

/*
 * yap-1107-parity-pill (bounce fix: open states measured the 72 x 28 nub) —
 * YappyPill sizes the shell from its draw loop, which a background headless
 * target may not be running. The pill-matrix capture forces frames with its
 * screenshot; the measure did not. Bring the target forward and force frames
 * (a throwaway 1 px screenshot) until the measured box holds still across two
 * reads, so the box measured is the box the capture paints. Bounded: 12 passes.
 */
export async function pumpUntilSettled(cdp, s, selector) {
  await cdp.send("Page.bringToFront", {}, s).catch(() => {});
  await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true }, s).catch(() => {});
  const read = () => evaluate(cdp, s, `(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return ""; const r = e.getBoundingClientRect(); return r.left.toFixed(2) + "|" + r.top.toFixed(2) + "|" + r.width.toFixed(2) + "|" + r.height.toFixed(2); })()`).catch(() => "");
  let last = null;
  for (let pass = 0; pass < 12; pass++) {
    await cdp.send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 1, height: 1, scale: 1 } }, s).catch(() => {});
    await sleep(120);
    const now = await read();
    if (now && now === last) return now;
    last = now;
  }
  return last;
}

export async function measureApp(cdp, base, st, tones) {
  const a = st.app || {};
  const page = a.page || "float.html";
  const opts = { sprites: a.sprites || [] };
  if (page === "float.html") {
    const { s, targetId } = await newSession(cdp, a.width || PILL_W, a.height || PILL_H, CROP_SCALE, pillFixtureScript(tones || {}) + "\n" + TAURI_SHIM);
    try {
      // the pill-matrix backdrop, so a transparent window measures like its capture
      await cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 46, g: 49, b: 56, a: 1 } }, s);
      await cdp.send("Page.navigate", { url: base + "/float.html" + (a.query ? "?" + a.query.replace(/^\?/, "") : "") }, s);
      await settlePhase(cdp, s, new URLSearchParams(a.query || "").get("phase"), SETTLE_MS);
      await fitPillPanel(cdp, s, { w: a.width || PILL_W, h: a.height || PILL_H }, CROP_SCALE, SETTLE_MS);
      await pumpUntilSettled(cdp, s, a.selector);
      const m = await measureSelector(cdp, s, a.selector, opts);
      m.phaseApplied = await evaluate(cdp, s, "document.documentElement.dataset.phase || null").catch(() => null);
      return m;
    } finally {
      await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
    }
  }
  const fxPath = resolve(ROOT, a.fixtures || "scripts/smoke-fixtures.json");
  const fixtures = JSON.parse(readFileSync(fxPath, "utf8"));
  delete fixtures._README;
  // yap-1107-parity-home: a state may pin the colour scheme and the time of day
  // (app.colorScheme "light"|"dark"|"system", app.clock "HH:MM"|"off") so the
  // app is measured as the light, daytime page it copies — not as the Mac's
  // appearance and the real clock happen to be (scripts/headless-chrome.mjs).
  const clockJs = a.clock && a.clock !== "off" ? pinnedClockScript(a.clock) : "";
  const { s, targetId } = await newSession(cdp, a.width || 980, a.height || 700, CROP_SCALE, clockJs + "window.__YAP_FIXTURES__ = " + JSON.stringify(fixtures) + ";\n" + TAURI_SHIM);
  try {
    if (a.colorScheme) await pinColorScheme(cdp, s, a.colorScheme);
    await cdp.send("Page.navigate", { url: base + "/" + page }, s);
    await sleep(1200);
    if (a.view) {
      await evaluate(cdp, s, `(() => { const want = ${JSON.stringify(a.view)}; const b = [...document.querySelectorAll("nav.nav button, nav button, .nav-item")].find(b => (b.innerText||"").trim().toLowerCase().startsWith(want)); if (b) b.click(); return !!b; })()`);
      await sleep(1500);
    }
    /* yap-1107-parity-news — "clicks": CSS selectors clicked in order after the
       view (a Settings tab, an onboarding primary button), so a state behind a
       click is measured in the same bundle the capture walks. A selector that
       matches nothing is reported, never skipped. */
    for (const raw of a.clicks || []) {
      /* A leading "?" marks a click that only exists in a passing state (the
         News pair's "Skip to the stories" while the joint delivery plays):
         clicked when present, skipped when the state already moved on. */
      const optional = raw.startsWith("?");
      const sel = optional ? raw.slice(1) : raw;
      const hit = await evaluate(cdp, s, `(() => { const b = document.querySelector(${JSON.stringify(sel)}); if (!b || b.disabled) return false; b.click(); return true; })()`);
      if (!hit && optional) continue;
      if (!hit) return { found: false, error: "click target " + JSON.stringify(sel) + " matched nothing or was disabled" };
      await sleep(900);
    }
    return await measureSelector(cdp, s, a.selector, opts);
  } finally {
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  }
}

/* ── run ───────────────────────────────────────────────────────────────── */

function parseArgs(argv) {
  const a = { manifest: null, captures: null, out: null, check: false, appUrl: null, build: false, selfTest: false };
  const need = (k, v) => { if (v === undefined || v.startsWith("--")) usage(k + " needs a value"); return v; };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--manifest") a.manifest = resolve(need(k, argv[++i]));
    else if (k === "--captures") a.captures = resolve(need(k, argv[++i]));
    else if (k === "--out") a.out = resolve(need(k, argv[++i]));
    else if (k === "--app-url") a.appUrl = need(k, argv[++i]).replace(/\/$/, "");
    else if (k === "--check") a.check = true;
    else if (k === "--build") a.build = true;
    else if (k === "--self-test-must-fail") a.selfTest = true;
    else usage("unknown argument " + k);
  }
  if (!a.selfTest && (!a.manifest || !a.captures || !a.out)) usage("--manifest, --captures and --out are required");
  if (a.appUrl && a.build) usage("--app-url and --build are exclusive");
  return a;
}
function usage(msg) {
  console.error("parity-sheet: " + msg + "\nusage: node scripts/parity-sheet.mjs --manifest <m.json> --captures <dir> --out <sheet.html> [--check] [--app-url <base> | --build]\n       node scripts/parity-sheet.mjs --self-test-must-fail");
  process.exit(2);
}

const today = () => new Date().toISOString().slice(0, 10);
const loadPng = (f) => (f && existsSync(f) ? decodePng(readFileSync(f)) : null);

async function run(A) {
  const t0 = Date.now();
  const manifest = JSON.parse(readFileSync(A.manifest, "utf8"));
  const states = Array.isArray(manifest.states) ? manifest.states : [];
  if (!states.length) { console.error("parity-sheet: the manifest names no states"); return 2; }
  const designDir = manifest.designDir ? resolve(manifest.designDir) : DESIGN_DIR;
  let work = null, preview = null, base = A.appUrl;
  let chrome;
  try {
    if (A.build) {
      work = mkdtempSync(join(tmpdir(), "yap-parity-sheet-"));
      const dist = buildMatrixBundle(DESKTOP, work, "parity-sheet");
      const served = await servePreview(DESKTOP, dist, work, "parity-sheet");
      preview = served.proc; base = served.base;
    }
    try {
      chrome = await openChrome(CHROME, "yap-parity-sheet");
    } catch (e) {
      console.error("parity-sheet: " + e.message);
      return 3;
    }
    const { cdp } = chrome;
    /* artifact side: one page load per design page, every selector of it */
    const byPage = new Map();
    for (const st of states) {
      const p = (st.artifact && st.artifact.page) || "pill.html";
      if (!byPage.has(p)) byPage.set(p, []);
      byPage.get(p).push(st);
    }
    const artRec = new Map();
    for (const [page, sts] of byPage) {
      const rec = await cropDesign(cdp, { page, designDir, frameMs: manifest.frameMs || DEFAULT_FRAME_MS, selectors: sts.map((s) => s.artifact.selector) });
      sts.forEach((s, i) => artRec.set(s.id, rec.crops[i]));
    }
    const tones = base ? await pillTones(cdp, base) : null;
    if (base && states.some((s) => (s.app && s.app.page || "float.html") === "float.html") && !tones) {
      console.error("parity-sheet: " + base + "/float.html published no __YAP_PILL_PHASES__ — not a YAP_PILL_MATRIX=1 build (INSTRUMENT failure)");
      return 3;
    }
    const rows = [];
    for (const st of states) {
      const art = artRec.get(st.id);
      let app;
      if (!base) app = { found: false, error: "no app DOM to measure (pass --app-url or --build)" };
      else app = await measureApp(cdp, base, st, tones).catch((e) => ({ found: false, error: e.message }));
      const capPath = st.app && st.app.capture ? findCapture(A.captures, st.app.capture) : null;
      const cap = loadPng(capPath);
      const artPng = art && art.png ? decodePng(art.png) : null;
      const r = judgeState(st, art, app, cap, artPng);
      rows.push({ ...r, artPng });
      console.log((r.pass ? "PASS " : "FAIL ") + st.id.padEnd(28) + "max delta " + r.maxDeltaPct + "%");
      for (const f of r.findings) console.log("       " + f.id + " — " + f.detail);
    }
    mkdirSync(dirname(A.out), { recursive: true });
    writeFileSync(A.out, renderSheet(rows, { date: today(), manifest: A.manifest }));
    const failing = rows.filter((r) => !r.pass).length;
    console.log("parity-sheet: " + rows.length + " state(s), " + failing + " failing -> " + A.out);
    console.error("parity-sheet event=done states=" + rows.length + " failing=" + failing + " check=" + A.check + " ms=" + (Date.now() - t0));
    return A.check && failing ? 1 : 0;
  } finally {
    if (chrome) {
      try { chrome.ws.close(); } catch {}
      chrome.proc.kill("SIGKILL");
    }
    if (preview) preview.kill("SIGKILL");
    await sleep(200);
    if (chrome) rmSync(chrome.profile, { recursive: true, force: true });
    if (work) rmSync(work, { recursive: true, force: true });
  }
}

/* ── --self-test-must-fail ─────────────────────────────────────────────── */
/*
 * No browser: feeds judgeState + renderSheet a CLEAN control state (artifact and
 * app identical, a 2x capture painted to match) that must PASS, a capture that
 * is deliberately SHRUNK (13 percent narrower) and RECOLOURED (text, sprite
 * pixels) with its sprite pushed out of its container, which must fire dim,
 * color, sprite-color, sprite-pixels and sprite-cropped, a 1x capture that must
 * fire capture-not-2x, and a state with no capture, which must fire
 * missing-capture. The sheet it writes must inline every image. Exits 1 (the
 * run carries deliberate breaks, so --check would fail) when every detector
 * fired; 9 when one is blind or the control over-fires.
 */
function solid(w, h, rgba) {
  const data = Buffer.alloc(w * h * 4);
  for (let i = 0; i < w * h; i++) data.set(rgba, i * 4);
  return { w, h, data };
}
function paint(img, x, y, w, h, rgba) {
  for (let yy = y; yy < y + h; yy++) for (let xx = x; xx < x + w; xx++) if (xx >= 0 && yy >= 0 && xx < img.w && yy < img.h) img.data.set(rgba, (yy * img.w + xx) * 4);
}
const rgb = (hex) => [parseInt(hex.slice(1, 3), 16), parseInt(hex.slice(3, 5), 16), parseInt(hex.slice(5, 7), 16), 255];

function selfTest() {
  const SPRITE = "#f4c542", GREEN = "#1f6e43", WHITE = "#ffffff", BG = "#2e3138";
  const measured = (o) => ({
    found: true, viewport: { w: 300, h: 140, dpr: 2 }, fontPxRoot: 13, fontPx: [11, 13],
    padding: { t: 6, r: 12, b: 6, l: 12 }, gap: { row: 0, col: 8 }, colors: [GREEN, WHITE],
    box: { x: 10, y: 10, w: 120, h: 40 },
    sprites: [{ tag: "canvas", cls: "yappy", box: { x: 20, y: 15, w: 30, h: 30 }, intrinsicW: 60, scale: 0.5, inside: true, clippedBy: null, pixels: { sampled: true, opaquePx: 3600, palette: [{ hex: SPRITE, share: 0.8 }] } }],
    ...o,
  });
  const capture = (scale, box, sprite, spriteHex) => {
    const img = solid(300 * scale, 140 * scale, rgb(BG));
    paint(img, box.x * scale, box.y * scale, box.w * scale, box.h * scale, rgb(GREEN));
    paint(img, sprite.x * scale, sprite.y * scale, sprite.w * scale, sprite.h * scale, rgb(spriteHex));
    return img;
  };
  const art = measured({});
  const artPng = solid(240, 80, rgb(GREEN));
  paint(artPng, 20, 10, 60, 60, rgb(SPRITE));
  const st = (id, capture) => ({ id, artifact: { page: "pill.html", selector: ".pill" }, app: { page: "float.html", selector: ".pill", capture }, expect: { w: 120, h: 40, fontPx: [11, 13] } });

  const clean = judgeState(st("control-clean", "clean.png"), art, measured({}), capture(2, art.box, art.sprites[0].box, SPRITE), artPng);
  const shrunkBox = { x: 10, y: 10, w: 104, h: 40 };
  const outSprite = { x: 110, y: 15, w: 30, h: 30 };
  const brokenApp = measured({
    box: shrunkBox, colors: ["#ff0000", WHITE], fontPx: [11, 12],
    sprites: [{ tag: "canvas", cls: "yappy", box: outSprite, intrinsicW: 60, scale: 0.5, inside: false, clippedBy: null, pixels: { sampled: true, opaquePx: 3600, palette: [{ hex: "#ff00ff", share: 0.8 }] } }],
  });
  const broken = judgeState(st("shrunk-recoloured", "broken.png"), art, brokenApp, capture(2, shrunkBox, outSprite, "#ff00ff"), artPng);
  const onex = judgeState(st("one-x-capture", "onex.png"), art, measured({}), capture(1, art.box, art.sprites[0].box, SPRITE), artPng);
  const missing = judgeState(st("no-capture", "absent.png"), art, measured({}), null, artPng);

  const fired = (r) => new Set(r.findings.map((f) => f.id));
  const want = [
    ["control-clean passes", clean.pass],
    ["shrunk -> dim", fired(broken).has("dim")],
    ["recoloured -> color", fired(broken).has("color")],
    ["recoloured sprite -> sprite-color", fired(broken).has("sprite-color")],
    ["recoloured capture -> sprite-pixels", fired(broken).has("sprite-pixels")],
    ["sprite out of container -> sprite-cropped", fired(broken).has("sprite-cropped")],
    ["1x capture -> capture-not-2x", fired(onex).has("capture-not-2x")],
    ["no capture -> missing-capture", fired(missing).has("missing-capture")],
  ];
  const rows = [clean, broken, onex, missing].map((r) => ({ ...r, artPng }));
  const scratch = mkdtempSync(join(tmpdir(), "yap-parity-selftest-"));
  let selfContained = false;
  try {
    const html = renderSheet(rows, { date: today(), manifest: "(self-test)" });
    const f = join(scratch, "sheet.html");
    writeFileSync(f, html);
    const back = readFileSync(f, "utf8");
    const imgs = back.match(/<img [^>]*src="([^"]*)"/g) || [];
    selfContained = imgs.length >= 7 && imgs.every((t) => t.includes('src="data:image/png;base64,')) && !/(src|href)="(https?:|file:)/.test(back);
    // the round trip: an inlined PNG decodes back to the crop it was made from
    const first = back.match(/data:image\/png;base64,([^"]+)/);
    const dec = first ? decodePng(Buffer.from(first[1], "base64")) : null;
    selfContained = selfContained && !!dec && dec.w === 240 && dec.h === 80;
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  want.push(["sheet is self-contained (every image inlined, decodes back)", selfContained]);
  const failing = rows.filter((r) => !r.pass).length;
  want.push(["--check over this run would exit 1", failing === 3]);
  let blind = 0;
  for (const [what, ok] of want) { console.log((ok ? "caught  " : "BLIND   ") + what); if (!ok) blind++; }
  for (const r of rows) for (const f of r.findings) console.log("        " + r.id + ": " + f.id + " — " + f.detail);
  console.error("parity-sheet event=self-test checks=" + want.length + " blind=" + blind);
  if (blind) { console.log("parity-sheet: SELF-TEST FAILED — " + blind + " detector(s) blind or over-firing (exit 9)"); return 9; }
  console.log("parity-sheet: DETECTORS NOT BLIND — every planted defect was caught and the clean control passed (exit 1 by design)");
  return 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const A = parseArgs(process.argv.slice(2));
  process.exit(A.selfTest ? selfTest() : await run(A));
}
