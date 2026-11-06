#!/usr/bin/env node
/**
 * yap-call-1 — headless PNGs of the call-offer window (call-offer.html), one
 * per lifecycle state, for the PR's before/after evidence.
 *
 * The offer lives in its OWN window (label "call-offer", 340x176 logical —
 * lib.rs ensure_call_offer_window) because that window is content-protected
 * and the pill's recording dot must never be. A browser cannot drive the
 * backend's state machine (call_offer.rs), so each capture answers
 * `call_offer_state` with a fixed CallOfferView through the shared Tauri shim
 * (scripts/headless-chrome.mjs TAURI_SHIM) — the same headless Chrome
 * (--headless=new, never a visible window, never the app) the pill matrix uses.
 *
 * STATES: zoom offer, FaceTime "your side" offer, browser call "site unknown",
 * the one-click chip after expiry, the chip after a rejected Record (error
 * line), and "Call ended — stop recording?". Plus reduce-motion on the offer.
 *
 * It FAILS (exit 1) when a capture draws nothing, when a visible text box is
 * empty, or when any visible box leaves the 340x176 window — structural, never
 * pixel equality.
 *
 * USAGE  node scripts/call-offer-shots.mjs --out <scratch dir>
 *        The PNGs are PR evidence, never a commit: --out must be outside the
 *        repo. Builds to a scratch outDir (never desktop/dist), serves it with
 *        vite preview on a free port, and stops both before it exits.
 * ENV    YAP_SMOKE_CHROME (Chromium path), YAP_CALL_OFFER_SETTLE_MS (default 900).
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
const W = 340; // lib.rs ensure_call_offer_window
const H = 176;
const SCALE = 2;
const SETTLE_MS = Number(process.env.YAP_CALL_OFFER_SETTLE_MS || 900);
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const base = {
  offerId: "offer-1",
  appName: "Zoom",
  bundleId: "us.zoom.xos",
  copy: "Zoom call — record it?",
  siteLine: null,
  yourSideOnly: false,
  countdownMsLeft: 10_000,
};

/** Each state: the CallOfferView `call_offer_state` answers, the selector that
 * must be on screen, and (optionally) a click to make before the shot. */
const STATES = [
  { id: "offer-zoom", view: { ...base, phase: "offering" }, want: '[data-call-offer="offer"]' },
  {
    id: "offer-facetime-your-side",
    view: { ...base, phase: "offering", appName: "FaceTime", bundleId: "com.apple.FaceTime", copy: "FaceTime call — record your side?", yourSideOnly: true },
    want: '[data-call-offer="offer"]',
  },
  {
    id: "offer-browser-site-unknown",
    view: { ...base, phase: "offering", appName: "Browser", bundleId: "com.google.Chrome", copy: "Browser call — record it?", siteLine: "site unknown" },
    want: ".call-offer-site",
  },
  { id: "offer-reduced-motion", view: { ...base, phase: "offering" }, want: '[data-call-offer="offer"]', reduceMotion: true },
  { id: "chip-after-expiry", view: { ...base, phase: "chip" }, want: '[data-call-offer="chip"]' },
  { id: "chip-record-rejected", view: { ...base, phase: "chip" }, want: ".call-offer-error", click: ".call-offer-chip", reject: "That call has ended, so there is nothing to record." },
  { id: "call-ended", view: { ...base, phase: "end_prompt" }, want: '[data-call-offer="ended"]' },
];

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) if (argv[i] === "--out") out.out = argv[++i];
  return out;
}

