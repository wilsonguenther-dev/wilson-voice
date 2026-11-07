#!/usr/bin/env node
/**
 * chatter-setting-preview.mjs — before/after PNGs for yap-1106-chatter-setting.
 *
 *   BEFORE: Settings › Companion had no "Yappy talks" row (the 50-take
 *           quieting was hidden).
 *   AFTER:  the row — Always / Less over time / Only milestones — with the one
 *           plain sentence and the take count beside it: a new user (12 takes),
 *           a heavy user (120 takes), and Only milestones saved with keyboard
 *           focus on it.
 *
 * The labels and sentences are read out of desktop/src/pill/chatter.ts (text
 * only, no TS compile) so the preview cannot drift from what Settings shows.
 * Drawn in headless Chrome via scripts/headless-chrome.mjs; never a visible
 * window, never the real app. Colours from tokens.css.
 *
 *   node scripts/chatter-setting-preview.mjs --out <dir> [--self-test]
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
const CHATTER_TS = readFileSync(join(ROOT, "desktop/src/pill/chatter.ts"), "utf8");
const COMPANION_TSX = readFileSync(join(ROOT, "desktop/src/views/settings/Companion.tsx"), "utf8");
const TOKENS = readFileSync(join(ROOT, "desktop/src/tokens.css"), "utf8");

function options() {
  const m = CHATTER_TS.match(/CHATTER_OPTIONS[^=]*=\s*\[([\s\S]*?)\n\];/);
  if (!m) throw new Error("chatter.ts has no CHATTER_OPTIONS");
  return [...m[1].matchAll(/\["(\w+)", "([^"]+)", "([^"]+)"\]/g)].map((x) => ({ id: x[1], label: x[2], blurb: x[3] }));
}
function sentence(mode) {
  const body = CHATTER_TS.slice(CHATTER_TS.indexOf("export function chatterSentence"));
  const key = mode === "less" ? "default:" : `case "${mode}":`;
  const at = body.indexOf(key);
  if (at < 0) throw new Error("chatter.ts chatterSentence has no " + mode);
  const m = body.slice(at).match(/return "([^"]+)";/);
  if (!m) throw new Error("chatter.ts chatterSentence " + mode + " has no line");
  return m[1];
}
// Mirrors chatter.ts chatterStatus / takeCountLabel.
const status = (mode, n) => (mode !== "less" ? "" : n + 1 >= 50 ? "Yappy is talking less now." : (50 - 1 - n === 1 ? "1 take until he talks less." : `${50 - 1 - n} takes until he talks less.`));
const countLabel = (n) => `Takes so far: ${n.toLocaleString("en-US")}`;

function assertSourcesMatch() {
  for (const s of ["Yappy talks", "CHATTER_OPTIONS.map", "chatterSentence(chatter)", "takeCountLabel(takes)", "aria-pressed={selected}"]) {
    if (!COMPANION_TSX.includes(s)) throw new Error("preview drifted from Companion.tsx: " + JSON.stringify(s));
  }
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const page = (title, body) => `<!doctype html><html><head><meta charset="utf-8"><style>
${TOKENS}
body{margin:0;padding:20px;background:var(--bg, #f6f5f2);color:var(--text, #1d1d1f);font:14px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif}
.cap{font:500 11px ui-monospace,Menlo,monospace;color:var(--muted, #6b6b70);margin:0 0 10px}
.panel{background:var(--surface, #fff);border:1px solid var(--border, #e4e2dc);border-radius:12px;padding:16px;margin-bottom:12px}
h3{margin:0 0 12px;font-size:14px;font-weight:600}
.row{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:8px}
.profile{all:unset;box-sizing:border-box;display:flex;flex-direction:column;gap:2px;padding:10px 12px;border:1px solid var(--border, #e4e2dc);border-radius:10px}
.profile strong{font-weight:600}.profile span{font-size:12px;color:var(--muted, #6b6b70)}
.profile.active{border-color:var(--accent, #2f6fde);box-shadow:inset 0 0 0 1px var(--accent, #2f6fde)}
.profile.focus{outline:2px solid var(--accent, #2f6fde);outline-offset:2px}
.hint{margin:10px 0 0;font-size:12px;line-height:1.4;color:var(--muted, #6b6b70)}
.count{color:var(--text, #1d1d1f);font-weight:600;font-variant-numeric:tabular-nums;white-space:nowrap}
</style></head><body><p class="cap">${esc(title)}</p>${body}</body></html>`;

const voicePanel = `<div class="panel"><h3>Yappy's voice</h3><p class="hint" style="margin:0">What Yappy says while you talk and when you finish.</p></div>`;
function talksPanel(mode, takes, focus = null) {
  const buttons = options().map((o) => `<button class="profile${o.id === mode ? " active" : ""}${o.id === focus ? " focus" : ""}" aria-pressed="${o.id === mode}"><strong>${esc(o.label)}</strong><span>${esc(o.blurb)}</span></button>`).join("");
  const st = status(mode, takes);
  return `<div class="panel"><h3>Yappy talks</h3><div class="row">${buttons}</div><p class="hint">${esc(sentence(mode))} <span class="count">${esc(countLabel(takes))}${st ? " · " + esc(st) : ""}</span></p></div>`;
}

const JOBS = [
  { file: "chatter-settings-before.png", html: () => page("before: Settings › Companion (the 50-take quieting was hidden)", voicePanel) },
  { file: "chatter-settings-after-new-user.png", html: () => page("after: Yappy talks, default Less over time, 12 takes", voicePanel + talksPanel("less", 12)) },
  { file: "chatter-settings-after-heavy-user.png", html: () => page("after: Less over time, 120 takes (quiet now)", voicePanel + talksPanel("less", 120)) },
  { file: "chatter-settings-after-milestones-focus.png", html: () => page("after: Only milestones saved, keyboard focus on it", voicePanel + talksPanel("milestones", 120, "milestones")) },
  { file: "chatter-settings-after-always.png", html: () => page("after: Always", voicePanel + talksPanel("always", 120)) },
];

async function shoot(cdp, file, doc) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 720, height: 400, deviceScaleFactor: 2, mobile: false }, s);
  const { frameTree } = await cdp.send("Page.getFrameTree", {}, s);
  await cdp.send("Page.setDocumentContent", { frameId: frameTree.frame.id, html: doc }, s);
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(150);
  const h = await evaluate(cdp, s, "Math.ceil(document.body.scrollHeight)");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 720, height: Math.max(160, h), deviceScaleFactor: 2, mobile: false }, s);
  const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
  await cdp.send("Target.closeTarget", { targetId });
}

async function main() {
  const argv = process.argv.slice(2);
  const outAt = argv.indexOf("--out");
  const out = outAt >= 0 ? argv[outAt + 1] : null;
  assertSourcesMatch();
  if (argv.includes("--self-test")) {
    for (const o of options()) console.log(`${o.id}: ${o.label} — ${sentence(o.id)}`);
    return;
  }
  if (!out) throw new Error("--out <dir> is required");
  mkdirSync(out, { recursive: true });
  const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-chatter-setting");
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
    console.error("chatter-setting-preview: " + e.message);
    process.exit(1);
  });
}
