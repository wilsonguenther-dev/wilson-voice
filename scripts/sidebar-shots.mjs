#!/usr/bin/env node
/**
 * yap-1107-sidebar-version-line-fits — proves the sidebar's version line (and
 * everything else in the 220 px rail) never runs past the sidebar, for the
 * shipped version AND for long test-build versions. Headless only
 * (scripts/headless-chrome.mjs, --headless=new, deviceScaleFactor 2); no npm
 * dependencies.
 *
 * The INTEGRATOR runs it against bundles it already built:
 *
 *   (cd desktop && npm run build)                         # this branch -> desktop/dist
 *   # optional, for the before/after crops: the same build of origin/main
 *   node scripts/sidebar-shots.mjs --dist desktop/dist [--before-dist <origin-main>/desktop/dist] \
 *     --out docs/loop/evidence/yap-1107-sidebar-version-line-fits
 *
 * __APP_VERSION__ is a compile-time constant, so it is FORCED at serve time:
 * every quoted copy of the baked version (desktop/package.json "version", or
 * --baked-version) in the served .js is rewritten to the scene's version. The
 * page then asserts the brand line really shows the forced version, so a
 * rewrite that missed is an INSTRUMENT failure, never a silent pass.
 *
 * Scenes: versions "0.9.4" (shipped), "0.9.4-loop3.8" (the build Wilson walked)
 * and a 40-character worst case, at 1190x773 and 900x600.
 *   Test versions (acceptance 1, 2): .brand-tag right <= the sidebar inner right
 *   edge (rect right - padding-right - border-right), scrollWidth <= clientWidth
 *   for .brand-block and .sidebar, every nav item / label / count, Dictate and
 *   the model ribbon inside the rail too; the full version stays in the DOM text,
 *   and the title attribute is set exactly when the version is clipped.
 *   Shipped "0.9.4" (acceptance 3): one line, text exactly
 *   "v0.9.4 · local · private", the glyphs inside the sidebar's border, and,
 *   with --before-dist, a brand-block crop byte-identical to origin/main's.
 * PNGs (acceptance 4): <out>/{after,before}-<scene>-<WxH>.png (full window) and
 * <out>/brand-{after,before}-<scene>-<WxH>.png (brand-block crop).
 *
 * yap-1107-sidebar-collapses-to-icons — the ICON RAIL scenes (always run after
 * the version scenes; `--only-rail` skips the version scenes). PNGs at 2x go to
 * --rail-out (default docs/loop/evidence/yap-1107-sidebar-collapses-to-icons):
 *   rail-expanded-1190x773.png        acceptance 1 (220 px sidebar, toggle visible)
 *   rail-collapsed-1190x773.png       acceptance 1 + 2 (56 px rail, remembered "1")
 *   rail-hover-tooltip-1190x773.png   acceptance 2 (tooltip on hover)
 *   rail-focus-tooltip-1190x773.png   acceptance 2 (tooltip on keyboard focus)
 *   rail-narrow-899x600.png, rail-narrow-720x520.png   acceptance 3 (automatic rail)
 *   sidebar-<scene>.png crops, artifact-home-sidebar.png (the approved page's rail)
 * and asserts: the first .shell ever rendered already wears .rail when "1" is
 * stored (no flash); no text overflows the rail (every visible label, count,
 * button inside the sidebar's border, scrollWidth <= clientWidth, and the rail
 * never taller than the window at 720x520); every nav item has its icon and an
 * accessible name; the active item, the count dots, the brand mark and Dictate
 * are visible in the rail; the main column is wider by EXACTLY 220 - 56 = 164 px
 * collapsed vs expanded; the hover/focus tooltip text equals data-tip and fits
 * the window; Ctrl-Cmd-S toggles and writes localStorage; Cmd-1..Cmd-7 switch
 * the view; Tab walks the nav in the same order expanded and collapsed.
 *
 * Exit 0 all good, 1 a layout assertion failed, 3 instrument failure. The
 * localhost servers, Chrome and its temp profile are stopped/deleted before exit.
 */
