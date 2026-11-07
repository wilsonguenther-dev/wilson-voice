#!/usr/bin/env node
/**
 * Renders Pill page section 16 (page.html, the film-crew option C next to A and
 * B) to PNGs with headless Chrome: the whole section plus one PNG per moment
 * (start, 1:00, 10:00, 1:00:00, dictating mid-meeting, stop). Design artifact
 * only; nothing here ships in the app.
 *
 *   node docs/design/pill-film-crew-2026-11/render.mjs
 *
 * Uses the repo's one headless instrument (scripts/headless-chrome.mjs:
 * --headless=new, its own temp profile, never a window). Renders at device
 * scale 2, the pixel density of a Retina Mac, so "true size" in the PNG is what
 * the pill looks like on Wilson's screen. The page runs with ?still=1, which
 * pins every scene to one moment so two runs give the same pixels. Chrome is
 * stopped by its own PID and its temp profile deleted before this exits.
 * ENV YAP_SMOKE_CHROME (Chromium path, shared with the smoke).
 */
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { evaluate, openChrome, sleep } from "../../../scripts/headless-chrome.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "png");
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const WIDTH = 1760;
const SCALE = 2;

mkdirSync(OUT, { recursive: true });
let chrome = null;
let code = 0;
try {
  chrome = await openChrome(CHROME, "yap-film-crew");
  const { cdp } = chrome;
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: WIDTH, height: 1200, deviceScaleFactor: SCALE, mobile: false }, s);
  await cdp.send("Page.navigate", { url: pathToFileURL(join(HERE, "page.html")).href + "?still=1" }, s);
  let ready = false;
  for (let i = 0; i < 100 && !ready; i++) {
    await sleep(100);
    ready = await evaluate(cdp, s, 'document.readyState === "complete" && document.documentElement.dataset.fcReady === "1"').catch(() => false);
  }
  if (!ready) throw new Error("section 16 never finished drawing (fcReady unset)");
  await evaluate(cdp, s, "document.fonts ? document.fonts.ready.then(() => true) : true");
  const height = await evaluate(cdp, s, "document.documentElement.scrollHeight");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: WIDTH, height, deviceScaleFactor: SCALE, mobile: false }, s);
  await sleep(600);
  const shots = await evaluate(
    cdp,
    s,
    `JSON.stringify([...document.querySelectorAll("[data-shot]")].map(function (el) {
       var r = el.getBoundingClientRect(); return { name: el.dataset.shot, x: r.left, y: r.top + scrollY, w: r.width, h: r.height }; }))`
  );
  const list = JSON.parse(shots);
  if (list.length !== 7) throw new Error("expected 7 shots (section + 6 moments), found " + list.length);
  for (const sh of list) {
    const pad = sh.name === "section" ? 16 : 6;
    const clip = { x: Math.max(0, sh.x - pad), y: Math.max(0, sh.y - pad), width: sh.w + pad * 2, height: sh.h + pad * 2, scale: 1 };
    const png = await cdp.send("Page.captureScreenshot", { format: "png", clip, captureBeyondViewport: true }, s);
    const file = join(OUT, "film-crew-" + sh.name + ".png");
    writeFileSync(file, Buffer.from(png.data, "base64"));
    console.log("render wrote " + file.slice(HERE.length + 1) + " " + Math.round(clip.width * SCALE) + "x" + Math.round(clip.height * SCALE));
  }
  for (const [id, err] of cdp.errors || []) console.log("render page-error " + id + " " + err);
} catch (e) {
  console.error("render failed: " + e.message);
  code = 1;
} finally {
  if (chrome) {
    try { chrome.ws.close(); } catch {}
    const exited = new Promise((r) => { chrome.proc.once("exit", r); setTimeout(r, 4000); });
    chrome.proc.kill("SIGTERM");
    await exited;
    rmSync(chrome.profile, { recursive: true, force: true });
  }
}
process.exit(code);
