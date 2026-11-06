#!/usr/bin/env node
/**
 * yap24-UI3 — the pill's visual-QA matrix.
 *
 * The 2026-09-15 run's builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C
 * and Y5-D: a green gate cannot see a pill. This is the instrument that can.
 * It photographs float.html in EVERY phase x dock {bottom,left,right} x
 * character {classic,yappy}, and FAILS on three structural conditions:
 *
 *   empty-text    a text slot that is on screen but says nothing (the phase
 *                 line, a gate note, a progress numeral, a speech bubble)
 *   clipped-text  text cut off — by its own overflow box, or by the edge of the
 *                 300x140 float window
 *   overflow      any visible box that leaves the float window
 *
 * yap-1106-one-pill adds the ONE-PILL checks, for every phase x {no meeting,
 * meeting recording} x dock (Yappy; the legacy Classic pill keeps 300x140):
 *
 *   one-pill-overlap  the painted hint, bubble, clock or progress box overlaps
 *                     another, or the character box the layout published
 *   clock-detached    the clock box is more than 12 pt from the character box
 *   panel-not-union   the window is not exactly the union of the layout's
 *                     content boxes plus the 10 pt margin (pill/state.ts)
 *   slot-off-layout   a painted slot is not inside the box the layout gave it
 *
 * The window is sized to the layout's panel (window.__YAP_PILL_LAYOUT__), the
 * way float_pill.rs set_panel_frame sizes the NSPanel, before it is judged.
 *
 * plus one INSTRUMENT check, phase-not-applied, so a typo or a stale bundle can
 * never pass as a photo of `idle`. Structural, never pixel equality: a
 * legitimate restyle must not have to relitigate a PNG.
 *
 * HOW A PHASE IS FORCED. A real phase needs the backend to emit the right
 * events in order, which a browser cannot. So the matrix builds the bundle
 * with YAP_PILL_MATRIX=1, which compiles in float-main's DEV-ONLY handle
 * (src/pill/forced.ts): `float.html?phase=&pill=&dock=`. A normal
 * `npm run build` defines it false and rollup drops it — the released float
 * window ignores its query string. The build goes to a SCRATCH outDir, never
 * desktop/dist, so this never clobbers the bundle the app or the smoke judges.
 *
 * THE PHASE LIST IS NOT KEPT HERE. The page publishes it from the pill's own
 * table (`matrixPhases()` = ALL_LIVE_PHASES through `phaseVisual`), so a phase a
 * PILL item adds is photographed with no edit to this file.
 *
 * WHAT IS DRIVEN (read before believing a result): the SAME headless Chrome
 * smoke-windowed.mjs drives (scripts/headless-chrome.mjs, --headless=new — never
 * a visible window, never the app), at the float window's real size (300x140
 * logical, float_pill.rs PILL_W/PILL_H) and Retina scale 2, with the Tauri
 * bridge shimmed. It is NOT the WKWebView. The backdrop behind the transparent
 * window is a flat dark desktop grey so white-on-glass copy is legible in a PNG.
 *
 * USAGE
 *   node scripts/pill-matrix.mjs --out <dir>             every phase x 3 docks x 2 characters
 *   node scripts/pill-matrix.mjs --out <dir> --only gated  one phase x 3 x 2 (what each PILL item runs)
 *   node scripts/pill-matrix.mjs --self-test-must-fail   prove the detectors are not blind
 *
 *   --out   a SCRATCH dir (default: a fresh mktemp dir). The PNGs are evidence
 *           for a PR, never a commit — do not point this inside the repo.
 *   --only  a phase by variant (model-loading) or product id (model_loading).
 *
 * ENV  YAP_SMOKE_CHROME (Chromium path, shared with the smoke), YAP_PILL_PORT
 *      (preview port; default: a free one), YAP_PILL_SETTLE_MS (default 900).
 *
 * EXIT  0 clean · 1 a capture carries a finding · 2 usage · 3 environment /
 *       instrument failure · 9 self-test found the detectors blind.
 */

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync, openSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { TAURI_SHIM, evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
const W = 300; // float_pill.rs PILL_W
const H = 140; // float_pill.rs PILL_H
const SCALE = 2;
const DOCKS = ["bottom", "left", "right"];
const CHARACTERS = ["classic", "yappy"];
/* yap-1106-one-pill — every capture also runs with and without a meeting. */
const MEETINGS = ["none", "recording"];
const SETTLE_MS = Number(process.env.YAP_PILL_SETTLE_MS || 900);
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

/* ── args ───────────────────────────────────────────────────────────────── */

function parseArgs(argv) {
  const a = { out: null, only: null, selfTest: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--out" || k === "--only") {
      const v = argv[++i];
      if (!v || v.startsWith("--")) usage(k + " needs a value");
      a[k.slice(2)] = v;
    } else if (k.startsWith("--out=")) a.out = k.slice(6);
    else if (k.startsWith("--only=")) a.only = k.slice(7);
    else if (k === "--self-test-must-fail") a.selfTest = true;
    else usage("unknown argument " + k);
  }
  return a;
}
function usage(msg) {
  console.error("pill-matrix: " + msg + "\nusage: node scripts/pill-matrix.mjs [--out <dir>] [--only <phase>] [--self-test-must-fail]");
  process.exit(2);
}
const ARGS = parseArgs(process.argv.slice(2));

