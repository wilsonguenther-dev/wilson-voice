#!/usr/bin/env node
/**
 * yap-1107-meetings-row-actions-no-overlay — the Meetings list's row geometry,
 * measured, plus 2x PNGs for the PR evidence. Wilson (2026-11-07 12:47Z, Yap
 * Test 0.9.4-loop3.8, ~1190x773): hovering a row put an Export ▾ / Delete bar
 * ON TOP of the title, date and chips, and the Export list was clipped.
 *
 * Pattern: call-offer-shots.mjs / license-purchase-shots.mjs — vite build to a
 * scratch dir (never desktop/dist), vite preview on a free port, real Chrome
 * --headless=new (scripts/headless-chrome.mjs; never a window, never the app),
 * the shared TAURI_SHIM answering invoke() from scripts/smoke-fixtures.json
 * plus a meetings fixture: 5 rows across Today / Yesterday — a long title with
 * You + Call chips and notes ready, a short title, a Transcribing row, an
 * in-person row and a failed row (Retry, the one status button a row keeps).
 * Viewport 1190x773 logical at deviceScaleFactor 2.
 *
 * CHECKS (exit 1 on any failure, 3 on an instrument/environment failure):
 *   A1  idle, hover (each row in turn), focus-within and selected: for every
 *       row, no visible action rect (any button / menu trigger in the row other
 *       than the row's own open button) intersects any content rect (title,
 *       time/duration, state chip, person chips, detail line); title and
 *       time/duration are visible (not display:none / visibility:hidden /
 *       opacity 0), inside the row, and not clipped unless ellipsised by design.
 *   A2  every row's height AND top are identical in idle and hover (0 px).
 *   A3  Export opened (detail header, with the pointer having hovered a row):
 *       the whole menu is inside the viewport, elementFromPoint at each item's
 *       centre is that item, and moving the pointer from the trigger into the
 *       menu keeps it open. The row's right-click menu is checked the same way.
 *   A4  the right-click menu offers Export Markdown… / Copy as Markdown / Show
 *       in Finder / Delete meeting; Delete shows the Undo bar ("s to undo"),
 *       and Undo restores the row.
 *
 * USAGE   node scripts/meetings-row-shots.mjs [--out <dir>] [--label after|before] [--dist <built dist>]
 *   --out    default docs/loop/evidence/yap-1107-meetings-row-actions-no-overlay
 *   --label  PNG name prefix (default "after"). The BEFORE shots come from the
 *            same script run in an origin/main worktree with --label before; it
 *            is EXPECTED to exit 1 there (the overlap is the defect).
 *   --dist   reuse an already-built desktop dist instead of building.
 * ENV     YAP_SMOKE_CHROME (Chromium path), YAP_ROW_SHOTS_SETTLE_MS (default 700).
 * Everything it starts (vite preview, Chrome, temp profile, scratch build) is
 * stopped/deleted before it exits.
 */
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { TAURI_SHIM, evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ID = "yap-1107-meetings-row-actions-no-overlay";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
const W = 1190;
const H = 773;
const SCALE = 2;
const SETTLE_MS = Number(process.env.YAP_ROW_SHOTS_SETTLE_MS || 700);
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

const arg = (k, d) => {
  const i = process.argv.indexOf(k);
  return i > 0 ? process.argv[i + 1] : d;
};
const OUT = resolve(arg("--out", join(ROOT, "docs", "loop", "evidence", ID)));
const LABEL = arg("--label", "after");
const PREBUILT = arg("--dist", null);

/* ── the meetings fixture ─────────────────────────────────────────────── */
const now = Date.now();
const iso = (msAgo) => new Date(now - msAgo).toISOString();
const HOUR = 3600_000;
const meeting = (o) => ({
  source: "manual",
  kind: "virtual",
  endedAt: null,
  error: null,
  processedThroughSeconds: 0,
  audioKept: true,
  micWavPath: "/tmp/fixture-mic.wav",
  sysWavPath: null,
  tapRebuilds: null,
  summary: null,
  summaryModel: null,
  segmentCount: 40,
  ...o,
  createdAt: o.startedAt,
});
const MEETINGS = [
  meeting({
    id: "m-long-0001",
    title: "Quarterly planning with the whole launch team, pricing, onboarding and the App Store review notes",
    startedAt: iso(1 * HOUR),
    durationSeconds: 3125,
    state: "complete",
    sysWavPath: "/tmp/fixture-sys.wav",
    summary: "Launch moves to the 14th; checkout must be ready first.\n- owner: Wilson",
    summaryModel: "fixture",
  }),
  meeting({ id: "m-short-0002", title: "Retro", startedAt: iso(3 * HOUR), durationSeconds: 412, state: "complete", kind: "in_person" }),
  meeting({
    id: "m-trans-0003",
    title: "Design sync",
    startedAt: iso(26 * HOUR),
    durationSeconds: 1800,
    state: "transcribing",
    processedThroughSeconds: 600,
    segmentCount: 0,
  }),
  meeting({ id: "m-room-0004", title: "1:1", startedAt: iso(28 * HOUR), durationSeconds: 1500, state: "complete", kind: "unknown" }),
  meeting({
    id: "m-fail-0005",
    title: "Customer call that stopped early",
    startedAt: iso(30 * HOUR),
    durationSeconds: 95,
    state: "failed",
    error: "Transcription didn't finish.",
    segmentCount: 0,
  }),
];

function fixtures() {
  const base = JSON.parse(readFileSync(join(ROOT, "scripts", "smoke-fixtures.json"), "utf8"));
  delete base._README;
  return {
    ...base,
    list_meetings: MEETINGS,
    get_meeting: { meeting: MEETINGS[0], segments: [], audioOnDisk: true },
    delete_meeting: null,
  };
}

/* ── page-side measurements ───────────────────────────────────────────── */
const MEASURE = String.raw`((only) => {
  const vis = (el) => {
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return cs.display !== "none" && cs.visibility !== "hidden" && Number(cs.opacity) > 0.01 && r.width > 0 && r.height > 0;
  };
  const R = (el) => { const r = el.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom, h: r.height }; };
  const hit = (a, b) => Math.min(a.r, b.r) - Math.max(a.l, b.l) > 0.5 && Math.min(a.b, b.b) - Math.max(a.t, b.t) > 0.5;
  const out = [];
  for (const row of document.querySelectorAll("li.meeting-row[data-meeting-id]")) {
    const id = row.dataset.meetingId;
    const rr = R(row);
    if (only && id !== only) { out.push({ id, top: rr.t, height: rr.h, bad: [] }); continue; }
    const bad = [];
    const content = [];
    for (const sel of [".meeting-row-title", ".meeting-row-when", ".meeting-state", ".person-chip", ".meeting-row-detail"])
      for (const el of row.querySelectorAll(sel)) if (vis(el)) content.push([sel, el]);
    for (const sel of [".meeting-row-title", ".meeting-row-when"]) {
      const el = row.querySelector(sel);
      if (!el) { bad.push("missing " + sel); continue; }
      if (!vis(el)) { bad.push("hidden " + sel); continue; }
      if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).textOverflow !== "ellipsis") bad.push("clipped " + sel);
      const r = R(el);
      if (r.l < rr.l - 0.5 || r.r > rr.r + 0.5 || r.t < rr.t - 0.5 || r.b > rr.b + 0.5) bad.push("outside-row " + sel);
    }
    const actions = [...row.querySelectorAll("button, [role=button], [aria-haspopup]")].filter(
      (el) => !el.classList.contains("meeting-row-main") && vis(el) && !content.some(([, c]) => c === el || c.contains(el)),
    );
    for (const a of actions)
      for (const [sel, c] of content)
        if (hit(R(a), R(c))) bad.push("overlap '" + (a.textContent || "").trim().slice(0, 24) + "' x " + sel);
    out.push({ id, top: rr.t, height: rr.h, actions: actions.length, content: content.length, bad });
  }
  return out;
})`;

const MENU_CHECK = String.raw`((name) => {
  const m = document.querySelector('.floating-menu[data-floating-menu="' + name + '"]') || document.querySelector('[role="menu"]');
  if (!m) return { open: false, bad: ["menu not open"] };
  const r = m.getBoundingClientRect();
  const bad = [];
  if (r.left < 0 || r.top < 0 || r.right > innerWidth || r.bottom > innerHeight) bad.push("menu outside viewport " + JSON.stringify([r.left, r.top, r.right, r.bottom]));
  const items = [...m.querySelectorAll('[role="menuitem"]')];
  for (const it of items) {
    const b = it.getBoundingClientRect();
    const at = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
    if (!(at === it || it.contains(at))) bad.push("item covered/clipped: " + it.textContent.trim());
  }
  return { open: true, rect: [r.left, r.top, r.right, r.bottom], labels: items.map((i) => i.textContent.trim()), bad };
})`;

/* ── CDP helpers ──────────────────────────────────────────────────────── */
async function mouse(cdp, s, type, x, y, button = "none", extra = {}) {
  await cdp.send("Input.dispatchMouseEvent", { type, x, y, button, ...extra }, s);
}
async function click(cdp, s, x, y, button = "left") {
  const buttons = button === "right" ? 2 : 1;
  await mouse(cdp, s, "mouseMoved", x, y);
  await mouse(cdp, s, "mousePressed", x, y, button, { clickCount: 1, buttons });
  await mouse(cdp, s, "mouseReleased", x, y, button, { clickCount: 1, buttons: 0 });
}
async function key(cdp, s, keyName, code, vk) {
  await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", key: keyName, code, windowsVirtualKeyCode: vk }, s);
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key: keyName, code, windowsVirtualKeyCode: vk }, s);
}
async function centre(cdp, s, selector) {
  return evaluate(
    cdp,
    s,
    `(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return null; const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2, left: r.left, top: r.top, right: r.right, bottom: r.bottom }; })()`,
  );
}
async function waitFor(cdp, s, expr, ms = 8000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (await evaluate(cdp, s, expr).catch(() => false)) return true;
    await sleep(100);
  }
  return false;
}
async function shot(cdp, s, name) {
  await sleep(SETTLE_MS);
  const png = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(join(OUT, `${LABEL}-${name}.png`), Buffer.from(png.data, "base64"));
}

