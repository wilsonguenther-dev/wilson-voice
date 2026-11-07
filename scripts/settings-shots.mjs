#!/usr/bin/env node
/**
 * yap-1106-settings-redesign — headless PNGs of the five-section Settings at a
 * wide window (1280x900, label rail) and a narrow one (860x700, icon rail),
 * plus the structural checks the item names:
 *
 *   - the left list is exactly General, Dictation, Meetings, Yappy & pill,
 *     Models & privacy, with "Search settings" above it;
 *   - the pinned footer carries "Account & help" and Done, at the bottom of
 *     the Settings pane, and it does not move when the main column scrolls;
 *   - each settings block renders once (no duplicate data-settings-block);
 *   - the app sidebar is 184 px wide over 900 px, 64 px under it, and under
 *     900 px every nav label is visually hidden while every nav icon shows;
 *   - typing "retention" lists a hit that names its section (search works).
 *
 * Same pattern as home-shots.mjs: vite build to a scratch outDir, vite preview
 * on a free port, real Chrome --headless=new (never a window, never the app),
 * fixtures from scripts/smoke-fixtures-daily.json through the shared
 * TAURI_SHIM, everything stopped and deleted before exit. Run it on the base
 * (before) and on this branch (after) with different --out dirs.
 *
 * USAGE  node scripts/settings-shots.mjs --out <scratch dir>   (exit 1 on any failed check)
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
const SIZES = [
  { w: 1280, h: 900, rail: 184 },
  { w: 860, h: 700, rail: 64 },
];
const SECTIONS = ["General", "Dictation", "Meetings", "Yappy & pill", "Models & privacy"];
const SETTLE_MS = Number(process.env.YAP_SETTINGS_SHOTS_SETTLE_MS || 1200);
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

function fixtures() {
  const daily = JSON.parse(readFileSync(join(ROOT, "scripts", "smoke-fixtures-daily.json"), "utf8"));
  delete daily._README;
  return daily;
}

function freePort() {
  return new Promise((res, rej) => {
    const s = createServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

// Opens Settings from the sidebar, waits for the five-section pane, checks it.
const OPEN = String.raw`(async () => {
  const btn = [...document.querySelectorAll(".nav .nav-item")].find((b) => b.title === "Settings");
  if (!btn) return false;
  btn.click();
  for (let i = 0; i < 100 && !document.querySelector(".settings .settings-list"); i++)
    await new Promise((r) => setTimeout(r, 50));
  return !!document.querySelector(".settings .settings-list");
})()`;

const measure = (rail) => String.raw`(async () => {
  const bad = [];
  const q = (s) => document.querySelector(s);
  const want = ${JSON.stringify(SECTIONS)};
  const items = [...document.querySelectorAll(".settings-list .settings-list-item")].map((b) => b.textContent.trim());
  if (JSON.stringify(items) !== JSON.stringify(want)) bad.push("list " + JSON.stringify(items));
  if (!q('.settings-search input[type="search"]')) bad.push("no search field");
  const bar = q(".settings-done-bar"), main = q(".settings-main"), pane = q(".settings");
  if (!bar || !main || !pane) return { bad: bad.concat(["missing settings parts"]) };
  const labels = [...bar.querySelectorAll("button")].map((b) => b.textContent.trim());
  if (!labels.includes("Account & help") || !labels.includes("Done")) bad.push("footer " + JSON.stringify(labels));
  const blocks = [...document.querySelectorAll("[data-settings-block]")].map((n) => n.dataset.settingsBlock);
  if (new Set(blocks).size !== blocks.length) bad.push("duplicate block " + blocks.join(","));
  const side = q(".sidebar").getBoundingClientRect().width;
  if (Math.abs(side - ${rail}) > 1) bad.push("sidebar width " + side + " != ${rail}");
  if (${rail} === 64) {
    for (const l of document.querySelectorAll(".nav .nav-label"))
      if (l.getBoundingClientRect().width > 1) { bad.push("nav label visible under 900px"); break; }
    for (const ic of document.querySelectorAll(".nav .nav-icon"))
      if (ic.getBoundingClientRect().width < 10) { bad.push("nav icon hidden"); break; }
  }
  const before = bar.getBoundingClientRect().top;
  main.scrollTop = main.scrollHeight;
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  if (bar.getBoundingClientRect().top !== before) bad.push("Done bar moved on scroll");
  main.scrollTop = 0;
  return { bad, items: items.length, blocks: blocks.length, side };
})()`;

// Types a query through React's value setter and checks a hit names its section.
const SEARCH = String.raw`(async () => {
  const input = document.querySelector('.settings-search input[type="search"]');
  if (!input) return "no search field";
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  set.call(input, "retention");
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await new Promise((r) => setTimeout(r, 300));
  const text = document.querySelector(".settings-main")?.textContent ?? "";
  const ok = /Meetings/.test(text) && /etention/.test(text);
  set.call(input, "");
  input.dispatchEvent(new Event("input", { bubbles: true }));
  return ok ? "" : "search for retention listed no Meetings hit";
})()`;

async function run() {
  const args = process.argv.slice(2);
  const i = args.indexOf("--out");
  const outDir = resolve(i >= 0 ? args[i + 1] : mkdtempSync(join(tmpdir(), "yap-settings-shots-")));
  if (outDir === ROOT || outDir.startsWith(ROOT + "/")) {
    console.error("settings-shots: --out is inside the repo; use a scratch dir");
    return 2;
  }
  mkdirSync(outDir, { recursive: true });
  const work = mkdtempSync(join(tmpdir(), "yap-settings-build-"));
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("settings-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    return 3;
  }
  const dist = join(work, "dist");
  const fd = openSync(join(work, "vite-build.log"), "w");
  const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
  if (b.status !== 0) {
    console.error("settings-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
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
    chrome = await openChrome(CHROME, "yap-settings-shots");
    const { cdp } = chrome;
    for (const { w, h, rail } of SIZES) {
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      await cdp.send("Page.enable", {}, s);
      await cdp.send("Runtime.enable", {}, s);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: 2, mobile: false }, s);
      await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source }, s);
      await cdp.send("Page.navigate", { url }, s);
      const deadline = Date.now() + 10000;
      while (Date.now() < deadline && !(await evaluate(cdp, s, "!!document.querySelector('.nav .nav-item')").catch(() => false))) await sleep(100);
      await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
      const opened = await evaluate(cdp, s, OPEN).catch(() => false);
      await sleep(SETTLE_MS);
      const m = opened ? await evaluate(cdp, s, measure(rail)) : { bad: ["Settings never opened"] };
      const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
      const file = join(outDir, "settings-" + w + "x" + h + ".png");
      writeFileSync(file, Buffer.from(shot.data, "base64"));
      const bad = [...(m?.bad ?? ["no measurement"])];
      if (opened) {
        const miss = await evaluate(cdp, s, SEARCH).catch((e) => "search threw " + e.message);
        if (miss) bad.push(miss);
      }
      console.log((bad.length ? "FAIL " : "ok   ") + w + "x" + h + " list=" + m?.items + " blocks=" + m?.blocks + " sidebar=" + m?.side + " -> " + file + (bad.length ? "  " + bad.join("; ") : ""));
      if (bad.length) failures.push(w + "x" + h);
      await cdp.send("Page.close", {}, s).catch(() => {});
    }
  } catch (e) {
    console.error("settings-shots: " + e.message);
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
