#!/usr/bin/env node
/**
 * yap-1106-home-full-width-room (+ home-contained-scroll-footer) — headless
 * PNGs of the round-two Home at 900 and 1400 px window heights, plus the
 * structural checks the item's acceptance names:
 *
 *   - the room's scale is a whole number (2, 3 or 4) and its stage is exactly
 *     160 x scale CSS px tall, filling the main column's width;
 *   - yap-1106-fb-24: the wooden pixel frame is on all four sides, the brass
 *     pegs at the corners and the 28 px plate centred, nothing clipped;
 *   - the window never scrolls; only .content does (overscroll contained);
 *   - scrolling .content to the end moves neither the sidebar nor the footer;
 *   - Home ends: at most 5 recent rows, then the "See all" row; no
 *     "Show older takes" control anywhere;
 *   - the footer is pinned to the window's bottom edge.
 *
 * Same pattern as call-offer-shots.mjs: vite build to a scratch outDir, vite
 * preview on a free port, real Chrome --headless=new (never a window, never the
 * app), fixtures from scripts/smoke-fixtures-daily.json through the shared
 * TAURI_SHIM, everything stopped and deleted before exit.
 *
 * USAGE  node scripts/home-shots.mjs --out <scratch dir>   (exit 1 on any failed check)
 */
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { TAURI_SHIM, evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
const WIDTH = 1280;
const HEIGHTS = [900, 1400];
const SETTLE_MS = Number(process.env.YAP_HOME_SHOTS_SETTLE_MS || 1200);
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

function fixtures() {
  const daily = JSON.parse(readFileSync(join(ROOT, "scripts", "smoke-fixtures-daily.json"), "utf8"));
  delete daily._README;
  // Twelve bland takes so Home must stop at 5 and show "See all".
  const takes = Array.from({ length: 12 }, (_, i) => ({
    id: "home-shot-" + i,
    text: "A bland sample take number " + (i + 1) + " for the Home layout check.",
    backend: "native",
    asrSeconds: 0.4,
    wordCount: 11,
    createdAt: new Date(Date.UTC(2026, 10, 6, 9, 59 - i)).toISOString(),
  }));
  return { ...daily, get_history: takes };
}

function freePort() {
  return new Promise((res, rej) => {
    const s = createServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

const MEASURE = String.raw`(async () => {
  const bad = [];
  const q = (s) => document.querySelector(s);
  const house = q(".yappy-house"), stage = q(".yh-stage"), content = q(".content");
  const side = q(".sidebar"), foot = q("[data-app-footer]");
  if (!house || !stage || !content || !side || !foot) return { bad: ["missing shell parts"] };
  const scale = Number(house.dataset.roomScale);
  // homeLayout: ROOM 320 x 160, whole-number scales 4 / 3 / 2 (the layered studio).
  if (![2, 3, 4].includes(scale)) bad.push("room scale " + house.dataset.roomScale);
  const sr = stage.getBoundingClientRect();
  if (Math.abs(sr.height - 160 * scale) > 0.5) bad.push("stage height " + sr.height + " != " + 160 * scale);
  // yap-1106-fb-24 — the wooden pixel frame, sampled from the canvas backing store:
  // the room is gridded "contain" at an integer cell, centred; each art px is one
  // cell x cell square. Frame visible on all four sides, plate centred, nothing clipped.
  const g2 = stage.getContext("2d");
  const cell = Math.max(1, Math.min(Math.floor(stage.width / 320), Math.floor(stage.height / 160)));
  const ox = Math.floor((stage.width - 320 * cell) / 2), oy = Math.floor((stage.height - 160 * cell) / 2);
  if (ox < 0 || oy < 0) bad.push("room clipped ox=" + ox + " oy=" + oy);
  const art = (x, y) => {
    const d = g2.getImageData(ox + x * cell + (cell >> 1), oy + y * cell + (cell >> 1), 1, 1).data;
    return "#" + [d[0], d[1], d[2]].map((v) => v.toString(16).padStart(2, "0")).join("");
  };
  const want = [
    [40, 0, "#3a2418", "top edge"], [0, 80, "#3a2418", "left edge"], [40, 159, "#2a1a10", "bottom edge"], [319, 80, "#2a1a10", "right edge"],
    [3, 3, "#e8c86a", "top-left peg"], [317, 3, "#e8c86a", "top-right peg"], [3, 157, "#e8c86a", "bottom-left peg"], [317, 157, "#e8c86a", "bottom-right peg"],
    [146, 155, "#f0d27a", "plate left"], [173, 155, "#f0d27a", "plate right"],
  ];
  if (g2) for (const [x, y, c, what] of want) { const got = art(x, y); if (got !== c) bad.push("frame " + what + " " + got + " != " + c); }
  else bad.push("no 2d stage");
  if (g2 && (art(145, 155) === "#f0d27a" || art(174, 155) === "#f0d27a")) bad.push("plate not 28 px");
  const frame = q(".yh-frame").getBoundingClientRect();
  const col = content.getBoundingClientRect();
  if (frame.width < col.width - 60) bad.push("room not full width " + frame.width + " of " + col.width);
  if (getComputedStyle(content).overscrollBehaviorY !== "contain") bad.push("overscroll not contained");
  const rows = document.querySelectorAll(".recent-row").length;
  if (rows > 5) bad.push("recent rows " + rows);
  if (!q('[data-home="see-all"]')) bad.push("no See all row");
  if (/Show older takes/.test(document.body.textContent)) bad.push("Show older takes present");
  const before = { side: side.getBoundingClientRect().top, foot: foot.getBoundingClientRect().top };
  content.scrollTop = content.scrollHeight;
  window.scrollTo(0, 99999);
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const after = { side: side.getBoundingClientRect().top, foot: foot.getBoundingClientRect().top };
  if (before.side !== after.side) bad.push("sidebar moved");
  if (before.foot !== after.foot) bad.push("footer moved");
  if ((document.scrollingElement || document.documentElement).scrollTop !== 0) bad.push("window scrolled");
  const fb = foot.getBoundingClientRect().bottom;
  if (Math.abs(fb - innerHeight) > 1) bad.push("footer not at the bottom " + fb + " vs " + innerHeight);
  content.scrollTop = 0;
  return { bad, scale, scrolls: content.scrollHeight > content.clientHeight, rows };
})()`;

async function run() {
  const args = process.argv.slice(2);
  const i = args.indexOf("--out");
  const outDir = resolve(i >= 0 ? args[i + 1] : mkdtempSync(join(tmpdir(), "yap-home-shots-")));
  if (outDir === ROOT || outDir.startsWith(ROOT + "/")) {
    console.error("home-shots: --out is inside the repo; use a scratch dir");
    return 2;
  }
  mkdirSync(outDir, { recursive: true });
  const work = mkdtempSync(join(tmpdir(), "yap-home-build-"));
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("home-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    return 3;
  }
  const dist = join(work, "dist");
  const fd = openSync(join(work, "vite-build.log"), "w");
  const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
  if (b.status !== 0) {
    console.error("home-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
    rmSync(work, { recursive: true, force: true });
    return 3;
  }
  const port = await freePort();
  const pfd = openSync(join(work, "vite-preview.log"), "w");
  const preview = spawn(vite, ["preview", "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], { cwd: DESKTOP, stdio: ["ignore", pfd, pfd] });
  const url = "http://127.0.0.1:" + port + "/index.html";
  const source = "window.__YAP_FIXTURES__ = " + JSON.stringify(fixtures()) + ";\n" + TAURI_SHIM;
  let chrome = null;
  const failures = [];
  try {
    let up = false;
    for (let n = 0; n < 120 && !up; n++) {
      try { up = (await fetch(url)).ok; } catch {}
      if (!up) await sleep(250);
    }
    if (!up) throw new Error("vite preview never served " + url);
    chrome = await openChrome(CHROME, "yap-home-shots");
    const { cdp } = chrome;
    for (const h of HEIGHTS) {
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      await cdp.send("Page.enable", {}, s);
      await cdp.send("Runtime.enable", {}, s);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: WIDTH, height: h, deviceScaleFactor: 2, mobile: false }, s);
      await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source }, s);
      await cdp.send("Page.navigate", { url }, s);
      const deadline = Date.now() + 10000;
      while (Date.now() < deadline && !(await evaluate(cdp, s, "!!document.querySelector('.yappy-house')").catch(() => false))) await sleep(100);
      await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
      await sleep(SETTLE_MS);
      const m = await evaluate(cdp, s, MEASURE);
      const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
      const file = join(outDir, "home-" + WIDTH + "x" + h + ".png");
      writeFileSync(file, Buffer.from(shot.data, "base64"));
      const bad = m?.bad ?? ["no measurement"];
      console.log((bad.length ? "FAIL " : "ok   ") + h + "px scale=" + m?.scale + " scrolls=" + m?.scrolls + " rows=" + m?.rows + " -> " + file + (bad.length ? "  " + bad.join("; ") : ""));
      if (bad.length) failures.push(String(h));
      await cdp.send("Page.close", {}, s).catch(() => {});
    }
  } catch (e) {
    console.error("home-shots: " + e.message);
    failures.push("instrument");
  } finally {
    if (chrome) {
      try { chrome.ws.close(); } catch {}
      chrome.proc.kill("SIGKILL");
      rmSync(chrome.profile, { recursive: true, force: true });
    }
    preview.kill("SIGKILL");
    rmSync(work, { recursive: true, force: true });
  }
  return failures.length ? 1 : 0;
}

run().then((code) => process.exit(code));
