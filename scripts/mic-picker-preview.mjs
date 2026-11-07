#!/usr/bin/env node
/**
 * mic-picker-preview.mjs — before/after PNGs for WF-09 (microphone picker).
 *
 * Two surfaces, both drawn in headless Chrome (--headless=new via
 * scripts/headless-chrome.mjs; never a visible window, never the real app):
 *
 *   1. the menu bar's Dictation group, before (no Microphone submenu) and
 *      after ("Microphone ▸" under Hands-free, opened), in three states:
 *      nothing picked, a USB mic picked, the picked mic unplugged;
 *   2. Settings › Audio › Microphone in every lifecycle state: loading,
 *      ready, picked-but-unplugged, no device at all, and the list error.
 *
 * The menu rows follow `tray_menu::microphone_menu` and the Settings copy
 * follows `views/settings/parts/MicrophonePicker.tsx`; the format strings are
 * checked against those sources so a preview cannot silently drift. Devices
 * are fixtures. Colours come from desktop/src/tokens.css.
 *
 *   node scripts/mic-picker-preview.mjs --out <dir> [--self-test]
 *
 * WF-10 adds the fallback order ("If a mic disconnects"), the "Other devices"
 * group for virtual / loopback inputs, the next-in-order hint and the pill's
 * "Mic disconnected — continuing on …" line (mic-order-*.png, mic-pill-*.png).
 *
 * NO npm DEPENDENCIES. Cleans up its Chrome process and profile dir on exit.
 */

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const TRAY_RS = readFileSync(join(ROOT, "desktop/src-tauri/src/tray_menu.rs"), "utf8");
const PICKER_TSX = readFileSync(join(ROOT, "desktop/src/views/settings/parts/MicrophonePicker.tsx"), "utf8");
const RECORD_RS = readFileSync(join(ROOT, "desktop/src-tauri/src/record.rs"), "utf8");
const TOKENS = readFileSync(join(ROOT, "desktop/src/tokens.css"), "utf8");

function assertSourcesMatch() {
  const need = [
    [TRAY_RS, 'label: "Microphone"'],
    [TRAY_RS, "Not Connected"],
    [TRAY_RS, "ID_HANDS_FREE"],
    [PICKER_TSX, "Looking for microphones…"],
    [PICKER_TSX, "isn't connected, so Yap uses the system default until it's back."],
    [PICKER_TSX, "No microphone is connected. Plug one in and it shows up here."],
    [PICKER_TSX, "Yap couldn't list your microphones."],
    [PICKER_TSX, "Your next take records from "],
    [PICKER_TSX, "If a mic disconnects"],
    [PICKER_TSX, "Yap keeps recording on the next mic in this order. Drag to rearrange."],
    [PICKER_TSX, 'label="Other devices"'],
    [PICKER_TSX, "next in your order, until it's back."],
    [RECORD_RS, "Mic disconnected — continuing on {to}"],
  ];
  for (const [src, s] of need) if (!src.includes(s)) throw new Error("preview drifted from source: " + JSON.stringify(s));
}

const BUILT_IN = "MacBook Pro Microphone";
const MV7 = "Shure MV7";

const DICTATION = [
  { label: "Start Dictation" },
  { label: "Hands-free", checked: false },
  { label: "Paste Last Transcript" },
  { label: "Copy Last Transcript" },
  { label: "Undo AI Edit (Paste Raw)" },
];

