#!/usr/bin/env node
/**
 * menubar-preview.mjs — before/after PNGs of the menu-bar menu
 * (yap-1106-menubar-redesign + WF-07 upcoming meetings + WF-11 Languages).
 *
 * The real menu is a native NSMenu built by lib.rs FROM
 * `tray_menu::MENU_LAYOUT` + `TRAY_ITEMS`. A native menu cannot be rendered
 * headlessly, so this script does the next honest thing: it PARSES those two
 * constants out of `desktop/src-tauri/src/tray_menu.rs` (the single source the
 * app builds from) at two revisions, draws each as a macOS-style menu in
 * headless Chrome (--headless=new via scripts/headless-chrome.mjs; never a
 * visible window, never the real app) and writes one PNG per state.
 *
 * Only the ORDER, GROUPING and LABELS are real. The calendar rows' text follows
 * `tray_menu::upcoming_menu` (the format strings are checked against the
 * source below so the preview cannot silently drift from the Rust); the
 * meetings themselves are fixtures. Runtime text the app sets later (license
 * line, accelerators, the live meeting clock) is shown as the static label.
 *
 *   node scripts/menubar-preview.mjs --out <dir> [--before <git rev>] [--self-test]
 *
 *   --before     revision for the "before" menu (default origin/main)
 *   --self-test  parse both revisions and print the menus; no Chrome
 *
 * NO npm DEPENDENCIES. Cleans up its Chrome process and profile dir on exit.
 */

import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { evaluate, openChrome, sleep } from "./headless-chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TRAY_RS = "desktop/src-tauri/src/tray_menu.rs";
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

function parseArgs(argv) {
  const a = { out: null, before: "origin/main", selfTest: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--out") a.out = argv[++i];
    else if (k === "--before") a.before = argv[++i];
    else if (k === "--self-test") a.selfTest = true;
    else throw new Error("unknown argument " + k);
  }
  if (!a.selfTest && !a.out) throw new Error("--out <dir> is required");
  return a;
}

/* ── parse tray_menu.rs ─────────────────────────────────────────────────── */

