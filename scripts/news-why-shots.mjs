#!/usr/bin/env node
/**
 * yap-1107-news-no-off-topic-stories — headless 2x PNG of the Home Yappy News
 * briefing on Wilson's real topics (developer, stocks, business, tech, world,
 * culture, science; no sports), plus the checks acceptance line 4 names:
 *
 *   - every story card shows a "Why this story" line of the form
 *     "From <outlet> (<category>) · matches <topic>";
 *   - no card's outlet is a sports or entertainment outlet (PGA TOUR,
 *     Golfweek, ESPN, Variety, ...), and no card names a golf event.
 *
 * The stories are what news::rank returns for those topics (category, why
 * and topic are the backend's fields); which stories survive is proven by the
 * Rust tests in news/sources.rs over tests/fixtures/news_headlines_60.json.
 * Same pattern as home-shots.mjs: vite build to a scratch outDir, vite
 * preview on a free port, real Chrome --headless=new (never a window, never
 * the app), fixtures through TAURI_SHIM, everything stopped and deleted.
 *
 * USAGE  node scripts/news-why-shots.mjs --out <dir outside the repo, or docs/loop/evidence/...>
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
const HEIGHT = 1400;
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const TOPICS = ["developer", "stocks", "business", "tech", "world", "culture", "science"];
const DENIED_OUTLETS = ["PGA TOUR", "Golfweek", "Golf Digest", "ESPN", "Bleacher Report", "The Athletic", "Variety", "People", "TMZ", "Deadline"];

const NOW = Date.now();
function ranked(i, source, topic, category, title, outlet, why) {
  return {
    source, topic, title, url: "https://example.com/news-why/" + i, outlet, outlets: 1,
    publishedMs: NOW - i * 3600_000, points: null, comments: null, snippet: null,
    score: 5 - i * 0.1, urlKey: "example.com/news-why/" + i, titleKey: title.toLowerCase(),
    category, classifiedBy: source === "hacker_news" ? "source" : "keywords", why,
  };
}
const STORIES = [
  ranked(0, "google_news", "tech", "tech", "Nvidia unveils next-generation AI chip", "The Verge", "From The Verge (Tech) · matches Tech"),
  ranked(1, "google_news", "stocks", "stocks", "Technology stocks fall as bond yields climb", "Reuters", "From Reuters (Stocks) · matches Stocks"),
  ranked(2, "hacker_news", "developer", "developer", "Rust 1.91 released with faster compile times", null, "From Hacker News (Developer) · matches Developer"),
  ranked(3, "google_news", "science", "science", "The science of sleep: why your brain needs deep rest", "NPR", "From NPR (Science) · matches Science"),
  ranked(4, "google_news", "world", "world", "Japan elects first female prime minister", "CNN", "From CNN (World) · matches World"),
];

function fixtures() {
  const daily = JSON.parse(readFileSync(join(ROOT, "scripts", "smoke-fixtures-daily.json"), "utf8"));
  delete daily._README;
  const prefs = { ...daily.get_news_prefs, topics: TOPICS, tickers: [], custom: [] };
  return {
    ...daily,
    get_news_prefs: prefs,
    news_briefing_cached: STORIES,
    news_refresh: { ran: false, skipped: "not_due", message: null, stories: [], quotes: [], view: prefs },
    news_explain_top: { lines: [], fallback: "user", sent: 0, explained: 0 },
    news_quotes_check: { ran: false, skipped: "not_due", quotes: [] },
  };
}

function freePort() {
  return new Promise((res, rej) => {
    const s = createServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

const MEASURE = `(() => {
  const bad = [];
  const cards = [...document.querySelectorAll("[data-news-story]")];
  if (!cards.length) return { bad: ["no story cards rendered"], cards: 0 };
  const denied = ${JSON.stringify(DENIED_OUTLETS)};
  for (const c of cards) {
    const why = c.querySelector("[data-news-why]");
    const text = why ? why.textContent.trim() : "";
    if (!/^From .+ \\(.+\\) \\u00b7 matches .+$/.test(text)) bad.push("card " + c.dataset.newsStory + " why line: " + JSON.stringify(text));
    const src = (c.querySelector("[data-news-source]") || {}).textContent || "";
    if (denied.some((d) => src.includes(d) || text.includes(d))) bad.push("sports/entertainment outlet on card " + c.dataset.newsStory);
    if (/World Wide Technology|putt|birdie/i.test(c.textContent)) bad.push("golf story on card " + c.dataset.newsStory);
  }
  const box = document.querySelector(".news-briefing");
  if (box) box.scrollIntoView({ block: "start" });
  return { bad, cards: cards.length };
})()`;

async function main() {
  const i = process.argv.indexOf("--out");
  if (i < 0 || !process.argv[i + 1]) {
    console.error("usage: node scripts/news-why-shots.mjs --out <dir>");
    return 2;
  }
  const outDir = resolve(process.argv[i + 1]);
  mkdirSync(outDir, { recursive: true });
  const work = mkdtempSync(join(tmpdir(), "yap-news-why-"));
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("news-why-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    rmSync(work, { recursive: true, force: true });
    return 3;
  }
  const dist = join(work, "dist");
  const fd = openSync(join(work, "vite-build.log"), "w");
  const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
  if (b.status !== 0) {
    console.error("news-why-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
    rmSync(work, { recursive: true, force: true });
    return 3;
  }
  const port = await freePort();
  const pfd = openSync(join(work, "vite-preview.log"), "w");
  const preview = spawn(vite, ["preview", "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], { cwd: DESKTOP, stdio: ["ignore", pfd, pfd] });
  const url = "http://127.0.0.1:" + port + "/index.html";
  const source = "window.__YAP_FIXTURES__ = " + JSON.stringify(fixtures()) + ";\n" + TAURI_SHIM;
  let chrome = null;
  let failed = false;
  try {
    let up = false;
    for (let n = 0; n < 120 && !up; n++) {
      try { up = (await fetch(url)).ok; } catch {}
      if (!up) await sleep(250);
    }
    if (!up) throw new Error("vite preview never served " + url);
    chrome = await openChrome(CHROME, "yap-news-why-shots");
    const { cdp } = chrome;
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, s);
    await cdp.send("Runtime.enable", {}, s);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: WIDTH, height: HEIGHT, deviceScaleFactor: 2, mobile: false }, s);
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source }, s);
    await cdp.send("Page.navigate", { url }, s);
    const deadline = Date.now() + 12000;
    while (Date.now() < deadline && !(await evaluate(cdp, s, "!!document.querySelector('[data-news-story]')").catch(() => false))) await sleep(150);
    await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
    await sleep(800);
    const m = await evaluate(cdp, s, MEASURE);
    await sleep(300);
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
    const file = join(outDir, "news-why-" + WIDTH + "x" + HEIGHT + "@2x.png");
    writeFileSync(file, Buffer.from(shot.data, "base64"));
    const bad = m?.bad ?? ["no measurement"];
    console.log((bad.length ? "FAIL " : "ok   ") + "cards=" + m?.cards + " -> " + file + (bad.length ? "  " + bad.join("; ") : ""));
    failed = bad.length > 0;
    await cdp.send("Page.close", {}, s).catch(() => {});
  } catch (e) {
    console.error("news-why-shots: " + e.message);
    failed = true;
  } finally {
    if (chrome) {
      try { chrome.ws.close(); } catch {}
      chrome.proc.kill("SIGKILL");
      rmSync(chrome.profile, { recursive: true, force: true });
    }
    preview.kill("SIGKILL");
    rmSync(work, { recursive: true, force: true });
  }
  return failed ? 1 : 0;
}

main().then((code) => process.exit(code));
