#!/usr/bin/env node
/**
 * Y0-E — the structural windowed smoke DRIVER.
 *
 * Shipped ahead of the nine UI items it judges, because the merge gate is eight
 * headless commands (tsc, vitest, vite, two cargo builds, two cargo test runs)
 * and not one of them can see "looks broken". This is the instrument that can.
 *
 * It is STRUCTURAL, never pixel equality against a golden image: a legitimate
 * restyle must never have to relitigate a PNG. The five conditions are facts
 * about the DOM and the layout box tree, so they survive any restyle and only
 * fire on something a human would call broken.
 *
 * NO npm DEPENDENCIES. Node 24 ships a global WebSocket, so this speaks the
 * Chrome DevTools Protocol directly. Adding Playwright here would mean a
 * ~400 MB browser download inside a loop worktree that is deleted at Drain.
 *
 * WHAT IS DRIVEN, PRECISELY (read this before believing a result):
 * the harness loads the SAME built `desktop/dist` bundle the Tauri app loads
 * (tauri.conf.json frontendDist = "../dist"), served by `vite preview`, inside
 * headless Chrome at exact viewport sizes. It is NOT the WKWebView, because
 * WKWebView has no scriptable inspector on macOS — Safari Web Inspector cannot
 * be driven from a script. `window.__TAURI_INTERNALS__` is therefore shimmed
 * (see TAURI_SHIM) so the views render their unfed structure instead of a
 * white page. Everything the five detectors look at — text presence, spinner
 * lifetime, document scroll width, layout boxes, row counts — is a property of
 * that bundle and that viewport, not of the native shell.
 */

import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/* ── the contract ───────────────────────────────────────────────────────── */

const VIEWS = [
  "home",
  "permissions",
  "meetings",
  "insights",
  "dictionary",
  "scratchpad",
  "settings",
];

const SIZES = [
  { w: Number(process.env.YAP_SMOKE_W1 || 980), h: Number(process.env.YAP_SMOKE_H1 || 700) },
  { w: Number(process.env.YAP_SMOKE_W2 || 720), h: Number(process.env.YAP_SMOKE_H2 || 520) },
];

const EMPTY_STATE_ATTR = process.env.YAP_SMOKE_EMPTY_ATTR || "data-empty-state";
const SPINNER_BUDGET_MS = Number(process.env.YAP_SMOKE_SPINNER_MS || 10_000);

const args = new Set(process.argv.slice(2));
const CHECK_ONLY = args.has("--check-only");
const SELF_TEST = args.has("--self-test-must-fail");
const URL_BASE = process.env.YAP_SMOKE_URL || "http://127.0.0.1:5273";
const SHOTS_ROOT = process.env.YAP_SMOKE_SHOTS || "docs/smoke-shots";
const CHROME =
  process.env.YAP_SMOKE_CHROME ||
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

/* ── the Tauri shim ─────────────────────────────────────────────────────── */
/*
 * Injected before any bundle script runs. Unknown commands REJECT on purpose:
 * App.tsx seeds every list with an empty initial state and only replaces it on
 * a resolved invoke, so a rejection leaves the view rendering its real unfed
 * shape — which is exactly the surface these detectors judge. Resolving a
 * wrong-shaped object instead would throw inside render and blank the view,
 * turning an instrument into a noise generator.
 */
const FIXTURES = JSON.parse(
  readFileSync(new URL("./smoke-fixtures.json", import.meta.url), "utf8")
);
delete FIXTURES._README;