export function parseTrayMenu(src) {
  const consts = new Map();
  for (const m of src.matchAll(/pub const (\w+): &str = "((?:[^"\\]|\\.)*)";/g)) {
    consts.set(m[1], m[2].replace(/\\u\{([0-9a-fA-F]+)\}/g, (_, h) => String.fromCodePoint(parseInt(h, 16))));
  }
  const str = (tok) => {
    const t = tok.trim();
    if (t.startsWith('"')) return t.slice(1, -1);
    if (!consts.has(t)) throw new Error("tray_menu.rs: unknown const " + t);
    return consts.get(t);
  };
  const items = new Map();
  for (const m of src.matchAll(/TrayItem\s*\{\s*id:\s*(\w+),\s*label:\s*("[^"]*"|\w+),\s*kind:\s*ItemKind::(\w+)/g)) {
    items.set(str(m[1]), { id: str(m[1]), label: str(m[2]), kind: m[3] });
  }
  const start = src.indexOf("pub const MENU_LAYOUT");
  if (start < 0) throw new Error("tray_menu.rs: no MENU_LAYOUT");
  const body = src.slice(src.indexOf("= [", start), src.indexOf("];", start));
  const layout = [];
  for (const m of body.matchAll(/Slot::(Separator|Item\((\w+)\))/g)) {
    if (m[1] === "Separator") layout.push({ sep: true });
    else {
      const id = str(m[2]);
      if (!items.has(id)) throw new Error("MENU_LAYOUT names " + id + " with no TrayItem");
      layout.push(items.get(id));
    }
  }
  return { consts, items, layout };
}

function sourceAt(rev) {
  if (rev === "WORKTREE") return readFileSync(join(ROOT, TRAY_RS), "utf8");
  return execFileSync("git", ["-C", ROOT, "show", rev + ":" + TRAY_RS], { encoding: "utf8" });
}

/* ── the WF-07 rows, mirroring tray_menu::upcoming_menu ─────────────────── */

const FIXTURE_MEETINGS = [
  { title: "Standup", clock: "2:00 PM", link: true },
  { title: "Design review", clock: "3:30 PM", link: true },
  { title: null, clock: "5:00 PM", link: false },
];

function assertFormatsMatchSource(src) {
  // If any of these strings changes in the Rust, the preview must change too.
  for (const needle of [
    'format!("Next: {t} \\u{b7} {clock}")',
    'format!("Next meeting {clock}")',
    'format!("Record \\u{201c}{t}\\u{201d} \\u{b7} {clock}")',
    'format!("Record Meeting at {clock}")',
  ]) {
    if (!src.includes(needle)) throw new Error("tray_menu.rs upcoming_menu format drifted: " + needle);
  }
}

function upcomingRows(parsed, { hideTitles, meetings }) {
  const noMore = parsed.consts.get("NO_MORE_MEETINGS_LABEL");
  const first = meetings[0];
  const next = !first
    ? noMore
    : !hideTitles && first.title
      ? `Next: ${first.title} · ${first.clock}`
      : `Next meeting ${first.clock}`;
  const rows = [{ label: next, disabled: true }];
  if (meetings.length) {
    rows.push({
      label: parsed.items.get(parsed.consts.get("ID_UPCOMING")).label,
      submenu: meetings.map((m) =>
        !hideTitles && m.title
          ? `Record “${m.title}” · ${m.clock}`
          : `Record Meeting at ${m.clock}`
      ),
    });
  }
  rows.push({ label: parsed.items.get(parsed.consts.get("ID_HIDE_MEETING_TITLES")).label, checked: hideTitles });
  return rows;
}

const LANGUAGE_NAMES = { en: "English", es: "Spanish", fr: "French" };

/**
 * WF-11 — the Languages submenu as `tray_menu::language_menu` builds it:
 * Auto-detect (only with a choice in the set and a model that can detect),
 * each language in picker order, the check on what the chip shows, then a
 * separator and "Language Settings…". Labels come from the source consts.
 */
function languageRow(parsed, { set, picked, canDetect = true }) {
  const id = parsed.consts.get("ID_LANGUAGES");
  const auto = parsed.consts.get("LANGUAGE_AUTO_LABEL");
  const settings = parsed.consts.get("LANGUAGE_SETTINGS_LABEL");
  if (!id || !auto || !settings || !parsed.items.has(id)) {
    throw new Error("tray_menu.rs: no Languages submenu at this revision");
  }
  const order = Object.keys(LANGUAGE_NAMES);
  const langs = [...set].sort((a, b) => order.indexOf(a) - order.indexOf(b));
  const picks = [...(set.length > 1 && canDetect ? ["auto"] : []), ...langs];
  const submenu = picks.map((p) => ({ label: p === "auto" ? auto : LANGUAGE_NAMES[p], checked: p === picked }));
  submenu.push({ sep: true }, { label: settings });
  return { label: parsed.items.get(id).label, submenu };
}

/** The rows as the app shows them in one state. */
function menuFor(parsed, state) {
  const out = [];
  const kindId = parsed.consts.get("ID_MEETING_KIND");
  const toggleId = parsed.consts.get("ID_MEETING_TOGGLE");
  const rec = state.recording;
  if (rec) {
    // apply_meeting_menu: header at 0, the toggle (now Stop) at 1, a separator.
    out.push({ label: "Recording · Standup", disabled: true });
    out.push({ label: "Stop Meeting · 12:04" });
    out.push({ sep: true });
  }
  for (const slot of parsed.layout) {
    if (slot.sep) {
      out.push({ sep: true });
      continue;
    }
    if (rec && slot.id === toggleId) continue;
    out.push({
      label: slot.label,
      disabled: slot.kind === "StateLabel" || (rec && slot.id === kindId),
      submenuArrow: slot.kind === "KindSubmenu",
    });
    if (slot.id === kindId && state.calendar) out.push(...upcomingRows(parsed, state.calendar));
    // WF-11 — inserted under Hands-free (under Microphone once the device
    // list lands; the Microphone submenu is not drawn by this preview).
    if (slot.id === parsed.consts.get("ID_HANDS_FREE") && state.languages) {
      out.push(languageRow(parsed, state.languages));
    }
  }
  // A group emptied by the recording move leaves a doubled separator; NSMenu
  // shows both, so the preview does too.
  return out;
}

/* ── render ─────────────────────────────────────────────────────────────── */

function esc(s) {
  return String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
}

function html(title, rows) {
  const openSub = rows.find((r) => r.submenu);
  const li = rows
    .map((r) => {
      if (r.sep) return `<div class="sep"></div>`;
      const cls = ["row", r.disabled ? "dis" : "", r === openSub ? "hot" : ""].join(" ");
      const check = r.checked ? "✓" : "";
      const arrow = r.submenu || r.submenuArrow ? `<span class="arrow">›</span>` : "";
      return `<div class="${cls}"><span class="check">${check}</span><span class="lbl">${esc(r.label)}</span>${arrow}</div>`;
    })
    .join("");
  const sub = openSub
    ? `<div class="menu sub">${openSub.submenu
        .map((t) => (typeof t === "string" ? { label: t } : t))
        .map((t) =>
          t.sep
            ? `<div class="sep"></div>`
            : `<div class="row"><span class="check">${t.checked ? "✓" : ""}</span><span class="lbl">${esc(t.label)}</span></div>`
        )
        .join("")}</div>`
    : "";
  return `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;background:#2e3138;font:13px -apple-system,BlinkMacSystemFont,"SF Pro Text","Helvetica Neue",sans-serif;-webkit-font-smoothing:antialiased}
  .cap{color:#c9ccd3;font-size:12px;padding:14px 18px 0}
  .wrap{display:flex;align-items:flex-start;gap:2px;padding:10px 18px 18px}
  .menu{background:rgba(246,246,246,.97);border-radius:8px;box-shadow:0 0 0 .5px rgba(0,0,0,.35),0 10px 30px rgba(0,0,0,.35);padding:5px;min-width:250px;color:#1d1d1f}
  .row{display:flex;align-items:center;height:22px;padding:0 8px 0 4px;border-radius:4px;white-space:nowrap}
  .row.hot{background:#2f6fe4;color:#fff}
  .row.dis{color:#8d8d92}
  .check{width:18px;text-align:center;font-size:12px}
  .lbl{flex:1}
  .arrow{margin-left:18px;font-size:15px}
  .sep{height:1px;background:rgba(0,0,0,.12);margin:5px 9px}
  .sub{margin-top:var(--subtop,0)}
  </style></head><body><div class="cap">${esc(title)}</div><div class="wrap"><div class="menu main">${li}</div>${sub}</div>
  <script>
  const hot=document.querySelector('.row.hot');const sub=document.querySelector('.sub');
  if(hot&&sub){sub.style.setProperty('--subtop',(hot.offsetTop-5)+'px')}
  </script></body></html>`;
}

const STATES = [
  { name: "idle", title: "Calendar off", state: {} },
  { name: "recording", title: "A meeting recording", state: { recording: true } },
  {
    name: "calendar",
    title: "Use my calendar on: next meeting + today's meetings",
    state: { calendar: { hideTitles: false, meetings: FIXTURE_MEETINGS } },
    afterOnly: true,
  },
  {
    name: "calendar-hidden-titles",
    title: "Use my calendar on, Hide Meeting Titles checked",
    state: { calendar: { hideTitles: true, meetings: FIXTURE_MEETINGS } },
    afterOnly: true,
  },
  {
    name: "calendar-empty-day",
    title: "Use my calendar on, nothing left today",
    state: { calendar: { hideTitles: false, meetings: [] } },
    afterOnly: true,
  },
  {
    name: "languages-auto",
    title: "Languages: English + Spanish, Auto-detect on",
    state: { languages: { set: ["es", "en"], picked: "auto" } },
    afterOnly: true,
  },
  {
    name: "languages-spanish",
    title: "Languages: Spanish picked (from the tray or the pill's chip)",
    state: { languages: { set: ["en", "es"], picked: "es" } },
    afterOnly: true,
  },
  {
    name: "languages-no-detect",
    title: "Languages: the speech model cannot detect, so no Auto-detect row",
    state: { languages: { set: ["en", "es", "fr"], picked: "en", canDetect: false } },
    afterOnly: true,
  },
  {
    name: "languages-one",
    title: "Languages: one language (default install)",
    state: { languages: { set: ["en"], picked: "en" } },
    afterOnly: true,
  },
];

async function shoot(cdp, file, doc) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 720, height: 760, deviceScaleFactor: 2, mobile: false }, s);
  const { frameId } = await cdp.send("Page.getFrameTree", {}, s).then((r) => ({ frameId: r.frameTree.frame.id }));
  await cdp.send("Page.setDocumentContent", { frameId, html: doc }, s);
  await evaluate(cdp, s, "document.fonts.ready.then(() => true)");
  await sleep(150);
  const h = await evaluate(cdp, s, "Math.ceil(document.body.scrollHeight)");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: 720, height: Math.max(200, h), deviceScaleFactor: 2, mobile: false }, s);
  const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
  await cdp.send("Target.closeTarget", { targetId });
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const afterSrc = sourceAt("WORKTREE");
  assertFormatsMatchSource(afterSrc);
  const sides = [
    { side: "before", parsed: parseTrayMenu(sourceAt(args.before)) },
    { side: "after", parsed: parseTrayMenu(afterSrc) },
  ];
  const jobs = [];
  for (const { side, parsed } of sides) {
    for (const st of STATES) {
      if (st.afterOnly && side === "before") continue;
      jobs.push({ file: `menubar-${side}-${st.name}.png`, title: `${side}: ${st.title}`, rows: menuFor(parsed, st.state) });
    }
  }
  if (args.selfTest) {
    for (const j of jobs) {
      console.log("\n# " + j.file);
      for (const r of j.rows) console.log(r.sep ? "  ────" : "  " + (r.checked ? "✓ " : "") + r.label + (r.submenu ? "  ›  [" + r.submenu.map((t) => (typeof t === "string" ? t : t.sep ? "──" : (t.checked ? "✓ " : "") + t.label)).join(" | ") + "]" : ""));
    }
    return;
  }
  mkdirSync(args.out, { recursive: true });
  const { proc, cdp, ws, profile } = await openChrome(CHROME, "yap-menubar");
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
    console.error("menubar-preview: " + e.message);
    process.exit(1);
  });
}
