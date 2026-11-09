#!/usr/bin/env node
/**
 * yap24-UI3 — the pill's visual-QA matrix.
 *
 * The 2026-09-15 run's builders logged "NOT visually QA'd" on Y2-B, Y5-B, Y5-C
 * and Y5-D: a green gate cannot see a pill. This is the instrument that can.
 * It photographs float.html in EVERY phase x dock {bottom,left,right} x
 * character {yappy} (Classic was retired by yap-1106-retire-classic-pill), and
 * FAILS on three structural conditions:
 *
 *   empty-text    a text slot that is on screen but says nothing (the phase
 *                 line, a gate note, a progress numeral, a speech bubble)
 *   clipped-text  text cut off — by its own overflow box, or by the edge of the
 *                 300x140 float window
 *   overflow      any visible box that leaves the float window
 *
 * yap-1106-one-pill adds the ONE-PILL checks, for every phase x {no meeting,
 * meeting recording} x dock (Yappy):
 *
 *   one-pill-overlap  the painted hint, bubble, clock or progress box overlaps
 *                     another, or the character box the layout published
 *   clock-detached    the clock box is more than 12 pt from the character box
 *   panel-not-union   the window is not exactly the union of the layout's
 *                     content boxes plus the 10 pt margin (pill/state.ts)
 *   slot-off-layout   a painted slot is not inside the box the layout gave it
 *
 * fix-b1-2 adds the CARD checks. Every card on screen (the cards named by
 * desktop/src/pill/state.ts CARD_CLASSES — copy card, meeting offer, WF-12's
 * language offer, coaching nudge) is a painted slot too, so a card over the
 * character, bubble, hint or clock box (or over another card) is a
 * one-pill-overlap, a card outside the row the layout reserved is a
 * slot-off-layout, and:
 *
 *   card-outside-panel  any pixel of a card past the panel edge (the NSPanel
 *                       clips it) — judged at every dock x {no meeting,
 *                       meeting}, so the lang-offer scene covers all six
 *
 * The window is sized to the layout's panel (window.__YAP_PILL_LAYOUT__), the
 * way float_pill.rs set_panel_frame sizes the NSPanel, before it is judged.
 *
 * plus one INSTRUMENT check, phase-not-applied, so a typo or a stale bundle can
 * never pass as a photo of `idle`. Structural, never pixel equality: a
 * legitimate restyle must not have to relitigate a PNG.
 *
 * HOW A PHASE IS FORCED. A real phase needs the backend to emit the right
 * events in order, which a browser cannot. So the matrix builds the bundle
 * with YAP_PILL_MATRIX=1, which compiles in float-main's DEV-ONLY handle
 * (src/pill/forced.ts): `float.html?phase=&pill=&dock=`. A normal
 * `npm run build` defines it false and rollup drops it — the released float
 * window ignores its query string. The build goes to a SCRATCH outDir, never
 * desktop/dist, so this never clobbers the bundle the app or the smoke judges.
 *
 * THE PHASE LIST IS NOT KEPT HERE. The page publishes it from the pill's own
 * table (`matrixPhases()` = ALL_LIVE_PHASES through `phaseVisual`), so a phase a
 * PILL item adds is photographed with no edit to this file.
 *
 * WHAT IS DRIVEN (read before believing a result): the SAME headless Chrome
 * smoke-windowed.mjs drives (scripts/headless-chrome.mjs, --headless=new — never
 * a visible window, never the app), at the float window's real size (300x140
 * logical, float_pill.rs PILL_W/PILL_H) and Retina scale 2, with the Tauri
 * bridge shimmed. It is NOT the WKWebView. The backdrop behind the transparent
 * window is a flat dark desktop grey so white-on-glass copy is legible in a PNG.
 *
 * USAGE
 *   node scripts/pill-matrix.mjs --out <dir>             every phase x 3 docks x 2 characters
 *   node scripts/pill-matrix.mjs --out <dir> --only gated  one phase x 3 x 2 (what each PILL item runs)
 *   node scripts/pill-matrix.mjs --out <dir> --phase meeting-saved  the Saved stop beat x 3 docks
 *   node scripts/pill-matrix.mjs --out <dir> --phase meeting-notes  yap-1107: every notes transition x 3 docks
 *   node scripts/pill-matrix.mjs --self-test-must-fail   prove the detectors are not blind
 *   node scripts/pill-matrix.mjs --clicks [--only <phase>]  yap-1106-fb-02: click every control
 *           (`[data-pill-control]`) of the one pill at its centre and four inner
 *           corners, once each, in every dock x {no meeting, meeting}; assert ONE
 *           click lands on that control per press, the control is >= 24 x 24 pt,
 *           and every probe point is inside the hit rects the page told the
 *           NSPanel (window.__YAP_PILL_HITBOXES__). Writes clicks.json to --out.
 *
 *   --out   a SCRATCH dir (default: a fresh mktemp dir). The PNGs are evidence
 *           for a PR, never a commit — do not point this inside the repo.
 *   --only  a phase by variant (model-loading) or product id (model_loading).
 *   --phase meeting-saved — fix-b8-1: only the "Saved" stop beat, on the idle
 *           and listening (stopped mid-take) rows, at every dock. The meeting
 *           records, the page settles, then the matrix emits the `meeting`
 *           status "stopped" through the page's own listener (the event name is
 *           READ from desktop/src/meetings/consent.ts) and captures inside the
 *           2.4 s hold. A beat that is not painted is a saved-beat-missing
 *           finding; a page with no listener to stop is phase-not-applied.
 *           The full matrix runs the saved state too (MEETINGS).
 *   --sky   yap-1106-pill-sky: the time of day the pill's sky is pinned to
 *           (dawn|day|dusk|night, or "all" to walk every phase x dock under
 *           each of the four). Default day, so a PNG never depends on the
 *           hour the matrix ran. A sky the page did not apply (<html data-sky>)
 *           is a phase-not-applied finding.
 *
 * ENV  YAP_SMOKE_CHROME (Chromium path, shared with the smoke), YAP_PILL_PORT
 *      (preview port; default: a free one), YAP_PILL_SETTLE_MS (default 900).
 *
 * EXIT  0 clean · 1 a capture carries a finding · 2 usage · 3 environment /
 *       instrument failure · 9 self-test found the detectors blind.
 */

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  TAURI_SHIM,
  buildMatrixBundle,
  evaluate,
  fitPillPanel,
  openChrome,
  pillFixtureScript,
  servePreview as servePreviewShared,
  settlePhase,
  sleep,
} from "./headless-chrome.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DESKTOP = join(ROOT, "desktop");
const W = 300; // float_pill.rs PILL_W
const H = 140; // float_pill.rs PILL_H
const SCALE = 2;
const DOCKS = ["bottom", "left", "right"];
/* yap-1106-retire-classic-pill — Classic is retired; Yappy is the only pill. */
const CHARACTERS = ["yappy", "hoot", "tidy", "gulp", "shelly"]; // yap-1106-main-character-switch: every character that can sit in the pill
/* yap-1106-one-pill — every capture also runs with and without a meeting. */
/* fix-b8-1-meeting-saved-beat-gets-a-slot — and in the 2.4 s "Saved" stop beat. */
let MEETINGS = ["none", "recording", "saved"];
/* What the matrix waits after emitting the stop, before it fits and shoots (well inside SAVED_HOLD_MS). */
const SAVED_EMIT_SETTLE_MS = 300;
const SETTLE_MS = Number(process.env.YAP_PILL_SETTLE_MS || 900);
/* yap-1106-pill-sky — the Home room's four times of day (desktop/src/pill/sky.ts SKY_TIMES). */
const SKY_TIMES = ["dawn", "day", "dusk", "night"];
const CHROME =
  process.env.YAP_SMOKE_CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";

/* ── args ───────────────────────────────────────────────────────────────── */

