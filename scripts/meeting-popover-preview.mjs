#!/usr/bin/env node
/**
 * meeting-popover-preview.mjs — before/after PNGs of the pill's Meeting
 * popover (WF-08).
 *
 * The real popover is a native popup menu that slots.tsx builds from
 * `popoverEntries` (desktop/src/pill/meetingPopover.ts) over the answer of the
 * `meeting_popover` command (`calendar::meeting_popover`). A native menu
 * cannot be rendered headlessly, so — like scripts/menubar-preview.mjs — this
 * script PARSES the labels out of calendar.rs (the single source the command
 * sends) and checks the row wording in meetingPopover.ts, then draws each
 * state as a macOS-style menu in headless Chrome (--headless=new through
 * scripts/headless-chrome.mjs; never a visible window, never the real app).
 *
 * States: before (the old kind picker), and after — connected with meetings,
 * connected and empty, not connected. Meetings are fixtures.
 *
 *   node scripts/meeting-popover-preview.mjs --out <dir> [--self-test]
 *
 * NO npm DEPENDENCIES. Cleans up its Chrome process and profile dir on exit.
 */

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CALENDAR_RS = "desktop/src-tauri/src/calendar.rs";
const POPOVER_TS = "desktop/src/pill/meetingPopover.ts";
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

function parseArgs(argv) {
  const a = { out: null, selfTest: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--out") a.out = argv[++i];
    else if (k === "--self-test") a.selfTest = true;
    else throw new Error("unknown argument " + k);
  }
  if (!a.selfTest && !a.out) throw new Error("--out <dir> is required");
  return a;
}

export function popoverLabels(src) {
  const get = (name) => {
    const m = src.match(new RegExp(`pub const ${name}: &str =\\s*"((?:[^"\\\\]|\\\\.)*)";`));
    if (!m) throw new Error(`${CALENDAR_RS}: no ${name}`);
    return m[1];
  };
  return {
    empty: get("POPOVER_EMPTY_LABEL"),
    connect: get("POPOVER_CONNECT_LABEL"),
    recordNow: get("POPOVER_RECORD_NOW_LABEL"),
    max: Number((src.match(/pub const POPOVER_MAX: usize = (\d+);/) || [])[1] || NaN),
  };
}

function assertRowWording(ts) {
  for (const needle of ["`Record \\u201c${m.title}\\u201d \\u00b7 ${m.clock}`", "`Record Meeting at ${m.clock}`"]) {
    if (!ts.includes(needle)) throw new Error(`${POPOVER_TS} recordRowText drifted: ${needle}`);
  }
}

const KINDS = ["In person", "Virtual", "Not sure"];
const FIXTURE = [
  { title: "Standup", clock: "2:00 PM" },
  { title: null, clock: "3:30 PM" },
  { title: "Design review", clock: "5:00 PM" },
  { title: "Fourth (never shown)", clock: "6:00 PM" },
];
const row = (m) => (m.title ? `Record “${m.title}” · ${m.clock}` : `Record Meeting at ${m.clock}`);

function states(L) {
  const recordNow = { label: L.recordNow, submenu: KINDS };
  return [
    { file: "meeting-popover-before.png", title: "before: the Meeting button's kind picker", rows: KINDS.map((k) => ({ label: k })) },
    {
      file: "meeting-popover-after-meetings.png",
      title: "after: calendar connected, meetings today",
      rows: [...FIXTURE.slice(0, L.max).map((m) => ({ label: row(m) })), { sep: true }, recordNow],
    },
    { file: "meeting-popover-after-empty.png", title: "after: calendar connected, nothing left today", rows: [{ label: L.empty, disabled: true }, { sep: true }, recordNow] },
    {
      file: "meeting-popover-after-not-connected.png",
      title: "after: no calendar connected",
      rows: [{ label: L.empty, disabled: true }, { label: L.connect }, { sep: true }, recordNow],
    },
  ];
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

function html(title, rows) {
  const items = rows
    .map((r) =>
      r.sep
        ? '<div class="sep"></div>'
        : `<div class="row${r.disabled ? " off" : ""}"><span>${esc(r.label)}</span>${r.submenu ? '<span class="chev">›</span>' : ""}</div>`,
    )
    .join("");
  const sub = rows.find((r) => r.submenu);
  const subHtml = sub ? `<div class="menu sub">${sub.submenu.map((k) => `<div class="row"><span>${esc(k)}</span></div>`).join("")}</div>` : "";
  return `<!doctype html><html><head><meta charset="utf-8"><style>
    body{margin:0;padding:28px;background:#d9dde3;font:13px -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif;color:#1d1d1f}
    h1{font-size:12px;font-weight:500;color:#555;margin:0 0 14px}
    .pill{width:132px;height:30px;border-radius:15px;background:#1c1c1e;margin:0 0 6px 40px;display:flex;align-items:center;justify-content:flex-end;padding-right:12px;box-sizing:border-box}
    .pill i{width:18px;height:18px;border-radius:9px;background:#3a3a3c;outline:2px solid #6e9bff}
    .wrap{display:flex;align-items:flex-start;gap:2px;margin-left:120px}
    .menu{min-width:220px;padding:5px;border-radius:8px;background:rgba(246,246,246,.96);box-shadow:0 0 0 .5px rgba(0,0,0,.25),0 8px 24px rgba(0,0,0,.18)}
    .row{display:flex;justify-content:space-between;gap:24px;padding:3px 10px;border-radius:4px;line-height:16px}
    .row:first-child:not(.off){background:#0a64d8;color:#fff}
    .sub .row:first-child{background:none;color:inherit}
    .off{color:#9a9a9a}
    .sep{height:1px;margin:5px 10px;background:rgba(0,0,0,.12)}
    .chev{opacity:.6}
    .sub{margin-top:${Math.max(0, rows.findIndex((r) => r.submenu)) * 22}px}
  </style></head><body><h1>${esc(title)}</h1><div class="pill"><i></i></div><div class="wrap"><div class="menu">${items}</div>${subHtml}</div></body></html>`;
}

async function shoot(cdp, file, doc) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 640, height: 400, deviceScaleFactor: 2, mobile: false }, s);
  const { frameId } = await cdp.send("Page.getFrameTree", {}, s).then((r) => ({ frameId: r.frameTree.frame.id }));
  await cdp.send("Page.setDocumentContent", { frameId, html: doc }, s);
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(150);
  const h = await evaluate(cdp, s, "Math.ceil(document.body.scrollHeight)");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 640, height: Math.max(160, h), deviceScaleFactor: 2, mobile: false }, s);
  const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
  await cdp.send("Target.closeTarget", { targetId });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const labels = popoverLabels(readFileSync(join(ROOT, CALENDAR_RS), "utf8"));
  if (!(labels.max >= 1)) throw new Error(`${CALENDAR_RS}: no POPOVER_MAX`);
  assertRowWording(readFileSync(join(ROOT, POPOVER_TS), "utf8"));
  const jobs = states(labels);
  if (args.selfTest) {
    for (const j of jobs) {
      console.log("\n# " + j.file);
      for (const r of j.rows) console.log(r.sep ? "  ────" : "  " + (r.disabled ? "(off) " : "") + r.label + (r.submenu ? "  ›  [" + r.submenu.join(" | ") + "]" : ""));
    }
    return;
  }
  mkdirSync(args.out, { recursive: true });
  const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-meeting-popover");
  try {
    for (const j of jobs) {
      await shoot(cdp, join(args.out, j.file), html(j.title, j.rows));
      console.log("wrote " + join(args.out, j.file));
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
    console.error("meeting-popover-preview: " + e.message);
    process.exit(1);
  });
}