import { createServer } from "node:http";
import { readFileSync, rmSync, writeFileSync, mkdirSync, existsSync, statSync } from "node:fs";
import { extname, join, resolve, normalize } from "node:path";
import { TAURI_SHIM, evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf(k);
  return i > 0 ? process.argv[i + 1] : d;
};
const ROOT = new URL("..", import.meta.url).pathname;
const DIST = resolve(arg("--dist", join(ROOT, "desktop/dist")));
const BEFORE = arg("--before-dist", null) ? resolve(arg("--before-dist")) : null;
const OUT = resolve(arg("--out", join(ROOT, "docs/loop/evidence/yap-1107-sidebar-version-line-fits")));
const BAKED = arg("--baked-version", JSON.parse(readFileSync(join(ROOT, "desktop/package.json"), "utf8")).version);
/* The approved design page, for an artifact crop of its sidebar brand next to ours. */
const ARTIFACT = resolve(
  arg("--artifact", join(process.env.HOME || "", "Obsidian/Wilson-Brain/Projects/Yap/design-2026-11-06/home.html")),
);
const RAIL_OUT = resolve(arg("--rail-out", join(ROOT, "docs/loop/evidence/yap-1107-sidebar-collapses-to-icons")));
const ONLY_RAIL = process.argv.includes("--only-rail");
const RAIL_KEY = "yap.sidebar.collapsed";
const EXPANDED_PX = 220;
const RAIL_PX = 56;
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const WORST = "0.9.4-loop3.8+integration.099403a".padEnd(40, "0");
const SCENES = [
  ["release", "0.9.4"],
  ["loop3", "0.9.4-loop3.8"],
  ["worst40", WORST],
];
const SIZES = [
  [1190, 773],
  [900, 600],
];
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2", ".png": "image/png", ".svg": "image/svg+xml", ".json": "application/json", ".wasm": "application/wasm" };

const FIXTURES = JSON.parse(readFileSync(join(ROOT, "scripts/smoke-fixtures.json"), "utf8"));
delete FIXTURES._README;

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const BAKED_RE = new RegExp("([\"'`])" + escapeRe(BAKED) + "\\1", "g");

/** A static server for one dist whose .js has __APP_VERSION__ swapped per request (?v=...). */
async function serve(dist) {
  let forced = BAKED;
  let rewrites = 0;
  const server = createServer((req, res) => {
    const rel = normalize(decodeURIComponent((req.url || "/").split("?")[0])).replace(/^(\.\.[/\\])+/, "");
    const file = join(dist, rel === "/" ? "index.html" : rel);
    if (!file.startsWith(dist) || !existsSync(file) || statSync(file).isDirectory()) {
      res.writeHead(404).end();
      return;
    }
    const ext = extname(file);
    let body = readFileSync(file);
    if (ext === ".js") {
      const src = body.toString("utf8");
      const out = src.replace(BAKED_RE, (_m, q) => {
        rewrites++;
        return q + forced + q;
      });
      body = Buffer.from(out, "utf8");
    }
    res.writeHead(200, { "content-type": TYPES[ext] || "application/octet-stream", "cache-control": "no-store" });
    res.end(body);
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  return {
    base: `http://127.0.0.1:${server.address().port}`,
    setVersion(v) {
      forced = v;
      rewrites = 0;
    },
    rewrites: () => rewrites,
    close: () => server.close(),
  };
}

/* Runs in the page: every measurement the assertions need, as plain data. */
const MEASURE = String.raw`(() => {
  const sb = document.querySelector(".sidebar");
  if (!sb) return { missing: "no .sidebar (onboarding overlay or boot error?)" };
  const cs = getComputedStyle(sb);
  const r = sb.getBoundingClientRect();
  const inner = r.right - parseFloat(cs.paddingRight) - parseFloat(cs.borderRightWidth);
  const border = r.right - parseFloat(cs.borderRightWidth);
  const box = (el) => {
    const b = el.getBoundingClientRect();
    return { right: b.right, height: b.height, scrollWidth: el.scrollWidth, clientWidth: el.clientWidth };
  };
  const bb = sb.querySelector(".brand-block");
  const tag = sb.querySelector(".brand-tag");
  const ver = sb.querySelector(".brand-ver");
  let glyphRight = null;
  if (tag) {
    const range = document.createRange();
    range.selectNodeContents(tag);
    glyphRight = range.getBoundingClientRect().right;
  }
  const parts = [];
  sb.querySelectorAll(".brand-tag > span, .nav-item, .nav-label, .nav-item .count, .dictate-side, .sidebar-foot .model-ribbon")
    .forEach((el) => {
      const b = box(el);
      if (b.height === 0) return; // display:none (the narrow strip hides the tag)
      parts.push({ name: (el.className || el.tagName) + ":" + (el.textContent || "").trim().slice(0, 24), ...b });
    });
  const lh = tag ? parseFloat(getComputedStyle(tag).lineHeight) || 16 : 16;
  /* Any "Yap <version>" text outside the sidebar (the page footer, when the base has one). */
  const footers = [...document.querySelectorAll("footer, [class*=foot]")]
    .filter((el) => !sb.contains(el) && /\bYap v?\d/.test(el.textContent || ""))
    .map((el) => ({ text: (el.textContent || "").replace(/\s+/g, " ").trim().slice(0, 120), scrollWidth: el.scrollWidth, clientWidth: el.clientWidth }));
  return {
    inner, border, sidebar: box(sb), brandBlock: bb && box(bb), tag: tag && box(tag),
    tagText: tag ? tag.textContent : null, lineHeight: lh, glyphRight,
    ver: ver && { ...box(ver), title: ver.getAttribute("title") },
    /* The crop spans the WHOLE rail width (not just the brand block), so a line
       that crowds into the sidebar padding is inside the before/after crop. */
    brandRect: bb && (() => { const b = bb.getBoundingClientRect(); return { x: r.x, y: b.y, width: r.width, height: b.height }; })(),
    parts, footers,
  };
})()`;

async function capture(cdp, srv, version, w, h) {
  srv.setVersion(version);
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  try {
    await cdp.send("Page.enable", {}, s);
    await cdp.send("Runtime.enable", {}, s);
    await cdp.send("Network.enable", {}, s);
    await cdp.send("Network.setCacheDisabled", { cacheDisabled: true }, s);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 2, mobile: false }, s);
    await cdp.send(
      "Page.addScriptToEvaluateOnNewDocument",
      { source: "window.__YAP_FIXTURES__ = " + JSON.stringify(FIXTURES) + ";\n" + TAURI_SHIM },
      s,
    );
    await cdp.send("Page.navigate", { url: `${srv.base}/index.html?v=${encodeURIComponent(version)}` }, s);
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      const ready = await evaluate(cdp, s, "document.readyState === 'complete' && !!document.querySelector('.sidebar .brand-tag')").catch(() => false);
      if (ready) break;
      await sleep(100);
    }
    await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
    await sleep(400);
    const m = await evaluate(cdp, s, MEASURE);
    const full = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
    let crop = null;
    if (m.brandRect) {
      const c = m.brandRect;
      crop = await cdp.send("Page.captureScreenshot", { format: "png", clip: { x: c.x, y: c.y, width: c.width, height: c.height, scale: 1 } }, s);
    }
    return { m, full: full.data, crop: crop && crop.data };
  } finally {
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  }
}