function parseArgs(argv) {
  const a = { out: null, only: null, sky: "day", phase: null, selfTest: false, clicks: false };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (k === "--out" || k === "--only" || k === "--sky" || k === "--phase") {
      const v = argv[++i];
      if (!v || v.startsWith("--")) usage(k + " needs a value");
      a[k.slice(2)] = v;
    } else if (k.startsWith("--out=")) a.out = k.slice(6);
    else if (k.startsWith("--only=")) a.only = k.slice(7);
    else if (k.startsWith("--sky=")) a.sky = k.slice(6);
    else if (k.startsWith("--phase=")) a.phase = k.slice(8);
    else if (k === "--self-test-must-fail") a.selfTest = true;
    else if (k === "--clicks") a.clicks = true;
    else usage("unknown argument " + k);
  }
  if (a.phase !== null && a.phase !== "meeting-saved" && a.phase !== "meeting-notes") usage("--phase knows two scenes: meeting-saved, meeting-notes");
  if (a.phase !== null && a.only !== null) usage("--phase and --only are exclusive");
  if (a.sky !== "all" && !SKY_TIMES.includes(a.sky)) usage("--sky must be one of " + SKY_TIMES.join("|") + "|all");
  return a;
}
function usage(msg) {
  console.error("pill-matrix: " + msg + "\nusage: node scripts/pill-matrix.mjs [--out <dir>] [--only <phase>] [--phase meeting-saved] [--sky <dawn|day|dusk|night|all>] [--clicks] [--self-test-must-fail]");
  process.exit(2);
}
const ARGS = parseArgs(process.argv.slice(2));

/* ── the shim's answers, per capture ────────────────────────────────────── */
/*
 * Runs before the bundle, after TAURI_SHIM's table is read — so it must set
 * __YAP_FIXTURES__ FIRST. The capsules keep a little state of their own off
 * `get_status` (the shell's live/busy classes, Yappy's pose), so the status a
 * phase answers is derived from its treatment TONE, which the page itself
 * published: no per-phase table lives here either.
 */
/* yap-1107-parity-kit — the body lives in scripts/headless-chrome.mjs
   (pillFixtureScript), shared with scripts/parity-sheet.mjs. */
const fixtureScript = (tones) => pillFixtureScript(tones);

/* fix-b8-1 — the meeting status event name, READ from its one declaration
   (desktop/src/meetings/consent.ts MEETING_EVENT), never re-typed here. */
function meetingEventFromConsent() {
  const src = readFileSync(join(DESKTOP, "src", "meetings", "consent.ts"), "utf8");
  const m = /export const MEETING_EVENT = "([^"]+)"/.exec(src);
  if (!m) {
    console.error("pill-matrix: MEETING_EVENT not found in desktop/src/meetings/consent.ts (INSTRUMENT failure)");
    process.exit(3);
  }
  return m[1];
}
/*
 * fix-b8-1 — runs AFTER TAURI_SHIM: remembers every listener the page registers
 * for the meeting status event, and exposes __YAP_MATRIX_STOP_MEETING__(),
 * which delivers one "stopped" status to them exactly as the backend would
 * (Tauri v2 calls the transformCallback'd handler with { event, id, payload }).
 * Returns how many listeners heard it, so a stop nobody heard is a finding.
 */
function meetingStopShim(eventName) {
  return String.raw`
(() => {
  const T = window.__TAURI_INTERNALS__;
  if (!T) return;
  const NAME = ${JSON.stringify(eventName)};
  const inner = T.invoke;
  const handlers = [];
  T.invoke = (cmd, payload) => {
    if (cmd === "plugin:event|listen" && payload && payload.event === NAME) handlers.push(payload.handler);
    return inner(cmd, payload);
  };
  window.__YAP_MATRIX_STOP_MEETING__ = () => {
    const stopped = { recording: false, id: null, title: null, elapsedSeconds: 0, elapsedLabel: "00:00:00", captureAvailable: true };
    let n = 0;
    for (const h of handlers) {
      const f = window["_" + h];
      if (typeof f === "function") { f({ event: NAME, id: 0, payload: stopped }); n++; }
    }
    return n;
  };
})();
`;
}
const MEETING_STOP_SHIM = meetingStopShim(meetingEventFromConsent());

/*
 * yap-1107-pill-writing-notes-always-clears — the notes scene's shim. Answers
 * `list_notes_jobs` from window.__YAP_MATRIX_NOTES_JOBS__ (the "store"; [] by
 * default, exactly what an idle backend answers), remembers every
 * `meeting:progress` listener and exposes __YAP_MATRIX_PROGRESS__(payload).
 * With ?notesScene=1 it shortens the pill's notes timers (90 s stall, 5 s /
 * 30 s re-check, 10 s result hold) so one capture walk sees every transition.
 */
const NOTES_SCENE_SHIM = String.raw`
(() => {
  const T = window.__TAURI_INTERNALS__;
  if (!T) return;
  window.__YAP_MATRIX_NOTES_JOBS__ = [];
  const inner = T.invoke;
  const handlers = [];
  T.invoke = (cmd, payload) => {
    if (cmd === "list_notes_jobs") return Promise.resolve(window.__YAP_MATRIX_NOTES_JOBS__);
    if (cmd === "plugin:event|listen" && payload && payload.event === "meeting:progress") handlers.push(payload.handler);
    return inner(cmd, payload);
  };
  window.__YAP_MATRIX_PROGRESS__ = (p) => {
    let n = 0;
    for (const h of handlers) {
      const f = window["_" + h];
      if (typeof f === "function") { f({ event: "meeting:progress", id: 0, payload: p }); n++; }
    }
    return n;
  };
  if (new URLSearchParams(location.search).get("notesScene") === "1") {
    const SHORT = { 90000: 1200, 5000: 300, 30000: 800, 10000: 1000 };
    const st = window.setTimeout.bind(window), si = window.setInterval.bind(window);
    window.setTimeout = (f, ms, ...a) => st(f, SHORT[ms] ?? ms, ...a);
    window.setInterval = (f, ms, ...a) => si(f, SHORT[ms] ?? ms, ...a);
  }
})();
`;

/* ── the detectors, evaluated inside the page ───────────────────────────── */
/*
 * One function, serialized into the page, used by BOTH the matrix and the
 * self-test, so the self-test proves THIS code is non-vacuous.
 *
 * The float window is html/body/#root at overflow:hidden — that is the WINDOW
 * EDGE, not a container. So unlike the smoke's out-of-bounds check, those three
 * never excuse a box: anything past them is cut off on screen.
 */
/* fix-b1-2 — the card list is READ from the one list the shell measures
   (pill/state.ts CARD_CLASSES), so the matrix can never judge a different set
   of cards than float-main lays out. */
function cardSelectorFromState() {
  const src = readFileSync(join(DESKTOP, "src", "pill", "state.ts"), "utf8");
  const m = /export const CARD_CLASSES = \[([^\]]*)\]/.exec(src);
  const classes = m ? [...m[1].matchAll(/"([a-z0-9-]+)"/g)].map((x) => x[1]) : [];
  if (!classes.length) {
    console.error("pill-matrix: CARD_CLASSES not found in desktop/src/pill/state.ts (INSTRUMENT failure)");
    process.exit(3);
  }
  return classes.map((c) => "." + c).join(", ");
}
const CARD_SELECTOR = cardSelectorFromState();