/* ── the shim's answers, per capture ────────────────────────────────────── */
/*
 * Runs before the bundle, after TAURI_SHIM's table is read — so it must set
 * __YAP_FIXTURES__ FIRST. The capsules keep a little state of their own off
 * `get_status` (ClassicPill's live/busy classes, Yappy's pose), so the status a
 * phase answers is derived from its treatment TONE, which the page itself
 * published: no per-phase table lives here either.
 */
function fixtureScript(tones) {
  return String.raw`
(() => {
  const q = new URLSearchParams(location.search);
  const tone = (${JSON.stringify(tones)})[q.get("phase")] || "calm";
  window.__YAP_FIXTURES__ = {
    get_settings: { pillStyle: q.get("pill") || "classic", pillPosition: q.get("dock") || "bottom", companionTone: "friendly" },
    get_status: { recording: tone === "live", busy: tone === "busy", message: tone === "live" ? "Recording" : tone === "busy" ? "Transcribing" : "Ready" },
    // yap-1106-one-pill — a meeting on screen, and the panel-frame request the
    // layout makes (answered, so it is never an unhandled rejection).
    meeting_status: q.get("meeting") === "recording"
      ? { recording: true, id: "matrix-meeting", title: "Matrix meeting", elapsedSeconds: 217, elapsedLabel: "00:03:37", captureAvailable: true }
      : { recording: false, id: null, title: null, elapsedSeconds: 0, elapsedLabel: "00:00:00", captureAvailable: false },
    pill_set_panel_frame: null,
  };
})();
`;
}

/* ── the detectors, evaluated inside the page ───────────────────────────── */
/*
 * One function, serialized into the page, used by BOTH the matrix and the
 * self-test, so the self-test proves THIS code is non-vacuous.
 *
 * The float window is html/body/#root at overflow:hidden — that is the WINDOW
 * EDGE, not a container. So unlike the smoke's out-of-bounds check, those three
 * never excuse a box: anything past them is cut off on screen.
 */