/** The approved Home page's sidebar brand (".brand", the first one), cropped at 2x. */
async function artifactCrop(cdp) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  try {
    await cdp.send("Page.enable", {}, s);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1190, height: 773, deviceScaleFactor: 2, mobile: false }, s);
    await cdp.send("Page.navigate", { url: "file://" + ARTIFACT }, s);
    await sleep(1500);
    const c = await evaluate(cdp, s, `(() => { const el = document.querySelector(".brand"); if (!el) return null;
      const side = el.parentElement.getBoundingClientRect(); const b = el.getBoundingClientRect();
      return { x: side.x, y: b.y, width: side.width, height: b.height }; })()`);
    if (!c) {
      console.log("sidebar-shots: the design artifact has no .brand; skipped the artifact crop");
      return;
    }
    const shot = await cdp.send("Page.captureScreenshot", { format: "png", clip: { ...c, scale: 1 } }, s);
    writeFileSync(join(OUT, "brand-artifact-home.png"), Buffer.from(shot.data, "base64"));
    console.log(`sidebar-shots artifact  brand crop ${c.width.toFixed(0)}x${c.height.toFixed(0)} -> brand-artifact-home.png`);
  } finally {
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  }
}

/** Layout assertions for one scene; returns failure strings (empty = pass). */
function check(scene, version, m) {
  const bad = [];
  const EPS = 0.5;
  if (m.missing) return [m.missing];
  if (!m.tagText || !m.tagText.includes("v" + version)) {
    bad.push(`INSTRUMENT: brand line shows ${JSON.stringify(m.tagText)}, not v${version} (version rewrite missed)`);
    return bad;
  }
  if (m.sidebar.scrollWidth > m.sidebar.clientWidth) bad.push(`.sidebar scrollWidth ${m.sidebar.scrollWidth} > clientWidth ${m.sidebar.clientWidth}`);
  for (const p of m.parts) {
    if (p.right > m.inner + EPS) bad.push(`${p.name} right ${p.right.toFixed(1)} > sidebar inner right ${m.inner.toFixed(1)}`);
    if (p.scrollWidth > p.clientWidth && !/brand-ver/.test(p.name)) bad.push(`${p.name} scrollWidth ${p.scrollWidth} > clientWidth ${p.clientWidth}`);
  }
  for (const f of m.footers) {
    if (!f.text.includes(version)) bad.push(`footer "${f.text}" does not show the full version ${version}`);
    if (f.scrollWidth > f.clientWidth) bad.push(`footer "${f.text}" overflows (${f.scrollWidth} > ${f.clientWidth})`);
  }
  if (scene === "release") {
    if (m.tagText.replace(/\s+/g, " ").trim() !== `v${version} · local · private`) bad.push(`shipped line changed: ${JSON.stringify(m.tagText)}`);
    if (m.tag.height > m.lineHeight * 1.5) bad.push(`shipped line wrapped (height ${m.tag.height} for line-height ${m.lineHeight})`);
    if (m.glyphRight > m.border + EPS) bad.push(`shipped glyphs end at ${m.glyphRight.toFixed(1)}, past the sidebar border ${m.border.toFixed(1)}`);
    return bad;
  }
  if (m.tag.right > m.inner + EPS) bad.push(`.brand-tag right ${m.tag.right.toFixed(1)} > sidebar inner right ${m.inner.toFixed(1)}`);
  if (m.brandBlock.scrollWidth > m.brandBlock.clientWidth) bad.push(`.brand-block scrollWidth ${m.brandBlock.scrollWidth} > clientWidth ${m.brandBlock.clientWidth}`);
  if (!m.ver) {
    bad.push("test build has no .brand-ver span");
  } else {
    const clipped = m.ver.scrollWidth > m.ver.clientWidth;
    if (clipped && m.ver.title !== "v" + version) bad.push(`version clipped but title is ${JSON.stringify(m.ver.title)}`);
    if (!clipped && m.ver.title != null) bad.push(`version not clipped but carries a title ${JSON.stringify(m.ver.title)}`);
  }
  return bad;
}