const DETECTORS = String.raw`
function __pillDetect(want) {
  const out = [];
  const tol = 1;
  const W = window.innerWidth, H = window.innerHeight;
  const de = document.documentElement;
  const WINDOW = new Set([de, document.body, document.getElementById("root")]);

  if (want) {
    // yap-1106-main-character-switch — ONE pill surface: every character sits in
    // the one-pill stage (.kami-stage); the friend is painted inside it.
    const capsule = document.querySelector(want.pill === "classic" ? ".pill" : ".kami-stage");
    const bad = [];
    const wantPhase = want.phaseId || want.id;
    if (de.dataset.phase !== wantPhase) bad.push("html[data-phase]=" + de.dataset.phase + " (asked " + wantPhase + ")");
    // yap24-PILL1 — a license scene must actually be on the page.
    if (want.license && de.dataset.license !== want.license) bad.push("html[data-license]=" + de.dataset.license + " (asked " + want.license + ")");
    // WF-12 — an offer scene (lang-offer) must actually draw its card.
    if (want.offer && !document.querySelector('[data-language-offer="' + want.offer + '"]')) bad.push("no [data-language-offer=" + want.offer + "] card");
    if (want.chip && !document.querySelector('[data-style-chip="' + want.chip + '"]')) bad.push("no [data-style-chip=" + want.chip + "] chip");
    // WF-02 — a drag scene must actually be in the blob, with its drop zones up.
    if (want.drag && de.dataset.pillDrag !== want.drag) bad.push("html[data-pill-drag]=" + de.dataset.pillDrag + " (asked " + want.drag + ")");
    if (want.drag && !de.dataset.pillDragTarget) bad.push("no html[data-pill-drag-target] (drop zones not lit)");
    // yap-1106-pill-sky — the pinned time of day must be the sky on screen.
    if (want.sky && de.dataset.sky !== want.sky) bad.push("html[data-sky]=" + de.dataset.sky + " (asked " + want.sky + ")");
    if (de.dataset.dock !== want.dock) bad.push("html[data-dock]=" + de.dataset.dock + " (asked " + want.dock + ")");
    // yap-1106-friend-visits — the friend scene must paint Tidy, crisp, unless a
    // meeting records (then the quiet rule hides every visitor).
    if (want.id === "friend-visit") {
      const fv = document.querySelector('[data-friend-visit="tidy"]');
      // fix-b8-1 — the "Saved" stop beat is as quiet as the recording.
      if (want.meeting === "recording" || want.meeting === "saved") { if (fv) bad.push("friend visit drawn during a meeting"); }
      else if (!fv) bad.push("no [data-friend-visit=tidy] visitor");
      else { const cv = fv.querySelector("canvas"); if (!cv || !cv.width || !cv.height) bad.push("friend visit canvas not painted"); }
    }
    // yap-1106-stuck-pill-keeps-its-scene — a stuck/restarting engine keeps the
    // FULL scene: the health applied, its plain-words label shown, the stage
    // marked full, a non-empty painted canvas, and Yappy's box inside it.
    if (want.engine) {
      if (de.dataset.engineHealth !== want.engine) bad.push("html[data-engine-health]=" + de.dataset.engineHealth + " (asked " + want.engine + ")");
      const chip = document.querySelector('.engine-health[data-state="' + want.engine + '"]');
      if (!chip || !chip.textContent.trim()) bad.push("no engine-health label for " + want.engine);
      const stage = document.querySelector(".kami-stage");
      if (!stage || stage.dataset.scene !== "full") bad.push("scene=" + (stage ? stage.dataset.scene : "none"));
      const cv = document.querySelector(".kami-canvas");
      const r = cv ? cv.getBoundingClientRect() : null;
      if (!cv || !cv.width || !cv.height || !r || r.width < 1 || r.height < 1) bad.push("empty scene rect");
      else {
        const px = cv.getContext("2d").getImageData(0, 0, cv.width, cv.height).data;
        let opaque = 0;
        for (let i = 3; i < px.length; i += 4 * 7) if (px[i] > 0) opaque++;
        if (opaque < 16) bad.push("scene canvas is blank");
        const L = window.__YAP_PILL_LAYOUT__;
        const c = L && L.char;
        if (c && !(c.x >= r.left - 2 && c.y >= r.top - 2 && c.x + c.w <= r.right + 2 && c.y + c.h <= r.bottom + 2)) bad.push("character box outside the scene rect");
      }
    }
    if (!capsule) bad.push("no " + want.pill + " capsule rendered");
    else if (capsule.dataset.phase !== wantPhase) bad.push("capsule data-phase=" + capsule.dataset.phase);
    // yap-1106-main-character-switch — the stage must paint the character asked for.
    if (capsule && want.pill !== "classic" && capsule.dataset.occupant !== want.pill) bad.push("stage data-occupant=" + capsule.dataset.occupant + " (asked " + want.pill + ")");
    if (want.pill !== "classic" && document.querySelector(".pill")) bad.push("a second (legacy .pill) surface is on screen");
    // yap-1106-pill-notes-and-meeting / WF-03 — the rail scene must draw an OPEN
    // rail on the classic capsule (Yappy's legacy stage owns its whole window),
    // rotated with the dock: a row at the bottom, a column at a side dock while
    // no meeting banner holds the bar.
    if (want.id === "rail" && want.pill === "classic") {
      if (!document.querySelector('.slots[data-rail="open"]')) bad.push("rail scene drew no open rail");
      const meetingOn = want.meeting === "recording" || want.meeting === "saved";
      const wantAxis = want.dock === "bottom" || meetingOn ? "row" : "column";
      if (capsule && !meetingOn && capsule.getAttribute("data-bar-axis") !== wantAxis) {
        bad.push("rail axis " + capsule.getAttribute("data-bar-axis") + " at " + want.dock + " (want " + wantAxis + ")");
      }
    }
    if (bad.length) out.push({ id: "phase-not-applied", detail: bad.join("; ") });
  }

  function opacityOf(el) {
    let o = 1;
    for (let p = el; p && p.nodeType === 1; p = p.parentElement) o *= Number(getComputedStyle(p).opacity);
    return o;
  }
  function shown(el) {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden") return false;
    return opacityOf(el) > 0.05;
  }
  function clippedByContainer(el) {
    for (let p = el.parentElement; p && !WINDOW.has(p); p = p.parentElement) {
      const ox = getComputedStyle(p).overflowX;
      if (ox === "hidden" || ox === "clip" || ox === "auto" || ox === "scroll") return p;
    }
    return null;
  }
  const name = (el) => el.tagName.toLowerCase() + (el.className && typeof el.className === "string" ? "." + el.className.trim().split(/\s+/).join(".") : "");
  const box = (r) => "[" + Math.round(r.left) + "," + Math.round(r.top) + " " + Math.round(r.width) + "x" + Math.round(r.height) + "]";

  /* 1 — empty-text: a text slot on screen with nothing in it. */
  const SLOTS = ".phase-strip, .gate-note, .xscribe-count, .kami-progress-count, .kami-blocked span, .kami-bubble, [data-text-slot]";
  const empty = [...document.querySelectorAll(SLOTS)].filter((el) => shown(el) && !(el.innerText || "").trim());
  if (empty.length) out.push({ id: "empty-text", detail: empty.slice(0, 4).map(name).join(", ") + " on screen with no text" });

  /* The visually-hidden pattern (.sr-only): the element or an ancestor is
     clipped to nothing — clip: rect(0 0 0 0), clip-path: inset(50%), or a
     <=1x1 box with overflow hidden. Narrow on purpose: an ordinary text box
     that is merely too small is still a finding. */
  function visuallyHidden(el) {
    for (let p = el; p && p !== document.body; p = p.parentElement) {
      const cs = getComputedStyle(p);
      if (/^rect\(0(px)?,? 0(px)?,? 0(px)?,? 0(px)?\)$/.test(cs.clip)) return true;
      if (cs.clipPath === "inset(50%)") return true;
      const r = p.getBoundingClientRect();
      if (r.width <= 1 && r.height <= 1 && cs.overflow === "hidden") return true;
    }
    return false;
  }

  /* 2 — clipped-text: every visible run of text, measured by its own glyph
     extent (a Range), must fit its overflow box and the window. */
  const clippedTxt = [];
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let t = walker.nextNode(); t; t = walker.nextNode()) {
    if (!t.nodeValue.trim()) continue;
    const el = t.parentElement;
    if (!el || !shown(el)) continue;
    // yap24-PILL2: screen-reader-only text (Y5-J's .sr-only accessible
    // name) is clipped to a 1x1 box ON PURPOSE — it is never drawn, so it
    // can never be "cut off". Without this every phase failed the matrix
    // from the day Y5-J landed.
    if (visuallyHidden(el)) continue;
    const range = document.createRange();
    range.selectNodeContents(t);
    const r = range.getBoundingClientRect();
    if (r.width <= 0) continue;
    const label = JSON.stringify(t.nodeValue.trim().slice(0, 40));
    if (r.left < -tol || r.right > W + tol || r.top < -tol || r.bottom > H + tol) {
      clippedTxt.push(label + " in " + name(el) + " " + box(r) + " leaves the " + W + "x" + H + " window");
      continue;
    }
    if (el.scrollWidth > el.clientWidth + tol && el.clientWidth > 0) {
      const ox = getComputedStyle(el).overflowX;
      if (ox !== "visible") { clippedTxt.push(label + " in " + name(el) + " scrollWidth=" + el.scrollWidth + " > clientWidth=" + el.clientWidth); continue; }
    }
    const c = clippedByContainer(el);
    if (c) {
      const cr = c.getBoundingClientRect();
      if (r.left < cr.left - tol || r.right > cr.right + tol)
        clippedTxt.push(label + " in " + name(el) + " " + box(r) + " cut by " + name(c) + " " + box(cr));
    }
  }
  if (clippedTxt.length) out.push({ id: "clipped-text", detail: clippedTxt.slice(0, 4).join("; ") });

  /* 3 — overflow: a visible box past the window edge (the window clips it). */
  const strays = [];
  for (const el of document.body.querySelectorAll("*")) {
    if (WINDOW.has(el) || !shown(el)) continue;
    const r = el.getBoundingClientRect();
    if (r.left >= -tol && r.right <= W + tol && r.top >= -tol && r.bottom <= H + tol) continue;
    if (clippedByContainer(el)) continue;
    strays.push(name(el) + " " + box(r));
    if (strays.length >= 4) break;
  }
  if (strays.length) out.push({ id: "overflow", detail: "box(es) outside the " + W + "x" + H + " window: " + strays.join("; ") });

  /* 4 — yap-1106-one-pill: the ONE layout, judged against what was painted. */
  const L = window.__YAP_PILL_LAYOUT__;
  if (L && !L.legacy) {
    const MARGIN = 10; // pill/state.ts PANEL_MARGIN
    const REACH = 12;  // pill/state.ts CLOCK_REACH_PT
    const rectBox = (el) => { const r = el.getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; };
    const overlap = (a, b) => a.x < b.x + b.w - 0.5 && b.x < a.x + a.w - 0.5 && a.y < b.y + b.h - 0.5 && b.y < a.y + a.h - 0.5;
    const gap = (a, b) => Math.hypot(Math.max(0, b.x - (a.x + a.w), a.x - (b.x + b.w)), Math.max(0, b.y - (a.y + a.h), a.y - (b.y + b.h)));
    const inside = (a, b) => a.x >= b.x - 1 && a.y >= b.y - 1 && a.x + a.w <= b.x + b.w + 1 && a.y + a.h <= b.y + b.h + 1;
    const painted = [["character", L.char, L.char]];
    const slot = (n, sel, want) => { const el = document.querySelector(sel); if (el && shown(el)) painted.push([n, rectBox(el), want]); };
    slot("hint", ".phase-strip", L.hint);
    slot("bubble", ".kami-bubble", L.bubble);
    slot("clock", "[data-pill-slot=clock]", L.clock);
    slot("progress", ".kami-progress", L.progress);
    /* fix-b1-2 — EVERY card on screen, not the first: two can be up at once. */
    const cardsOut = [];
    for (const el of document.querySelectorAll(${JSON.stringify(CARD_SELECTOR)})) {
      if (!shown(el)) continue;
      const b = rectBox(el);
      painted.push(["card " + name(el), b, L.card]);
      if (b.x < -tol || b.y < -tol || b.x + b.w > W + tol || b.y + b.h > H + tol)
        cardsOut.push(name(el) + " " + box({ left: b.x, top: b.y, width: b.w, height: b.h }) + " past the " + W + "x" + H + " panel");
    }
    if (cardsOut.length) out.push({ id: "card-outside-panel", detail: cardsOut.join("; ") });
    const ov = [];
    for (let i = 0; i < painted.length; i++) for (let j = i + 1; j < painted.length; j++)
      // yap-1107-one-pill-during-meeting — at rest the meeting scene IS the character box.
      if (!(L.sceneCharacter && painted[i][0] === "character" && painted[j][0] === "clock") && overlap(painted[i][1], painted[j][1])) ov.push(painted[i][0] + "/" + painted[j][0] + " " + box({ left: painted[j][1].x, top: painted[j][1].y, width: painted[j][1].w, height: painted[j][1].h }));
    if (ov.length) out.push({ id: "one-pill-overlap", detail: ov.join("; ") });
    const off = painted.filter(([n, b, want]) => n !== "character" && (!want || !inside(b, want))).map(([n, b]) => n + " " + box({ left: b.x, top: b.y, width: b.w, height: b.h }));
    if (off.length) out.push({ id: "slot-off-layout", detail: off.join("; ") });
    const clock = painted.find((p) => p[0] === "clock");
    // WF-02 — a dragged pill is the blob, Yappy's face and nothing else: the clock steps aside by design.
    if (want && want.meeting === "recording" && !clock && want.drag !== "blob") out.push({ id: "clock-detached", detail: "a meeting is recording but no clock slot is on screen" });
    // fix-b8-1 — the stop beat must be PAINTED in its own reserved slot, not clipped to 0x0.
    if (want && want.meeting === "saved" && want.drag !== "blob") {
      const beat = document.querySelector('[data-meeting-phase="saved"]');
      const r = beat ? beat.getBoundingClientRect() : null;
      if (!want.stopped) out.push({ id: "phase-not-applied", detail: "meeting-saved: the page registered no meeting listener to stop" });
      else if (!r || r.width < 1 || r.height < 1) out.push({ id: "saved-beat-missing", detail: "the meeting stopped but no Saved beat is painted" + (r ? " (" + r.width.toFixed(1) + "x" + r.height.toFixed(1) + ")" : "") });
      else if (!clock) out.push({ id: "clock-detached", detail: "the Saved beat is on screen but the layout reserved no clock slot" });
      if (de.dataset.meetingPhase !== "saved") out.push({ id: "phase-not-applied", detail: "html[data-meeting-phase]=" + de.dataset.meetingPhase + " (asked saved)" });
    }
    if (clock && gap(clock[1], L.char) > REACH) out.push({ id: "clock-detached", detail: "clock " + gap(clock[1], L.char).toFixed(1) + "pt from the character" });
    /* yap-1107-one-pill-during-meeting — ONE surface during a meeting: the clock
     * touches the character (a segment of the shell, never a capsule a gap away)
     * and the one .meeting-shell is painted exactly over the layout's shell. */
    if (clock && !(want && want.drag === "blob") && gap(clock[1], L.char) > 0.5) out.push({ id: "meeting-second-surface", detail: "clock " + gap(clock[1], L.char).toFixed(1) + "pt off the character: a second surface" });
    if (L.shell && !(want && want.drag === "blob")) {
      const shellEl = document.querySelector(".meeting-shell");
      if (!shellEl || !shown(shellEl)) out.push({ id: "meeting-second-surface", detail: "a meeting is on screen but its one shell is not painted" });
      else if (!inside(rectBox(shellEl), L.shell) || !inside(L.shell, rectBox(shellEl))) out.push({ id: "meeting-second-surface", detail: "shell painted at " + JSON.stringify(rectBox(shellEl)) + ", layout " + JSON.stringify(L.shell) });
    }
    const boxes = [L.char, L.bubble, L.hint, L.clock, L.progress, L.card].filter(Boolean);
    const ux = Math.min(...boxes.map((b) => b.x)), uy = Math.min(...boxes.map((b) => b.y));
    const uw = Math.max(...boxes.map((b) => b.x + b.w)) - ux, uh = Math.max(...boxes.map((b) => b.y + b.h)) - uy;
    /* yap-1106-pill-vertical-dock-world — a side dock is FLUSH to its screen
       edge: the edge side carries no margin, the inner side MARGIN (state.ts sideLayout). */
    const flush = want && (want.dock === "left" || want.dock === "right");
    const wantX = flush && want.dock === "left" ? 0 : MARGIN;
    const wantW = Math.ceil(uw + (flush ? 1 : 2) * MARGIN), wantH = Math.ceil(uh + 2 * MARGIN);
    if (Math.abs(ux - wantX) > 0.5 || Math.abs(uy - MARGIN) > 0.5 || L.panel.w !== wantW || L.panel.h !== wantH || Math.abs(W - L.panel.w) > tol || Math.abs(H - L.panel.h) > tol)
      out.push({ id: "panel-not-union", detail: "window " + W + "x" + H + ", layout panel " + L.panel.w + "x" + L.panel.h + ", union+margin " + wantW + "x" + wantH });
  }

  const strip = document.querySelector(".phase-strip");
  return { findings: out, line: strip && shown(strip) ? (strip.innerText || "").trim() : "" };
}
`;