const TAURI_SHIM = String.raw`
(() => {
  const EMPTY_LIST = /^(list_|get_history$|.*_series$|meeting_kind_choices$|list_crash_events$)/;
  const called = [];
  window.__YAP_SMOKE_INVOKES__ = called;
  /* Answers come from scripts/smoke-fixtures.json, injected as __YAP_FIXTURES__.
     Only values whose SHAPE is certain belong there: resolving a guessed object
     shape is worse than rejecting, because it replaces a component's
     well-formed initial state with a half-built one and throws inside render,
     which turns condition 1 into noise on every view at once. Measured: seeding
     get_settings with {} crashed the whole app with
     "Cannot read properties of undefined (reading 'map')". */
  const table = window.__YAP_FIXTURES__ || {};
  function invoke(cmd, payload) {
    called.push(cmd);
    if (typeof cmd === "string" && cmd.startsWith("plugin:")) return Promise.resolve(null);
    if (Object.prototype.hasOwnProperty.call(table, cmd)) return Promise.resolve(table[cmd]);
    if (EMPTY_LIST.test(cmd)) return Promise.resolve([]);
    return Promise.reject(new Error("smoke: no fixture for " + cmd));
  }
  let cbId = 0;
  window.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback(cb, once) {
      const id = ++cbId;
      const key = "_" + id;
      Object.defineProperty(window, key, {
        value: (r) => { if (once) Reflect.deleteProperty(window, key); return cb && cb(r); },
        writable: false, configurable: true,
      });
      return id;
    },
    convertFileSrc: (p) => p,
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    plugins: {},
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => Promise.resolve() };
  window.addEventListener("unhandledrejection", (e) => e.preventDefault());
})();
`;

/* ── the five detectors, evaluated inside the page ──────────────────────── */
/*
 * One function, serialized into the page, used by BOTH the real walk and
 * --self-test-must-fail. The self-test proves THIS code is non-vacuous; a
 * self-test that exercised a different copy would prove nothing.
 */
const DETECTORS = String.raw`
function __yapDetect(cfg) {
  const out = [];
  const tol = 1;
  const root =
    document.querySelector(".content") ||
    document.querySelector(".main") ||
    document.body;

  /* 1 — a view whose content region renders no text at all. */
  const text = (root.innerText || "").replace(/\s+/g, "");
  if (text.length === 0) out.push({ id: "no-text", detail: "content region has zero characters of rendered text" });

  /* 2 — a spinner still present after the budget. */
  const spin = document.querySelectorAll(
    '.spinner, .loading, .skeleton, [data-spinner], [aria-busy="true"]'
  );
  const liveSpin = [...spin].filter((el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden";
  });
  if (liveSpin.length)
    out.push({
      id: "spinner-stuck",
      detail: liveSpin.length + " spinner/skeleton element(s) still visible after " + cfg.spinnerMs + " ms: " +
        liveSpin.slice(0, 3).map((e) => e.tagName.toLowerCase() + "." + (e.className || "")).join(", "),
    });

  /* 3 — a horizontal scrollbar on the document. */
  const de = document.documentElement;
  if (de.scrollWidth > de.clientWidth + tol)
    out.push({
      id: "doc-hscroll",
      detail: "documentElement.scrollWidth=" + de.scrollWidth + " > clientWidth=" + de.clientWidth,
    });

  /* 4 — any element's box outside the window bounds.
     An element whose overflow an ancestor clips (overflow hidden/clip/auto/
     scroll) is legitimately contained and is NOT a finding; condition 3 already
     catches the document-level case. Below-the-fold is normal scrolling, so
     only the horizontal edges and the top edge are bounds. */
  const W = window.innerWidth, H = window.innerHeight;
  function clipped(el) {
    for (let p = el.parentElement; p && p !== document.documentElement; p = p.parentElement) {
      const ox = getComputedStyle(p).overflowX;
      if (ox === "hidden" || ox === "clip" || ox === "auto" || ox === "scroll") return true;
    }
    return false;
  }
  const strays = [];
  for (const el of document.body.querySelectorAll("*")) {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === "hidden" || cs.display === "none" || cs.opacity === "0") continue;
    const outside = r.left < -tol || r.right > W + tol || r.top < -tol;
    if (!outside) continue;
    if (clipped(el)) continue;
    strays.push(
      el.tagName.toLowerCase() + (el.className ? "." + String(el.className).split(" ")[0] : "") +
      " [" + Math.round(r.left) + "," + Math.round(r.top) + " " + Math.round(r.width) + "x" + Math.round(r.height) + "]"
    );
    if (strays.length >= 5) break;
  }
  if (strays.length)
    out.push({ id: "out-of-bounds", detail: "box(es) outside the " + W + "x" + H + " window: " + strays.join("; ") });

  /* 5 — zero rows AND no empty-state marker. */
  const rows = root.querySelectorAll("[data-row], li, tbody tr, .card, .entry, .item, .row");
  const empties = root.querySelectorAll("[" + cfg.emptyAttr + "]");
  if (rows.length === 0 && empties.length === 0)
    out.push({
      id: "silent-empty",
      detail: "zero rows rendered and no [" + cfg.emptyAttr + "] element — the view is empty without saying so",
    });

  /* The boot-error screen means the shim failed to feed the app, not that a
     view is broken. Report it separately so it can never masquerade as one. */
  const boot = document.body.innerText || "";
  const bootError = boot.includes("UI failed to load backend bridge")
    ? boot.replace(/\s+/g, " ").slice(0, 200)
    : null;

  return { bootError, findings: out, rows: rows.length, chars: text.length, invokes: (window.__YAP_SMOKE_INVOKES__ || []).length };
}
`;