/* ── yap-1107-sidebar-collapses-to-icons — icon rail scenes ─────────────── */

/* Runs before the app: seeds the remembered choice and records the class of
   the FIRST .shell ever inserted (a rail that arrives one frame late is a flash). */
const railSeed = (stored) => String.raw`
  try { localStorage.setItem(${JSON.stringify(RAIL_KEY)}, ${JSON.stringify(stored)}); } catch (e) {}
  window.__railFirst = null;
  new MutationObserver((_m, obs) => {
    const sh = document.querySelector(".shell");
    if (sh) { window.__railFirst = sh.className; obs.disconnect(); }
  }).observe(document, { childList: true, subtree: true });
`;

const RAIL_MEASURE = String.raw`(() => {
  const sb = document.querySelector(".sidebar");
  const main = document.querySelector(".main");
  const shell = document.querySelector(".shell");
  if (!sb || !main || !shell) return { missing: "no .shell/.sidebar/.main (onboarding or boot error?)" };
  const r = sb.getBoundingClientRect();
  const cs = getComputedStyle(sb);
  const left = r.left, border = r.right - parseFloat(cs.borderRightWidth);
  const hidden = (el) => { const c = getComputedStyle(el); return c.display === "none" || c.visibility === "hidden" || c.clipPath.startsWith("inset(50%"); };
  const parts = [];
  sb.querySelectorAll(".brand-block, .mark, .brand-text, .nav-item, .nav-label, .nav-item .count, .dictate-side, .rail-toggle, .rail-toggle span, .sidebar-foot .model-ribbon")
    .forEach((el) => {
      const b = el.getBoundingClientRect();
      if (b.width === 0 || b.height === 0 || hidden(el)) return;
      parts.push({ name: (el.className.baseVal ?? el.className) + ":" + (el.textContent || "").trim().slice(0, 20),
        left: b.left, right: b.right, bottom: b.bottom, width: b.width, scrollWidth: el.scrollWidth, clientWidth: el.clientWidth });
    });
  const items = [...sb.querySelectorAll(".nav-item")].map((el) => {
    const b = el.getBoundingClientRect();
    const count = el.querySelector(".count");
    const cb = count && count.getBoundingClientRect();
    return { name: (el.textContent || "").trim(), icon: !!el.querySelector("svg.nav-icon"), active: el.classList.contains("active"),
      w: b.width, h: b.height, bottom: b.bottom, tip: el.getAttribute("data-tip"), count: cb ? { w: cb.width, h: cb.height } : null };
  });
  const box = (sel) => { const el = sb.querySelector(sel); if (!el) return null; const b = el.getBoundingClientRect(); return { w: b.width, h: b.height, left: b.left, right: b.right, bottom: b.bottom, name: el.getAttribute("aria-label") || (el.textContent || "").trim() }; };
  return {
    shellClass: shell.className, railFirst: window.__railFirst, stored: localStorage.getItem(${JSON.stringify(RAIL_KEY)}),
    vw: innerWidth, vh: innerHeight, sidebarW: r.width, mainW: main.getBoundingClientRect().width, left, border,
    sbScroll: { sw: sb.scrollWidth, cw: sb.clientWidth, sh: sb.scrollHeight, ch: sb.clientHeight },
    parts, items, mark: box(".mark"), dictate: box(".dictate-side"), toggle: box(".rail-toggle"),
    h1: (document.querySelector(".main-head h1")?.textContent || "").trim(),
  };
})()`;

/* The tooltip of the element that has hover or focus: its ::after text and the
   width that text needs in the tooltip's font (pseudo-elements have no rect). */
