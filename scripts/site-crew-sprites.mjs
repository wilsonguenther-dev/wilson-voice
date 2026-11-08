#!/usr/bin/env node
/*
 * yap-1107-landing-page-tells-the-truth — the landing page's crew sprites.
 *
 * The site's "Yappy and the crew" section shows Yappy, Sal, Mochi and Cinder
 * with their REAL sprites from the approved design kit (Wilson's one exception
 * to "every picture is a capture of the build"). This never redraws or traces:
 * it loads design-2026-11-06/companions.html in headless Chrome, lets
 * assets/yappy.js + assets/friends-hd.js paint the #cast cards, and copies each
 * 96x96 card canvas's own pixels out as PNG. The caller doubles them with a
 * nearest-neighbour resize (the 2x crop) and encodes WebP.
 *
 *   node scripts/site-crew-sprites.mjs <companions.html> <out-dir>
 *   YAP_SMOKE_CHROME=<headless Chromium>   (never /Applications/Google Chrome.app)
 *
 * Headless only; the Chrome it starts and its temp profile are removed on exit.
 */
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { openChrome, evaluate, sleep } from "./headless-chrome.mjs";

const [page, out] = process.argv.slice(2);
if (!page || !out) {
  console.error("usage: site-crew-sprites.mjs <companions.html> <out-dir>");
  process.exit(2);
}
const CHROME = process.env.YAP_SMOKE_CHROME;
if (!CHROME || CHROME.startsWith("/Applications/Google Chrome.app")) {
  console.error("set YAP_SMOKE_CHROME to a headless Chromium (not /Applications/Google Chrome.app)");
  process.exit(2);
}
// CAST order on companions.html: yappy, hoot, tidy, gulp, shelly, ember, sal, mochi, cinder
const WANT = { yappy: 0, sal: 6, mochi: 7, cinder: 8 };

mkdirSync(out, { recursive: true });
const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-site-crew");
let code = 0;
try {
  const { targetId } = await cdp.send("Target.createTarget", { url: pathToFileURL(resolve(page)).href });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Runtime.enable", {}, sessionId);
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Page.bringToFront", {}, sessionId); // a background tab gets no animation frames
  let urls = null;
  for (let i = 0; i < 40 && !urls; i++) {
    await sleep(250);
    urls = await evaluate(
      cdp,
      sessionId,
      `(() => { const c = [...document.querySelectorAll("#cast .who canvas")];
         const painted = (x) => { const d = x.getContext("2d").getImageData(0, 0, x.width, x.height).data;
           for (let i = 3; i < d.length; i += 4) if (d[i]) return true; return false; };
         return c.length >= 9 && [0, 6, 7, 8].every((i) => painted(c[i])) ? c.map((x) => x.width + "x" + x.height + "|" + x.toDataURL("image/png")) : null; })()`
    );
  }
  if (!urls) throw new Error("#cast canvases never painted (" + (cdp.errors.get(sessionId) || []).join("; ") + ")");
  for (const [name, idx] of Object.entries(WANT)) {
    const [size, url] = urls[idx].split("|");
    const file = resolve(out, "crew-" + name + ".png");
    writeFileSync(file, Buffer.from(url.split(",")[1], "base64"));
    console.log("site-crew-sprites wrote name=" + name + " size=" + size);
  }
} catch (e) {
  console.error("site-crew-sprites failed: " + e.message);
  code = 1;
} finally {
  try { ws.close(); } catch {}
  await new Promise((r) => { proc.once("exit", r); proc.kill(); setTimeout(r, 5000); });
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}
process.exit(code);