const DETECTORS = String.raw`
function __pillDetect(want) {
  const out = [];
  const tol = 1;
  const W = window.innerWidth, H = window.innerHeight;
  const de = document.documentElement;
  const WINDOW = new Set([de, document.body, document.getElementById("root")]);

  if (want) {
    const capsule = document.querySelector(want.pill === "yappy" ? ".kami-stage" : ".pill");
    const bad = [];
    const wantPhase = want.phaseId || want.id;
    if (de.dataset.phase !== wantPhase) bad.push("html[data-phase]=" + de.dataset.phase + " (asked " + wantPhase + ")");
    // yap24-PILL1 — a license scene must actually be on the page.
    if (want.license && de.dataset.license !== want.license) bad.push("html[data-license]=" + de.dataset.license + " (asked " + want.license + ")");
    // WF-12 — an offer scene (lang-offer) must actually draw its card.
    if (want.offer && !document.querySelector('[data-language-offer="' + want.offer + '"]')) bad.push("no [data-language-offer=" + want.offer + "] card");
    if (want.chip && !document.querySelector('[data-style-chip="' + want.chip + '"]')) bad.push("no [data-style-chip=" + want.chip + "] chip");
    if (de.dataset.dock !== want.dock) bad.push("html[data-dock]=" + de.dataset.dock + " (asked " + want.dock + ")");
    if (!capsule) bad.push("no " + want.pill + " capsule rendered");
    else if (capsule.dataset.phase !== wantPhase) bad.push("capsule data-phase=" + capsule.dataset.phase);
    if (bad.length) out.push({ id: "phase-not-applied", detail: bad.join("; ") });
  }

  function opacityOf(el) {
    let o = 1;
    for (let p = el; p && p.nodeType === 1; p = p.parentElement) o *= Number(getComputedStyle(p).opacity);
    return o;
  }
  function shown(el) {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden") return false;
    return opacityOf(el) > 0.05;
  }
  function clippedByContainer(el) {
    for (let p = el.parentElement; p && !WINDOW.has(p); p = p.parentElement) {
      const ox = getComputedStyle(p).overflowX;
      if (ox === "hidden" || ox === "clip" || ox === "auto" || ox === "scroll") return p;
    }
    return null;
  }
  const name = (el) => el.tagName.toLowerCase() + (el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\s+/).join(".") : "");
  const box = (r) => "[" + Math.round(r.left) + "," + Math.round(r.top) + " " + Math.round(r.width) + "x" + Math.round(r.height) + "]";

  /* 1 — empty-text: a text slot on screen with nothing in it. */
  const SLOTS = ".phase-strip, .gate-note, .xscribe-count, .kami-progress-count, .kami-blocked span, .kami-bubble, [data-text-slot]";
  const empty = [...document.querySelectorAll(SLOTS)].filter((el) => shown(el) && !(el.innerText || "").trim());
  if (empty.length) out.push({ id: "empty-text", detail: empty.slice(0, 4).map(name).join(", ") + " on screen with no text" });

  /* The visually-hidden pattern (.sr-only): the element or an ancestor is
     clipped to nothing — clip: rect(0 0 0 0), clip-path: inset(50%), or a
     <=1x1 box with overflow hidden. Narrow on purpose: an ordinary text box
     that is merely too small is still a finding. */
  function visuallyHidden(el) {
    for (let p = el; p && p !== document.body; p = p.parentElement) {
      const cs = getComputedStyle(p);
      if (/^rect\(0(px)?,? 0(px)?,? 0(px)?,? 0(px)?\)$/.test(cs.clip)) return true;
      if (cs.clipPath === "inset(50%)") return true;
      const r = p.getBoundingClientRect();
      if (r.width <= 1 && r.height <= 1 && cs.overflow === "hidden") return true;
    }
    return false;
  }

  /* 2 — clipped-text: every visible run of text, measured by its own glyph
     extent (a Range), must fit its overflow box and the window. */
  const clippedTxt = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t = walker.nextNode(); t; t = walker.nextNode()) {
    if (!t.nodeValue.trim()) continue;
    const el = t.parentElement;
    if (!el || !shown(el)) continue;
    // yap24-PILL2: screen-reader-only text (Y5-J's .sr-only accessible
    // name) is clipped to a 1x1 box ON PURPOSE — it is never drawn, so it
    // can never be "cut off". Without this every phase failed the matrix
    // from the day Y5-J landed.
    if (visuallyHidden(el)) continue;
    const range = document.createRange();
    range.selectNodeContents(t);
    const r = range.getBoundingClientRect();
    if (r.width <= 0) continue;
    const label = JSON.stringify(t.nodeValue.trim().slice(0, 40));
    if (r.left < -tol || r.right > W + tol || r.top < -tol || r.bottom > H + tol) {
      clippedTxt.push(label + " in " + name(el) + " " + box(r) + " leaves the " + W + "x" + H + " window");
      continue;
    }
    if (el.scrollWidth > el.clientWidth + tol && el.clientWidth > 0) {
      const ox = getComputedStyle(el).overflowX;
      if (ox !== "visible") { clippedTxt.push(label + " in " + name(el) + " scrollWidth=" + el.scrollWidth + " > clientWidth=" + el.clientWidth); continue; }
    }
    const c = clippedByContainer(el);
    if (c) {
      const cr = c.getBoundingClientRect();
      if (r.left < cr.left - tol || r.right > cr.right + tol)
        clippedTxt.push(label + " in " + name(el) + " " + box(r) + " cut by " + name(c) + " " + box(cr));
    }
  }
  if (clippedTxt.length) out.push({ id: "clipped-text", detail: clippedTxt.slice(0, 4).join("; ") });

  /* 3 — overflow: a visible box past the window edge (the window clips it). */
  const strays = [];
  for (const el of document.body.querySelectorAll("*")) {
    if (WINDOW.has(el) || !shown(el)) continue;
    const r = el.getBoundingClientRect();
    if (r.left >= -tol && r.right <= W + tol && r.top >= -tol && r.bottom <= H + tol) continue;
    if (clippedByContainer(el)) continue;
    strays.push(name(el) + " " + box(r));
    if (strays.length >= 4) break;
  }
  if (strays.length) out.push({ id: "overflow", detail: "box(es) outside the " + W + "x" + H + " window: " + strays.join("; ") });

  /* 4 — yap-1106-one-pill: the ONE layout, judged against what was painted. */
  const L = window.__YAP_PILL_LAYOUT__;
  if (L && !L.legacy) {
    const MARGIN = 10; // pill/state.ts PANEL_MARGIN
    const REACH = 12;  // pill/state.ts CLOCK_REACH_PT
    const rectBox = (el) => { const r = el.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; };
    const overlap = (a, b) => a.x < b.x + b.w - 0.5 && b.x < a.x + a.w - 0.5 && a.y < b.y + b.h - 0.5 && b.y < a.y + a.h - 0.5;
    const gap = (a, b) => Math.hypot(Math.max(0, b.x - (a.x + a.w), a.x - (b.x + b.w)), Math.max(0, b.y - (a.y + a.h), a.y - (b.y + b.h)));
    const inside = (a, b) => a.x >= b.x - 1 && a.y >= b.y - 1 && a.x + a.w <= b.x + b.w + 1 && a.y + a.h <= b.y + b.h + 1;
    const painted = [["character", L.char, L.char]];
    const slot = (n, sel, want) => { const el = document.querySelector(sel); if (el && shown(el)) painted.push([n, rectBox(el), want]); };
    slot("hint", ".phase-strip", L.hint);
    slot("bubble", ".kami-bubble", L.bubble);
    slot("clock", "[data-pill-slot=clock]", L.clock);
    slot("progress", ".kami-progress", L.progress);
    const ov = [];
    for (let i = 0; i < painted.length; i++) for (let j = i + 1; j < painted.length; j++)
      if (overlap(painted[i][1], painted[j][1])) ov.push(painted[i][0] + "/" + painted[j][0] + " " + box({ left: painted[j][1].x, top: painted[j][1].y, width: painted[j][1].w, height: painted[j][1].h }));
    if (ov.length) out.push({ id: "one-pill-overlap", detail: ov.join("; ") });
    const off = painted.filter(([n, b, want]) => n !== "character" && (!want || !inside(b, want))).map(([n, b]) => n + " " + box({ left: b.x, top: b.y, width: b.w, height: b.h }));
    if (off.length) out.push({ id: "slot-off-layout", detail: off.join("; ") });
    const clock = painted.find((p) => p[0] === "clock");
    if (want && want.meeting === "recording" && !clock) out.push({ id: "clock-detached", detail: "a meeting is recording but no clock slot is on screen" });
    if (clock && gap(clock[1], L.char) > REACH) out.push({ id: "clock-detached", detail: "clock " + gap(clock[1], L.char).toFixed(1) + "pt from the character" });
    const boxes = [L.char, L.bubble, L.hint, L.clock, L.progress, L.card].filter(Boolean);
    const ux = Math.min(...boxes.map((b) => b.x)), uy = Math.min(...boxes.map((b) => b.y));
    const uw = Math.max(...boxes.map((b) => b.x + b.w)) - ux, uh = Math.max(...boxes.map((b) => b.y + b.h)) - uy;
    const wantW = Math.ceil(uw + 2 * MARGIN), wantH = Math.ceil(uh + 2 * MARGIN);
    if (Math.abs(ux - MARGIN) > 0.5 || Math.abs(uy - MARGIN) > 0.5 || L.panel.w !== wantW || L.panel.h !== wantH || Math.abs(W - L.panel.w) > tol || Math.abs(H - L.panel.h) > tol)
      out.push({ id: "panel-not-union", detail: "window " + W + "x" + H + ", layout panel " + L.panel.w + "x" + L.panel.h + ", union+margin " + wantW + "x" + wantH });
  }

  const strip = document.querySelector(".phase-strip");
  return { findings: out, line: strip && shown(strip) ? (strip.innerText || "").trim() : "" };
}
`;

