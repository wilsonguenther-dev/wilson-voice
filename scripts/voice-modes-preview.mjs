#!/usr/bin/env node
/**
 * voice-modes-preview.mjs — before/after PNGs for yap-1106-voice-modes.
 *
 *   1. Settings › Companion › the voice row: BEFORE ("Companion tone":
 *      Friendly / Rude / Rose) and AFTER ("Yappy's voice": Best friend /
 *      Badass / Professional / Rose), the after one also with keyboard focus
 *      on Professional and with Professional saved.
 *   2. One card per voice with a sample line for each take size (up to 5
 *      words, about 21, about 61, an essay) and its first during-take lines,
 *      read straight out of desktop/src/pill/voices.ts so the preview cannot
 *      drift from the table the pill uses.
 *
 * Drawn in headless Chrome (--headless=new via scripts/headless-chrome.mjs;
 * never a visible window, never the real app). Colours from tokens.css.
 *
 *   node scripts/voice-modes-preview.mjs --out <dir> [--self-test]
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
const BANDS = [["tiny", "Up to 5 words"], ["paragraph", "About 21 words"], ["memo", "About 61 words"], ["essay", "An essay"]];

/** Pull one voice's block out of voices.ts (text only — no TS compile). */
function voiceBlock(id) {
  const start = VOICES_TS.indexOf(`  ${id}: {\n    name:`);
  if (start < 0) throw new Error("voices.ts has no voice " + id);
  const end = VOICES_TS.indexOf("\n  },\n", start);
  const src = VOICES_TS.slice(start, end);
  const str = (re) => { const m = src.match(re); if (!m) throw new Error(`voices.ts ${id}: missing ${re}`); return m[1]; };
  const arr = (key) => {
    const m = src.match(new RegExp(`\\b${key}: \\[([^\\]]*)\\]`));
    if (!m) throw new Error(`voices.ts ${id}: missing ${key}`);
    return [...m[1].matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((x) => x[1]);
  };
  return {
    id, name: str(/name: "([^"]+)"/), blurb: str(/blurb: "([^"]+)"/),
    take: Object.fromEntries(BANDS.map(([b]) => [b, arr(b)])),
    during: arr("during"),
  };
}

function assertSourcesMatch() {
  for (const s of ["Yappy's voice", "VOICE_OPTIONS.map", "aria-pressed={selected}"]) {
    if (!COMPANION_TSX.includes(s)) throw new Error("preview drifted from Companion.tsx: " + JSON.stringify(s));
  }
}

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
// Mirrors voices.ts `typingSaved` (40 words a minute).
const saved = (w) => { const sec = Math.round((w / 40) * 60); return sec < 60 ? `${sec} s` : `${Math.max(1, Math.round(sec / 60))} min`; };
const fill = (s, w) => s.replace(/\{w\}/g, String(w)).replace(/\{saved\}/g, saved(w));

const page = (title, body) => `<!doctype html><html><head><meta charset="utf-8"><style>
${TOKENS}
body{margin:0;padding:20px;background:var(--bg, #f6f5f2);color:var(--text, #1d1d1f);font:14px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif}
.cap{font:500 11px ui-monospace,Menlo,monospace;color:var(--muted, #6b6b70);margin:0 0 10px}
.panel{background:var(--surface, #fff);border:1px solid var(--border, #e4e2dc);border-radius:12px;padding:16px}
h3{margin:0 0 12px;font-size:14px;font-weight:600}
.row{display:grid;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));gap:8px}
.profile{all:unset;box-sizing:border-box;display:flex;flex-direction:column;gap:2px;padding:10px 12px;border:1px solid var(--border, #e4e2dc);border-radius:10px;cursor:pointer}
.profile strong{font-weight:600}.profile span{font-size:12px;color:var(--muted, #6b6b70)}
.profile.active{border-color:var(--accent, #2f6fde);box-shadow:inset 0 0 0 1px var(--accent, #2f6fde)}
.profile.focus{outline:2px solid var(--accent, #2f6fde);outline-offset:2px}
.hint{margin:10px 0 0;font-size:12px;color:var(--muted, #6b6b70)}
.cards{display:grid;grid-template-columns:repeat(2,1fr);gap:12px}
.b{font:500 11px ui-monospace,Menlo,monospace;color:var(--muted, #6b6b70);margin:10px 0 4px}
.bubble{display:inline-block;background:#fff;border:1px solid var(--border, #e4e2dc);border-radius:10px;padding:4px 9px;margin:0 6px 4px 0;font-size:12px}
</style></head><body><p class="cap">${esc(title)}</p>${body}</body></html>`;

const BEFORE = [["Friendly", "Warm and encouraging"], ["Rude 😏", "Sassy — teases when you ramble"], ["Rose 🌹", "Sweet and adoring"]];

function settingsHtml(title, mode, active = "bestfriend", focus = null) {
  const opts = mode === "before"
    ? BEFORE.map(([n, b], i) => ({ id: String(i), name: n, blurb: b, on: i === 0 }))
    : IDS.map((id) => { const v = voiceBlock(id); return { id, name: v.name, blurb: v.blurb, on: id === active }; });
  const heading = mode === "before" ? "Companion tone" : "Yappy's voice";
  const hint = mode === "before"
    ? "How Yappy reacts when you finish, whichever companion you picked."
    : "What Yappy says while you talk and when you finish. It applies to whichever companion sits in the pill.";
  const buttons = opts.map((o) => `<button class="profile${o.on ? " active" : ""}${o.id === focus ? " focus" : ""}" aria-pressed="${o.on}"><strong>${esc(o.name)}</strong><span>${esc(o.blurb)}</span></button>`).join("");
  return page(title, `<div class="panel"><h3>${esc(heading)}</h3><div class="row">${buttons}</div><p class="hint">${esc(hint)}</p></div>`);
}

function linesHtml(title) {
  const cards = IDS.map((id) => {
    const v = voiceBlock(id);
    const bands = BANDS.map(([b, label], i) => `<div class="b">${label}</div>${v.take[b].slice(0, 2).map((l) => `<span class="bubble">${esc(fill(l, [3, 21, 61, 220][i]))}</span>`).join("")}`).join("");
    return `<div class="panel"><h3>${esc(v.name)}</h3><div class="hint" style="margin:0">${esc(v.blurb)}</div>${bands}<div class="b">While you talk</div>${v.during.slice(0, 4).map((l) => `<span class="bubble">${esc(l)}</span>`).join("")}</div>`;
  }).join("");
  return page(title, `<div class="cards">${cards}</div>`);
}

const JOBS = [
  { file: "voice-settings-before.png", html: () => settingsHtml("before: Settings › Companion › Companion tone", "before") },
  { file: "voice-settings-after.png", html: () => settingsHtml("after: Settings › Companion › Yappy's voice (default)", "after") },
  { file: "voice-settings-after-professional-focus.png", html: () => settingsHtml("after: Professional saved, keyboard focus on it", "after", "professional", "professional") },
  { file: "voice-lines-after.png", html: () => linesHtml("after: sample lines per voice and take size") },
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
  const selfTest = argv.includes("--self-test");
  const outAt = argv.indexOf("--out");
  const out = outAt >= 0 ? argv[outAt + 1] : null;
  assertSourcesMatch();
  if (selfTest) {
    for (const id of IDS) {
      const v = voiceBlock(id);
      const n = new Set([...BANDS.flatMap(([b]) => v.take[b]), ...v.during]).size;
      console.log(`${v.name}: ${n} take+during lines`);
    }
    return;
  }
  if (!out) throw new Error("--out <dir> is required");
  mkdirSync(out, { recursive: true });
  const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-voice-modes");
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
    console.error("voice-modes-preview: " + e.message);
    process.exit(1);
  });
}
