#!/usr/bin/env node
/**
 * yap-1106-transcript-grid — the Meetings transcript's layout check.
 *
 * In-person transcripts crushed into a 36 px column: `TranscriptList` draws the
 * `.seg-who` speaker cell only on a two-track meeting, while App.css laid every
 * row out as THREE grid columns, so a single-track row's prose auto-placed into
 * the 2.6 rem speaker column and `word-break: break-word` wrapped it inside
 * words ("unde / rstan / d"). Two-track meetings rendered fine, which is why it
 * "fixed itself sometimes". A unit test cannot see a grid column; this can.
 *
 * WHAT IS DRIVEN (the pill-matrix pattern, scripts/pill-matrix.mjs): the
 * dev-tooling preview `dev/meeting-transcript-preview.html` — the REAL
 * `TranscriptList` inside the app's own chrome against fixed segment sets — is
 * built with YAP_DEV_TOOLING=1 into a SCRATCH outDir (never desktop/dist),
 * served by `vite preview` on a free port, and opened in HEADLESS Playwright
 * WebKit, the engine Tauri uses on macOS. Never a visible window, never the
 * app. Every browser and server this starts is closed before it exits.
 *
 * THE MEASURE: for every `<li>` in `.transcript`, `.seg-text` must be at least
 * HALF the row's width (the two gutters together are under 9 rem of an 88 ch
 * measure, so a healthy row is ~80 %; the crushed one was ~5 %). Checked in
 * three scenes x two widths:
 *
 *   mic-only   one track, no speaker cell          (the crushed case)
 *   blank-tap  a call whose far side never spoke — rendered as one track
 *   two-track  You / Call, three cells per row     (the case that always worked)
 *   at 1280 px (desktop) and 720 px (the <= 900 px media query's narrow grid)
 *
 * plus one INSTRUMENT check per scene — the page must have drawn rows — so an
 * empty preview can never pass as a photo of a layout.
 *
 * USAGE
 *   node scripts/transcript-layout.mjs                 all scenes x both widths
 *   node scripts/transcript-layout.mjs --out <dir>     also write one PNG per case
 *   node scripts/transcript-layout.mjs --self-test-must-fail
 *       re-applies the pre-fix CSS inside the page and requires the detector to
 *       fire (exits 1 BY DESIGN; 9 if it stayed blind)
 *
 * ENV  YAP_PLAYWRIGHT      a node_modules dir holding `playwright` (the app does
 *                          not ship it; a sibling project's install works)
 *      YAP_TRANSCRIPT_PORT preview port (default: a free one)
 *
 * EXIT  0 clean · 1 a case carries a finding · 2 usage · 3 environment /
 *       instrument failure · 9 self-test found the detector blind.
 */

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const DESKTOP = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ROOT = resolve(DESKTOP, "..");
const PAGE = "/dev/meeting-transcript-preview.html";
const SCENES = ["mic-only", "blank-tap", "two-track"];
const WIDTHS = [1280, 720];
const HEIGHT = 800;
const MIN_TEXT_SHARE = 0.5;

/* ── args ───────────────────────────────────────────────────────────────── */

function usage(msg) {
  console.error("transcript-layout: " + msg + "\nusage: node scripts/transcript-layout.mjs [--out <dir>] [--self-test-must-fail]");
  process.exit(2);
}
function parseArgs(argv) {
  const a = { out: null, selfTest: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--out") {
      const v = argv[++i];
      if (!v || v.startsWith("--")) usage("--out needs a value");
      a.out = v;
    } else if (k.startsWith("--out=")) a.out = k.slice(6);
    else if (k === "--self-test-must-fail") a.selfTest = true;
    else usage("unknown argument " + k);
  }
  return a;
}
const ARGS = parseArgs(process.argv.slice(2));

/* ── playwright ─────────────────────────────────────────────────────────── */
/*
 * The app does not depend on playwright (a 50 MB browser download is not a
 * price every `npm ci` should pay for one layout check), so it is resolved
 * from desktop/node_modules when present, else from YAP_PLAYWRIGHT. Missing
 * is an ENVIRONMENT failure (3), never a layout verdict.
 */