function freePort() {
  return new Promise((res, rej) => {
    const s = createServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

function fixtureScript(st) {
  const reject = st.reject ? JSON.stringify(st.reject) : "null";
  return String.raw`
window.__YAP_FIXTURES__ = { call_offer_state: ${JSON.stringify(st.view)} };
` + TAURI_SHIM + String.raw`
(() => {
  const msg = ${reject};
  const inner = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = (cmd, payload) =>
    msg && cmd === "accept_call_offer" ? Promise.reject(msg) : inner(cmd, payload);
})();
`;
}

const DETECT = String.raw`(() => {
  const bad = [];
  const root = document.querySelector(".call-offer");
  if (!root) return ["nothing-drawn"];
  for (const el of root.querySelectorAll("*")) {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    if (r.width === 0 || r.height === 0 || cs.visibility === "hidden" || cs.display === "none") continue;
    if (r.left < -0.5 || r.top < -0.5 || r.right > innerWidth + 0.5 || r.bottom > innerHeight + 0.5)
      bad.push("overflow " + el.className);
    if (/^(P|BUTTON)$/.test(el.tagName) && !el.textContent.trim()) bad.push("empty-text " + el.className);
    if (el.scrollWidth > el.clientWidth + 1 && cs.overflow !== "visible") bad.push("clipped-text " + el.className);
  }
  return bad;
})()`;

async function run() {
  const args = parseArgs(process.argv.slice(2));
  const outDir = resolve(args.out || mkdtempSync(join(tmpdir(), "yap-call-offer-shots-")));
  const SMOKE_SHOTS = join(ROOT, "docs", "smoke-shots") + "/";
  if ((outDir === ROOT || outDir.startsWith(ROOT + "/")) && !outDir.startsWith(SMOKE_SHOTS)) {
    console.error("call-offer-shots: --out is inside the repo; use a scratch dir");
    return 2;
  }
  mkdirSync(outDir, { recursive: true });
  const work = mkdtempSync(join(tmpdir(), "yap-call-offer-build-"));
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("call-offer-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    return 3;
  }
  const dist = join(work, "dist");
  const fd = openSync(join(work, "vite-build.log"), "w");
  const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
  if (b.status !== 0) {
    console.error("call-offer-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
    rmSync(work, { recursive: true, force: true });
    return 3;
  }
  const port = await freePort();
  const pfd = openSync(join(work, "vite-preview.log"), "w");
  const preview = spawn(vite, ["preview", "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], { cwd: DESKTOP, stdio: ["ignore", pfd, pfd] });
  const url = "http://127.0.0.1:" + port + "/call-offer.html";
  let chrome = null;
  const failures = [];
  try {
    let up = false;
    for (let i = 0; i < 120 && !up; i++) {
      try { up = (await fetch(url)).ok; } catch {}
      if (!up) await sleep(250);
    }
    if (!up) throw new Error("vite preview never served " + url);
    chrome = await openChrome(CHROME, "yap-call-offer-shots");
    const { cdp } = chrome;
    for (const st of STATES) {
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      await cdp.send("Page.enable", {}, s);
      await cdp.send("Runtime.enable", {}, s);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
      await cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 46, g: 49, b: 56, a: 1 } }, s);
      if (st.reduceMotion)
        await cdp.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: "reduce" }] }, s);
      await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: fixtureScript(st) }, s);
      await cdp.send("Page.navigate", { url }, s);
      const deadline = Date.now() + 8000;
      const sel = JSON.stringify(st.click || st.want);
      while (Date.now() < deadline && !(await evaluate(cdp, s, `!!document.querySelector(${sel})`).catch(() => false))) await sleep(100);
      if (st.click) {
        await evaluate(cdp, s, `document.querySelector(${sel}).click(), true`);
        const w = JSON.stringify(st.want);
        const d2 = Date.now() + 4000;
        while (Date.now() < d2 && !(await evaluate(cdp, s, `!!document.querySelector(${w})`).catch(() => false))) await sleep(100);
      }
      await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
      await sleep(SETTLE_MS);
      const present = await evaluate(cdp, s, `!!document.querySelector(${JSON.stringify(st.want)})`);
      const bad = await evaluate(cdp, s, DETECT);
      if (!present) bad.push("missing " + st.want);
      const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
      const file = join(outDir, "call-offer-" + st.id + ".png");
      writeFileSync(file, Buffer.from(shot.data, "base64"));
      console.log((bad.length ? "FAIL " : "ok   ") + st.id + " -> " + file + (bad.length ? "  " + bad.join("; ") : ""));
      if (bad.length) failures.push(st.id);
      await cdp.send("Page.close", {}, s).catch(() => {});
    }
  } catch (e) {
    console.error("call-offer-shots: " + e.message);
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
  console.log("call-offer-shots: " + (failures.length ? failures.length + " failed" : "all " + STATES.length + " states ok") + " — " + outDir);
  return failures.length ? 1 : 0;
}

run().then((code) => process.exit(code));