const TIP_MEASURE = (sel) => String.raw`(() => {
  const el = document.querySelector(${JSON.stringify(sel)});
  if (!el) return { missing: "no " + ${JSON.stringify(sel)} };
  const pc = getComputedStyle(el, "::after");
  const content = pc.content;
  const probe = document.createElement("span");
  probe.textContent = el.getAttribute("data-tip") || "";
  probe.style.cssText = "position:fixed;visibility:hidden;white-space:nowrap;font:" + pc.font + ";padding:" + pc.padding + ";border:" + pc.border;
  document.body.appendChild(probe);
  const w = probe.getBoundingClientRect().width;
  probe.remove();
  const b = el.getBoundingClientRect();
  const sb = document.querySelector(".sidebar").getBoundingClientRect();
  return { tip: el.getAttribute("data-tip"), content, tipLeft: b.right + parseFloat(pc.left) - b.width, need: w, vw: innerWidth, sidebarRight: sb.right,
    focused: document.activeElement === el };
})()`;

async function railTarget(cdp, srv, w, h, stored) {
  srv.setVersion(BAKED);
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Network.setCacheDisabled", { cacheDisabled: true }, s).catch(() => {});
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 2, mobile: false }, s);
  await cdp.send("Page.addScriptToEvaluateOnNewDocument",
    { source: railSeed(stored) + "\nwindow.__YAP_FIXTURES__ = " + JSON.stringify(FIXTURES) + ";\n" + TAURI_SHIM }, s);
  await cdp.send("Page.navigate", { url: `${srv.base}/index.html?rail=${stored}` }, s);
  const deadline = Date.now() + 10000;
  while (Date.now() < deadline) {
    if (await evaluate(cdp, s, "document.readyState === 'complete' && !!document.querySelector('.sidebar .nav-item')").catch(() => false)) break;
    await sleep(100);
  }
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(400);
  return { s, close: () => cdp.send("Target.closeTarget", { targetId }).catch(() => {}) };
}

async function shoot(cdp, s, name, sidebarOnly) {
  const full = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(join(RAIL_OUT, `rail-${name}.png`), Buffer.from(full.data, "base64"));
  if (sidebarOnly) {
    const c = await evaluate(cdp, s, `(() => { const b = document.querySelector(".sidebar").getBoundingClientRect();
      return { x: b.x, y: b.y, width: Math.min(innerWidth - b.x, b.width + 220), height: b.height }; })()`);
    const crop = await cdp.send("Page.captureScreenshot", { format: "png", clip: { ...c, scale: 1 } }, s);
    writeFileSync(join(RAIL_OUT, `sidebar-${name}.png`), Buffer.from(crop.data, "base64"));
  }
}

const key = (cdp, s, code, keyName, vk, modifiers) =>
  cdp.send("Input.dispatchKeyEvent", { type: "keyDown", code, key: keyName, windowsVirtualKeyCode: vk, modifiers }, s)
    .then(() => cdp.send("Input.dispatchKeyEvent", { type: "keyUp", code, key: keyName, windowsVirtualKeyCode: vk, modifiers }, s));

/** Tab from the top of the document; the nav item names in focus order. */
async function tabOrder(cdp, s) {
  await evaluate(cdp, s, "(document.activeElement && document.activeElement.blur && document.activeElement.blur(), true)");
  const seen = [];
  for (let i = 0; i < 24; i++) {
    await key(cdp, s, "Tab", "Tab", 9, 0);
    const n = await evaluate(cdp, s, `(() => { const a = document.activeElement; return a && a.classList.contains("nav-item") ? a.querySelector(".nav-label").textContent : null; })()`);
    if (n && !seen.includes(n)) seen.push(n);
  }
  return seen;
}

function railCheck(name, m, { rail }) {
  const bad = [];
  const EPS = 0.5;
  if (m.missing) return [m.missing];
  const isRail = /\brail\b/.test(m.shellClass);
  if (isRail !== rail) bad.push(`.shell is "${m.shellClass}", expected rail=${rail}`);
  if (rail && !/\brail\b/.test(m.railFirst || "")) bad.push(`first .shell was "${m.railFirst}" (rail arrived late: a flash)`);
  const wantW = rail ? RAIL_PX : EXPANDED_PX;
  if (Math.abs(m.sidebarW - wantW) > EPS) bad.push(`sidebar ${m.sidebarW.toFixed(1)} px, expected ${wantW}`);
  if (m.sbScroll.sw > m.sbScroll.cw) bad.push(`.sidebar scrollWidth ${m.sbScroll.sw} > clientWidth ${m.sbScroll.cw}`);
  if (m.sbScroll.sh > m.sbScroll.ch) bad.push(`.sidebar content ${m.sbScroll.sh} px taller than the rail ${m.sbScroll.ch} px (does not fit ${m.vw}x${m.vh})`);
  for (const p of m.parts) {
    if (p.left < m.left - EPS || p.right > m.border + EPS) bad.push(`${p.name} [${p.left.toFixed(1)}, ${p.right.toFixed(1)}] outside the rail [${m.left.toFixed(1)}, ${m.border.toFixed(1)}]`);
    if (p.scrollWidth > p.clientWidth) bad.push(`${p.name} text overflows (${p.scrollWidth} > ${p.clientWidth})`);
    if (p.bottom > m.vh + EPS) bad.push(`${p.name} bottom ${p.bottom.toFixed(1)} below the window ${m.vh}`);
  }
  if (!m.items.length) bad.push("no nav items");
  for (const it of m.items) {
    if (!it.icon) bad.push(`nav "${it.name}" has no icon`);
    if (!it.name) bad.push("a nav item has no accessible name");
    if (rail && !it.tip) bad.push(`nav "${it.name}" has no tooltip in the rail`);
    if (it.count && (it.count.w < 5 || it.count.h < 5)) bad.push(`nav "${it.name}" count badge not visible (${it.count.w}x${it.count.h})`);
  }
  if (!m.items.some((it) => it.active && it.w > 0)) bad.push("no visible active nav item");
  if (!m.mark || m.mark.w < 20) bad.push("brand mark not visible");
  if (!m.dictate || m.dictate.w < 32 || !m.dictate.name) bad.push("Dictate not reachable/named");
  return bad;
}