/* ── build + serve the matrix bundle ────────────────────────────────────── */

/* yap-1107-parity-kit — freePort / build / preview moved to
   scripts/headless-chrome.mjs (one copy, shared with parity-sheet.mjs). */
const buildBundle = (work) => buildMatrixBundle(DESKTOP, work, "pill-matrix");
const servePreview = (dist, work) => servePreviewShared(DESKTOP, dist, work, "pill-matrix");

/* ── one page, navigated once per capture ───────────────────────────────── */

async function openPage(cdp, tones) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Runtime.enable", {}, s);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
  // A flat dark desktop behind the transparent window, so white copy reads.
  await cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 46, g: 49, b: 56, a: 1 } }, s);
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: fixtureScript(tones) + "\n" + TAURI_SHIM + "\n" + MEETING_STOP_SHIM + "\n" + NOTES_SCENE_SHIM }, s);
  return s;
}

/* yap-1106-one-pill — size the window to the layout's panel, the way
 * float_pill.rs set_panel_frame sizes the NSPanel, then let it re-lay out. */
async function fitPanel(cdp, s) {
  return fitPillPanel(cdp, s, { w: W, h: H }, SCALE, SETTLE_MS);
}

async function settle(cdp, s, wantId) {
  return settlePhase(cdp, s, wantId, SETTLE_MS);
}

