#!/usr/bin/env node
/**
 * yap-1107-globe-key-conflict-guard — acceptance line 5: 2x captures of the
 * Globe key line and its button on the Permissions page, in the two states
 * globeKeyRow.test.tsx pins: "conflict" (unset pref, Turn it off) and "off"
 * (pref 0 after Turn it off, Put it back).
 *
 * Pattern: settings-banner-shots.mjs — vite build to a scratch dir (never
 * desktop/dist), vite preview on a free port, real Chrome --headless=new via
 * scripts/headless-chrome.mjs (never a window, never the app), TAURI_SHIM
 * answering invoke() from scripts/smoke-fixtures.json plus globe_key_status.
 * Writes <out>/yap-1107-globe-key-conflict-guard-{conflict,off}@2x.png (the
 * row element) and -page@2x.png variants (the Permissions list in context).
 * Exit 1 if a state's row, line or button is missing; 3 on an environment
 * failure. Everything it starts is stopped and deleted.
 *
 * USAGE  node scripts/globe-key-shots.mjs [--out docs/loop/evidence]
 */
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { TAURI_SHIM, evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ID = "yap-1107-globe-key-conflict-guard";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
const W = 1190;
const H = 773;
const SCALE = 2;
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const arg = (k, d) => {
  const i = process.argv.indexOf(k);
  return i > 0 ? process.argv[i + 1] : d;
};
const OUT = resolve(arg("--out", join(ROOT, "docs", "loop", "evidence")));

// Mirrors globe_key.rs status() for the two pinned states.
const EMOJI_LINE = "Your Mac opens the emoji picker on a lone Fn tap; Yap can turn that off";
const OFF_LINE = "A lone Fn tap now does nothing. macOS applies this fully after you restart your Mac.";
const STATES = [
  {
    name: "conflict",
    button: "Turn it off",
    status: {
      pref: null,
      conflict: { kind: "emoji_picker", pref: null, line: EMOJI_LINE },
      line: EMOJI_LINE,
      canTurnOff: true,
      canPutBack: false,
      turnOffLabel: "Turn it off",
      putBackLabel: "Put it back",
    },
  },
  {
    name: "off",
    button: "Put it back",
    status: {
      pref: 0,
      conflict: null,
      line: OFF_LINE,
      canTurnOff: false,
      canPutBack: true,
      turnOffLabel: "Turn it off",
      putBackLabel: "Put it back",
    },
  },
];

function fixtures(globe) {
  const base = JSON.parse(readFileSync(join(ROOT, "scripts", "smoke-fixtures.json"), "utf8"));
  return { ...base, globe_key_status: globe };
}
function freePort() {
  return new Promise((res, rej) => {
    const srv = createServer();
    srv.on("error", rej);
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      srv.close(() => res(port));
    });
  });
}
async function waitFor(cdp, s, expr, ms = 15000) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (await evaluate(cdp, s, `!!(${expr})`).catch(() => false)) return true;
    await sleep(100);
  }
  throw new Error("timed out waiting for " + expr);
}
async function shot(cdp, s, rect, file) {
  const pad = 8;
  const { data } = await cdp.send(
    "Page.captureScreenshot",
    {
      format: "png",
      captureBeyondViewport: false,
      clip: { x: Math.max(0, rect.x - pad), y: Math.max(0, rect.y - pad), width: rect.w + pad * 2, height: rect.h + pad * 2, scale: 1 },
    },
    s,
  );
  writeFileSync(file, Buffer.from(data, "base64"));
}

async function run() {
  mkdirSync(OUT, { recursive: true });
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("globe-key-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    return 3;
  }
  const work = mkdtempSync(join(tmpdir(), "yap-globe-key-shots-"));
  const dist = join(work, "dist");
  const fd = openSync(join(work, "vite-build.log"), "w");
  const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
  if (b.status !== 0) {
    console.error("globe-key-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
    rmSync(work, { recursive: true, force: true });
    return 3;
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
  try {
    let up = false;
    for (let i = 0; i < 120 && !up; i++) {
      try { up = (await fetch(url)).ok; } catch {}
      if (!up) await sleep(250);
    }
    if (!up) throw new Error("vite preview never served " + url);
    chrome = await openChrome(CHROME, "yap-globe-key-shots");
    const { cdp } = chrome;
    for (const st of STATES) {
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      await cdp.send("Page.enable", {}, s);
      await cdp.send("Runtime.enable", {}, s);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
      await cdp.send(
        "Page.addScriptToEvaluateOnNewDocument",
        { source: "window.__YAP_FIXTURES__ = " + JSON.stringify(fixtures(st.status)) + ";\n" + TAURI_SHIM },
        s,
      );
      await cdp.send("Page.navigate", { url }, s);
      await waitFor(cdp, s, "document.readyState === 'complete' && !!document.querySelector('nav button, .nav-item')");
      const clicked = await evaluate(
        cdp,
        s,
        `(() => { const b = [...document.querySelectorAll("nav.nav button, nav button, .nav-item")].find((b) => (b.innerText || "").trim().toLowerCase().startsWith("permissions")); if (b) b.click(); return !!b; })()`,
      );
      if (!clicked) { failures.push(st.name + ": no Permissions nav entry"); console.log("FAIL " + failures.at(-1)); continue; }
      try {
        await waitFor(cdp, s, `!!document.querySelector('[data-globe-key="${st.name}"]')`);
      } catch {
        failures.push(st.name + ": no [data-globe-key] row");
        console.log("FAIL " + failures.at(-1));
        await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
        continue;
      }
      await sleep(500);
      const m = await evaluate(
        cdp,
        s,
        `(() => { const r = document.querySelector('[data-globe-key="${st.name}"]'); r.scrollIntoView({ block: "center" });
          const q = r.getBoundingClientRect(); const list = r.closest("ul,ol") || r.parentElement; const l = list.getBoundingClientRect();
          const btn = [...r.querySelectorAll("button")].map((b) => b.textContent.trim());
          return { line: r.querySelector("p")?.textContent?.trim(), btn, row: { x: q.x, y: q.y, w: q.width, h: q.height }, list: { x: l.x, y: l.y, w: l.width, h: Math.min(l.height, ${H} - l.y) } }; })()`,
      );
      await sleep(200);
      const want = st.status.line;
      if (m.line !== want) failures.push(`${st.name}: line "${m.line}" != "${want}"`);
      if (m.btn.length !== 1 || m.btn[0] !== st.button) failures.push(`${st.name}: buttons ${JSON.stringify(m.btn)} != ["${st.button}"]`);
      await shot(cdp, s, m.row, join(OUT, `${ID}-${st.name}@2x.png`));
      await shot(cdp, s, m.list, join(OUT, `${ID}-${st.name}-page@2x.png`));
      console.log(`${st.name}: line="${m.line}" buttons=${JSON.stringify(m.btn)} row=${JSON.stringify(m.row)}`);
      await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
    }
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
  for (const f of failures) console.log("FAIL " + f);
  console.log(`globe-key-shots: ${failures.length ? failures.length + " failure(s)" : "all checks passed"}; PNGs in ${OUT}`);
  return failures.length ? 1 : 0;
}

run().then(
  (code) => process.exit(code),
  (e) => {
    console.error("globe-key-shots: " + (e && e.stack ? e.stack : e));
    process.exit(3);
  },
);