async function railScenes(cdp, srv) {
  mkdirSync(RAIL_OUT, { recursive: true });
  const fails = [];
  const report = (name, bad, info) => {
    console.log(`sidebar-shots rail ${name.padEnd(26)} ${bad.length ? "FAIL" : "ok  "} ${info}`);
    for (const b of bad) console.log("    " + b);
    if (bad.length) fails.push(name);
  };
  const info = (m) => (m.missing ? m.missing : `shell="${m.shellClass}" sidebar=${m.sidebarW.toFixed(1)} main=${m.mainW.toFixed(1)} first="${m.railFirst}" stored=${m.stored}`);

  // Expanded, then collapsed (relaunch with "1" stored), at Wilson's window.
  let t = await railTarget(cdp, srv, 1190, 773, "0");
  const exp = await evaluate(cdp, t.s, RAIL_MEASURE);
  await shoot(cdp, t.s, "expanded-1190x773", true);
  let bad = railCheck("expanded", exp, { rail: false });
  if (!exp.missing && !exp.toggle) bad.push("no collapse control in the expanded sidebar");
  const tabExpanded = await tabOrder(cdp, t.s);
  // Ctrl-Cmd-S collapses and is remembered; again expands.
  await key(cdp, t.s, "KeyS", "s", 83, 2 | 4);
  await sleep(300);
  const afterKey = await evaluate(cdp, t.s, RAIL_MEASURE);
  if (!/\brail\b/.test(afterKey.shellClass || "") || afterKey.stored !== "1") bad.push(`Ctrl-Cmd-S did not collapse+remember (shell="${afterKey.shellClass}", stored=${afterKey.stored})`);
  await key(cdp, t.s, "KeyS", "s", 83, 2 | 4);
  await sleep(300);
  const back = await evaluate(cdp, t.s, RAIL_MEASURE);
  if (/\brail\b/.test(back.shellClass || "") || back.stored !== "0") bad.push(`second Ctrl-Cmd-S did not expand (shell="${back.shellClass}", stored=${back.stored})`);
  report("expanded 1190x773", bad, info(exp) + ` tab=${tabExpanded.length}`);
  // Cmd-1..Cmd-7 in the expanded state.
  const NAMES = exp.items ? exp.items.map((it) => it.name.replace(/\d+$/, "").trim()) : [];
  const navBad = [];
  for (let i = 0; i < 7 && i < NAMES.length; i++) {
    await key(cdp, t.s, `Digit${i + 1}`, String(i + 1), 49 + i, 4);
    await sleep(120);
    const h1 = await evaluate(cdp, t.s, `(document.querySelector(".main-head h1")?.textContent || "").trim()`);
    if (h1 !== NAMES[i]) navBad.push(`expanded Cmd-${i + 1} shows "${h1}", expected "${NAMES[i]}"`);
  }
  await t.close();

  t = await railTarget(cdp, srv, 1190, 773, "1");
  const col = await evaluate(cdp, t.s, RAIL_MEASURE);
  await shoot(cdp, t.s, "collapsed-1190x773", true);
  bad = railCheck("collapsed", col, { rail: true });
  if (!col.missing && !exp.missing) {
    const delta = col.mainW - exp.mainW;
    if (Math.abs(delta - (EXPANDED_PX - RAIL_PX)) > 0.5) bad.push(`main column widened by ${delta.toFixed(2)} px, expected exactly ${EXPANDED_PX - RAIL_PX}`);
  }
  if (!col.missing && !col.toggle) bad.push("no expand control in the rail");
  const tabRail = await tabOrder(cdp, t.s);
  if (JSON.stringify(tabRail) !== JSON.stringify(tabExpanded) || tabRail.length !== (exp.items || []).length)
    bad.push(`Tab order differs: expanded ${JSON.stringify(tabExpanded)} vs rail ${JSON.stringify(tabRail)}`);
  report("collapsed 1190x773", bad, info(col) + ` delta=${col.missing || exp.missing ? "?" : (col.mainW - exp.mainW).toFixed(2)}`);
  for (let i = 0; i < 7 && i < NAMES.length; i++) {
    await key(cdp, t.s, `Digit${i + 1}`, String(i + 1), 49 + i, 4);
    await sleep(120);
    const h1 = await evaluate(cdp, t.s, `(document.querySelector(".main-head h1")?.textContent || "").trim()`);
    if (h1 !== NAMES[i]) navBad.push(`rail Cmd-${i + 1} shows "${h1}", expected "${NAMES[i]}"`);
  }
  report("Cmd-1..Cmd-7 both states", navBad, `${NAMES.slice(0, 7).join(", ")}`);

  // Hover tooltip on Meetings (or the 4th item), then keyboard-focus tooltip.
  const tipBad = [];
  const sel = ".sidebar .nav-item:nth-child(4)";
  const c = await evaluate(cdp, t.s, `(() => { const b = document.querySelector(${JSON.stringify(sel)}).getBoundingClientRect(); return { x: b.x + b.width / 2, y: b.y + b.height / 2 }; })()`);
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: c.x, y: c.y }, t.s);
  await sleep(250);
  const hov = await evaluate(cdp, t.s, TIP_MEASURE(sel));
  await shoot(cdp, t.s, "hover-tooltip-1190x773", true);
  const tipOk = (tm, how) => {
    if (tm.missing) return tipBad.push(tm.missing);
    if (!tm.tip || tm.content !== JSON.stringify(tm.tip)) tipBad.push(`${how}: ::after content ${tm.content} is not data-tip ${JSON.stringify(tm.tip)}`);
    if (tm.sidebarRight + 12 + tm.need > tm.vw) tipBad.push(`${how}: tooltip needs ${tm.need.toFixed(0)} px past the rail, window is ${tm.vw}`);
  };
  tipOk(hov, "hover");
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: c.x + 600, y: c.y }, t.s);
  await evaluate(cdp, t.s, "(document.activeElement && document.activeElement.blur && document.activeElement.blur(), true)");
  for (let i = 0; i < 24; i++) {
    await key(cdp, t.s, "Tab", "Tab", 9, 0);
    if (await evaluate(cdp, t.s, `document.activeElement === document.querySelector(${JSON.stringify(sel)})`)) break;
  }
  await sleep(200);
  const foc = await evaluate(cdp, t.s, TIP_MEASURE(sel));
  if (!foc.focused) tipBad.push("could not Tab to the 4th nav item");
  tipOk(foc, "focus");
  await shoot(cdp, t.s, "focus-tooltip-1190x773", true);
  report("tooltips hover+focus", tipBad, `tip=${JSON.stringify(hov.tip)}`);
  await t.close();

  // Automatic rail under 900 px, expanded choice stored; 900 itself stays expanded.
  for (const [w, h, rail] of [[899, 600, true], [720, 520, true], [900, 600, false]]) {
    t = await railTarget(cdp, srv, w, h, "0");
    const m = await evaluate(cdp, t.s, RAIL_MEASURE);
    const name = `${rail ? "narrow" : "wide"}-${w}x${h}`;
    if (rail) await shoot(cdp, t.s, name, true);
    const b = railCheck(name, m, { rail });
    if (rail && !m.missing && m.toggle) b.push("the collapse control shows under 900 px (the rail is automatic there)");
    report(`${rail ? "auto rail" : "no auto rail"} ${w}x${h}`, b, info(m));
    await t.close();
  }
  return fails.length;
}

