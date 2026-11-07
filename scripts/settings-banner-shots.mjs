#!/usr/bin/env node
/**
 * yap-1107-settings-banner-no-overlay — app-level banners vs the Settings view,
 * measured, plus 2x PNGs for the PR evidence. Wilson (2026-11-07 14:33Z, Yap
 * Test 0.9.4-loop3.11, ~1190x773): the Secure Input banner sat ON TOP of the
 * Settings > Advanced "Model providers" section header. Rule: banners push
 * content down, never cover it.
 *
 * Pattern: meetings-row-shots.mjs / home-shots.mjs — vite build to a scratch
 * dir (never desktop/dist), vite preview on a free port, real Chrome
 * --headless=new (scripts/headless-chrome.mjs; never a window, never the app),
 * the shared TAURI_SHIM answering invoke() from scripts/smoke-fixtures.json
 * with get_status overridden per scenario:
 *   off     no banner (the baseline the deltas are measured from)
 *   secure  secureInputBlocked from ANOTHER app (the red banner, with detail)
 *   hotkey  hotkeyDegraded (fn not delivering; the red degraded banner)
 *   quiet   secureInputOwnField (Yap's own key field: the quiet one-line row)
 * Settings open on Advanced (the tab holding Model providers), viewport
 * 1190x773 logical at deviceScaleFactor 2, two scroll states each:
 *   top     .content scrolled to 0
 *   header  .content scrolled so the h3's top edge sits on the scroller's top
 *           edge, i.e. directly under the banner row
 *
 * CHECKS (exit 1 on any failure, 3 on an instrument/environment failure):
 *   A1/A2  in every banner scenario and both scroll states: the "Model
 *          providers" h3 rect has zero intersection with every banner rect
 *          (.banner, .secure-input-quiet, .hide-hint, .perm-repair outside
 *          .content); elementFromPoint at the h3 centre is the h3 (or inside
 *          it); the h3 is fully inside the .content scroller; every banner's
 *          bottom is at or above .content's top; every banner is
 *          position:static and not squashed (height >= scrollHeight).
 *   A3     toggling the banner on moves .content's top AND the h3 (scrollTop 0)
 *          down by exactly the banner block height — the sum of the banners'
 *          outer heights, margins included — within 0.5 px; the delta is
 *          printed per scenario.
 *   W      the banner row [data-main-banners] exists, is position:static and
 *          flex-shrink:0 (the fix's structure; absent on a pre-fix base).
 *   A4     PNGs: <label>-<scenario>-<state>.png in --out.
 *
 * USAGE   node scripts/settings-banner-shots.mjs [--out <dir>] [--label after|before] [--dist <built dist>]
 *   --out    default docs/loop/evidence/yap-1107-settings-banner-no-overlay
 *   --label  PNG name prefix (default "after"). The BEFORE shots come from the
 *            same script run in an origin/main worktree with --label before; it
 *            is EXPECTED to exit 1 there (W at least; A1 if the overlap shows).
 *   --dist   reuse an already-built desktop dist instead of building.
 * ENV     YAP_SMOKE_CHROME (Chromium path), YAP_BANNER_SHOTS_SETTLE_MS (default 600).
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

const ID = "yap-1107-settings-banner-no-overlay";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
const W = 1190;
const H = 773;
const SCALE = 2;
const SETTLE_MS = Number(process.env.YAP_BANNER_SHOTS_SETTLE_MS || 600);
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const EPS = 0.5;

const arg = (k, d) => {
  const i = process.argv.indexOf(k);
  return i > 0 ? process.argv[i + 1] : d;
};
const OUT = resolve(arg("--out", join(ROOT, "docs", "loop", "evidence", ID)));
const LABEL = arg("--label", "after");
const PREBUILT = arg("--dist", null);

/* ── fixtures ─────────────────────────────────────────────────────────── */
const SCENARIOS = [
  { name: "off", status: {} },
  {
    name: "secure",
    status: {
      secureInputBlocked: true,
      secureInputOwnField: false,
      secureInputDetail:
        "1Password is holding Secure Input. Lock 1Password or close its window; dictation from the menu bar still works.",
    },
  },
  {
    name: "hotkey",
    status: {
      hotkeyRegistered: true,
      hotkeyDegraded: "fn isn't reaching Yap — Input Monitoring is off. Use ⌃⌥Space to dictate until you turn it on.",
    },
  },
  { name: "quiet", status: { secureInputBlocked: false, secureInputOwnField: true } },
];