/* ── CDP over a bare WebSocket ──────────────────────────────────────────── */

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    /* A page that throws on mount renders nothing, and "renders nothing" is
       condition 1. Without the exception text, condition 1 says "broken" and
       cannot say WHY — so the instrument collects every uncaught exception and
       console error and files them with the capture. */
    this.errors = new Map();
    ws.addEventListener("message", (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id && this.pending.has(m.id)) {
        const { resolve, reject } = this.pending.get(m.id);
        this.pending.delete(m.id);
        m.error ? reject(new Error(m.error.message)) : resolve(m.result);
        return;
      }
      if (!m.method || !m.sessionId) return;
      const bucket = this.errors.get(m.sessionId) || [];
      if (m.method === "Runtime.exceptionThrown") {
        const d = m.params.exceptionDetails;
        bucket.push("uncaught: " + (d.exception?.description || d.text));
      } else if (m.method === "Runtime.consoleAPICalled" && m.params.type === "error") {
        bucket.push("console.error: " + m.params.args.map((a) => a.description || a.value).join(" "));
      } else return;
      this.errors.set(m.sessionId, bucket);
    });
  }
  send(method, params = {}, sessionId) {
    const id = ++this.id;
    const msg = { id, method, params };
    if (sessionId) msg.sessionId = sessionId;
    this.ws.send(JSON.stringify(msg));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function waitForJson(url, tries = 100) {
  for (let i = 0; i < tries; i++) {
    try {
      const r = await fetch(url);
      if (r.ok) return await r.json();
    } catch {}
    await sleep(100);
  }
  throw new Error("timed out waiting for " + url);
}

async function openChrome() {
  if (!existsSync(CHROME)) {
    throw new Error(
      "Chrome not found at " + CHROME + " — this smoke needs a Chromium to speak CDP to. " +
      "Set YAP_SMOKE_CHROME. (This is an ENVIRONMENT failure, not a UI defect.)"
    );
  }
  const port = 9300 + Math.floor(Math.random() * 400);
  const profile = join(tmpdir(), "yap-smoke-" + process.pid + "-" + port);
  const proc = spawn(
    CHROME,
    [
      "--headless=new",
      "--remote-debugging-port=" + port,
      "--user-data-dir=" + profile,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-extensions",
      "--hide-scrollbars=false",
      "about:blank",
    ],
    { stdio: "ignore" }
  );
  const version = await waitForJson("http://127.0.0.1:" + port + "/json/version");
  const ws = new WebSocket(version.webSocketDebuggerUrl);
  await new Promise((res, rej) => {
    ws.addEventListener("open", res, { once: true });
    ws.addEventListener("error", rej, { once: true });
  });
  return { proc, cdp: new Cdp(ws), ws };
}

async function newPage(cdp, url) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Runtime.enable", {}, sessionId);
  await cdp.send(
    "Page.addScriptToEvaluateOnNewDocument",
    { source: "window.__YAP_FIXTURES__ = " + JSON.stringify(FIXTURES) + ";\n" + TAURI_SHIM },
    sessionId
  );
  if (url) await cdp.send("Page.navigate", { url }, sessionId);
  return sessionId;
}