/* ── build + serve the matrix bundle ────────────────────────────────────── */

function freePort() {
  if (process.env.YAP_PILL_PORT) return Promise.resolve(Number(process.env.YAP_PILL_PORT));
  return new Promise((res, rej) => {
    const s = createServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

function buildBundle(work) {
  const dist = join(work, "dist");
  const log = join(work, "vite-build.log");
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("pill-matrix: " + vite + " missing — run npm ci in desktop/ (ENVIRONMENT failure, not a UI defect)");
    process.exit(3);
  }
  console.log("pill-matrix: building float.html with YAP_PILL_MATRIX=1 -> " + dist);
  const fd = openSync(log, "w");
  const r = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], {
    cwd: DESKTOP,
    env: { ...process.env, YAP_PILL_MATRIX: "1" },
    stdio: ["ignore", fd, fd],
  });
  if (r.status !== 0) {
    console.error("pill-matrix: vite build exited " + r.status + "\n" + readFileSync(log, "utf8"));
    process.exit(3);
  }
  return dist;
}

async function servePreview(dist, work) {
  const port = await freePort();
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  const fd = openSync(join(work, "vite-preview.log"), "w");
  const proc = spawn(vite, ["preview", "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], {
    cwd: DESKTOP,
    stdio: ["ignore", fd, fd],
  });
  const base = "http://127.0.0.1:" + port;
  for (let i = 0; i < 120; i++) {
    try { if ((await fetch(base + "/float.html")).ok) return { proc, base }; } catch {}
    if (proc.exitCode !== null) break;
    await sleep(250);
  }
  proc.kill("SIGKILL");
  console.error("pill-matrix: vite preview never came up on " + base + "\n" + readFileSync(join(work, "vite-preview.log"), "utf8"));
  process.exit(3);
}

