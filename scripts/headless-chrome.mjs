/**
 * The headless Chrome both structural instruments drive: smoke-windowed.mjs
 * (the main window's seven views) and pill-matrix.mjs (the float pill's phase x
 * dock x character matrix). ONE copy, so the two can never disagree about how
 * Chrome is launched (--headless=new: never a visible window) or how CDP is
 * spoken.
 *
 * NO npm DEPENDENCIES. Node 24 ships a global WebSocket, so this speaks the
 * Chrome DevTools Protocol directly — no Playwright, no ~400 MB browser
 * download inside a loop worktree.
 *
 * Extracted VERBATIM from smoke-windowed.mjs (yap24-UI3); the only changes are
 * the exports, the Chrome path and profile prefix becoming parameters, and the
 * profile dir being returned so a caller can delete it.
 */

import { spawn, spawnSync } from "node:child_process";
import { existsSync, openSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

export class Cdp {
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

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function waitForJson(url, tries = 100) {
  for (let i = 0; i < tries; i++) {
    try {
      const r = await fetch(url);
      if (r.ok) return await r.json();
    } catch {}
    await sleep(100);
  }
  throw new Error("timed out waiting for " + url);
}

export async function openChrome(CHROME, profilePrefix = "yap-smoke") {
  if (!existsSync(CHROME)) {
    throw new Error(
      "Chrome not found at " + CHROME + " — this smoke needs a Chromium to speak CDP to. " +
      "Set YAP_SMOKE_CHROME. (This is an ENVIRONMENT failure, not a UI defect.)"
    );
  }
  const port = 9300 + Math.floor(Math.random() * 400);
  const profile = join(tmpdir(), profilePrefix + "-" + process.pid + "-" + port);
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
  return { proc, cdp: new Cdp(ws), ws, profile };
}

export async function evaluate(cdp, sessionId, expression) {
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

/* ── the Tauri shim (moved verbatim from smoke-windowed.mjs) ───────────── */
/*
 * Injected before any bundle script runs; answers invoke() from
 * window.__YAP_FIXTURES__ (which the caller injects first). Unknown commands
 * REJECT on purpose: every view seeds an empty initial state and only replaces
 * it on a resolved invoke, so a rejection leaves it rendering its real unfed
 * shape. See smoke-windowed.mjs for the full rationale.
 */
export const TAURI_SHIM = String.raw`
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

/* ── the float bundle's build + preview server + forced-phase fixtures ─── */
/*
 * yap-1107-parity-kit — moved here from pill-matrix.mjs so the pill matrix and
 * scripts/parity-sheet.mjs (which measures the app side of the parity sheet
 * from the SAME forced-phase float.html?phase=&pill=&dock=) share ONE copy.
 * `label` only prefixes the messages; behaviour is the pill matrix's verbatim.
 */

export function freePort(envPort = process.env.YAP_PILL_PORT) {
  if (envPort) return Promise.resolve(Number(envPort));
  return new Promise((res, rej) => {
    const s = createServer();
    s.once("error", rej);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => res(port)); });
  });
}

/* Builds desktop/ with YAP_PILL_MATRIX=1 (compiles in src/pill/forced.ts) into
   <work>/dist — a SCRATCH outDir, never desktop/dist. Exits 3 on failure: a
   missing vite or a red build is an ENVIRONMENT failure, not a UI defect. */
export function buildMatrixBundle(desktop, work, label = "pill-matrix") {
  const dist = join(work, "dist");
  const log = join(work, "vite-build.log");
  const vite = join(desktop, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error(label + ": " + vite + " missing — run npm ci in desktop/ (ENVIRONMENT failure, not a UI defect)");
    process.exit(3);
  }
  console.log(label + ": building float.html with YAP_PILL_MATRIX=1 -> " + dist);
  const fd = openSync(log, "w");
  const r = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], {
    cwd: desktop,
    env: { ...process.env, YAP_PILL_MATRIX: "1" },
    stdio: ["ignore", fd, fd],
  });
  if (r.status !== 0) {
    console.error(label + ": vite build exited " + r.status + "\n" + readFileSync(log, "utf8"));
    process.exit(3);
  }
  return dist;
}

/* `vite preview` of a built dist on a free 127.0.0.1 port. The caller owns
   `proc` and must kill it (and only it) on exit. */
export async function servePreview(desktop, dist, work, label = "pill-matrix", probe = "/float.html") {
  const port = await freePort();
  const vite = join(desktop, "node_modules", ".bin", "vite");
  const fd = openSync(join(work, "vite-preview.log"), "w");
  const proc = spawn(vite, ["preview", "--outDir", dist, "--port", String(port), "--strictPort", "--host", "127.0.0.1"], {
    cwd: desktop,
    stdio: ["ignore", fd, fd],
  });
  const base = "http://127.0.0.1:" + port;
  for (let i = 0; i < 120; i++) {
    try { if ((await fetch(base + probe)).ok) return { proc, base }; } catch {}
    if (proc.exitCode !== null) break;
    await sleep(250);
  }
  proc.kill("SIGKILL");
  console.error(label + ": vite preview never came up on " + base + "\n" + readFileSync(join(work, "vite-preview.log"), "utf8"));
  process.exit(3);
}

/*
 * The shim's answers for one forced-phase float.html capture. Runs before the
 * bundle, after TAURI_SHIM's table is read — so it must set __YAP_FIXTURES__
 * FIRST. The capsules keep a little state of their own off `get_status` (the
 * shell's live/busy classes, Yappy's pose), so the status a phase answers is
 * derived from its treatment TONE, which the page itself published
 * (__YAP_PILL_PHASES__): no per-phase table lives in any instrument.
 */
export function pillFixtureScript(tones) {
  return String.raw`
(() => {
  const q = new URLSearchParams(location.search);
  const tone = (${JSON.stringify(tones)})[q.get("phase")] || "calm";
  window.__YAP_FIXTURES__ = {
    get_settings: { pillStyle: q.get("pill") || "yappy", pillPosition: q.get("dock") || "bottom", companionTone: "friendly" },
    get_status: { recording: tone === "live", busy: tone === "busy", message: tone === "live" ? "Recording" : tone === "busy" ? "Transcribing" : "Ready" },
    // yap-1106-one-pill — a meeting on screen, and the panel-frame request the
    // layout makes (answered, so it is never an unhandled rejection).
    // fix-b8-1 — meeting=saved starts recording too; pill-matrix --phase
    // meeting-saved then delivers the stop after settle (meetingStopShim).
    meeting_status: q.get("meeting") === "recording" || q.get("meeting") === "saved"
      // yap-1106-pill-rec-headphones-hoot-tape — elapsed=<seconds> in the URL puts the
      // sun clock at any minute (0, 600 = the glance, 3600 = dusk, 3660 = moon).
      ? (() => { const e = Math.max(0, Math.floor(Number(q.get("elapsed") ?? 217)) || 0); const p = (n) => String(n).padStart(2, "0"); return { recording: true, id: "matrix-meeting", title: "Matrix meeting", elapsedSeconds: e, elapsedLabel: p(Math.floor(e / 3600)) + ":" + p(Math.floor((e % 3600) / 60)) + ":" + p(e % 60), captureAvailable: true }; })()
      : { recording: false, id: null, title: null, elapsedSeconds: 0, elapsedLabel: "00:00:00", captureAvailable: false },
    pill_set_panel_frame: null,
  };
})();
`;
}

/* Waits for a forced phase to apply (<html data-phase>), fonts, then settleMs. */
export async function settlePhase(cdp, s, wantId, settleMs) {
  const deadline = Date.now() + 8000;
  for (;;) {
    const st = await evaluate(cdp, s, `document.readyState + "|" + (document.documentElement.dataset.phase || "")`).catch(() => "");
    if (st.startsWith("complete|") && (!wantId || st.endsWith("|" + wantId))) break;
    if (Date.now() > deadline) break; // judged as phase-not-applied by the caller
    await sleep(100);
  }
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(settleMs); // let the strip fade and the capsule expand finish
}

/* yap-1106-one-pill — size the window to the layout's panel, the way
 * float_pill.rs set_panel_frame sizes the NSPanel, then let it re-lay out. */
export async function fitPillPanel(cdp, s, fallback, scale, settleMs) {
  const read = () => evaluate(cdp, s, "(window.__YAP_PILL_LAYOUT__ && window.__YAP_PILL_LAYOUT__.panel) || null").catch(() => null);
  let panel = await read();
  if (!panel) return { w: fallback.w, h: fallback.h };
  // float-main re-sends pill_set_panel_frame on every panel change, so the
  // NSPanel follows a layout that settles after the resize (a meeting stop
  // landing mid-fit). Follow it the same way, a few passes at most.
  for (let pass = 0; pass < 4; pass++) {
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: panel.w, height: panel.h, deviceScaleFactor: scale, mobile: false }, s);
    await sleep(Math.min(settleMs, 400));
    const next = await read();
    if (!next || (next.w === panel.w && next.h === panel.h)) break;
    panel = next;
  }
  return { w: panel.w, h: panel.h };
}