async function loadPlaywright() {
  const tried = [];
  const roots = [join(DESKTOP, "package.json")];
  if (process.env.YAP_PLAYWRIGHT) roots.push(join(resolve(process.env.YAP_PLAYWRIGHT), "package.json"));
  for (const r of roots) {
    try {
      const file = createRequire(r).resolve("playwright");
      const mod = await import(pathToFileURL(file).href);
      console.log("transcript-layout: playwright from " + dirname(file));
      return mod.webkit ? mod : mod.default;
    } catch (e) {
      tried.push(dirname(r) + " (" + (e && e.code ? e.code : e) + ")");
    }
  }
  console.error(
    "transcript-layout: playwright not found — tried " + tried.join(", ") +
    ". Install it (`npm i -D playwright && npx playwright install webkit`) or point YAP_PLAYWRIGHT at a node_modules that has it (ENVIRONMENT failure, not a layout defect).",
  );
  process.exit(3);
}

/* ── build + serve the dev-tooling bundle ───────────────────────────────── */

function freePort() {
  if (process.env.YAP_TRANSCRIPT_PORT) return Promise.resolve(Number(process.env.YAP_TRANSCRIPT_PORT));
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
    console.error("transcript-layout: " + vite + " missing — run npm ci in desktop/ (ENVIRONMENT failure, not a UI defect)");
    process.exit(3);
  }
  console.log("transcript-layout: building " + PAGE + " with YAP_DEV_TOOLING=1 -> " + dist);
  const fd = openSync(log, "w");
  const r = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], {
    cwd: DESKTOP,
    env: { ...process.env, YAP_DEV_TOOLING: "1" },
    stdio: ["ignore", fd, fd],
  });
  if (r.status !== 0) {
    console.error("transcript-layout: vite build exited " + r.status + "\n" + readFileSync(log, "utf8"));
    process.exit(3);
  }
  return dist;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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
    try { if ((await fetch(base + PAGE)).ok) return { proc, base }; } catch {}
    if (proc.exitCode !== null) break;
    await sleep(250);
  }
  proc.kill("SIGKILL");
  console.error("transcript-layout: vite preview never came up on " + base + "\n" + readFileSync(join(work, "vite-preview.log"), "utf8"));
  process.exit(3);
}

/* ── the detector, evaluated inside the page ────────────────────────────── */
/*
 * One function for the run AND the self-test, so the self-test proves THIS
 * code is non-vacuous. Returns every row's measure; a finding is a row whose
 * prose is under MIN_TEXT_SHARE of the row, or a row with no `.seg-text`.
 */
function measureRows(minShare) {
  const rows = [...document.querySelectorAll("ol.transcript > li")];
  const out = { rows: rows.length, twoTrack: !!document.querySelector("ol.transcript.two-track"), findings: [], measures: [] };
  rows.forEach((li, i) => {
    const text = li.querySelector(".seg-text");
    const lr = li.getBoundingClientRect();
    if (!text) {
      out.findings.push("row " + i + ": no .seg-text");
      return;
    }
    const tr = text.getBoundingClientRect();
    const share = lr.width > 0 ? tr.width / lr.width : 0;
    const m = { row: i, li: Math.round(lr.width), text: Math.round(tr.width), textHeight: Math.round(tr.height), share: Math.round(share * 1000) / 1000, lastChild: li.lastElementChild === text };
    out.measures.push(m);
    if (share < minShare) out.findings.push("row " + i + ": .seg-text " + m.text + "x" + m.textHeight + " px is " + Math.round(share * 100) + "% of the " + m.li + " px row (floor " + Math.round(minShare * 100) + "%)");
    if (!m.lastChild) out.findings.push("row " + i + ": .seg-text is not the last cell");
  });
  return out;
}

/* The CSS the fix replaced, re-applied on top of the bundle: a three-column
   single-track template, the prose auto-placed, breaks inside words. */
const PRE_FIX_CSS =
  ".transcript:not(.two-track) li{grid-template-columns:4.6rem 2.6rem 1fr}" +
  ".seg-text{grid-column:auto;word-break:break-word;overflow-wrap:normal}";

/* ── main ───────────────────────────────────────────────────────────────── */