/* ── one page, navigated once per capture ───────────────────────────────── */

async function openPage(cdp, tones) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
  // A flat dark desktop behind the transparent window, so white copy reads.
  await cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 46, g: 49, b: 56, a: 1 } }, s);
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: fixtureScript(tones) + "\n" + TAURI_SHIM }, s);
  return s;
}

/* yap-1106-one-pill — size the window to the layout's panel, the way
 * float_pill.rs set_panel_frame sizes the NSPanel, then let it re-lay out. */
async function fitPanel(cdp, s) {
  const panel = await evaluate(cdp, s, "(window.__YAP_PILL_LAYOUT__ && window.__YAP_PILL_LAYOUT__.panel) || null").catch(() => null);
  if (!panel) return { w: W, h: H };
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: panel.w, height: panel.h, deviceScaleFactor: SCALE, mobile: false }, s);
  await sleep(Math.min(SETTLE_MS, 400));
  return { w: panel.w, h: panel.h };
}

async function settle(cdp, s, wantId) {
  const deadline = Date.now() + 8000;
  for (;;) {
    const st = await evaluate(cdp, s, `document.readyState + "|" + (document.documentElement.dataset.phase || "")`).catch(() => "");
    if (st.startsWith("complete|") && (!wantId || st.endsWith("|" + wantId))) break;
    if (Date.now() > deadline) break; // judged as phase-not-applied below
    await sleep(100);
  }
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(SETTLE_MS); // let the strip fade and the capsule expand finish
}