async function evaluate(cdp, sessionId, expression) {
  const r = await cdp.send(
    "Runtime.evaluate",
    { expression, returnByValue: true, awaitPromise: true },
    sessionId
  );
  if (r.exceptionDetails) {
    throw new Error("page evaluate threw: " + (r.exceptionDetails.exception?.description || r.exceptionDetails.text));
  }
  return r.result.value;
}

async function setSize(cdp, sessionId, w, h) {
  await cdp.send(
    "Emulation.setDeviceMetricsOverride",
    { width: w, height: h, deviceScaleFactor: 1, mobile: false },
    sessionId
  );
}

const CFG = () => `{ emptyAttr: ${JSON.stringify(EMPTY_STATE_ATTR)}, spinnerMs: ${SPINNER_BUDGET_MS} }`;

/* ── --check-only ───────────────────────────────────────────────────────── */

function checkOnly() {
  const pointer = join(SHOTS_ROOT, "LATEST");
  if (!existsSync(pointer)) {
    console.error(
      "check-only: no captured run at " + pointer + ".\n" +
      "--check-only RE-ASSERTS the last capture. It is a pre-flight convenience, never a\n" +
      "substitute for one, so with nothing captured it can only fail."
    );
    return 3;
  }
  const run = readFileSync(pointer, "utf8").trim();
  const resultPath = join(SHOTS_ROOT, run, "result.json");
  if (!existsSync(resultPath)) {
    console.error("check-only: pointer names run '" + run + "' but " + resultPath + " is missing.");
    return 3;
  }
  const result = JSON.parse(readFileSync(resultPath, "utf8"));
  const bad = result.views.filter((v) => v.findings.length);
  console.log("check-only: re-asserting run " + run + " (" + result.views.length + " view/size captures)");
  for (const v of bad)
    for (const f of v.findings)
      console.log("  FAIL " + v.view + "@" + v.size + "  " + f.id + " — " + f.detail);
  if (bad.length) {
    console.error("check-only: " + bad.length + " capture(s) carry findings. See " + resultPath);
    return 1;
  }
  console.log("check-only: last run was clean.");
  return 0;
}

/* ── --self-test-must-fail ──────────────────────────────────────────────── */
/*
 * Injects one synthetic view that violates all five conditions at once and
 * requires the detector to report every one of them. The MODE IS NAMED
 * must-fail because the run it performs contains a deliberate break, so a
 * satisfied self-test still exits NON-ZERO (1). A detector that stayed silent
 * exits 9 — louder, and a different number, because that is the alarm.
 */
const SYNTHETIC_BROKEN_VIEW = String.raw`
document.documentElement.innerHTML = '<head><style>body{margin:0}</style></head><body>' +
  '<div class="content" style="width:100%"></div>' +
  '<div class="spinner" style="position:absolute;top:0;left:0;width:24px;height:24px;background:#000"></div>' +
  '<div style="width:4000px;height:20px;background:#f00"></div>' +
  '</body>';
'injected';
`;

async function selfTest() {
  const expect = ["no-text", "spinner-stuck", "doc-hscroll", "out-of-bounds", "silent-empty"];
  const { proc, cdp, ws } = await openChrome();
  try {
    const s = await newPage(cdp, null);
    await setSize(cdp, s, SIZES[0].w, SIZES[0].h);
    await evaluate(cdp, s, SYNTHETIC_BROKEN_VIEW);
    await evaluate(cdp, s, DETECTORS + ";'loaded'");
    const res = await evaluate(cdp, s, `__yapDetect(${CFG()})`);
    const got = res.findings.map((f) => f.id);
    const missed = expect.filter((e) => !got.includes(e));
    console.log("self-test: synthetic broken view injected (no text / stuck spinner / 4000px row / zero rows / no [" + EMPTY_STATE_ATTR + "])");
    for (const f of res.findings) console.log("  detector fired: " + f.id + " — " + f.detail);
    if (missed.length) {
      console.error("SELF-TEST VACUOUS — detector stayed blind to: " + missed.join(", "));
      console.log("SELF_TEST=vacuous");
      return 9;
    }
    console.error(
      "self-test-must-fail: all " + expect.length + " detectors fired on the synthetic break, so this run " +
      "carries a deliberate failure and exits non-zero BY DESIGN. That non-zero IS the proof."
    );
    console.log("SELF_TEST=satisfied");
    return 1;
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
  }
}

