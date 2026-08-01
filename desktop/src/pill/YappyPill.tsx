/**
 * YappyPill — the pixel-chick companion, as an actual PILL.
 * At rest the capsule is a SKY-BLUE pill zoomed onto Yappy's tiny face. When you
 * dictate, a pull-back camera pans out so the pixel WORLD (sky + grass) fills the
 * ENTIRE capsule edge-to-edge — no black box-in-a-box. Longer prompts add props:
 * a paragraph → notepad + pencil in the wing; a few paragraphs → a desk + a pixel
 * typewriter + glasses (receptionist); an essay → glasses, filing the whole story.
 * Driven by the real app events (get_status/status/recording/audio_level/transcript),
 * the shared MouthDriver, and the tone data table (rude/friendly/rose).
 */
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MouthDriver } from "./mouth";
import { reactiveLine, DEFAULT_TONE, bucketFor, type Bucket } from "./tone";

interface AppStatus { recording: boolean; busy: boolean; message: string }
interface Transcript { wordCount: number; text: string }
interface PillSettings { companionTone?: string }
type Phase = "idle" | "listening" | "thinking" | "done" | "sleepy";
type ChatTone = "rude" | "friendly" | "rose";
type Prop = "none" | "pad" | "desk";

// ── tone-aware working chatter (final "done" line comes from reactiveLine) ──
const WORK: Record<ChatTone, Partial<Record<Bucket, string[]>>> = {
  rude: {
    medium: ["ugh, writing…", "noting your rant…"],
    long: ["filing this mess…", "typing, typing…", "you done yet?"],
    epic: ["an ESSAY, huh…", "still typing…", "chapter two??", "my wings hurt…"],
  },
  friendly: {
    medium: ["let me note that…", "jotting it down…"],
    long: ["one moment, filing…", "typing it up…", "receptionist mode!"],
    epic: ["that's an essay…", "formulating…", "chapter two…", "stamp, file, next!"],
  },
  rose: {
    medium: ["noting every word…", "so worth writing 🌹"],
    long: ["typing it up 🌹", "filing carefully…", "almost, love…"],
    epic: ["your whole story 🌹", "typing it all…", "every word matters…"],
  },
};
// live chatter tone tracks the user's configured companion tone (rude/friendly/
// rose, YV27); anything unknown falls back to friendly. The tone is read LIVE off
// the settings the float already subscribes to — never a hardcoded constant.
const toChatTone = (t?: string): ChatTone => (t === "rude" || t === "rose" ? t : "friendly");
const chatty = (tone: ChatTone, w: number) => { const o = WORK[tone][bucketFor(w)]; return !!(o && o.length); };
const workLine = (tone: ChatTone, w: number, n: number) => { const o = WORK[tone][bucketFor(w)]; return o && o.length ? o[n % o.length] : ""; };
// prop/persona keyed off the transcript word count
const propFor = (w: number): Prop => { const b = bucketFor(w); return b === "medium" ? "pad" : (b === "long" || b === "epic") ? "desk" : "none"; };

// ── LIVE escalation WHILE listening ── the real word count is unknown until the
// transcript arrives, so estimate it from how long you've ACTUALLY been speaking:
// only voiced audio advances the tier — sitting in silence must NOT move it along.
type LiveTier = "quick" | "notes" | "desk" | "essay";
const LIVE_TIERS: LiveTier[] = ["quick", "notes", "desk", "essay"];
// live level (0..1) above this counts as speech; anything quieter is treated as silence
const SPEAK_LEVEL = 0.08;
// rough speaking rate → estimate words from seconds of actual voiced speech
const WORDS_PER_SEC = 2.5;
const wordsFromVoiced = (voicedSec: number): number => voicedSec * WORDS_PER_SEC;
// live tier keyed off ESTIMATED words (not wall-clock elapsed): quick → notes → desk → essay
const liveTierForWords = (estWords: number): LiveTier => estWords < 8 ? "quick" : estWords < 25 ? "notes" : estWords < 60 ? "desk" : "essay";
const LIVE_PROP: Record<LiveTier, Prop> = { quick: "none", notes: "pad", desk: "desk", essay: "desk" };
// tone-aware live line spoken the moment you cross into a new tier (quick has none)
const LIVE_LINE: Partial<Record<LiveTier, Record<ChatTone, string>>> = {
  notes: { rude: "ok, noting…", friendly: "ooh, lots to say!", rose: "tell me more 🌹" },
  desk: { rude: "a whole rant…", friendly: "okay, big one!", rose: "i'm all ears 🌹" },
  essay: { rude: "an ESSAY, live?!", friendly: "wow, keep going!", rose: "forever, love 🌹" },
};