async function shoot(cdp, s, file) {
  const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
}

/* ── contact sheet: every capture on one image ──────────────────────────── */

async function contactSheet(cdp, rows, outDir) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const cols = DOCKS.length * CHARACTERS.length * MEETINGS.length;
  const CW = Math.max(W, ...rows.map((r) => r.w || W)), CH = Math.max(H, ...rows.map((r) => r.h || H));
  const width = 24 + 150 + cols * (CW + 12);
  const height = 60 + Math.ceil(rows.length / cols) * (CH + 30);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, s);
  const head = ["", ...CHARACTERS.flatMap((c) => DOCKS.flatMap((d) => MEETINGS.map((m) => c + " · " + d + (m === "none" ? "" : " · meeting"))))];
  const byPhase = new Map();
  for (const r of rows) (byPhase.get(r.id) || byPhase.set(r.id, []).get(r.id)).push(r);
  let html = '<html><body style="margin:12px;background:#1d1f24;color:#e6e6e6;font:12px -apple-system,system-ui,sans-serif">' +
    '<table style="border-collapse:separate;border-spacing:6px"><tr>' + head.map((h) => "<th>" + h + "</th>").join("") + "</tr>";
  for (const [id, list] of byPhase) {
    html += '<tr><td style="width:150px;vertical-align:middle"><b>' + id + "</b></td>";
    for (const c of CHARACTERS) for (const d of DOCKS) for (const m of MEETINGS) {
      const r = list.find((x) => x.character === c && x.dock === d && x.meeting === m);
      const img = r ? "data:image/png;base64," + readFileSync(r.shot).toString("base64") : "";
      const ring = r && r.findings.length ? "outline:2px solid #ef4444" : "";
      html += '<td style="vertical-align:top"><img style="width:' + (r ? r.w : W) + "px;height:" + (r ? r.h : H) + "px;" + ring + '" src="' + img + '"></td>';
    }
    html += "</tr>";
  }
  html += "</table></body></html>";
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Page.navigate", { url: "about:blank" }, s);
  await evaluate(cdp, s, "document.open();document.write(" + JSON.stringify(html) + ");document.close();true");
  await sleep(300);
  // Trim the sheet to the table it actually drew.
  const tall = await evaluate(cdp, s, "Math.ceil(document.body.getBoundingClientRect().bottom) + 12");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: tall, deviceScaleFactor: 1, mobile: false }, s);
  const file = join(outDir, "contact-sheet.png");
  await shoot(cdp, s, file);
  return file;
}

/* ── main ───────────────────────────────────────────────────────────────── */