function fixtures(statusPatch) {
  const base = JSON.parse(readFileSync(join(ROOT, "scripts", "smoke-fixtures.json"), "utf8"));
  delete base._README;
  return {
    ...base,
    get_status: { ...base.get_status, modelReady: true, ...statusPatch },
    // Settings > Advanced mounts ModelProviders, which needs a ProvidersView
    // object; the shim's generic list_* answer ([]) would crash the view.
    list_model_providers: base.list_model_providers ?? {
      providers: [
        {
          id: "bundled", kind: "bundled", label: "Bundled", baseUrl: "", model: "", contextTokens: 0, timeoutS: 0,
          leaves: false, leavesLabel: "no", leavesHost: null, keySaved: false, keyHint: "", deletable: false,
        },
      ],
      notesProvider: "",
      polishProvider: "",
      wiredKinds: ["bundled"],
      activeLeaves: [],
    },
  };
}

/* ── page-side measurement ────────────────────────────────────────────── */
const MEASURE = String.raw`(() => {
  const R = (el) => { const r = el.getBoundingClientRect(); return { l: r.left, t: r.top, r: r.right, b: r.bottom, h: r.height }; };
  const vis = (el) => {
    const cs = getComputedStyle(el);
    const r = el.getBoundingClientRect();
    return cs.display !== "none" && cs.visibility !== "hidden" && Number(cs.opacity) > 0.01 && r.width > 0 && r.height > 0;
  };
  const content = document.querySelector(".content");
  const h3 = content && [...content.querySelectorAll("h3")].find((h) => h.textContent.trim() === "Model providers");
  if (!content || !h3) return { ready: false };
  const wrap = document.querySelector("[data-main-banners]");
  const banners = [...document.querySelectorAll(".banner, .secure-input-quiet, .hide-hint, .perm-repair")]
    .filter((el) => !content.contains(el) && vis(el))
    .map((el) => {
      const cs = getComputedStyle(el);
      const r = R(el);
      return {
        cls: el.className,
        rect: r,
        outer: r.h + parseFloat(cs.marginTop) + parseFloat(cs.marginBottom),
        position: cs.position,
        flexShrink: cs.flexShrink,
        scrollHeight: el.scrollHeight,
      };
    });
  const hr = R(h3);
  const at = document.elementFromPoint((hr.l + hr.r) / 2, (hr.t + hr.b) / 2);
  // The header nearest the top of the scroller: at scrollTop 0 the Advanced
  // panel is taller than the 773 px viewport, so "Model providers" sits below
  // the fold and the header directly under the banner is the first h3.
  const leadEl = [...content.querySelectorAll("h3")].find((h) => vis(h));
  const lr = leadEl ? R(leadEl) : null;
  const leadAt = lr && document.elementFromPoint((lr.l + lr.r) / 2, (lr.t + lr.b) / 2);
  const wcs = wrap && getComputedStyle(wrap);
  const mcs = getComputedStyle(document.querySelector(".main"));
  return {
    ready: true,
    h3: hr,
    h3Hit: !!at && (at === h3 || h3.contains(at)),
    lead: lr ? { rect: lr, text: leadEl.textContent.trim().slice(0, 40), hit: !!leadAt && (leadAt === leadEl || leadEl.contains(leadAt)) } : null,
    hitWhat: at ? at.tagName.toLowerCase() + (at.className ? "." + String(at.className).split(" ").join(".") : "") : null,
    content: { ...R(content), scrollTop: content.scrollTop, clientHeight: content.clientHeight },
    banners,
    wrap: wrap ? { rect: R(wrap), position: wcs.position, flexShrink: wcs.flexShrink } : null,
    main: { display: mcs.display, flexDirection: mcs.flexDirection, height: document.querySelector(".main").getBoundingClientRect().height },
  };
})()`;

const SCROLL_TOP = `(() => { const c = document.querySelector(".content"); c.scrollTop = 0; return c.scrollTop; })()`;
const SCROLL_HEADER = String.raw`(() => {
  const c = document.querySelector(".content");
  const h = [...c.querySelectorAll("h3")].find((x) => x.textContent.trim() === "Model providers");
  c.scrollTop = 0;
  c.scrollTop = h.getBoundingClientRect().top - c.getBoundingClientRect().top;
  return c.scrollTop;
})()`;