async function run() {
  const pw = await loadPlaywright();
  const work = mkdtempSync(join(tmpdir(), "yap-transcript-layout-"));
  const outDir = ARGS.out ? resolve(ARGS.out) : null;
  const SMOKE_SHOTS = join(ROOT, "docs", "smoke-shots") + "/";
  if (outDir && (outDir === ROOT || outDir.startsWith(ROOT + "/")) && !outDir.startsWith(SMOKE_SHOTS)) {
    console.error("transcript-layout: --out " + outDir + " is inside the repo. The PNGs are PR evidence, never a commit — use a scratch dir.");
    return 2;
  }
  if (outDir) mkdirSync(outDir, { recursive: true });
  const dist = buildBundle(work);
  const { proc: preview, base } = await servePreview(dist, work);
  let browser = null;
  const results = [];
  let failed = 0;
  try {
    try {
      browser = await pw.webkit.launch({ headless: true });
    } catch (e) {
      console.error("transcript-layout: WebKit did not launch — `npx playwright install webkit` in the install YAP_PLAYWRIGHT names (ENVIRONMENT failure): " + e.message);
      return 3;
    }
    console.log("transcript-layout: headless WebKit " + browser.version());
    const cases = ARGS.selfTest ? [{ scene: "mic-only", width: 1280 }] : WIDTHS.flatMap((width) => SCENES.map((scene) => ({ scene, width })));
    for (const c of cases) {
      const ctx = await browser.newContext({ viewport: { width: c.width, height: HEIGHT }, deviceScaleFactor: 1 });
      const page = await ctx.newPage();
      const pageErrors = [];
      page.on("pageerror", (e) => pageErrors.push(String(e)));
      try {
        await page.goto(base + PAGE + "#" + c.scene, { waitUntil: "load" });
        await page.waitForSelector("ol.transcript > li", { timeout: 8000 }).catch(() => null);
        await page.evaluate(() => document.fonts.ready.then(() => true));
        if (ARGS.selfTest) {
          const clean = await page.evaluate(measureRows, MIN_TEXT_SHARE);
          console.log("self-test: real capture before the break: " + (clean.findings.length ? clean.findings.join("; ") : "clean (" + clean.rows + " rows)"));
          await page.addStyleTag({ content: PRE_FIX_CSS });
          await sleep(100);
        }
        const r = await page.evaluate(measureRows, MIN_TEXT_SHARE);
        const id = c.scene + "@" + c.width;
        if (r.rows === 0) r.findings.unshift("INSTRUMENT: the page drew no transcript rows" + (pageErrors.length ? " (page errors: " + pageErrors.join(" | ") + ")" : ""));
        if (c.scene === "two-track" ? !r.twoTrack : r.twoTrack) r.findings.unshift("INSTRUMENT: expected " + (c.scene === "two-track" ? "a two-track" : "a single-track") + " list for scene " + c.scene);
        const shot = outDir ? join(outDir, c.scene + "-" + c.width + ".png") : null;
        if (shot) await page.screenshot({ path: shot, fullPage: false });
        const minShare = r.measures.length ? Math.min(...r.measures.map((m) => m.share)) : 0;
        const line = id + ": " + r.rows + " rows, narrowest .seg-text " + Math.round(minShare * 100) + "% of its row" + (r.measures[0] ? " (e.g. " + r.measures[0].text + "x" + r.measures[0].textHeight + " px of " + r.measures[0].li + " px)" : "");
        if (ARGS.selfTest) {
          const fired = r.findings.filter((f) => !f.startsWith("INSTRUMENT")).length > 0;
          for (const f of r.findings) console.log("  detector fired: " + f);
          if (!fired) {
            console.error("SELF-TEST VACUOUS — the detector stayed blind to the pre-fix CSS (" + line + ")");
            return 9;
          }
          console.error("self-test-must-fail: the detector fired on the pre-fix CSS; exiting 1 BY DESIGN. " + line);
          return 1;
        }
        results.push({ id, ...r, shot });
        if (r.findings.length) {
          failed++;
          console.log("FAIL " + line);
          for (const f of r.findings) console.log("     " + f);
        } else console.log("ok   " + line);
      } finally {
        await ctx.close();
      }
    }
    if (outDir) writeFileSync(join(outDir, "result.json"), JSON.stringify({ floor: MIN_TEXT_SHARE, cases: results }, null, 2));
    console.log("transcript-layout: " + results.length + " cases, " + failed + " carrying findings" + (outDir ? " — PNGs + result.json in " + outDir + " (PR evidence, never a commit)" : ""));
    return failed ? 1 : 0;
  } finally {
    if (browser) await browser.close().catch(() => {});
    preview.kill("SIGKILL");
    await sleep(200);
    rmSync(work, { recursive: true, force: true });
  }
}

process.exit(await run());