async function shoot(cdp, s, file) {
  const shot = await cdp.send("Page.captureScreenshot", { format: "png" }, s);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
}

/*
 * yap-1107-pill-writing-notes-always-clears — the notes scene: every pill
 * transition of a meeting's notes, at every dock, @SCALE (2x): recording ->
 * saved -> transcribing -> Writing notes -> Still writing -> notes ready (the
 * terminal EVENT is never sent: the store answers it, on window focus) -> rest,
 * and -> failed -> rest. Each step checks the words on the pill and runs the
 * structural detectors; the PNGs + contact sheet are the evidence.
 */
async function meetingNotesScene(cdp, base, phases, outDir) {
  const idle = phases.find((p) => p.phase === "idle" && !p.license && !p.query);
  if (!idle) { console.error("pill-matrix: --phase meeting-notes found no plain idle row (INSTRUMENT failure)"); return 3; }
  MEETINGS = ["notes"];
  const tones = Object.fromEntries(phases.flatMap((p) => [[p.phase, p.tone], [p.id, p.tone]]));
  const s = await openPage(cdp, tones);
  const ID = "matrix-meeting";
  const ev = (state, over = {}) => JSON.stringify({ id: ID, state, percent: 0, processedThroughSeconds: 0, durationSeconds: 217, error: null, ...over });
  const store = (rows) => evaluate(cdp, s, "window.__YAP_MATRIX_NOTES_JOBS__ = " + JSON.stringify(rows) + ";true");
  const emit = (state, over) => evaluate(cdp, s, "window.__YAP_MATRIX_PROGRESS__ ? window.__YAP_MATRIX_PROGRESS__(" + ev(state, over) + ") : 0");
  const job = (phase, reason = null) => [{ id: ID, phase, percent: phase === "notes_ready" ? 100 : 0, reason }];
  const FAIL = "the transcript changed while the notes were being written. Press Retry notes to write them again.";
  // The pill shows short words (the after slot is CLOCK_W.after = 184 px of a
  // 300 px window); the full sentences are the tooltip / accessible name, and
  // the text read below includes [title] and [aria-label], so both are checked.
  const LABELS = ["Writing notes", "Still writing", "Notes ready", "Notes failed", FAIL];
  const visible = (sel) => evaluate(cdp, s, "(() => { const el = document.querySelector('.meeting-after-label'); return el ? el.innerText : ''; })()").then((t) => String(t).includes(sel));
  const captures = [];
  for (const dock of DOCKS) {
    const shot = async (step, want, gone = []) => {
      const size = await fitPanel(cdp, s);
      await evaluate(cdp, s, DETECTORS + ";true");
      const res = await evaluate(cdp, s, "__pillDetect(" + JSON.stringify({ id: idle.id, phaseId: idle.phaseId, pill: "yappy", dock, meeting: "notes", sky: "day", stopped: 0 }) + ")");
      const text = String(await evaluate(cdp, s, "document.body.innerText + ' ' + Array.from(document.querySelectorAll('[title],[aria-label]')).map((e) => (e.getAttribute('title') || '') + ' ' + (e.getAttribute('aria-label') || '')).join(' ')"));
      const findings = res.findings.slice();
      for (const w of want) if (!text.includes(w)) findings.push({ id: "notes-label-missing", detail: step + ": the pill does not say \"" + w + "\"" });
      for (const g of gone) if (text.includes(g)) findings.push({ id: "notes-label-stuck", detail: step + ": the pill still says \"" + g + "\"" });
      const file = join(outDir, "notes-" + step + "__yappy__" + dock + "__notes.png");
      await shoot(cdp, s, file);
      captures.push({ id: "notes-" + step, phase: "idle", tone: idle.tone, character: "yappy", dock, meeting: "notes", sky: "day", w: size.w, h: size.h, line: res.line, findings, pageErrors: (cdp.errors.get(s) || []).slice(0, 4), shot: file });
      console.log((findings.length ? "FAIL " : "ok   ") + ("notes-" + step).padEnd(22) + dock.padEnd(7) + file);
      for (const f of findings) console.log("       " + f.id + " — " + f.detail);
    };
    const load = async (meeting) => {
      cdp.errors.set(s, []);
      await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
      await cdp.send("Page.navigate", { url: base + "/float.html?phase=" + encodeURIComponent(idle.phase) + "&pill=yappy&dock=" + dock + "&meeting=" + meeting + "&sky=day&notesScene=1" }, s);
      await settle(cdp, s, idle.phaseId || idle.id);
    };
    // The success road.
    await load("recording");
    await shot("1-recording", []);
    await evaluate(cdp, s, "window.__YAP_MATRIX_STOP_MEETING__ ? window.__YAP_MATRIX_STOP_MEETING__() : 0");
    await sleep(SAVED_EMIT_SETTLE_MS);
    await shot("2-saved", []);
    await sleep(3000); // the 2.4 s Saved beat lets go
    await emit("transcribing", { percent: 40 });
    await sleep(400);
    await shot("3-transcribing", [], LABELS);
    await store(job("summarizing"));
    await emit("complete", { percent: 100 });
    await emit("notes_queued");
    await emit("summarizing", { percent: 100 });
    await sleep(500);
    await shot("4-writing-notes", ["Writing notes"]);
    await sleep(1500); // the (shortened) 90 s stall timer fires
    await shot("5-still-writing", ["Still writing\u2026", "Still writing... tap to open notes"]);
    if (!(await visible("Still writing\u2026"))) captures[captures.length - 1].findings.push({ id: "notes-label-missing", detail: "5-still-writing: the visible pill label is not \"Still writing\u2026\"" });
    // The terminal event is LOST: only the store says done; a focus re-reads it.
    await store(job("notes_ready"));
    await evaluate(cdp, s, "window.dispatchEvent(new Event('focus'));true");
    await sleep(400);
    await shot("6-notes-ready", ["Notes ready"], ["Writing notes", "Still writing"]);
    await sleep(1400); // the (shortened) 10 s result hold
    await shot("7-rest", [], LABELS);
    // The failure road.
    await load("none");
    await store(job("summarizing"));
    await emit("notes_queued");
    await emit("summarizing", { percent: 100 });
    await sleep(500);
    await shot("8-writing-notes", ["Writing notes"]);
    await store(job("notes_failed", FAIL));
    await emit("notes_failed", { error: FAIL });
    await sleep(400);
    await shot("9-failed", ["Notes failed \u2014 Open", FAIL], ["Writing notes"]);
    if (!(await visible("Notes failed \u2014 Open"))) captures[captures.length - 1].findings.push({ id: "notes-label-missing", detail: "9-failed: the visible pill label is not \"Notes failed \u2014 Open\"" });
    await sleep(1400);
    await shot("10-rest-after-failed", [], LABELS);
  }
  const sheet = await contactSheet(cdp, captures, outDir);
  const failed = captures.filter((c) => c.findings.length);
  writeFileSync(join(outDir, "result.json"), JSON.stringify({ window: W + "x" + H + "@" + SCALE + "x", scene: "meeting-notes", captures: captures.length, failing: failed.length, contactSheet: sheet, captures_: captures }, null, 2));
  console.log("pill-matrix: meeting-notes " + captures.length + " captures, " + failed.length + " carrying findings -> " + outDir);
  return failed.length ? 1 : 0;
}

