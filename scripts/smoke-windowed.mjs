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

import { mkdirSync, writeFileSync, readFileSync, existsSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { TAURI_SHIM, evaluate, openChrome as openHeadlessChrome, sleep } from "./headless-chrome.mjs";

/* ── the contract ───────────────────────────────────────────────────────── */

/* The seven Nav views, in sidebar order, keyed by their Nav label. */
const NAV_VIEWS = [
  "home",
  "permissions",
  "meetings",
  "insights",
  "dictionary",
  "scratchpad",
  "settings",
];

/* The first-run overlay (Onboarding.tsx, which mounts ModelSetup's hook) is
   not a Nav view: App.tsx renders it INSTEAD of the shell while
   settings.onboarded is false. It is captured as the pseudo-view "onboarding"
   whenever the fixture set shows it, before the Nav walk. */
const ONBOARDING = "onboarding";

/* YAP_SMOKE_VIEWS — a comma list of Nav labels, case-insensitive (plus the
   pseudo-view "Onboarding"). Unset or empty = every view. An unknown label is
   an ARGUMENT error (exit 2), never a silently shorter walk: a typo that
   walked nothing would report a clean run on zero captures. */
function selectViews(raw) {
  const all = [ONBOARDING, ...NAV_VIEWS];
  if (!raw || !raw.trim()) return all;
  const want = raw.split(",").map((v) => v.trim().toLowerCase()).filter(Boolean);
  const unknown = want.filter((v) => !all.includes(v));
  if (unknown.length) {
    console.error(
      "YAP_SMOKE_VIEWS names unknown view(s): " + unknown.join(", ") +
      ". Known (case-insensitive): Onboarding, " + NAV_VIEWS.map((v) => v[0].toUpperCase() + v.slice(1)).join(", ")
    );
    process.exit(2);
  }
  return all.filter((v) => want.includes(v));
}
const SELECTED = selectViews(process.env.YAP_SMOKE_VIEWS);
const VIEWS = NAV_VIEWS.filter((v) => SELECTED.includes(v));
const WANT_ONBOARDING = SELECTED.includes(ONBOARDING);

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
/* Y7-B — one smoke invocation is ONE run directory holding TWO fixture states
   (seeded and empty) plus the pill matrix. The shell names the run and the
   state; run on its own, the driver keeps Y0-E's single-state layout. */
const RUN_NAME = process.env.YAP_SMOKE_RUN || new Date().toISOString().replace(/[:.]/g, "-");
const STATE = (process.env.YAP_SMOKE_STATE || "").trim();
const PREFIX = STATE ? STATE + "-" : "";
const RESULT_FILE = STATE ? "result-" + STATE + ".json" : "result.json";
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
/* YAP_SMOKE_FIXTURES — a path to the fixture set (resolved against the cwd,
   which smoke-windowed.sh pins to the repo root). Unset = the original
   scripts/smoke-fixtures.json, so behaviour is unchanged when it is not given.
   Persona sets: scripts/smoke-fixtures-first-run.json, scripts/smoke-fixtures-daily.json. */
const FIXTURES_PATH = process.env.YAP_SMOKE_FIXTURES
  ? resolve(process.env.YAP_SMOKE_FIXTURES)
  : new URL("./smoke-fixtures.json", import.meta.url).pathname;
if (!existsSync(FIXTURES_PATH)) {
  console.error("YAP_SMOKE_FIXTURES: no fixture file at " + FIXTURES_PATH);
  process.exit(2);
}
const FIXTURES = JSON.parse(readFileSync(FIXTURES_PATH, "utf8"));
delete FIXTURES._README;

/* TAURI_SHIM lives in scripts/headless-chrome.mjs since yap24-UI3 (shared with
   pill-matrix.mjs, which answers it from its own per-phase fixture table). */

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
/* Lives in scripts/headless-chrome.mjs since yap24-UI3, shared with
   pill-matrix.mjs so the two instruments launch the same headless Chrome. */
const openChrome = () => openHeadlessChrome(CHROME, "yap-smoke");

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


async function setSize(cdp, sessionId, w, h) {
  await cdp.send(
    "Emulation.setDeviceMetricsOverride",
    { width: w, height: h, deviceScaleFactor: 1, mobile: false },
    sessionId
  );
}

const CFG = () => `{ emptyAttr: ${JSON.stringify(EMPTY_STATE_ATTR)}, spinnerMs: ${SPINNER_BUDGET_MS} }`;

/* ── --check-only ───────────────────────────────────────────────────────── */

/* The records --check-only re-asserts: every fixture state's result file, plus
   the pill matrix's, plus one synthetic "missing-shot" finding per PNG a record
   names that is no longer on disk (a capture whose evidence is gone is not a
   capture). Pure over the run directory, so --self-test-must-fail can feed it a
   deliberately broken record and prove it is not blind. */
function loadRun(dir) {
  const views = [];
  const files = readdirSync(dir).filter((f) => /^result(-[a-z0-9-]+)?\.json$/.test(f)).sort();
  for (const f of files) {
    const r = JSON.parse(readFileSync(join(dir, f), "utf8"));
    for (const v of r.views || []) views.push({ ...v, state: v.state || r.state || "-" });
  }
  const pillPath = join(dir, "pill", "result.json");
  if (existsSync(pillPath)) {
    const r = JSON.parse(readFileSync(pillPath, "utf8"));
    for (const c of r.captures_ || [])
      views.push({
        state: "pill",
        view: "pill:" + (c.phase || c.id || "?") + "/" + (c.dock || "?") + "/" + (c.character || c.pill || "?"),
        size: r.window,
        findings: c.findings || [],
        shot: c.file || c.shot || null,
      });
  }
  for (const v of views)
    if (v.shot && !existsSync(v.shot))
      v.findings = [...v.findings, { id: "missing-shot", detail: "result names " + v.shot + " but it is not on disk" }];
  return { files, views, pill: existsSync(pillPath) };
}

function assertViews(views) {
  const bad = views.filter((v) => v.findings.length);
  for (const v of bad)
    for (const f of v.findings)
      console.log("  FAIL " + v.state + "/" + v.view + "@" + v.size + "  " + f.id + " — " + f.detail);
  return bad.length;
}

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
  const dir = join(SHOTS_ROOT, run);
  if (!existsSync(dir)) {
    console.error("check-only: pointer names run '" + run + "' but " + dir + " is missing.");
    return 3;
  }
  const loaded = loadRun(dir);
  if (!loaded.files.length) {
    console.error("check-only: run '" + run + "' has no result*.json in " + dir + ".");
    return 3;
  }
  const { views } = loaded;
  if (SELF_TEST) {
    /* The re-assertion's own non-vacuity proof: append one record that carries
       every structural finding id and require the SAME assertViews to count it. */
    const ids = ["no-text", "spinner-stuck", "doc-hscroll", "out-of-bounds", "silent-empty"];
    const synthetic = {
      state: "self-test", view: "__synthetic-broken__", size: SIZES[0].w + "x" + SIZES[0].h,
      findings: ids.map((id) => ({ id, detail: "synthetic — injected by --check-only --self-test-must-fail" })),
    };
    const before = views.filter((v) => v.findings.length).length;
    console.log("check-only self-test: re-asserting run " + run + " (" + views.length + " captures) plus one synthetic broken record");
    const after = assertViews([...views, synthetic]);
    if (after !== before + 1) {
      console.error("SELF-TEST VACUOUS — the re-assertion did not count the synthetic broken record.");
      console.log("SELF_TEST=vacuous");
      return 9;
    }
    console.error("check-only --self-test-must-fail: the synthetic break was reported, so this exits non-zero BY DESIGN.");
    console.log("SELF_TEST=satisfied");
    return 1;
  }
  console.log("check-only: re-asserting run " + run + " (" + views.length + " captures across " +
    loaded.files.join(", ") + (loaded.pill ? " + pill/result.json" : "") + ")");
  const bad = assertViews(views);
  if (bad) {
    console.error("check-only: " + bad + " capture(s) carry findings. See " + dir);
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

/* ── the first-run overlay ──────────────────────────────────────────────── */
/*
 * Captures every onboarding step reachable by its primary button (Welcome ->
 * Permissions -> ...) as onboarding-<n>, judged by the SAME five detectors as
 * the Nav views. It stops at the first step whose primary button is absent or
 * does not change the step, so it never drives a permission prompt: on the
 * first-run persona nothing is granted, and the Permissions step offers
 * "continue without" instead of a primary Next.
 */
async function captureOnboarding(cdp, s, label, outDir, views) {
  let n = 0;
  let prev = null;
  for (let step = 1; step <= 4; step++) {
    await sleep(500);
    await evaluate(cdp, s, DETECTORS + ";'loaded'");
    const res = await evaluate(cdp, s, `__yapDetect(${CFG()})`);
    const heading = await evaluate(
      cdp,
      s,
      `((document.querySelector(".onboard-step h1, .onboard-step h2, .onboard-step h3") || {}).innerText || "").trim()`
    );
    if (heading === prev) break;
    prev = heading;
    n++;
    const name = ONBOARDING + "-" + n;
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
    const file = join(outDir, PREFIX + name + "-" + label + ".png");
    writeFileSync(file, Buffer.from(shot.data, "base64"));
    const pageErrors = (cdp.errors.get(s) || []).slice(0, 6);
    views.push({ view: name, size: label, heading, findings: res.findings, rows: res.rows, chars: res.chars, shot: file, nav: "overlay", pageErrors });
    const tag = res.findings.length ? "FAIL" : "ok  ";
    console.log(tag + " " + name.padEnd(13) + label.padEnd(9) + "rows=" + String(res.rows).padEnd(4) + "chars=" + String(res.chars).padEnd(6) + file + "  [" + heading + "]");
    for (const f of res.findings) console.log("       " + f.id + " — " + f.detail);
    const moved = await evaluate(
      cdp,
      s,
      `(() => { const b = document.querySelector(".onboard-actions button.primary"); if (!b || b.disabled) return false; b.click(); return true; })()`
    );
    if (!moved) break;
  }
  return n;
}

/* ── the eight SettingsTabs ─────────────────────────────────────────────── */

async function judgeAfterSpinner(cdp, s) {
  const deadline = Date.now() + SPINNER_BUDGET_MS;
  let res;
  for (;;) {
    await evaluate(cdp, s, DETECTORS + ";'loaded'");
    res = await evaluate(cdp, s, `__yapDetect(${CFG()})`);
    const stuck = res.findings.some((f) => f.id === "spinner-stuck");
    if (!stuck || Date.now() > deadline) break;
    await sleep(400);
  }
  return res;
}

async function captureSettingsTabs(cdp, s, label, outDir, views) {
  const tabs = await evaluate(
    cdp,
    s,
    `[...document.querySelectorAll('.settings-subnav [role="tab"]')].map(b => (b.innerText || "").trim())`
  );
  if (!tabs.length) {
    views.push({ view: "settings-tabs", size: label, findings: [{ id: "no-settings-tabs", detail: "Settings rendered no [role=tab] in .settings-subnav" }], rows: 0, chars: 0, shot: null, nav: "none", pageErrors: [] });
    console.log("FAIL settings-tabs  " + label + " no SettingsTab buttons found");
    return;
  }
  for (let i = 0; i < tabs.length; i++) {
    const slug = tabs[i].toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "tab" + i;
    const name = "settings-" + slug;
    const clicked = await evaluate(
      cdp,
      s,
      `(() => { const b = document.querySelectorAll('.settings-subnav [role="tab"]')[${i}]; if (!b) return false; b.click(); return true; })()`
    );
    await sleep(300);
    const res = await judgeAfterSpinner(cdp, s);
    if (!clicked) res.findings.push({ id: "no-nav-button", detail: "SettingsTab #" + i + " (" + tabs[i] + ") vanished before it could be clicked" });
    const selected = await evaluate(
      cdp,
      s,
      `((document.querySelectorAll('.settings-subnav [role="tab"]')[${i}] || {}).getAttribute || (() => null)).call(document.querySelectorAll('.settings-subnav [role="tab"]')[${i}], "aria-selected")`
    );
    if (selected !== "true") res.findings.push({ id: "tab-not-selected", detail: "clicked SettingsTab '" + tabs[i] + "' but aria-selected=" + selected });
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
    const file = join(outDir, PREFIX + name + "-" + label + ".png");
    writeFileSync(file, Buffer.from(shot.data, "base64"));
    const pageErrors = (cdp.errors.get(s) || []).slice(0, 6);
    views.push({ view: name, size: label, findings: res.findings, rows: res.rows, chars: res.chars, shot: file, nav: "settings-tab", pageErrors });
    const tag = res.findings.length ? "FAIL" : "ok  ";
    console.log(tag + " " + name.padEnd(20) + label.padEnd(9) + "rows=" + String(res.rows).padEnd(4) + "chars=" + String(res.chars).padEnd(6) + file);
    for (const f of res.findings) console.log("       " + f.id + " — " + f.detail);
  }
}

/* ── the capture walk ───────────────────────────────────────────────────── */

async function capture() {
  const run = RUN_NAME;
  const outDir = join(SHOTS_ROOT, run);
  mkdirSync(outDir, { recursive: true });

  const { proc, cdp, ws } = await openChrome();
  const views = [];
  try {
    for (const size of SIZES) {
      const s = await newPage(cdp, URL_BASE + "/index.html");
      await setSize(cdp, s, size.w, size.h);
      await sleep(1200);
      const label = size.w + "x" + size.h;

      /* ── the first-run overlay, when this fixture set shows it ── */
      const overlay = await evaluate(cdp, s, `!!document.querySelector(".onboard-overlay")`);
      if (overlay) {
        const steps = WANT_ONBOARDING ? await captureOnboarding(cdp, s, label, outDir, views) : 0;
        /* The Nav views are unreachable behind the overlay, so leave it the way
           a user who skips does: Back to the first step, then its Skip button
           (onSkip -> finishOnboarding -> save_settings, which the first-run
           fixture set resolves). */
        const left = await evaluate(
          cdp,
          s,
          `(async () => {
             for (let i = 0; i < 4; i++) {
               const skip = [...document.querySelectorAll(".onboard-actions button")]
                 .find(b => /skip/i.test(b.innerText || ""));
               if (skip) { skip.click(); break; }
               const back = [...document.querySelectorAll(".onboard-actions button")]
                 .find(b => /back/i.test(b.innerText || ""));
               if (!back) break;
               back.click();
               await new Promise(r => setTimeout(r, 300));
             }
             await new Promise(r => setTimeout(r, 600));
             return document.querySelector(".onboard-overlay") ? "still-open" : "dismissed";
           })()`
        );
        console.log("     onboarding@" + label + ": " + steps + " step(s) captured, overlay " + left);
        if (left !== "dismissed" && VIEWS.length)
          console.error("     onboarding overlay would not dismiss — the Nav walk below runs against the overlay");
      } else if (WANT_ONBOARDING && process.env.YAP_SMOKE_VIEWS) {
        console.log("     onboarding@" + label + ": not shown by this fixture set (settings.onboarded is true)");
      }

      for (const view of VIEWS) {
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
        const res = await judgeAfterSpinner(cdp, s);
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
        const file = join(outDir, PREFIX + view + "-" + label + ".png");
        writeFileSync(file, Buffer.from(shot.data, "base64"));

        const pageErrors = (cdp.errors.get(s) || []).slice(0, 6);
        views.push({ view, size: label, findings: res.findings, rows: res.rows, chars: res.chars, shot: file, nav, pageErrors });
        const tag = res.findings.length ? "FAIL" : "ok  ";
        console.log(tag + " " + view.padEnd(12) + label.padEnd(9) + "rows=" + String(res.rows).padEnd(4) + "chars=" + String(res.chars).padEnd(6) + file);
        for (const f of res.findings) console.log("       " + f.id + " — " + f.detail);
        for (const e of pageErrors) console.log("       page: " + e.split("\n")[0]);

        /* Y7-B — Settings is eight panels behind one Nav entry. Walk every
           SettingsTab (the segmented sub-nav's role=tab buttons, so a tab a
           later item adds is captured with no edit here) and judge each one
           with the same detectors. Zero tabs is a finding, never a skip. */
        if (view === "settings" && nav === "clicked") await captureSettingsTabs(cdp, s, label, outDir, views);
      }
      await cdp.send("Page.close", {}, s).catch(() => {});
    }
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
  }

  const failed = views.filter((v) => v.findings.length);
  for (const v of views) v.state = STATE || "-";
  const result = {
    run,
    state: STATE || null,
    url: URL_BASE,
    fixtures: FIXTURES_PATH,
    selected: SELECTED,
    sizes: SIZES.map((s) => s.w + "x" + s.h),
    emptyStateAttr: EMPTY_STATE_ATTR,
    spinnerBudgetMs: SPINNER_BUDGET_MS,
    captures: views.length,
    failing: failed.length,
    views,
  };
  writeFileSync(join(outDir, RESULT_FILE), JSON.stringify(result, null, 2));
  writeFileSync(join(SHOTS_ROOT, "LATEST"), run + "\n");

  console.log("");
  console.log("run " + run + " — " + views.length + " captures, " + failed.length + " carrying findings");
  console.log("shots + " + RESULT_FILE + ": " + outDir);
  return failed.length ? 1 : 0;
}

/* ── main ───────────────────────────────────────────────────────────────── */

/* --check-only wins over --self-test-must-fail: together they self-test the
   RE-ASSERTION (no browser), which is what the pre-flight runs. */
const code = CHECK_ONLY ? checkOnly() : SELF_TEST ? await selfTest() : await capture();
process.exit(code);
