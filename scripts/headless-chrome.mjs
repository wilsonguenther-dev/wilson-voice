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

import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
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