function freePort() {
  return new Promise((res, rej) => {
    const srv = createServer();
    srv.once("error", rej);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => res(port));
    });
  });
}

/* ── the run ──────────────────────────────────────────────────────────── */
async function run() {
  mkdirSync(OUT, { recursive: true });
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("meetings-row-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    return 3;
  }
  const work = mkdtempSync(join(tmpdir(), "yap-meetings-row-shots-"));
  let dist = PREBUILT ? resolve(PREBUILT) : join(work, "dist");
  if (!PREBUILT) {
    const fd = openSync(join(work, "vite-build.log"), "w");
    const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
    if (b.status !== 0) {
      console.error("meetings-row-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
      rmSync(work, { recursive: true, force: true });
      return 3;
    }
  }
  const port = await freePort();
  const pfd = openSync(join(work, "vite-preview.log"), "w");
  const preview = spawn(vite, ["preview", "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], {
    cwd: DESKTOP,
    stdio: ["ignore", pfd, pfd],
  });
  const url = "http://127.0.0.1:" + port + "/index.html";
  let chrome = null;
  const failures = [];
  const fail = (check, detail) => {
    failures.push(`${check} ${detail}`);
    console.log(`FAIL ${check} ${detail}`);
  };
  try {
    let up = false;
    for (let i = 0; i < 120 && !up; i++) {
      try { up = (await fetch(url)).ok; } catch {}
      if (!up) await sleep(250);
    }
    if (!up) throw new Error("vite preview never served " + url);
    chrome = await openChrome(CHROME, "yap-meetings-row-shots");
    const { cdp } = chrome;
    const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
    const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, s);
    await cdp.send("Runtime.enable", {}, s);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
    await cdp.send(
      "Page.addScriptToEvaluateOnNewDocument",
      { source: "window.__YAP_FIXTURES__ = " + JSON.stringify(fixtures()) + ";\n" + TAURI_SHIM },
      s,
    );
    await cdp.send("Page.navigate", { url }, s);
    await waitFor(cdp, s, "document.readyState === 'complete' && !!document.querySelector('nav button, .nav-item')");
    await evaluate(
      cdp,
      s,
      `(() => { const b = [...document.querySelectorAll("nav.nav button, nav button, .nav-item")].find((b) => (b.innerText || "").trim().toLowerCase().startsWith("meetings")); if (b) b.click(); return !!b; })()`,
    );
    if (!(await waitFor(cdp, s, `document.querySelectorAll("li.meeting-row[data-meeting-id]").length >= 4`))) {
      throw new Error("the Meetings list never showed the fixture rows (FIXTURE/INSTRUMENT failure)");
    }
    await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
    const groups = await evaluate(cdp, s, `[...document.querySelectorAll(".meetings-group-label")].map((h) => h.textContent.trim())`);
    console.log("groups " + JSON.stringify(groups));
    if (!Array.isArray(groups) || groups.length < 2) fail("fixture", "expected rows in two date groups, got " + JSON.stringify(groups));

    const measure = (only = null) => evaluate(cdp, s, `${MEASURE}(${JSON.stringify(only)})`);
    const judge = (state, rows) => {
      for (const r of rows) for (const b of r.bad) fail("A1", `${state} ${r.id}: ${b}`);
    };

    // idle — pointer parked outside the list, nothing focused.
    await mouse(cdp, s, "mouseMoved", W - 4, H - 4);
    await evaluate(cdp, s, "(document.activeElement && document.activeElement.blur && document.activeElement.blur(), true)");
    await sleep(250);
    const idle = await measure();
    judge("idle", idle);
    console.log("idle " + JSON.stringify(idle.map((r) => [r.id, r.height, r.actions])));
    await shot(cdp, s, "idle");

    // hover — each row in turn; A2: no row's height or top moves.
    for (const r of idle) {
      const c = await centre(cdp, s, `li.meeting-row[data-meeting-id="${r.id}"]`);
      if (!c) { fail("A1", "row vanished " + r.id); continue; }
      await mouse(cdp, s, "mouseMoved", c.x, c.y);
      await sleep(250);
      const rows = await measure(r.id);
      judge("hover", rows);
      for (const cur of rows) {
        const was = idle.find((x) => x.id === cur.id);
        if (!was) continue;
        const dh = Math.abs(cur.height - was.height);
        const dt = Math.abs(cur.top - was.top);
        if (dh > 0.01 || dt > 0.01) fail("A2", `hover ${r.id}: row ${cur.id} moved (height Δ${dh.toFixed(2)} px, top Δ${dt.toFixed(2)} px)`);
      }
      if (r.id === idle[0].id) await shot(cdp, s, "hover");
    }

    // focus-within — the row's open button takes keyboard focus.
    await mouse(cdp, s, "mouseMoved", W - 4, H - 4);
    for (const r of idle) {
      await evaluate(cdp, s, `(() => { const b = document.querySelector('li.meeting-row[data-meeting-id="${r.id}"] button'); if (b) b.focus(); return !!b; })()`);
      await sleep(150);
      judge("focus-within", await measure(r.id));
    }

    // selected — ↓ from the body selects the first row (§3.3 keyboard).
    await evaluate(cdp, s, "(document.activeElement && document.activeElement.blur && document.activeElement.blur(), true)");
    await key(cdp, s, "ArrowDown", "ArrowDown", 40);
    await sleep(250);
    const sel = await evaluate(cdp, s, `document.querySelectorAll("li.meeting-row.selected").length`);
    if (!sel) fail("A1", "ArrowDown selected no row");
    judge("selected", await measure());
    await shot(cdp, s, "selected");

    // A4 — the row's right-click menu: three export choices + Delete.
    const second = await centre(cdp, s, `li.meeting-row[data-meeting-id="${idle[1].id}"]`);
    await click(cdp, s, second.x, second.y, "right");
    await sleep(250);
    const rowMenu = await evaluate(cdp, s, `${MENU_CHECK}("row")`);
    console.log("row menu " + JSON.stringify(rowMenu));
    for (const b of rowMenu.bad) fail("A3", "row menu: " + b);
    const want = ["Export Markdown…", "Copy as Markdown", "Show in Finder", "Delete meeting"];
    if (JSON.stringify(rowMenu.labels) !== JSON.stringify(want)) fail("A4", "row menu labels " + JSON.stringify(rowMenu.labels));
    await shot(cdp, s, "row-menu");
    const del = await centre(cdp, s, '.floating-menu [data-menu-item="delete"]');
    if (del) {
      await click(cdp, s, del.x, del.y);
      const undo = await waitFor(cdp, s, `(document.querySelector(".undo-bar")?.textContent || "").includes("s to undo")`, 3000);
      if (!undo) fail("A4", "Delete did not show the Undo bar");
      const gone = await evaluate(cdp, s, `!document.querySelector('li.meeting-row[data-meeting-id="${idle[1].id}"]')`);
      if (!gone) fail("A4", "the deleted row is still in the list during its Undo");
      await evaluate(cdp, s, `(() => { const b = [...document.querySelectorAll(".undo-bar button")].find((b) => b.textContent.trim() === "Undo"); if (b) b.click(); return !!b; })()`);
      const back = await waitFor(cdp, s, `!!document.querySelector('li.meeting-row[data-meeting-id="${idle[1].id}"]')`, 3000);
      if (!back) fail("A4", "Undo did not restore the row");
    } else fail("A4", "no Delete item in the row menu");

    // A3 — Export from the detail header, opened with a row hovered.
    const first = await centre(cdp, s, `li.meeting-row[data-meeting-id="${idle[0].id}"] .meeting-row-main`);
    await click(cdp, s, first.x, first.y);
    if (!(await waitFor(cdp, s, `!!document.querySelector('.meeting-detail-head [aria-haspopup="menu"]')`))) {
      fail("A3", "the detail header never showed Export");
    } else {
      await mouse(cdp, s, "mouseMoved", first.x, first.y); // a hovered row, as Wilson had it
      const trig = await centre(cdp, s, '.meeting-detail-head [aria-haspopup="menu"]');
      await click(cdp, s, trig.x, trig.y);
      await sleep(250);
      const m = await evaluate(cdp, s, `${MENU_CHECK}("export")`);
      console.log("export menu " + JSON.stringify(m));
      for (const b of m.bad) fail("A3", "export menu: " + b);
      if (m.open && JSON.stringify(m.labels) !== JSON.stringify(want.slice(0, 3))) fail("A4", "export labels " + JSON.stringify(m.labels));
      if (m.open) {
        // Walk the pointer from the trigger to the first item in small steps.
        const item = await centre(cdp, s, '.floating-menu [role="menuitem"]');
        for (let i = 1; i <= 12; i++) {
          await mouse(cdp, s, "mouseMoved", trig.x + ((item.x - trig.x) * i) / 12, trig.y + ((item.y - trig.y) * i) / 12);
          await sleep(30);
        }
        const still = await evaluate(cdp, s, `!!document.querySelector('.floating-menu[data-floating-menu="export"]')`);
        if (!still) fail("A3", "the export menu closed while the pointer moved from the trigger into it");
        await shot(cdp, s, "export-open");
        await key(cdp, s, "Escape", "Escape", 27);
        await sleep(200);
        const closed = await evaluate(cdp, s, `!document.querySelector('.floating-menu')`);
        if (!closed) fail("A3", "Escape did not close the export menu");
      }
    }
    const errs = chrome.cdp.errors.get(s) || [];
    if (errs.length) console.log("page errors (informational): " + JSON.stringify(errs.slice(0, 5)));
    await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
  } finally {
    if (chrome) {
      await chrome.cdp.send("Browser.close").catch(() => {});
      chrome.ws.close();
      await sleep(300);
      if (chrome.proc.exitCode === null) chrome.proc.kill("SIGKILL");
      rmSync(chrome.profile, { recursive: true, force: true });
    }
    if (preview.exitCode === null) preview.kill("SIGTERM");
    await sleep(200);
    if (preview.exitCode === null) preview.kill("SIGKILL");
    rmSync(work, { recursive: true, force: true });
  }
  console.log(`meetings-row-shots ${LABEL}: ${failures.length ? failures.length + " failure(s)" : "all checks passed"}; PNGs in ${OUT}`);
  return failures.length ? 1 : 0;
}

run().then(
  (code) => process.exit(code),
  (e) => {
    console.error("meetings-row-shots: " + (e?.stack || e));
    process.exit(3);
  },
);