/* ── CDP helpers ──────────────────────────────────────────────────────── */
async function waitFor(cdp, s, expr, ms = 8000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (await evaluate(cdp, s, expr).catch(() => false)) return true;
    await sleep(100);
  }
  return false;
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
const overlap = (a, b) => Math.min(a.r, b.r) - Math.max(a.l, b.l) > 0.01 && Math.min(a.b, b.b) - Math.max(a.t, b.t) > 0.01;

/* ── the run ──────────────────────────────────────────────────────────── */
async function run() {
  mkdirSync(OUT, { recursive: true });
  const vite = join(DESKTOP, "node_modules", ".bin", "vite");
  if (!existsSync(vite)) {
    console.error("settings-banner-shots: " + vite + " missing — npm ci in desktop/ (ENVIRONMENT failure)");
    return 3;
  }
  const work = mkdtempSync(join(tmpdir(), "yap-settings-banner-shots-"));
  const dist = PREBUILT ? resolve(PREBUILT) : join(work, "dist");
  if (!PREBUILT) {
    const fd = openSync(join(work, "vite-build.log"), "w");
    const b = spawnSync(vite, ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "warn"], { cwd: DESKTOP, stdio: ["ignore", fd, fd] });
    if (b.status !== 0) {
      console.error("settings-banner-shots: vite build exited " + b.status + "\n" + readFileSync(join(work, "vite-build.log"), "utf8"));
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
    chrome = await openChrome(CHROME, "yap-settings-banner-shots");
    const { cdp } = chrome;
    const results = {};

    for (const sc of SCENARIOS) {
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      await cdp.send("Page.enable", {}, s);
      await cdp.send("Runtime.enable", {}, s);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
      await cdp.send(
        "Page.addScriptToEvaluateOnNewDocument",
        { source: "window.__YAP_FIXTURES__ = " + JSON.stringify(fixtures(sc.status)) + ";\n" + TAURI_SHIM },
        s,
      );
      await cdp.send("Page.navigate", { url }, s);
      await waitFor(cdp, s, "document.readyState === 'complete' && !!document.querySelector('nav button, .nav-item')");
      await evaluate(
        cdp,
        s,
        `(() => { const b = [...document.querySelectorAll("nav.nav button, nav button, .nav-item")].find((b) => (b.innerText || "").trim().toLowerCase().startsWith("settings")); if (b) b.click(); return !!b; })()`,
      );
      await waitFor(cdp, s, `!!document.querySelector('.settings-subnav [role="tab"]')`);
      await evaluate(
        cdp,
        s,
        `(() => { const t = [...document.querySelectorAll('.settings-subnav [role="tab"]')].find((t) => t.textContent.trim() === "Advanced"); if (t) t.click(); return !!t; })()`,
      );
      if (!(await waitFor(cdp, s, `${MEASURE}.ready === true`))) {
        throw new Error(`[${sc.name}] Settings > Advanced never showed the "Model providers" h3 (FIXTURE/INSTRUMENT failure)`);
      }
      await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
      // The status poll must have landed the scenario's banner before measuring.
      const want = { secure: ".banner.blocked", hotkey: ".banner.blocked", quiet: ".secure-input-quiet" }[sc.name];
      if (want && !(await waitFor(cdp, s, `!!document.querySelector(${JSON.stringify(want)})`))) {
        fail("fixture", `[${sc.name}] the ${want} banner never rendered`);
      }
      results[sc.name] = {};
      for (const [state, expr] of [["top", SCROLL_TOP], ["header", SCROLL_HEADER]]) {
        await evaluate(cdp, s, expr);
        await sleep(SETTLE_MS);
        const m = await evaluate(cdp, s, MEASURE);
        results[sc.name][state] = m;
        console.log(`geometry ${sc.name}/${state} ` + JSON.stringify(m));
        const png = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
        writeFileSync(join(OUT, `${LABEL}-${sc.name}-${state}.png`), Buffer.from(png.data, "base64"));
        if (sc.name === "off") continue;
        const tag = `[${sc.name}/${state}]`;
        const check = sc.name === "secure" ? "A1" : "A2";
        if (!m.banners.length) fail(check, `${tag} no banner is visible`);
        for (const b of m.banners) {
          if (overlap(b.rect, m.h3)) fail(check, `${tag} ${b.cls} rect ${JSON.stringify(b.rect)} intersects the h3 ${JSON.stringify(m.h3)}`);
          if (b.rect.b > m.content.t + EPS) fail(check, `${tag} ${b.cls} bottom ${b.rect.b} is below .content top ${m.content.t}`);
          if (b.position !== "static") fail(check, `${tag} ${b.cls} is position:${b.position}`);
          if (b.rect.h + 1 < b.scrollHeight) fail(check, `${tag} ${b.cls} squashed: height ${b.rect.h} < scrollHeight ${b.scrollHeight}`);
        }
        // "header" state: the Model providers h3 sits just under the banner.
        // "top" state: the first header in .content is the one under the banner.
        const target = state === "header"
          ? { name: "Model providers", rect: m.h3, hit: m.h3Hit }
          : m.lead && { name: m.lead.text, rect: m.lead.rect, hit: m.lead.hit };
        if (!target) fail(check, `${tag} no visible h3 in .content`);
        else {
          for (const b of m.banners) {
            if (overlap(b.rect, target.rect)) fail(check, `${tag} ${b.cls} intersects the "${target.name}" h3`);
          }
          if (!target.hit) fail(check, `${tag} elementFromPoint at the "${target.name}" h3 centre is not the h3 (${m.hitWhat})`);
          if (target.rect.t < m.content.t - EPS || target.rect.b > m.content.b + EPS) {
            fail(check, `${tag} the "${target.name}" h3 ${JSON.stringify(target.rect)} is not fully inside .content ${JSON.stringify(m.content)}`);
          }
          console.log(`${check} ${tag} "${target.name}" h3 visible, hit, clear of every banner`);
        }
        if (!m.wrap) fail("W", `${tag} no [data-main-banners] row`);
        else if (m.wrap.position !== "static" || m.wrap.flexShrink !== "0") {
          fail("W", `${tag} [data-main-banners] is position:${m.wrap.position} flex-shrink:${m.wrap.flexShrink}`);
        }
      }
      const errs = chrome.cdp.errors?.get?.(s) || [];
      if (errs.length) console.log(`page errors ${sc.name} (informational): ` + JSON.stringify(errs.slice(0, 5)));
      await cdp.send("Target.closeTarget", { targetId }).catch(() => {});
    }

    // A3 — the push: .content and the h3 move down by exactly the block.
    const off = results.off?.top;
    const sum = (m) => m.banners.reduce((a, b) => a + b.outer, 0);
    for (const name of ["secure", "hotkey", "quiet"]) {
      const on = results[name]?.top;
      if (!off || !on) { fail("A3", `${name}: missing measurement`); continue; }
      const block = sum(on) - sum(off);
      const dContent = on.content.t - off.content.t;
      const dH3 = on.h3.t - off.h3.t;
      const dWrap = on.wrap && off.wrap ? on.wrap.rect.h - off.wrap.rect.h : null;
      console.log(`A3 ${name}: banner block ${block.toFixed(2)} px (row Δ ${dWrap === null ? "n/a" : dWrap.toFixed(2)}), .content top Δ ${dContent.toFixed(2)} px, h3 top Δ ${dH3.toFixed(2)} px`);
      if (block <= EPS) fail("A3", `${name}: toggling the banner on added no block height (${block.toFixed(2)} px)`);
      if (Math.abs(dContent - block) > EPS) fail("A3", `${name}: .content moved ${dContent.toFixed(2)} px, the banner block is ${block.toFixed(2)} px`);
      if (Math.abs(dH3 - block) > EPS) fail("A3", `${name}: the h3 moved ${dH3.toFixed(2)} px, the banner block is ${block.toFixed(2)} px`);
      if (dWrap !== null && Math.abs(dWrap - block) > EPS) fail("A3", `${name}: the banner row grew ${dWrap.toFixed(2)} px, the banners are ${block.toFixed(2)} px`);
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
  console.log(`settings-banner-shots ${LABEL}: ${failures.length ? failures.length + " failure(s)" : "all checks passed"}; PNGs in ${OUT}`);
  return failures.length ? 1 : 0;
}

run().then(
  (code) => process.exit(code),
  (e) => {
    console.error("settings-banner-shots: " + (e && e.stack ? e.stack : e));
    process.exit(3);
  },
);