/* ── the capture walk ───────────────────────────────────────────────────── */

async function capture() {
  const run = new Date().toISOString().replace(/[:.]/g, "-");
  const outDir = join(SHOTS_ROOT, run);
  mkdirSync(outDir, { recursive: true });

  const { proc, cdp, ws } = await openChrome();
  const views = [];
  try {
    for (const size of SIZES) {
      const s = await newPage(cdp, URL_BASE + "/index.html");
      await setSize(cdp, s, size.w, size.h);
      await sleep(1200);
      for (const view of VIEWS) {
        const label = size.w + "x" + size.h;
        const nav = await evaluate(
          cdp,
          s,
          `(() => {
             const want = ${JSON.stringify(view)};
             const btns = [...document.querySelectorAll("nav.nav button, nav button, .nav-item")];
             const hit = btns.find(b => (b.innerText||"").trim().toLowerCase().startsWith(want));
             if (!hit) return "no-nav-button";
             hit.click();
             return "clicked";
           })()`
        );
        /* Condition 2 is a DEADLINE, not a sample: poll until the spinner is
           gone or the budget is spent, then judge. */
        const deadline = Date.now() + SPINNER_BUDGET_MS;
        let res;
        for (;;) {
          await evaluate(cdp, s, DETECTORS + ";'loaded'");
          res = await evaluate(cdp, s, `__yapDetect(${CFG()})`);
          const stuck = res.findings.some((f) => f.id === "spinner-stuck");
          if (!stuck || Date.now() > deadline) break;
          await sleep(400);
        }
        /* A stale fixture is an INSTRUMENT failure, not a UI defect, and it
           produces a wall of convincing-looking structural findings. Name it. */
        if (res.bootError) {
          console.error(
            "FIXTURE STALE — the app rendered its boot-error screen instead of the UI:\n  " +
            res.bootError + "\nUpdate scripts/smoke-fixtures.json. No structural finding in this " +
            "run is trustworthy."
          );
          throw new Error("FIXTURE STALE: " + res.bootError);
        }
        if (nav === "no-nav-button")
          res.findings.push({ id: "no-nav-button", detail: "no nav button whose label starts with '" + view + "'" });

        const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
        const file = join(outDir, view + "-" + label + ".png");
        writeFileSync(file, Buffer.from(shot.data, "base64"));

        const pageErrors = (cdp.errors.get(s) || []).slice(0, 6);
        views.push({ view, size: label, findings: res.findings, rows: res.rows, chars: res.chars, shot: file, nav, pageErrors });
        const tag = res.findings.length ? "FAIL" : "ok  ";
        console.log(tag + " " + view.padEnd(12) + label.padEnd(9) + "rows=" + String(res.rows).padEnd(4) + "chars=" + String(res.chars).padEnd(6) + file);
        for (const f of res.findings) console.log("       " + f.id + " — " + f.detail);
        for (const e of pageErrors) console.log("       page: " + e.split("\n")[0]);
      }
      await cdp.send("Page.close", {}, s).catch(() => {});
    }
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
  }

  const failed = views.filter((v) => v.findings.length);
  const result = {
    run,
    url: URL_BASE,
    sizes: SIZES.map((s) => s.w + "x" + s.h),
    emptyStateAttr: EMPTY_STATE_ATTR,
    spinnerBudgetMs: SPINNER_BUDGET_MS,
    captures: views.length,
    failing: failed.length,
    views,
  };
  writeFileSync(join(outDir, "result.json"), JSON.stringify(result, null, 2));
  writeFileSync(join(SHOTS_ROOT, "LATEST"), run + "\n");

  console.log("");
  console.log("run " + run + " — " + views.length + " captures, " + failed.length + " carrying findings");
  console.log("shots + result.json: " + outDir);
  return failed.length ? 1 : 0;
}

/* ── main ───────────────────────────────────────────────────────────────── */

const code = CHECK_ONLY ? checkOnly() : SELF_TEST ? await selfTest() : await capture();
process.exit(code);