/** The approved Home page's whole sidebar, for the before/after/artifact row. */
async function artifactSidebar(cdp) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  try {
    await cdp.send("Page.enable", {}, s);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1190, height: 773, deviceScaleFactor: 2, mobile: false }, s);
    await cdp.send("Page.navigate", { url: "file://" + ARTIFACT }, s);
    await sleep(1500);
    const c = await evaluate(cdp, s, `(() => { const el = document.querySelector(".brand"); if (!el) return null;
      const b = el.parentElement.getBoundingClientRect(); return { x: b.x, y: b.y, width: b.width, height: Math.min(b.height, 773) }; })()`);
    if (!c || !c.width) return console.log("sidebar-shots rail: the design artifact has no .brand sidebar; skipped");
    const shot = await cdp.send("Page.captureScreenshot", { format: "png", clip: { ...c, scale: 1 } }, s);
    writeFileSync(join(RAIL_OUT, "artifact-home-sidebar.png"), Buffer.from(shot.data, "base64"));
    console.log(`sidebar-shots rail artifact sidebar ${c.width.toFixed(0)}x${c.height.toFixed(0)} -> artifact-home-sidebar.png`);
  } finally {
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  }
}

async function main() {
  for (const d of [DIST, BEFORE].filter(Boolean)) {
    if (!existsSync(join(d, "index.html"))) {
      console.error(`sidebar-shots: ${join(d, "index.html")} missing — build the bundle first (INSTRUMENT failure)`);
      return 3;
    }
  }
  if (WORST.length !== 40) {
    console.error(`sidebar-shots: worst-case version is ${WORST.length} chars, not 40 (INSTRUMENT failure)`);
    return 3;
  }
  mkdirSync(OUT, { recursive: true });
  const after = await serve(DIST);
  const before = BEFORE ? await serve(BEFORE) : null;
  let chrome;
  try {
    chrome = await openChrome(CHROME, "yap-sidebar-shots");
  } catch (e) {
    after.close();
    before?.close();
    console.error("sidebar-shots: " + e.message);
    return 3;
  }
  const { proc, cdp, ws, profile } = chrome;
  let failed = 0;
  let instrument = 0;
  try {
    for (const [w, h] of ONLY_RAIL ? [] : SIZES) {
      for (const [scene, version] of SCENES) {
        const size = `${w}x${h}`;
        const a = await capture(cdp, after, version, w, h);
        if (after.rewrites() === 0) {
          console.error(`sidebar-shots ${scene} ${size} INSTRUMENT: no quoted "${BAKED}" in the served .js (pass --baked-version)`);
          instrument++;
          continue;
        }
        writeFileSync(join(OUT, `after-${scene}-${size}.png`), Buffer.from(a.full, "base64"));
        if (a.crop) writeFileSync(join(OUT, `brand-after-${scene}-${size}.png`), Buffer.from(a.crop, "base64"));
        const bad = check(scene, version, a.m);
        if (bad.some((b) => b.startsWith("INSTRUMENT"))) instrument++;
        else if (bad.length) failed++;
        const info = a.m.missing
          ? a.m.missing
          : `tagRight=${a.m.tag.right.toFixed(1)} inner=${a.m.inner.toFixed(1)} glyphRight=${a.m.glyphRight?.toFixed(1)} ` +
            `brandBlock=${a.m.brandBlock.scrollWidth}/${a.m.brandBlock.clientWidth} sidebar=${a.m.sidebar.scrollWidth}/${a.m.sidebar.clientWidth} ` +
            `title=${JSON.stringify(a.m.ver?.title ?? null)} footers=${a.m.footers.length}`;
        console.log(`sidebar-shots ${scene.padEnd(8)} ${size.padEnd(9)} ${bad.length ? "FAIL" : "ok  "} ${info}`);
        for (const b of bad) console.log("    " + b);
        if (before) {
          const b = await capture(cdp, before, version, w, h);
          writeFileSync(join(OUT, `before-${scene}-${size}.png`), Buffer.from(b.full, "base64"));
          if (b.crop) writeFileSync(join(OUT, `brand-before-${scene}-${size}.png`), Buffer.from(b.crop, "base64"));
          if (scene === "release") {
            const sameRect = JSON.stringify(a.m.brandRect) === JSON.stringify(b.m.brandRect);
            const samePixels = !!a.crop && a.crop === b.crop;
            console.log(`sidebar-shots release  ${size.padEnd(9)} ${sameRect && samePixels ? "ok  " : "FAIL"} brand block before/after: rect ${sameRect ? "same" : "DIFFERS"}, pixels ${samePixels ? "identical" : "DIFFER"}`);
            if (!(sameRect && samePixels)) failed++;
          }
        }
      }
    }
    // yap-1107-sidebar-collapses-to-icons — the icon rail scenes.
    failed += await railScenes(cdp, after);
    if (existsSync(ARTIFACT)) await artifactSidebar(cdp);
    if (existsSync(ARTIFACT) && !ONLY_RAIL) await artifactCrop(cdp);
    if (!existsSync(ARTIFACT)) console.log(`sidebar-shots: no design artifact at ${ARTIFACT}; skipped the artifact crop`);
  } finally {
    await cdp.send("Browser.close").catch(() => {});
    ws.close();
    await sleep(300);
    if (proc.exitCode === null) proc.kill("SIGKILL");
    rmSync(profile, { recursive: true, force: true });
    after.close();
    before?.close();
  }
  if (!BEFORE) console.log("sidebar-shots: no --before-dist, so no before shots and no pixel-identical check (acceptance 3 needs it)");
  return instrument ? 3 : failed ? 1 : 0;
}

main().then(
  (code) => process.exit(code),
  (e) => {
    console.error("sidebar-shots: " + (e?.stack || e));
    process.exit(3);
  },
);