/* ── contact sheet: every capture on one image ──────────────────────────── */

async function contactSheet(cdp, rows, outDir) {
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId: s } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  const cols = DOCKS.length * CHARACTERS.length * MEETINGS.length;
  const CW = Math.max(W, ...rows.map((r) => r.w || W)), CH = Math.max(H, ...rows.map((r) => r.h || H));
  const width = 24 + 150 + cols * (CW + 12);
  const height = 60 + Math.ceil(rows.length / cols) * (CH + 30);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, s);
  const head = ["", ...CHARACTERS.flatMap((c) => DOCKS.flatMap((d) => MEETINGS.map((m) => c + " · " + d + (m === "none" ? "" : m === "saved" ? " · saved" : " · meeting"))))];
  const byPhase = new Map();
  // yap-1106-pill-sky — under --sky all, one row per phase x time of day.
  const skyRows = new Set(rows.map((r) => r.sky)).size > 1;
  const rowKey = (r) => r.id + (skyRows && r.sky ? " · " + r.sky : "");
  for (const r of rows) (byPhase.get(rowKey(r)) || byPhase.set(rowKey(r), []).get(rowKey(r))).push(r);
  let html = '<html><body style="margin:12px;background:#1d1f24;color:#e6e6e6;font:12px -apple-system,system-ui,sans-serif">' +
    '<table style="border-collapse:separate;border-spacing:6px"><tr>' + head.map((h) => "<th>" + h + "</th>").join("") + "</tr>";
  for (const [id, list] of byPhase) {
    html += '<tr><td style="width:150px;vertical-align:middle"><b>' + id + "</b></td>";
    for (const c of CHARACTERS) for (const d of DOCKS) for (const m of MEETINGS) {
      const r = list.find((x) => x.character === c && x.dock === d && x.meeting === m);
      const img = r ? "data:image/png;base64," + readFileSync(r.shot).toString("base64") : "";
      const ring = r && r.findings.length ? "outline:2px solid #ef4444" : "";
      html += '<td style="vertical-align:top"><img style="width:' + (r ? r.w : W) + "px;height:" + (r ? r.h : H) + "px;" + ring + '" src="' + img + '"></td>';
    }
    html += "</tr>";
  }
  html += "</table></body></html>";
  await cdp.send("Page.enable", {}, s);
  await cdp.send("Page.navigate", { url: "about:blank" }, s);
  await evaluate(cdp, s, "document.open();document.write(" + JSON.stringify(html) + ");document.close();true");
  await sleep(300);
  // Trim the sheet to the table it actually drew.
  const tall = await evaluate(cdp, s, "Math.ceil(document.body.getBoundingClientRect().bottom) + 12");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: tall, deviceScaleFactor: 1, mobile: false }, s);
  const file = join(outDir, "contact-sheet.png");
  await shoot(cdp, s, file);
  return file;
}

/* ── main ───────────────────────────────────────────────────────────────── */

async function run() {
  const work = mkdtempSync(join(tmpdir(), "yap-pill-matrix-"));
  const outDir = resolve(ARGS.out || mkdtempSync(join(tmpdir(), "yap-pill-shots-")));
  /* Y7-B — the one in-repo exception is docs/smoke-shots/, which .gitignore
     excludes: smoke-windowed.sh files the pill matrix beside the view captures
     of the same run, and none of it can reach a commit. */
  const SMOKE_SHOTS = join(ROOT, "docs", "smoke-shots") + "/";
  if ((outDir === ROOT || outDir.startsWith(ROOT + "/")) && !outDir.startsWith(SMOKE_SHOTS)) {
    console.error("pill-matrix: --out " + outDir + " is inside the repo. The PNGs are PR evidence, never a commit — use a scratch dir.");
    return 2;
  }
  mkdirSync(outDir, { recursive: true });
  const dist = buildBundle(work);
  const { proc: preview, base } = await servePreview(dist, work);
  let chrome;
  try {
    chrome = await openChrome(CHROME, "yap-pill-matrix");
  } catch (e) {
    preview.kill("SIGKILL");
    console.error("pill-matrix: " + e.message);
    return 3;
  }
  const { proc, cdp, ws, profile } = chrome;
  const captures = [];
  try {
    /* Discovery: the phase list comes off the page, from the pill's own table. */
    let s = await openPage(cdp, {});
    await cdp.send("Page.navigate", { url: base + "/float.html" }, s);
    await settle(cdp, s, null);
    const phases = await evaluate(cdp, s, "window.__YAP_PILL_PHASES__ || null");
    if (!Array.isArray(phases) || phases.length === 0) {
      console.error("pill-matrix: the bundle published no __YAP_PILL_PHASES__ — it was not built in matrix mode (INSTRUMENT failure)");
      return 3;
    }
    await cdp.send("Page.close", {}, s).catch(() => {});

    if (ARGS.selfTest) return await selfTest(cdp, base, phases);
    if (ARGS.clicks) return await clickMatrix(cdp, base, phases, outDir);
    if (ARGS.phase === "meeting-notes") return await meetingNotesScene(cdp, base, phases, outDir);

    let todo = phases;
    if (ARGS.phase === "meeting-saved") {
      // fix-b8-1 — the stop beat at rest and stopped mid-take, every dock.
      todo = phases.filter((p) => (p.phase === "idle" || p.phase === "listening") && !p.license && !p.query);
      MEETINGS = ["saved"];
      if (!todo.length) {
        console.error("pill-matrix: --phase meeting-saved found no plain idle/listening row (INSTRUMENT failure)");
        return 3;
      }
    }
    if (ARGS.only) {
      // A license scene (yap24-PILL1) rides the idle phase; `--only idle` means the plain idle row.
      todo = phases.filter((p) => p.id === ARGS.only || (p.phase === ARGS.only && !p.license && !p.query));
      if (!todo.length) {
        console.error("pill-matrix: --only " + ARGS.only + " is not a phase. Known: " + phases.map((p) => p.id).join(", "));
        return 2;
      }
    }
    const tones = Object.fromEntries(phases.flatMap((p) => [[p.phase, p.tone], [p.id, p.tone]]));
    s = await openPage(cdp, tones);
    console.log("pill-matrix: " + todo.length + " phase(s) x " + DOCKS.length + " docks x " + CHARACTERS.length + " characters x " + MEETINGS.length + " meeting states, legacy " + W + "x" + H + " / one-pill sized to its layout, @" + SCALE + "x -> " + outDir);
    const skies = ARGS.sky === "all" ? SKY_TIMES : [ARGS.sky];
    for (const sky of skies) for (const p of todo) {
      for (const character of CHARACTERS) {
        for (const dock of DOCKS) {
          for (const meeting of MEETINGS) {
            const q = "?phase=" + encodeURIComponent(p.phase) + "&pill=" + character + "&dock=" + dock + "&meeting=" + meeting + "&sky=" + sky + (p.license ? "&license=" + encodeURIComponent(p.license) : "") + (p.query ? "&" + p.query : "");
            cdp.errors.set(s, []);
            await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
            await cdp.send("Page.navigate", { url: base + "/float.html" + q }, s);
            await settle(cdp, s, p.phaseId || p.id);
            // fix-b8-1 — stop the meeting the page is recording, then capture inside the beat.
            let stopped = 0;
            if (meeting === "saved") {
              stopped = Number(await evaluate(cdp, s, "window.__YAP_MATRIX_STOP_MEETING__ ? window.__YAP_MATRIX_STOP_MEETING__() : 0")) || 0;
              await sleep(SAVED_EMIT_SETTLE_MS);
            }
            const size = await fitPanel(cdp, s);
            await evaluate(cdp, s, DETECTORS + ";true");
            const res = await evaluate(cdp, s, "__pillDetect(" + JSON.stringify({ id: p.id, phaseId: p.phaseId, license: p.license, offer: p.offer, chip: p.chip, drag: p.drag, engine: p.engine, pill: character, dock, meeting, sky, stopped }) + ")");
            const file = join(outDir, p.id + "__" + character + "__" + dock + "__" + meeting + (skies.length > 1 ? "__" + sky : "") + ".png");
            await shoot(cdp, s, file);
            const pageErrors = (cdp.errors.get(s) || []).slice(0, 4);
            captures.push({ id: p.id, phase: p.phase, tone: p.tone, character, dock, meeting, sky, w: size.w, h: size.h, line: res.line, findings: res.findings, pageErrors, shot: file });
            const tag = res.findings.length ? "FAIL" : "ok  ";
            console.log(tag + " " + p.id.padEnd(14) + character.padEnd(8) + dock.padEnd(7) + meeting.padEnd(10) + sky.padEnd(6) + (size.w + "x" + size.h).padEnd(8) + JSON.stringify(res.line).padEnd(34) + file);
            for (const f of res.findings) console.log("       " + f.id + " — " + f.detail);
            for (const e of pageErrors) console.log("       page: " + e.split("\n")[0]);
          }
        }
      }
    }
    const sheet = await contactSheet(cdp, captures, outDir);
    const failed = captures.filter((c) => c.findings.length);
    writeFileSync(join(outDir, "result.json"), JSON.stringify({ window: W + "x" + H + "@" + SCALE + "x", only: ARGS.only, skies, phases: todo.map((p) => p.id), captures: captures.length, failing: failed.length, contactSheet: sheet, captures_: captures }, null, 2));
    console.log("");
    console.log("pill-matrix: " + captures.length + " captures, " + failed.length + " carrying findings");
    console.log("pill-matrix: PNGs + result.json + contact-sheet.png in " + outDir + " (PR evidence — never commit them)");
    return failed.length ? 1 : 0;
  } finally {
    try { ws.close(); } catch {}
    proc.kill("SIGKILL");
    preview.kill("SIGKILL");
    await sleep(200);
    rmSync(profile, { recursive: true, force: true });
    rmSync(work, { recursive: true, force: true });
  }
}

