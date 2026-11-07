#!/usr/bin/env node
/**
 * curse-setting-preview.mjs — before/after PNGs for yap-1106-curse-setting.
 *
 *   BEFORE: Settings › Companion › Yappy's voice with Badass saved (no curse
 *           toggle anywhere).
 *   AFTER:  Badass saved, the curse toggle shown and OFF (the default);
 *           Badass with it ON and keyboard focus on it; Professional saved
 *           (the toggle is gone); plus sample Badass curse lines.
 *
 * Voice names, blurbs, curse lines and the toggle copy are read straight out
 * of desktop/src/pill/voices.ts and Companion.tsx (text only, no TS compile),
 * so the preview cannot drift from what the app ships.
 *
 * Drawn in headless Chrome (--headless=new via scripts/headless-chrome.mjs;
 * never a visible window, never the real app). Colours from tokens.css.
 *
 *   node scripts/curse-setting-preview.mjs --out <dir> [--self-test]
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
const VOICES_TS = readFileSync(join(ROOT, "desktop/src/pill/voices.ts"), "utf8");
const COMPANION_TSX = readFileSync(join(ROOT, "desktop/src/views/settings/Companion.tsx"), "utf8");
const TOKENS = readFileSync(join(ROOT, "desktop/src/tokens.css"), "utf8");

const IDS = ["bestfriend", "badass", "professional", "rose"];
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

function voiceBlock(id) {
  const start = VOICES_TS.indexOf(`  ${id}: {\n    name:`);
  if (start < 0) throw new Error("voices.ts has no voice " + id);
  const src = VOICES_TS.slice(start, VOICES_TS.indexOf("\n  },\n", start));
  const str = (re) => { const m = src.match(re); if (!m) throw new Error(`voices.ts ${id}: missing ${re}`); return m[1]; };
  const curse = src.match(/\bcurse: \[([^\]]*)\]/);
  return {
    id, name: str(/name: "([^"]+)"/), blurb: str(/blurb: "([^"]+)"/),
    curse: curse ? [...curse[1].matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((x) => x[1]) : [],
  };
}

/** The toggle's label + hint, as Companion.tsx ships them. */
function toggleCopy() {
  const label = COMPANION_TSX.match(/data-curse-toggle[\s\S]*?<span>([^<]+)<\/span>/);
  const hint = COMPANION_TSX.match(/data-curse-setting[\s\S]*?settings-hint">([^<]+)<\/p>/);
  if (!label || !hint) throw new Error("preview drifted from Companion.tsx: curse toggle copy not found");
  return { label: label[1].trim(), hint: hint[1].trim() };
}

const page = (title, body) => `<!doctype html><html><head><meta charset="utf-8"><style>
${TOKENS}
body{margin:0;padding:20px;background:var(--bg, #f6f5f2);color:var(--text, #1d1d1f);font:14px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif}
.cap{font:500 11px ui-monospace,Menlo,monospace;color:var(--muted, #6b6b70);margin:0 0 10px}
.panel{background:var(--surface, #fff);border:1px solid var(--border, #e4e2dc);border-radius:12px;padding:16px}
h3{margin:0 0 12px;font-size:14px;font-weight:600}
.row{display:grid;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));gap:8px}
.profile{all:unset;box-sizing:border-box;display:flex;flex-direction:column;gap:2px;padding:10px 12px;border:1px solid var(--border, #e4e2dc);border-radius:10px}
.profile strong{font-weight:600}.profile span{font-size:12px;color:var(--muted, #6b6b70)}
.profile.active{border-color:var(--accent, #2f6fde);box-shadow:inset 0 0 0 1px var(--accent, #2f6fde)}
.hint{margin:10px 0 0;font-size:12px;color:var(--muted, #6b6b70)}
.curse{margin-top:var(--space-4, 16px)}
.toggle{display:flex;gap:10px;align-items:flex-start;padding:12px;border-radius:12px;border:1px solid var(--border, #e4e2dc);background:var(--bg-card, #fff)}
.toggle input{margin:2px 0 0}
.toggle.focus input{outline:2px solid var(--accent, #2f6fde);outline-offset:2px}
.bubble{display:inline-block;background:#fff;border:1px solid var(--border, #e4e2dc);border-radius:10px;padding:4px 9px;margin:0 6px 4px 0;font-size:12px}
</style></head><body><p class="cap">${esc(title)}</p>${body}</body></html>`;

function settingsHtml(title, active, toggle /* null | "off" | "on" */, focus = false) {
  const buttons = IDS.map((id) => {
    const v = voiceBlock(id);
    return `<button class="profile${id === active ? " active" : ""}" aria-pressed="${id === active}"><strong>${esc(v.name)}</strong><span>${esc(v.blurb)}</span></button>`;
  }).join("");
  const c = toggleCopy();
  const curse = toggle
    ? `<div class="curse"><label class="toggle${focus ? " focus" : ""}"><input type="checkbox"${toggle === "on" ? " checked" : ""}><span>${esc(c.label)}</span></label><p class="hint">${esc(c.hint)}</p></div>`
    : "";
  return page(title, `<div class="panel"><h3>Yappy's voice</h3><div class="row">${buttons}</div><p class="hint">What Yappy says while you talk and when you finish. It applies to whichever companion sits in the pill.</p>${curse}</div>`);
}

function linesHtml(title) {
  const v = voiceBlock("badass");
  return page(title, `<div class="panel"><h3>${esc(v.name)} with the curse setting on</h3><p class="hint" style="margin:0 0 8px">About one finished-take line in four is one of these.</p>${v.curse.map((l) => `<span class="bubble">${esc(l)}</span>`).join("")}</div>`);
}

const JOBS = [
  { file: "curse-settings-before.png", html: () => settingsHtml("before: Badass saved, no curse setting", "badass", null) },
  { file: "curse-settings-after-off.png", html: () => settingsHtml("after: Badass saved, curse toggle shown, off by default", "badass", "off") },
  { file: "curse-settings-after-on-focus.png", html: () => settingsHtml("after: curse on, keyboard focus on the toggle", "badass", "on", true) },
  { file: "curse-settings-after-professional.png", html: () => settingsHtml("after: Professional saved, no curse toggle", "professional", null) },
  { file: "curse-lines-after.png", html: () => linesHtml("after: the Badass curse lines") },
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
  const c = toggleCopy();
  if (argv.includes("--self-test")) {
    console.log(`toggle: ${c.label} | ${c.hint}`);
    console.log(`badass curse lines: ${voiceBlock("badass").curse.length}`);
    return;
  }
  if (!out) throw new Error("--out <dir> is required");
  mkdirSync(out, { recursive: true });
  const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-curse-setting");
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
    console.error("curse-setting-preview: " + e.message);
    process.exit(1);
  });
}
