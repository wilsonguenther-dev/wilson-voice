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
    for (const [w, h] of SIZES) {
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
    if (existsSync(ARTIFACT)) await artifactCrop(cdp);
    else console.log(`sidebar-shots: no design artifact at ${ARTIFACT}; skipped the artifact crop`);
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