/* ── --clicks (yap-1106-fb-02-pill-first-click) ─────────────────────────── */
/*
 * Wilson 15:29Z: "the first time i click it the second time doesn't work the
 * third time it works". For every scene of the ONE pill (Yappy — the Classic
 * pill is retired) x dock x meeting state, every visible `[data-pill-control]`
 * is pressed at its centre and its four inner corners (1 pt in), ONE press per
 * point, through real CDP mouse events. Findings:
 *   control-too-small     the control is drawn under 24 x 24 pt
 *   probe-outside-hitbox  the point is not inside the rects the page published
 *                         for the NSPanel's click-through region
 *   probe-missed          the press produced no click on the control
 *   probe-double          the press produced more than one click
 *   probe-wrong-target    the press clicked something else
 * A control whose click changes the pill (a card's Dismiss) is re-probed on a
 * freshly loaded page for each point, so every point gets its own first press.
 */
const MIN_CONTROL_PT = 24;
const CLICK_LIST = String.raw`(() => {
  const els = [...document.querySelectorAll("[data-pill-control]")].filter((el) => {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && cs.visibility !== "hidden" && cs.display !== "none" && cs.pointerEvents !== "none";
  });
  window.__probeEls = els;
  window.__probeCounts = els.map(() => 0);
  window.__probeStray = 0;
  if (!window.__probeHooked) {
    window.__probeHooked = true;
    document.addEventListener("click", (e) => {
      const hit = e.target instanceof Element ? e.target.closest("[data-pill-control]") : null;
      const i = (window.__probeEls || []).indexOf(hit);
      if (i >= 0) window.__probeCounts[i]++;
      else window.__probeStray++;
    }, true);
  }
  return els.map((el) => {
    const r = el.getBoundingClientRect();
    return { name: el.getAttribute("data-pill-control"), x: r.left, y: r.top, w: r.width, h: r.height };
  });
})()`;

function probePoints(c) {
  const i = Math.min(1, c.w / 2, c.h / 2);
  return [
    ["centre", c.x + c.w / 2, c.y + c.h / 2],
    ["top-left", c.x + i, c.y + i],
    ["top-right", c.x + c.w - i, c.y + i],
    ["bottom-left", c.x + i, c.y + c.h - i],
    ["bottom-right", c.x + c.w - i, c.y + c.h - i],
  ];
}

async function clickMatrix(cdp, base, phases, outDir) {
  const todo = ARGS.only ? phases.filter((p) => p.id === ARGS.only || p.phase === ARGS.only) : phases;
  if (!todo.length) {
    console.error("pill-matrix --clicks: --only " + ARGS.only + " is not a phase");
    return 2;
  }
  const tones = Object.fromEntries(phases.flatMap((p) => [[p.phase, p.tone], [p.id, p.tone]]));
  const s = await openPage(cdp, tones);
  const rows = [];
  let probed = 0;
  for (const p of todo) {
    for (const dock of DOCKS) {
      for (const meeting of MEETINGS) {
        const q = "?phase=" + encodeURIComponent(p.phase) + "&pill=yappy&dock=" + dock + "&meeting=" + meeting + (p.license ? "&license=" + encodeURIComponent(p.license) : "") + (p.query ? "&" + p.query : "");
        const fresh = async () => {
          await cdp.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: SCALE, mobile: false }, s);
          await cdp.send("Page.navigate", { url: base + "/float.html" + q }, s);
          await settle(cdp, s, p.phaseId || p.id);
          await fitPanel(cdp, s);
          return await evaluate(cdp, s, CLICK_LIST);
        };
        const controls = await fresh();
        const ordinal = new Map();
        for (const c of controls) {
          const ord = ordinal.get(c.name) || 0;
          ordinal.set(c.name, ord + 1);
          const findings = [];
          if (c.w < MIN_CONTROL_PT - 0.5 || c.h < MIN_CONTROL_PT - 0.5) {
            findings.push({ id: "control-too-small", detail: c.name + " is " + c.w.toFixed(1) + "x" + c.h.toFixed(1) + " pt" });
          }
          for (const [where] of probePoints(c)) {
            // Every point gets the FIRST press on a freshly loaded page.
            const list = await fresh();
            const now = list.filter((x) => x.name === c.name)[ord];
            if (!now) {
              findings.push({ id: "probe-missed", detail: c.name + " " + where + ": control not on the reloaded page" });
              continue;
            }
            const idx = list.indexOf(now);
            const [, x, y] = probePoints(now).find((pt) => pt[0] === where);
            const hb = (await evaluate(cdp, s, "window.__YAP_PILL_HITBOXES__ || []")) || [];
            if (!hb.some((r) => x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])) {
              findings.push({ id: "probe-outside-hitbox", detail: c.name + " " + where + " (" + x.toFixed(1) + "," + y.toFixed(1) + ") is not in the panel's hit rects " + JSON.stringify(hb) });
            }
            // What the browser's own hit test puts under the point (rounded
            // corners, an overlapping slot, an ancestor's clip) — named in the
            // finding, so a miss says WHY instead of only "nothing".
            const under = await evaluate(cdp, s, "(() => { const e = document.elementFromPoint(" + x + "," + y + "); if (!e) return 'nothing'; const c = e.closest('[data-pill-control]'); return c ? 'control ' + c.getAttribute('data-pill-control') : e.tagName.toLowerCase() + (typeof e.className === 'string' && e.className.trim() ? '.' + e.className.trim().split(/\\s+/).join('.') : ''); })()");
            await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none", buttons: 0 }, s);
            await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 }, s);
            await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 }, s);
            await sleep(80);
            const got = await evaluate(cdp, s, "({ c: window.__probeCounts, stray: window.__probeStray })");
            const mine = got.c[idx] || 0;
            const others = got.c.reduce((a, b) => a + b, 0) - mine + (got.stray || 0);
            probed++;
            if (mine === 0) findings.push({ id: "probe-missed", detail: c.name + " " + where + ": the press clicked nothing on the control (the point hit-tests to " + under + ")" });
            if (mine > 1) findings.push({ id: "probe-double", detail: c.name + " " + where + ": " + mine + " clicks from one press" });
            if (others > 0) findings.push({ id: "probe-wrong-target", detail: c.name + " " + where + ": " + others + " click(s) landed elsewhere (on " + under + ")" });
          }
          rows.push({ id: p.id, dock, meeting, control: c.name, w: c.w, h: c.h, findings });
          console.log((findings.length ? "FAIL " : "ok   ") + p.id.padEnd(14) + dock.padEnd(7) + meeting.padEnd(10) + c.name);
          for (const f of findings) console.log("       " + f.id + " — " + f.detail);
        }
      }
    }
  }
  await cdp.send("Page.close", {}, s).catch(() => {});
  const failing = rows.filter((r) => r.findings.length);
  writeFileSync(join(outDir, "clicks.json"), JSON.stringify({ probes: probed, controls: rows.length, failing: failing.length, rows }, null, 2));
  console.log("pill-matrix --clicks: " + rows.length + " control(s), " + probed + " press(es), " + failing.length + " failing -> " + join(outDir, "clicks.json"));
  if (rows.length === 0) {
    console.error("pill-matrix --clicks: no [data-pill-control] on any scene — the bundle predates fb-02 (INSTRUMENT failure)");
    return 3;
  }
  return failing.length ? 1 : 0;
}