class SOD {
  private k1: number; private k2: number; private k3: number; private xp: number; private yv: number; private yd = 0;
  constructor(f: number, z: number, r: number, x0 = 0) { const pf = Math.PI * f; this.k1 = z / pf; this.k2 = 1 / ((2 * pf) * (2 * pf)); this.k3 = r * z / (2 * pf); this.xp = x0; this.yv = x0; }
  update(dt: number, x: number): number { const xd = (x - this.xp) / Math.max(dt, 1e-4); this.xp = x; const k2 = Math.max(this.k2, dt * dt / 2 + dt * this.k1 / 2, dt * this.k1); this.yv += dt * this.yd; this.yd += dt * (x + this.k3 * xd - this.yv - this.k1 * this.yd) / k2; return this.yv; }
}
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
// YV53 — on a side dock the window is parked flush to the screen edge, so the
// capsule is drawn this many px in from it: enough for its glow/shadow, not so
// much that it stops reading as docked. Mirrors the .stage inset in float.css.
const DOCK_PAD = 10;

export default function YappyPill() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const cv = canvasRef.current!, bubble = bubbleRef.current!;
    const ctx = cv.getContext("2d")!;
    // prefers-reduced-motion: render ONE calm static frame (resting pill, mouth
    // closed, level 0) instead of the 60fps loop — matches YappyHouse.
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    // ── wide scene buffer: the world fills its full width so it fills the pill ──
    const PW = 176, PH = 44, GY = 30, FX = 88, FY = 18;   // face at (FX,FY); grass starts at GY
    const os = document.createElement("canvas"); os.width = PW; os.height = PH; const octx = os.getContext("2d")!;
    const C: Record<string, string> = {
      sky1: "#8fd6ff", sky2: "#c6ecff", sun: "#fff2b0", cloud: "#fffaf0", grass: "#8fd06a", grassD: "#6fb84e",
      body: "#ffd24a", light: "#ffe9a0", dark: "#eba21f", outline: "#6b3a2e", cavity: "#7a2e14", belly: "#fff2c4",
      beak: "#ff9f1c", beakD: "#e07a12", eye: "#3a2f36", white: "#ffffff", blush: "#ff89a8", foot: "#f2861a", tongue: "#ff5d7a", sprout: "#e8a41f",
      paper: "#fffdf5", paperL: "#c9c3ad", desk: "#b06a3a", deskD: "#8a4f28", glass: "#2c2a36",
      pencil: "#ffcf3a", pencilT: "#ff8fb0", tw: "#4a4a55", twD: "#33333d", twKey: "#d8d8e2",
    };
    const ell = (cx: number, cy: number, rx: number, ry: number, col: string) => { octx.fillStyle = col; for (let y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) { const t = (y + .5 - cy) / ry; if (Math.abs(t) > 1) continue; const hw = rx * Math.sqrt(1 - t * t); octx.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); } };
    const rct = (x: number, y: number, w: number, h: number, col: string) => { octx.fillStyle = col; octx.fillRect(Math.round(x), Math.round(y), w, h); };

    interface SP { hop: number; sx: number; sy: number; blink: number; mood: number; beakF: number; sway: number; flap: number; world: number; prop: Prop; propIn: number; look: "down" | "fwd"; glasses: boolean; pencil: boolean }

    // the chick — front wing holds a pencil when noting / at the desk
    function drawChick(cx: number, feetY: number, A: SP) {
      const cy = feetY - 11 - A.hop; const rx = 11 + A.sx, ry = 11 + A.sy;
      ell(cx + rx - 1, cy + 2, 4, 3, C.outline); ell(cx + rx - 1, cy + 2, 3, 2, C.dark);              // tail
      ell(cx + rx - 2, cy + 1 - A.flap, 3, 4, C.outline); ell(cx + rx - 2, cy + 1 - A.flap, 2, 3, C.dark); // back wing
      ell(cx, cy, rx + 1, ry + 1, C.outline); ell(cx, cy, rx, ry, C.body); ell(cx - 3, cy - 4, rx - 4, ry - 5, C.light); ell(cx, cy + 5, rx - 5, ry - 5, C.belly);
      const sw = Math.round(A.sway); rct(cx - 1 + sw, cy - ry - 2, 1, 3, C.sprout); rct(cx + 1 + sw, cy - ry - 3, 1, 3, C.sprout);
      const wy = cy + 1 - A.flap;
      ell(cx - rx + 2, wy, 3, 4, C.outline); ell(cx - rx + 2, wy, 2, 3, C.body);
      if (A.pencil) { const px = cx - rx - 1, py = cy + 3; rct(px - 3, py - 3, 1, 1, C.pencilT); rct(px - 2, py - 2, 1, 1, C.pencil); rct(px - 1, py - 1, 1, 1, C.pencil); rct(px, py, 1, 1, C.pencil); rct(px + 1, py + 1, 1, 1, C.dark); }
      const ex = 5, ey = cy + 1;
      for (const s of [-1, 1]) { const eXc = cx + s * ex;
        if (A.mood > .55) { rct(eXc - 2, ey - 1, 1, 1, C.eye); rct(eXc - 1, ey - 2, 1, 1, C.eye); rct(eXc, ey - 2, 1, 1, C.eye); rct(eXc + 1, ey - 1, 1, 1, C.eye); }
        else if (A.blink > .5 || A.mood < -.5) { rct(eXc - 2, ey + (A.mood < -.5 ? 1 : 0), 4, 1, C.eye); }
        else if (A.look === "down") { ell(eXc, ey + 1, 2.1, 2.3, C.eye); rct(eXc - 1, ey, 1, 1, C.white); }
        else { ell(eXc, ey, 2.2, 2.7, C.eye); rct(eXc - 1, ey - 2, 1, 1, C.white); octx.globalAlpha = .7; rct(eXc + 1, ey + 1, 1, 1, C.white); octx.globalAlpha = 1; }
      }
      // reading glasses for the receptionist / essay personas
      if (A.glasses) { rct(cx - ex - 3, ey - 1, 1, 4, C.glass); rct(cx - ex + 2, ey - 1, 1, 4, C.glass); rct(cx - ex - 3, ey - 1, 5, 1, C.glass); rct(cx - ex - 3, ey + 2, 5, 1, C.glass);
        rct(cx + ex - 2, ey - 1, 1, 4, C.glass); rct(cx + ex + 3, ey - 1, 1, 4, C.glass); rct(cx + ex - 2, ey - 1, 5, 1, C.glass); rct(cx + ex - 2, ey + 2, 5, 1, C.glass); rct(cx - 2, ey, 4, 1, C.glass); }
      const by = cy + 5, f = A.beakF;
      rct(cx - 1, by - 1, 3, 1, C.beakD); rct(cx - 1, by, 3, 1, C.beak);
      if (f === 1) { rct(cx, by + 1, 1, 1, C.cavity); rct(cx - 1, by + 2, 3, 1, C.beakD); }
      else if (f >= 2) { rct(cx - 1, by + 1, 3, 1, C.cavity); rct(cx, by + 2, 1, 1, C.tongue); rct(cx - 1, by + 3, 3, 1, C.beakD); }
      else { rct(cx, by + 1, 1, 1, C.beak); }
      octx.globalAlpha = .7; rct(cx - rx + 2, cy + 3, 2, 1, C.blush); rct(cx + rx - 3, cy + 3, 2, 1, C.blush); octx.globalAlpha = 1;
      rct(cx - 3, cy + ry - 1, 1, 2, C.foot); rct(cx + 3, cy + ry - 1, 1, 2, C.foot);
    }

    function drawScene(A: SP) {
      octx.clearRect(0, 0, PW, PH);
      const w = A.world;
      { octx.globalAlpha = Math.max(0.8, w);
        octx.fillStyle = C.sky1; octx.fillRect(0, 0, PW, GY);
        octx.fillStyle = C.sky2; octx.fillRect(0, 0, PW, 7);
        ell(20, 8, 5, 5, C.sun);
        // slow drifting clouds
        const cd = (elapsed * 2) % (PW + 20), cX = (x: number) => ((x + cd) % (PW + 20)) - 10;
        ell(cX(120), 7, 7, 2, C.cloud); ell(cX(126), 7, 4, 2, C.cloud); ell(cX(64), 11, 6, 2, C.cloud);
        octx.fillStyle = C.grass; octx.fillRect(0, GY, PW, PH - GY); octx.fillStyle = C.grassD; octx.fillRect(0, GY, PW, 2);
        for (let x = 3; x < PW; x += 9) rct(x, GY - 2, 1, 2, C.grassD);
        octx.globalAlpha = 1;
      }
      const cx = FX - (A.prop === "desk" ? 6 : 0), feet = GY;
      // notepad slides in from the side when noting
      if (A.prop === "pad" && A.propIn > .05) { const px = cx + 13 + (1 - A.propIn) * 18, py = GY - 12;
        octx.globalAlpha = A.propIn; rct(px, py, 11, 12, C.paperL); rct(px + 1, py + 1, 9, 10, C.paper);
        for (let i = 0; i < 3; i++) rct(px + 2, py + 3 + i * 2, 7, 1, C.paperL); octx.globalAlpha = 1;
      }
      drawChick(cx, feet, A);
      // receptionist desk + a little pixel TYPEWRITER
      if (A.prop === "desk" && A.propIn > .05) { const dy = GY - 2 + (1 - A.propIn) * 16;
        octx.globalAlpha = Math.min(1, A.propIn + .2);
        rct(4, dy, PW - 8, 8, C.desk); rct(4, dy, PW - 8, 2, C.deskD); rct(4, dy + 7, PW - 8, 1, C.deskD);
        const tx = cx + 14, ty = dy - 6;
        rct(tx, ty + 2, 14, 5, C.tw); rct(tx, ty + 2, 14, 1, C.twD); rct(tx + 2, ty, 10, 3, C.tw);        // body + carriage
        rct(tx + 4, ty - 3, 6, 3, C.paper);                                                              // paper sticking up
        for (let k = 0; k < 4; k++) rct(tx + 2 + k * 3, ty + 5, 2, 1, C.twKey);                          // keys
        rct(cx - 26, dy + 1, 4, 5, C.sun);                                                               // small stamp/lamp
        octx.globalAlpha = 1;
      }
    }

    let W = 0, H = 0; const DPR = Math.min(devicePixelRatio || 1, 2);
    const resize = () => { const r = cv.getBoundingClientRect(); W = r.width; H = r.height; cv.width = W * DPR; cv.height = H * DPR; ctx.setTransform(DPR, 0, 0, DPR, 0, 0); };
    resize(); const ro = new ResizeObserver(resize); ro.observe(cv);

    let phase: Phase = "idle";
    const R = { sway: new SOD(2.2, .35, 1.4), open: new SOD(3.4, .7, 1.4), prop: new SOD(3, .7, 1.2) };
    const J = { phase: "ground", t: 0, y: 0, vy: 0, power: 0, crouch: 0 };
    const hop = (p: number) => { if (J.phase === "ground" || J.phase === "land") { J.phase = "crouch"; J.t = 0; J.power = p; } };
    const mouth = new MouthDriver();
    let blinkT = -1, nextBlink = 1.4, elapsed = 0, tPrev = 0, mood = 0, moodT = 0, idleFor = 0, level = 0, beakFrame = 0, beakHold = 0;
    let chatterTimer = 0, chatterN = 0, words = 0, doneUntil = 0, openV = 0, propV = 0;
    let voicedT = 0, liveSaid = -1;   // seconds of ACTUAL speech (gated on level) + highest live tier already reacted to
    // YV27 — live companion tone, read off settings (not a hardcoded constant).
    // Seeded from get_settings + kept current via the "settings" event the float
    // already emits on save, so a tone change takes effect without a remount.
    let chatTone: ChatTone = toChatTone(DEFAULT_TONE.tone === "off" ? undefined : DEFAULT_TONE.tone);
    const say = (t: string) => { bubble.textContent = t; bubble.classList.toggle("show", !!t); };

    const ACC: Record<Phase, string> = { idle: "351 95% 71%", listening: "351 95% 71%", thinking: "38 92% 55%", done: "152 69% 52%", sleepy: "230 20% 60%" };
    // listening → prop escalates by ESTIMATED words from actual speech (live); thinking/done → keyed off the transcript word count
    const wantPropFor = (p: Phase, w: number, estWords: number): Prop =>
      p === "listening" ? LIVE_PROP[liveTierForWords(estWords)]
        : (p === "thinking" || p === "done") ? propFor(w)
          : "none";

    function setPhase(p: Phase) {
      phase = p; idleFor = 0; chatterN = 0; chatterTimer = 0;
      if (p === "listening") { moodT = 0; hop(1); say(""); voicedT = 0; liveSaid = -1; }
      else if (p === "thinking") { moodT = 0; say(chatty(chatTone, words) ? workLine(chatTone, words, 0) : ""); }
      else if (p === "done") { moodT = 1; hop(1.2); burst(); say(reactiveLine(words, { tone: chatTone, curseFilter: DEFAULT_TONE.curseFilter }, words)); }
      else { moodT = 0; say(""); }
    }
    const sparkle: Array<{ x: number; y: number; vx: number; vy: number; life: number; heart: boolean }> = [];
    function burst() { for (let i = 0; i < 10; i++) { const a = Math.random() * 6.28, sp = 26 + Math.random() * 40; sparkle.push({ x: 0, y: -4, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp - 24, life: 1, heart: i % 3 === 0 }); } }

    // A synchronous cleanup can run before these listen() promises resolve
    // (StrictMode double-mount + Home-tab remounts). A `dead` flag unsubscribes
    // any listener that lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    invoke<AppStatus>("get_status").then((s) => setPhase(s.recording ? "listening" : s.busy ? "thinking" : "idle")).catch(() => {});
    // YV27 — seed the companion tone, then track it live off the settings event.
    invoke<PillSettings>("get_settings").then((s) => { chatTone = toChatTone(s.companionTone); }).catch(() => {});
    listen<PillSettings>("settings", (e) => { chatTone = toChatTone(e.payload?.companionTone); }).then((u) => (dead ? u() : unsubs.push(u)));
    const toIdle = () => { if (Date.now() > doneUntil) setPhase("idle"); };
    listen<AppStatus>("status", (e) => { const s = e.payload; if (s.recording) setPhase("listening"); else if (s.busy) setPhase("thinking"); else toIdle(); }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<boolean>("recording", (e) => { if (e.payload) { setPhase("listening"); mouth.reset(); } }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<number>("audio_level", (e) => { const v = typeof e.payload === "number" ? e.payload : 0; level = Math.max(0, Math.min(1, v)); }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<Transcript>("transcript", (e) => { words = e.payload?.wordCount ?? 0; setPhase("done"); doneUntil = Date.now() + 1600; window.setTimeout(() => { if (phase === "done") setPhase("idle"); }, 1600); }).then((u) => (dead ? u() : unsubs.push(u)));

    function rr(x: number, y: number, w: number, h: number, r: number) { ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath(); }

    let raf = 0, lastDraw = 0;
    // While idle/sleepy (silent mic, no hops or sparkles in flight) throttle the
    // redraw to ~18fps instead of 60 — idle breathing/drift stays perceptible but
    // this always-on pill stops burning a full-rate rAF. Active states run full-rate.
    const IDLE_FRAME_MS = 55;
    function loop(ts: number) {
      if (!reduce) raf = requestAnimationFrame(loop);   // schedule next frame (never under reduced-motion)
      const idleThrottle = (phase === "idle" || phase === "sleepy") && level < 0.02 && sparkle.length === 0 && J.phase === "ground";
      if (idleThrottle && lastDraw && ts - lastDraw < IDLE_FRAME_MS) return;   // skip this redraw, keep the rAF alive
      lastDraw = ts;
      const dt = Math.min(.05, (ts - tPrev) / 1000 || 0); tPrev = ts; elapsed += dt; idleFor += dt;
      if (phase === "idle" && idleFor > 12) setPhase("sleepy");
      if (blinkT < 0) { nextBlink -= dt; if (nextBlink <= 0) blinkT = 0; }
      let blink = 0; if (blinkT >= 0) { blinkT += dt; const d = .13; blink = blinkT < d / 2 ? blinkT / (d / 2) : blinkT < d ? 1 - (blinkT - d / 2) / (d / 2) : 0; if (blinkT > d) { blinkT = -1; nextBlink = 2 + Math.random() * 4; } }
      const active = phase === "listening" || phase === "thinking" || phase === "done";
      openV = R.open.update(dt, active ? 1 : 0); openV = Math.max(0, Math.min(1, openV));
      // live escalation WHILE talking — driven by ACTUAL speech, not a wall-clock timer: only
      // frames above the speaking threshold advance the estimate, so silence never moves it along.
      if (phase === "listening") {
        if (level > SPEAK_LEVEL) voicedT += dt;
        const liveTier = liveTierForWords(wordsFromVoiced(voicedT)); const idx = LIVE_TIERS.indexOf(liveTier);
        if (idx > liveSaid) { liveSaid = idx; const line = LIVE_LINE[liveTier]; if (line) say(line[chatTone]); }
      } else voicedT = 0;
      const estWords = wordsFromVoiced(voicedT);
      const wantProp = wantPropFor(phase, words, estWords);
      propV = R.prop.update(dt, wantProp !== "none" ? 1 : 0); propV = Math.max(0, Math.min(1, propV));
      mood += (moodT - mood) * Math.min(1, dt * 10);
      const grav = 2600;
      if (J.phase === "crouch") { J.t += dt; J.crouch = Math.min(1, J.t / .1); if (J.t >= .1) { J.phase = "air"; J.vy = -300 * J.power; J.crouch = 0; } }
      else if (J.phase === "air") { J.vy += grav * dt; J.y += J.vy * dt; if (J.y >= 0) { J.y = 0; J.phase = "land"; J.t = 0; J.vy = 0; } }
      else if (J.phase === "land") { J.t += dt; if (J.t > .13) J.phase = "ground"; }
      else if (phase === "listening" && Math.random() < dt * .5) hop(.7);
      const land = J.phase === "land" ? (1 - J.t / .13) : 0;
      const hopPx = (-J.y / 8) - J.crouch * 1.5;
      const sx = Math.round(land * 2 + J.crouch * 1.2);
      const sy = Math.round(-land * 2 - J.crouch * 1.6 + Math.max(-2, Math.min(2, -J.vy / 240)));
      const flap = Math.round(phase === "listening" ? Math.abs(Math.sin(elapsed * 14)) * 2 : 0);
      const sway = R.sway.update(dt, Math.sin(elapsed * 3) * 1 + (phase === "thinking" ? .6 : 0));
      const rms = (phase === "listening" || phase === "thinking") ? level : 0;
      const beak = mouth.push(rms, dt);
      beakHold -= dt;
      if (phase === "done") beakFrame = 0;
      else if (beakHold <= 0) { let w = beakFrame; if (beakFrame === 0 && beak > .28) w = 1; else if (beakFrame === 1 && beak > .6) w = 2; else if (beakFrame >= 2 && beak < .42) w = 1; else if (beakFrame === 1 && beak < .12) w = 0; if (w !== beakFrame) { beakFrame = w; beakHold = .05; } }
      if (phase === "thinking" && chatty(chatTone, words)) { chatterTimer -= dt; if (chatterTimer <= 0) { chatterN++; say(workLine(chatTone, words, chatterN)); chatterTimer = .9 + Math.random() * .5; } }
      for (const p of sparkle) { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += 120 * dt; p.life -= dt * 1.2; }
      for (let i = sparkle.length - 1; i >= 0; i--) if (sparkle[i].life <= 0) sparkle.splice(i, 1);

      const lookMode: "down" | "fwd" = (wantProp !== "none" && propV > .5) ? "down" : "fwd";
      // glasses ride the desk prop (desk/essay tiers), pencil rides the notepad (notes tier) — for both live + transcript tiers
      const glasses = wantProp === "desk" && propV > .5 && phase !== "idle";
      const pencil = wantProp === "pad" && propV > .4 && phase !== "idle";
      drawScene({ hop: hopPx, sx, sy, blink, mood, beakF: beakFrame, sway, flap, world: openV, prop: wantProp, propIn: propV, look: lookMode, glasses, pencil });

      // ── the capsule + pull-back camera (world fills the whole pill) ──
      ctx.clearRect(0, 0, W, H);
      const tierOpen = (wantProp === "desk") ? 1.16 : (wantProp === "pad") ? 1.05 : 1;
      // bigger open capsule so the chick + grass read larger
      const capW = lerp(72, 236 * tierOpen, openV), capH = lerp(22, 60 * tierOpen, openV);
      // YV53 — dock edge (read live off <html data-dock>, set by float-main): the
      // window is parked flush to that screen edge, so the capsule hugs it instead
      // of sitting mid-window. ONLY the anchor moves — capsule size and the camera
      // below are untouched, so the world keeps its aspect (never letterboxed).
      const dock = document.documentElement.dataset.dock;
      const cx = dock === "left" ? DOCK_PAD + capW / 2
        : dock === "right" ? W - DOCK_PAD - capW / 2
          : W / 2;
      const cy = H * 0.56;
      const x0 = cx - capW / 2, y0 = cy - capH / 2, acc = ACC[phase];
      // ambient glow when active
      if (openV > .05) { ctx.save(); ctx.shadowColor = `hsla(${acc}, ${.5 * openV})`; ctx.shadowBlur = 22 * openV; rr(x0, y0, capW, capH, capH / 2); ctx.fillStyle = "rgba(0,0,0,0.001)"; ctx.fill(); ctx.restore(); }
      // SKY-BLUE capsule fill (not obsidian)
      const g = ctx.createLinearGradient(0, y0, 0, y0 + capH); g.addColorStop(0, "#c6ecff"); g.addColorStop(1, "#8fd6ff");
      ctx.fillStyle = g; rr(x0, y0, capW, capH, capH / 2); ctx.fill();
      ctx.save(); rr(x0 + 1, y0 + 1, capW - 2, capH - 2, (capH - 2) / 2); ctx.clip(); ctx.imageSmoothingEnabled = false;
      // CAMERA: rest = zoomed onto the face; open = whole buffer (world fills the pill)
      const iw = capW - 2, ih = capH - 2;
      const viewW = lerp(26, PW * 0.8, openV);         // zoom in: see ~80% of the buffer at open (bigger chick + grass)
      const viewH = viewW * (ih / iw);                // match the capsule aspect → no distortion
      const vcx = FX, vcy = lerp(FY, PH * 0.5, openV); // pan from the face to the scene centre
      ctx.drawImage(os, vcx - viewW / 2, vcy - viewH / 2, viewW, viewH, x0 + 1, y0 + 1, iw, ih);
      // sparkles (done) inside the pill
      for (const q of sparkle) { ctx.save(); ctx.globalAlpha = Math.max(0, q.life); ctx.translate(cx + q.x, cy + q.y); ctx.fillStyle = q.heart ? "#ff6f9c" : "#ffd24a"; ctx.fillRect(-1, -1, 2, 2); ctx.restore(); }
      ctx.restore();
      ctx.lineWidth = 1; ctx.strokeStyle = openV > .05 ? `hsla(${acc}, ${.55})` : "rgba(255,255,255,0.12)"; rr(x0 + .5, y0 + .5, capW - 1, capH - 1, capH / 2); ctx.stroke();

      // speech bubble above the capsule — centred on it, but kept inside the
      // window so a docked (edge-hugging) capsule doesn't push it off-screen.
      const bw = bubble.classList.contains("show") ? bubble.offsetWidth : 0;
      bubble.style.left = (bw ? Math.max(bw / 2 + 4, Math.min(W - bw / 2 - 4, cx)) : cx) + "px";
      bubble.style.top = (y0 - 10) + "px";
    }
    // reduced-motion → paint one calm static frame and never loop; else run the rAF.
    if (reduce) loop(performance.now());
    else raf = requestAnimationFrame(loop);
    return () => { dead = true; cancelAnimationFrame(raf); ro.disconnect(); unsubs.forEach((u) => u()); };
  }, []);

  return (
    <div className="kami-stage">
      <div ref={bubbleRef} className="kami-bubble"></div>
      <canvas ref={canvasRef} className="kami-canvas" aria-hidden />
    </div>
  );
}
