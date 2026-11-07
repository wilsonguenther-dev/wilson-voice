#!/usr/bin/env node
/**
 * yap-wf-19 — PNGs of the Shortcuts screen's mouse-button panel in every
 * state, from its dev preview (desktop/src/mousePttPreview.tsx). Headless only
 * (scripts/headless-chrome.mjs, --headless=new); no npm dependencies.
 *
 * The INTEGRATOR runs this once per batch against a bundle it already built:
 *
 *   (cd desktop && YAP_DEV_TOOLING=1 npm run build)
 *   node scripts/mouse-ptt-shots.mjs --dist desktop/dist --out <dir>
 *
 * Scenes: off, hold, handsfree, listening, refused. Each capture also asserts
 * the panel's words (the binding line, or the refusal hint), so a PNG is never
 * the only proof. Exit 0 all good, 1 a scene showed the wrong words, 3
 * instrument failure. Everything it starts (a localhost static server, Chrome,
 * its temp profile) is stopped/deleted before it exits.
 */
import { createServer } from "node:http";
import { readFileSync, rmSync, writeFileSync, mkdirSync, existsSync, statSync } from "node:fs";
import { extname, join, resolve, normalize } from "node:path";
import { evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const arg = (k, d) => {
  const i = process.argv.indexOf(k);
  return i > 0 ? process.argv[i + 1] : d;
};
const DIST = resolve(arg("--dist", "desktop/dist"));
const OUT = resolve(arg("--out", "mouse-ptt-shots"));
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
// [scene, selector whose text is asserted, the words it must show]
const SCENES = [
  ["off", "[data-testid=mouse-ptt-line]", "Off"],
  ["hold", "[data-testid=mouse-ptt-line]", "Hold the button 4 to talk, double-click for hands-free"],
  ["handsfree", "[data-testid=mouse-ptt-line]", "Click the middle button to start, click again to stop"],
  ["listening", "[data-testid=mouse-ptt] [role=status]", "Press the middle button or a side button (Esc to cancel)."],
  ["refused", "[data-testid=mouse-ptt] [role=status]", "Left and right click can't be shortcuts — pick the middle button or a side button"],
];
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".woff2": "font/woff2", ".png": "image/png", ".svg": "image/svg+xml" };

async function main() {
  const page = join(DIST, "dev", "mouse-ptt-preview.html");
  if (!existsSync(page)) {
    console.error(`mouse-ptt-shots: ${page} missing — build with YAP_DEV_TOOLING=1 first (INSTRUMENT failure)`);
    return 3;
  }
  mkdirSync(OUT, { recursive: true });
  const server = createServer((req, res) => {
    const rel = normalize(decodeURIComponent((req.url || "/").split("?")[0])).replace(/^(\.\.[/\\])+/, "");
    const file = join(DIST, rel);
    if (!file.startsWith(DIST) || !existsSync(file) || statSync(file).isDirectory()) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { "content-type": TYPES[extname(file)] || "application/octet-stream" });
    res.end(readFileSync(file));
  });
  await new Promise((r) => server.listen(0, "127.0.0.1", r));
  const base = `http://127.0.0.1:${server.address().port}`;
  let chrome;
  try {
    chrome = await openChrome(CHROME, "yap-mouse-ptt-shots");
  } catch (e) {
    server.close();
    console.error("mouse-ptt-shots: " + e.message);
    return 3;
  }
  const { proc, cdp, ws, profile } = chrome;
  let failed = 0;
  try {
    for (const [scene, sel, want] of SCENES) {
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      await cdp.send("Page.enable", {}, s);
      await cdp.send("Runtime.enable", {}, s);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: 1100, height: 820, deviceScaleFactor: 2, mobile: false }, s);
      await cdp.send("Page.navigate", { url: `${base}/dev/mouse-ptt-preview.html#${scene}` }, s);
      const deadline = Date.now() + 8000;
      while (Date.now() < deadline) {
        const ready = await evaluate(cdp, s, "document.readyState === 'complete' && !!document.querySelector('[data-testid=mouse-ptt]')").catch(() => false);
        if (ready) break;
        await sleep(100);
      }
      await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
      await sleep(300);
      const got = await evaluate(cdp, s, `document.querySelector(${JSON.stringify(sel)})?.textContent?.trim() ?? null`);
      const ok = got === want;
      if (!ok) failed++;
      console.log(`mouse-ptt-shots ${scene} ${ok ? "ok" : "WRONG"} got=${JSON.stringify(got)} want=${JSON.stringify(want)}`);
      const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
      writeFileSync(join(OUT, `mouse-ptt-${scene}.png`), Buffer.from(shot.data, "base64"));
      await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
    }
  } finally {
    await cdp.send("Browser.close").catch(() => {});
    ws.close();
    await sleep(300);
    if (proc.exitCode === null) proc.kill("SIGKILL");
    rmSync(profile, { recursive: true, force: true });
    server.close();
  }
  return failed ? 1 : 0;
}

main().then(
  (code) => process.exit(code),
  (e) => {
    console.error("mouse-ptt-shots: " + (e?.stack || e));
    process.exit(3);
  },
);