/* ── --self-test-must-fail ──────────────────────────────────────────────── */
/*
 * Renders one REAL capture, then breaks it three ways in the page — an empty
 * phase line forced visible, a line of text wider than the window, a box hung
 * off the left edge — and requires every detector to fire. A satisfied
 * self-test exits 1 BY DESIGN (the run carries a deliberate break); a blind
 * detector exits 9.
 */
async function selfTest(cdp, base, phases) {
  const expect = ["empty-text", "clipped-text", "overflow"];
  const p = phases[0];
  const s = await openPage(cdp, { [p.phase]: p.tone });
  await cdp.send("Page.navigate", { url: base + "/float.html?phase=" + p.phase + "&pill=yappy&dock=bottom" }, s);
  await settle(cdp, s, p.id);
  await evaluate(cdp, s, DETECTORS + ";true");
  const before = await evaluate(cdp, s, "__pillDetect(" + JSON.stringify({ id: p.id, pill: "yappy", dock: "bottom" }) + ")");
  await evaluate(cdp, s, String.raw`(() => {
    const strip = document.querySelector(".phase-strip");
    strip.textContent = ""; strip.style.cssText = "opacity:1;transition:none;min-width:40px;min-height:10px;left:20px;top:10px;bottom:auto;transform:none";
    const wide = document.createElement("span");
    wide.textContent = "this sentence is far too long for a three hundred point pill and runs off the edge";
    wide.style.cssText = "position:absolute;left:10px;top:100px;white-space:nowrap;color:#fff;font:13px system-ui";
    document.body.appendChild(wide);
    const hang = document.createElement("div");
    hang.style.cssText = "position:absolute;left:-40px;top:40px;width:80px;height:20px;background:#f00";
    document.body.appendChild(hang);
    return true;
  })()`);
  const res = await evaluate(cdp, s, "__pillDetect(null)");
  const got = res.findings.map((f) => f.id);
  const missed = expect.filter((e) => !got.includes(e));
  console.log("self-test: real capture before the break: " + (before.findings.length ? before.findings.map((f) => f.id).join(", ") : "clean"));
  for (const f of res.findings) console.log("  detector fired: " + f.id + " — " + f.detail);
  if (missed.length) {
    console.error("SELF-TEST VACUOUS — detector stayed blind to: " + missed.join(", "));
    return 9;
  }
  /* yap-1106-one-pill — the one-pill detectors, on a real Yappy capture with a
     meeting: the clock dropped onto Yappy's face and a frame that is not the
     union of the content. */
  const expect2 = ["one-pill-overlap", "slot-off-layout", "panel-not-union"];
  const s2 = await openPage(cdp, { [p.phase]: p.tone });
  await cdp.send("Page.navigate", { url: base + "/float.html?phase=" + p.phase + "&pill=yappy&dock=left&meeting=recording" }, s2);
  await settle(cdp, s2, p.id);
  await fitPanel(cdp, s2);
  await evaluate(cdp, s2, DETECTORS + ";true");
  const clean = await evaluate(cdp, s2, "__pillDetect(null)");
  console.log("self-test: one-pill capture before the break: " + (clean.findings.length ? clean.findings.map((f) => f.id).join(", ") : "clean"));
  await evaluate(cdp, s2, String.raw`(() => {
    const L = window.__YAP_PILL_LAYOUT__;
    if (!L) return false;
    const c = document.querySelector("[data-pill-slot=clock]");
    if (c) { c.style.left = L.char.x + "px"; c.style.top = L.char.y + "px"; }
    window.__YAP_PILL_LAYOUT__ = Object.assign({}, L, { panel: { w: L.panel.w + 40, h: L.panel.h } });
    return true;
  })()`);
  const res2 = await evaluate(cdp, s2, "__pillDetect(null)");
  const got2 = res2.findings.map((f) => f.id);
  for (const f of res2.findings) console.log("  detector fired: " + f.id + " — " + f.detail);
  const missed2 = expect2.filter((e) => !got2.includes(e));
  if (missed2.length) {
    console.error("SELF-TEST VACUOUS — one-pill detector stayed blind to: " + missed2.join(", "));
    return 9;
  }
  /* fix-b1-2 — the card detectors, on a real Yappy lang-offer capture: the
     card dropped onto Yappy's face, and a card hanging past the panel edge. */
  const offerScene = phases.find((x) => x.offer);
  let expect3 = [];
  if (offerScene) {
    expect3 = ["one-pill-overlap", "card-outside-panel"];
    const s3 = await openPage(cdp, { [offerScene.phase]: offerScene.tone, [offerScene.id]: offerScene.tone });
    await cdp.send("Page.navigate", { url: base + "/float.html?phase=" + offerScene.phase + "&pill=yappy&dock=bottom&meeting=none&" + offerScene.query }, s3);
    await settle(cdp, s3, offerScene.phaseId || offerScene.id);
    await fitPanel(cdp, s3);
    await evaluate(cdp, s3, DETECTORS + ";true");
    const clean3 = await evaluate(cdp, s3, "__pillDetect(null)");
    console.log("self-test: lang-offer capture before the break: " + (clean3.findings.length ? clean3.findings.map((f) => f.id).join(", ") : "clean"));
    await evaluate(cdp, s3, String.raw`(() => {
      const L = window.__YAP_PILL_LAYOUT__;
      const card = document.querySelector(".language-offer-wrap");
      if (!L || !card) return false;
      // Onto Yappy's box, and wider than the panel, so it also leaves it.
      card.style.setProperty("top", L.char.y + "px", "important");
      card.style.setProperty("left", L.char.x + "px", "important");
      card.style.setProperty("width", window.innerWidth + "px", "important");
      card.style.setProperty("max-width", window.innerWidth + "px", "important");
      return true;
    })()`);
    const res3 = await evaluate(cdp, s3, "__pillDetect(null)");
    const got3 = res3.findings.map((f) => f.id);
    for (const f of res3.findings) console.log("  detector fired: " + f.id + " — " + f.detail);
    const missed3 = expect3.filter((e) => !got3.includes(e));
    if (missed3.length) {
      console.error("SELF-TEST VACUOUS — card detector stayed blind to: " + missed3.join(", "));
      return 9;
    }
  } else {
    console.error("SELF-TEST VACUOUS — no lang-offer scene in __YAP_PILL_PHASES__ to break a card on");
    return 9;
  }
  console.error("self-test-must-fail: all " + (expect.length + expect2.length + expect3.length) + " detectors fired on the synthetic breaks; exiting 1 BY DESIGN.");
  return 1;
}

process.exit(await run());