async function run() {
  const work = mkdtempSync(join(tmpdir(), "yap-pill-matrix-"));
  const outDir = resolve(ARGS.out || mkdtempSync(join(tmpdir(), "yap-pill-shots-")));
  /* Y7-B — the one in-repo exception is docs/smoke-shots/, which .gitignore
     excludes: smoke-windowed.sh files the pill matrix beside the view captures
     of the same run, and none of it can reach a commit. */
  const SMOKE_SHOTS = join(ROOT, "docs", "smoke-shots") + "/";
  if ((outDir === ROOT || outDir.startsWith(ROOT + "/")) && !outDir.startsWith(SMOKE_SHOTS)) {
    console.error("pill-matrix: --out " + outDir + " is inside the repo. The PNGs are PR evidence, never a commit — use a scratch dir.");
    return 2;
  }
  mkdirSync(outDir, { recursive: true });
  const dist = buildBundle(work);
  const { proc: preview, base } = await servePreview(dist, work);
  let chrome;
  try {
    chrome = await openChrome(CHROME, "yap-pill-matrix");
  } catch (e) {
    preview.kill("SIGKILL");
    console.error("pill-matrix: " + e.message);
    return 3;
  }
  const { proc, cdp, ws, profile } = chrome;
  const captures = [];
  try {
    /* Discovery: the phase list comes off the page, from the pill's own table. */
    let s = await openPage(cdp, {});
    await cdp.send("Page.navigate", { url: base + "/float.html" }, s);
    await settle(cdp, s, null);
    const phases = await evaluate(cdp, s, "window.__YAP_PILL_PHASES__ || null");
    if (!Array.isArray(phases) || phases.length === 0) {
      console.error("pill-matrix: the bundle published no __YAP_PILL_PHASES__ — it was not built in matrix mode (INSTRUMENT failure)");
      return 3;
    }
    await cdp.send("Page.close", {}, s).catch(() => {});

    if (ARGS.selfTest) return await selfTest(cdp, base, phases);

    let todo = phases;
    if (ARGS.only) {
      // A license scene (yap24-PILL1) rides the idle phase; `--only idle` means the plain idle row.
      todo = phases.filter((p) => p.id === ARGS.only || (p.phase === ARGS.only && !p.license && !p.query));
      if (!todo.length) {
        console.error("pill-matrix: --only " + ARGS.only + " is not a phase. Known: " + phases.map((p) => p.id).join(", "));
        return 2;
      }
    }
    const tones = Object.fromEntries(phases.flatMap((p) => [[p.phase, p.tone], [p.id, p.tone]]));
    s = await openPage(cdp, tones);
    console.log("pill-matrix: " + todo.length + " phase(s) x " + DOCKS.length + " docks x " + CHARACTERS.length + " characters x " + MEETINGS.length + " meeting states, legacy " + W + "x" + H + " / one-pill sized to its layout, @" + SCALE + "x -> " + outDir);
    for (const p of todo) {
      for (const character of CHARACTERS) {
        for (const dock of DOCKS) {
          for (const meeting of MEETINGS) {
            const q = "?phase=" + encodeURIComponent(p.phase) + "&pill=" + character + "&dock=" + dock + "&meeting=" + meeting + (p.license ? "&license=" + encodeURIComponent(p.license) : "") + (p.query ? "&" + p.query : "");
            cdp.errors.set(s, []);
            await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
            await cdp.send("Page.navigate", { url: base + "/float.html" + q }, s);
            await settle(cdp, s, p.phaseId || p.id);
            const size = await fitPanel(cdp, s);
            await evaluate(cdp, s, DETECTORS + ";true");
            const res = await evaluate(cdp, s, "__pillDetect(" + JSON.stringify({ id: p.id, phaseId: p.phaseId, license: p.license, offer: p.offer, chip: p.chip, pill: character, dock, meeting }) + ")");
            const file = join(outDir, p.id + "__" + character + "__" + dock + "__" + meeting + ".png");
            await shoot(cdp, s, file);
            const pageErrors = (cdp.errors.get(s) || []).slice(0, 4);
            captures.push({ id: p.id, phase: p.phase, tone: p.tone, character, dock, meeting, w: size.w, h: size.h, line: res.line, findings: res.findings, pageErrors, shot: file });
            const tag = res.findings.length ? "FAIL" : "ok  ";
            console.log(tag + " " + p.id.padEnd(14) + character.padEnd(8) + dock.padEnd(7) + meeting.padEnd(10) + (size.w + "x" + size.h).padEnd(8) + JSON.stringify(res.line).padEnd(34) + file);
            for (const f of res.findings) console.log("       " + f.id + " — " + f.detail);
            for (const e of pageErrors) console.log("       page: " + e.split("\n")[0]);
          }
        }
      }
    }
    const sheet = await contactSheet(cdp, captures, outDir);
    const failed = captures.filter((c) => c.findings.length);
    writeFileSync(join(outDir, "result.json"), JSON.stringify({ window: W + "x" + H + "@" + SCALE + "x", only: ARGS.only, phases: todo.map((p) => p.id), captures: captures.length, failing: failed.length, contactSheet: sheet, captures_: captures }, null, 2));
    console.log("");
    console.log("pill-matrix: " + captures.length + " captures, " + failed.length + " carrying findings");
    console.log("pill-matrix: PNGs + result.json + contact-sheet.png in " + outDir + " (PR evidence — never commit them)");
    return failed.length ? 1 : 0;
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
    preview.kill("SIGKILL");
    await sleep(200);
    rmSync(profile, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
}

/* ── --self-test-must-fail ──────────────────────────────────────────────── */
/*
 * Renders one REAL capture, then breaks it three ways in the page — an empty
 * phase line forced visible, a line of text wider than the window, a box hung
 * off the left edge — and requires every detector to fire. A satisfied
 * self-test exits 1 BY DESIGN (the run carries a deliberate break); a blind
 * detector exits 9.
 */
async function selfTest(cdp, base, phases) {
  const expect = ["empty-text", "clipped-text", "overflow"];
  const p = phases[0];
  const s = await openPage(cdp, { [p.phase]: p.tone });
  await cdp.send("Page.navigate", { url: base + "/float.html?phase=" + p.phase + "&pill=classic&dock=bottom" }, s);
  await settle(cdp, s, p.id);
  await evaluate(cdp, s, DETECTORS + ";true");
  const before = await evaluate(cdp, s, "__pillDetect(" + JSON.stringify({ id: p.id, pill: "classic", dock: "bottom" }) + ")");
  await evaluate(cdp, s, String.raw`(() => {
    const strip = document.querySelector(".phase-strip");
    strip.textContent = ""; strip.style.cssText = "opacity:1;transition:none;min-width:40px;min-height:10px;left:20px;top:10px;bottom:auto;transform:none";
    const wide = document.createElement("span");
    wide.textContent = "this sentence is far too long for a three hundred point pill and runs off the edge";
    wide.style.cssText = "position:absolute;left:10px;top:100px;white-space:nowrap;color:#fff;font:13px system-ui";
    document.body.appendChild(wide);
    const hang = document.createElement("div");
    hang.style.cssText = "position:absolute;left:-40px;top:40px;width:80px;height:20px;background:#f00";
    document.body.appendChild(hang);
    return true;
  })()`);
  const res = await evaluate(cdp, s, "__pillDetect(null)");
  const got = res.findings.map((f) => f.id);
  const missed = expect.filter((e) => !got.includes(e));
  console.log("self-test: real capture before the break: " + (before.findings.length ? before.findings.map((f) => f.id).join(", ") : "clean"));
  for (const f of res.findings) console.log("  detector fired: " + f.id + " — " + f.detail);
  if (missed.length) {
    console.error("SELF-TEST VACUOUS — detector stayed blind to: " + missed.join(", "));
    return 9;
  }
  /* yap-1106-one-pill — the one-pill detectors, on a real Yappy capture with a
     meeting: the clock dropped onto Yappy's face and a frame that is not the
     union of the content. */
  const expect2 = ["one-pill-overlap", "slot-off-layout", "panel-not-union"];
  const s2 = await openPage(cdp, { [p.phase]: p.tone });
  await cdp.send("Page.navigate", { url: base + "/float.html?phase=" + p.phase + "&pill=yappy&dock=left&meeting=recording" }, s2);
  await settle(cdp, s2, p.id);
  await fitPanel(cdp, s2);
  await evaluate(cdp, s2, DETECTORS + ";true");
  const clean = await evaluate(cdp, s2, "__pillDetect(null)");
  console.log("self-test: one-pill capture before the break: " + (clean.findings.length ? clean.findings.map((f) => f.id).join(", ") : "clean"));
  await evaluate(cdp, s2, String.raw`(() => {
    const L = window.__YAP_PILL_LAYOUT__;
    if (!L) return false;
    const c = document.querySelector("[data-pill-slot=clock]");
    if (c) { c.style.left = L.char.x + "px"; c.style.top = L.char.y + "px"; }
    window.__YAP_PILL_LAYOUT__ = Object.assign({}, L, { panel: { w: L.panel.w + 40, h: L.panel.h } });
    return true;
  })()`);
  const res2 = await evaluate(cdp, s2, "__pillDetect(null)");
  const got2 = res2.findings.map((f) => f.id);
  for (const f of res2.findings) console.log("  detector fired: " + f.id + " — " + f.detail);
  const missed2 = expect2.filter((e) => !got2.includes(e));
  if (missed2.length) {
    console.error("SELF-TEST VACUOUS — one-pill detector stayed blind to: " + missed2.join(", "));
    return 9;
  }
  console.error("self-test-must-fail: all " + (expect.length + expect2.length) + " detectors fired on the synthetic breaks; exiting 1 BY DESIGN.");
  return 1;
}

process.exit(await run());