function micRows(state) {
  const def = { label: `System Default (${BUILT_IN})`, checked: state === "default" || state === "unplugged" };
  const rows = [def, { label: BUILT_IN, checked: false }];
  if (state !== "unplugged") rows.push({ label: MV7, checked: state === "picked" });
  if (state === "unplugged") rows.push({ label: `${MV7} — Not Connected`, disabled: true });
  return rows;
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

function menuHtml(title, withMic, state) {
  const row = (r) =>
    `<div class="row${r.disabled ? " off" : ""}"><span class="ck">${r.checked ? "✓" : ""}</span>${esc(r.label)}${r.sub ? '<span class="arrow">›</span>' : ""}</div>`;
  const rows = [...DICTATION];
  if (withMic) rows.splice(2, 0, { label: "Microphone", sub: true, open: true });
  const body = rows
    .map((r) => (r.open ? `<div class="row hot"><span class="ck"></span>${esc(r.label)}<span class="arrow">›</span></div>` : row(r)))
    .join("");
  const sub = withMic ? `<div class="menu sub">${micRows(state).map(row).join("")}</div>` : "";
  return page(title, `<div class="wrap"><div class="menu">${body}</div>${sub}</div>`);
}

function settingsHtml(title, state) {
  const opts = (sel, missing) =>
    [`System Default (${BUILT_IN})`, BUILT_IN, MV7]
      .map((o, i) => `<option${i === sel ? " selected" : ""}>${esc(o)}</option>`)
      .join("") + (missing ? `<option selected disabled>${MV7} — not connected</option>` : "");
  let inner;
  if (state === "loading") inner = `<p class="hint">Looking for microphones…</p>`;
  else if (state === "error")
    inner = `<p class="hint">Yap couldn't list your microphones.</p><button class="ghost">Try again</button>`;
  else if (state === "empty")
    inner = `<label class="field"><span>Record from</span><select><option>System Default</option></select></label><p class="hint">No microphone is connected. Plug one in and it shows up here.</p>`;
  else if (state === "unplugged")
    inner = `<label class="field"><span>Record from</span><select>${opts(-1, true)}</select></label><p class="hint">${MV7} isn't connected, so Yap uses the system default until it's back.</p>`;
  else
    inner = `<label class="field"><span>Record from</span><select>${opts(2, false)}</select></label><p class="hint">Your next take records from ${MV7}.</p>`;
  return page(title, `<section class="panel"><h3>Microphone</h3>${inner}</section>`);
}

const AIRPODS = "AirPods Pro";
const BLACKHOLE = "BlackHole 2ch";

// WF-10 — Settings › Audio › Microphone with the fallback order underneath.
function orderHtml(title, state) {
  const unplugged = state === "fallback";
  const select = `<label class="field"><span>Record from</span><select size="6">
    <option>System Default (${esc(BUILT_IN)})</option><option>${esc(BUILT_IN)}</option>
    ${unplugged ? "" : `<option selected>${esc(MV7)}</option>`}<option>${esc(AIRPODS)}</option>
    <optgroup label="Other devices"><option>${esc(BLACKHOLE)}</option></optgroup>
    ${unplugged ? `<option selected disabled>${esc(MV7)} — not connected</option>` : ""}</select></label>`;
  const hint = unplugged
    ? `${MV7} isn't connected, so Yap uses ${AIRPODS}, next in your order, until it's back.`
    : `Your next take records from ${MV7}.`;
  if (state === "before")
    return page(title, `<section class="panel"><h3>Microphone</h3>${select}<p class="hint">${esc(hint)}</p></section>`);
  const rows = [
    { name: MV7, connected: !unplugged },
    { name: AIRPODS, connected: true },
    { name: BUILT_IN, connected: true },
  ];
  const list = rows
    .map(
      (r, i) =>
        `<li><span class="rank">${i + 1}</span><span class="nm">${esc(r.name)}${r.connected ? "" : '<span class="hint"> — not connected</span>'}</span><button class="ghost"${i === 0 ? " disabled" : ""}>Up</button><button class="ghost"${i === rows.length - 1 ? " disabled" : ""}>Down</button></li>`,
    )
    .join("");
  return page(
    title,
    `<section class="panel"><h3>Microphone</h3>${select}<p class="hint">${esc(hint)}</p>
     <div class="order"><h4>If a mic disconnects</h4><p class="hint">Yap keeps recording on the next mic in this order. Drag to rearrange. Other devices, like virtual and loopback inputs, are never picked on their own.</p><ol>${list}</ol></div></section>`,
  );
}

// WF-10 — the pill's line while a take continues on the next mic.
function pillHtml(title, switched) {
  const line = switched ? `Mic disconnected — continuing on ${AIRPODS}` : "Recording… release Fn or click Stop";
  return page(title, `<div class="pill"><span class="dot"></span>${esc(line)}</div>`);
}

function page(title, body) {
  return `<!doctype html><html><head><meta charset="utf-8"><style>${TOKENS}
  body{margin:0;padding:24px;background:var(--surface);color:var(--text-primary);font:13px -apple-system,system-ui,sans-serif}
  h1{font-size:12px;color:var(--text-muted);font-weight:500;margin:0 0 12px}
  .wrap{display:flex;align-items:flex-start;gap:2px}
  .menu{background:var(--surface-raised);border:1px solid var(--border-subtle);border-radius:8px;padding:5px;min-width:240px;box-shadow:0 8px 24px rgba(0,0,0,.18)}
  .sub{margin-top:30px}
  .row{display:flex;align-items:center;padding:4px 10px 4px 4px;border-radius:4px}
  .row.hot{background:var(--accent-action);color:var(--text-on-accent)}
  .row.off{color:var(--text-muted)}
  .ck{width:18px;text-align:center}
  .arrow{margin-left:auto;padding-left:16px}
  .panel{background:var(--surface-card);border:1px solid var(--border-subtle);border-radius:12px;padding:16px 18px;max-width:520px}
  .panel h3{margin:0 0 10px;font-size:14px}
  .field{display:flex;align-items:center;gap:12px}
  .field select{font:inherit;padding:5px 8px;min-width:280px}
  .hint{color:var(--text-muted);margin:10px 0 0}
  .order{margin-top:16px}.order h4{margin:0 0 4px;font-size:13px}
  .order ol{list-style:none;margin:8px 0 0;padding:0;display:grid;gap:4px}
  .order li{display:flex;align-items:center;gap:8px;padding:8px 12px;border:1px solid var(--border-subtle);border-radius:var(--radius-sm);background:var(--surface-raised)}
  .order .rank{min-width:1.5em;color:var(--text-muted)}.order .nm{flex:1}.order .ghost{margin-top:0}
  .pill{display:inline-flex;align-items:center;gap:8px;padding:8px 14px;border-radius:var(--radius-pill);background:var(--surface-inverse);color:var(--text-on-inverse)}
  .pill .dot{width:8px;height:8px;border-radius:50%;background:var(--status-live)}
  .ghost{margin-top:10px;font:inherit;background:none;border:1px solid var(--border-subtle);border-radius:6px;padding:5px 12px;color:var(--text-primary)}
  </style></head><body><h1>${esc(title)}</h1>${body}</body></html>`;
}

const JOBS = [
  { file: "mic-menu-before.png", html: () => menuHtml("before: Dictation group (no microphone choice)", false) },
  { file: "mic-menu-after-default.png", html: () => menuHtml("after: Microphone ▸ — nothing picked", true, "default") },
  { file: "mic-menu-after-picked.png", html: () => menuHtml("after: Microphone ▸ — Shure MV7 picked", true, "picked") },
  { file: "mic-menu-after-unplugged.png", html: () => menuHtml("after: Microphone ▸ — picked mic unplugged", true, "unplugged") },
  { file: "mic-settings-after-loading.png", html: () => settingsHtml("after: Settings › Audio — loading", "loading") },
  { file: "mic-settings-after-ready.png", html: () => settingsHtml("after: Settings › Audio — ready", "ready") },
  { file: "mic-settings-after-unplugged.png", html: () => settingsHtml("after: Settings › Audio — picked mic unplugged", "unplugged") },
  { file: "mic-settings-after-empty.png", html: () => settingsHtml("after: Settings › Audio — no microphone", "empty") },
  { file: "mic-settings-after-error.png", html: () => settingsHtml("after: Settings › Audio — list failed", "error") },
  { file: "mic-order-before.png", html: () => orderHtml("before (WF-09): no fallback order, virtual inputs mixed in", "before") },
  { file: "mic-order-after-ranked.png", html: () => orderHtml("after (WF-10): fallback order + Other devices", "ranked") },
  { file: "mic-order-after-fallback.png", html: () => orderHtml("after (WF-10): pinned mic unplugged → next in order", "fallback") },
  { file: "mic-pill-before.png", html: () => pillHtml("before: the pill during a take", false) },
  { file: "mic-pill-after-switched.png", html: () => pillHtml("after (WF-10): the mic vanished mid-take", true) },
];

async function shoot(cdp, file, doc) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 640, height: 400, deviceScaleFactor: 2, mobile: false }, s);
  const { frameTree } = await cdp.send("Page.getFrameTree", {}, s);
  await cdp.send("Page.setDocumentContent", { frameId: frameTree.frame.id, html: doc }, s);
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(150);
  const h = await evaluate(cdp, s, "Math.ceil(document.body.scrollHeight)");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 640, height: Math.max(160, h), deviceScaleFactor: 2, mobile: false }, s);
  const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
  await cdp.send("Target.closeTarget", { targetId });
}

async function main() {
  const argv = process.argv.slice(2);
  const selfTest = argv.includes("--self-test");
  const outAt = argv.indexOf("--out");
  const out = outAt >= 0 ? argv[outAt + 1] : null;
  assertSourcesMatch();
  if (selfTest) {
    for (const st of ["default", "picked", "unplugged"]) {
      console.log("# Microphone ▸ (" + st + ")");
      for (const r of micRows(st)) console.log("  " + (r.checked ? "✓ " : "  ") + r.label + (r.disabled ? "  [disabled]" : ""));
    }
    return;
  }
  if (!out) throw new Error("--out <dir> is required");
  mkdirSync(out, { recursive: true });
  const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-mic-picker");
  try {
    for (const j of JOBS) {
      await shoot(cdp, join(out, j.file), j.html());
      console.log("wrote " + join(out, j.file));
    }
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
    await sleep(200);
    rmSync(profile, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error("mic-picker-preview: " + e.message);
    process.exit(1);
  });
}
