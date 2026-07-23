/**
 * YappyPill — the pixel-art chick companion ("Yappy") living in a little world.
 * Chunky pixel diorama rendered to a low-res buffer scaled with smoothing off.
 * Driven by the app's real events: recording → runs up & listens (beak tracks
 * the live mic level via MouthDriver), busy → thinking + chatter, transcript →
 * yaps a word-count line (tone.ts). Idle → wanders & pecks, then sleeps.
 */
import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { MouthDriver } from "./mouth";
import { reactiveLine, DEFAULT_TONE, bucketFor } from "./tone";

interface AppStatus { recording: boolean; busy: boolean; message: string }
interface Transcript { wordCount: number; text: string }
type Phase = "idle" | "listening" | "thinking" | "done" | "sleepy";

const WORK: Record<string, string[]> = {
  medium: ["okay okay…", "typing it…"],
  long: ["formulating…", "almost there…", "writing it out…"],
  epic: ["formulating, formulating…", "that's an essay…", "hang on hang on…"],
};
const chatty = (w: number) => { const b = bucketFor(w); return b === "medium" || b === "long" || b === "epic"; };
const workLine = (w: number, n: number) => { const o = WORK[bucketFor(w)] || WORK.medium; return o[n % o.length]; };

class SOD {
  private k1: number; private k2: number; private k3: number; private xp: number; private yv = 0; private yd = 0;
  constructor(f: number, z: number, r: number, x0 = 0) { const pf = Math.PI * f; this.k1 = z / pf; this.k2 = 1 / ((2 * pf) * (2 * pf)); this.k3 = r * z / (2 * pf); this.xp = x0; this.yv = x0; }
  update(dt: number, x: number): number { const xd = (x - this.xp) / Math.max(dt, 1e-4); this.xp = x; const k2 = Math.max(this.k2, dt * dt / 2 + dt * this.k1 / 2, dt * this.k1); this.yv += dt * this.yd; this.yd += dt * (x + this.k3 * xd - this.yv - this.k1 * this.yd) / k2; return this.yv; }
}

export default function YappyPill() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const cv = canvasRef.current!, bubble = bubbleRef.current!;
    const ctx = cv.getContext("2d")!;

    // ── pixel diorama buffer ──
    const PW = 108, PH = 70, GY = 52;
    const os = document.createElement("canvas"); os.width = PW; os.height = PH; const octx = os.getContext("2d")!;
    const C: Record<string, string> = {
      sky1: "#ffe6c0", sky2: "#ffd39a", sun: "#fff2b0", cloud: "#fffaf0", grass: "#8fd06a", grassD: "#6fb84e",
      body: "#ffd24a", light: "#ffe9a0", dark: "#eba21f", outline: "#6b3a2e", cavity: "#7a2e14", belly: "#fff2c4",
      beak: "#ff9f1c", beakD: "#e07a12", eye: "#3a2f36", white: "#ffffff", blush: "#ff89a8", foot: "#f2861a", tongue: "#ff5d7a", sprout: "#e8a41f", pot: "#d8894a", leaf: "#79c257", petal: "#ff8fb0",
    };
    const ell = (cx: number, cy: number, rx: number, ry: number, col: string) => { octx.fillStyle = col; for (let y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) { const t = (y + .5 - cy) / ry; if (Math.abs(t) > 1) continue; const hw = rx * Math.sqrt(1 - t * t); octx.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); } };
    const rct = (x: number, y: number, w: number, h: number, col: string) => { octx.fillStyle = col; octx.fillRect(Math.round(x), Math.round(y), w, h); };

    function drawScene() {
      octx.fillStyle = C.sky1; octx.fillRect(0, 0, PW, GY);
      octx.fillStyle = C.sky2; octx.fillRect(0, GY - 14, PW, 14);
      ell(16, 13, 7, 7, C.sun);
      ell(74, 12, 7, 3, C.cloud); ell(80, 12, 5, 3, C.cloud); ell(40, 20, 6, 2, C.cloud);
      octx.fillStyle = C.grass; octx.fillRect(0, GY, PW, PH - GY);
      octx.fillStyle = C.grassD; octx.fillRect(0, GY, PW, 2);
      for (let x = 4; x < PW; x += 11) { rct(x, GY - 2, 1, 2, C.grassD); rct(x + 2, GY - 3, 1, 3, C.grassD); }
      rct(94, GY - 6, 1, 6, C.leaf); ell(94.5, GY - 7, 2, 2, C.petal); rct(94, GY - 7, 1, 1, C.sun);
      octx.fillStyle = C.pot; octx.fillRect(8, GY - 4, 7, 4); ell(11.5, GY - 6, 3, 3, C.leaf);
    }

    interface CP { hop: number; sx: number; sy: number; bob: number; step: number; blink: number; look: number; lookY: number; mood: number; beakF: number; sway: number; flap: number }
    function drawChick(cx: number, feetY: number, P: CP) {
      const cy = feetY - 13 - P.hop + P.bob;
      const rx = 13 + P.sx, ry = 13 + P.sy;
      octx.fillStyle = "rgba(60,90,40,.22)"; for (let y = feetY - 1; y <= feetY + 1; y++) { const t = (y - feetY) / 1.5; const hw = (11 - P.hop * .5) * Math.sqrt(Math.max(0, 1 - t * t)); octx.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); }
      ell(cx + rx - 1, cy + 2, 5, 4, C.outline); ell(cx + rx - 1, cy + 2, 4, 3, C.dark);
      ell(cx + rx - 2, cy + 1 - P.flap, 4, 6, C.outline); ell(cx + rx - 2, cy + 1 - P.flap, 3, 5, C.dark);
      ell(cx, cy, rx + 1, ry + 1, C.outline);
      ell(cx, cy, rx, ry, C.body);
      ell(cx - 3, cy - 4, rx - 4, ry - 5, C.light);
      ell(cx, cy + 5, rx - 5, ry - 5, C.belly);
      const sw = Math.round(P.sway); rct(cx - 1 + sw, cy - ry - 3, 1, 3, C.sprout); rct(cx + 1 + sw, cy - ry - 4, 1, 4, C.sprout); rct(cx + sw, cy - ry - 2, 1, 2, C.sprout);
      ell(cx - rx + 2, cy + 1 - P.flap, 4, 6, C.outline); ell(cx - rx + 2, cy + 1 - P.flap, 3, 5, C.body); rct(cx - rx + 1, cy - 1 - P.flap, 1, 4, C.dark);
      const ex = 6, ey = cy + 1;
      for (const s of [-1, 1]) { const eXc = cx + s * ex;
        if (P.mood > .55) { octx.fillStyle = C.eye; rct(eXc - 2, ey - 1, 1, 1, C.eye); rct(eXc - 1, ey - 2, 1, 1, C.eye); rct(eXc, ey - 2, 1, 1, C.eye); rct(eXc + 1, ey - 1, 1, 1, C.eye); }
        else if (P.blink > .5 || P.mood < -.5) { rct(eXc - 2, ey + (P.mood < -.5 ? 1 : 0), 4, 1, C.eye); }
        else { ell(eXc, ey, 2.4, 3, C.eye); rct(eXc - 1 + Math.round(P.look), ey - 2 + Math.round(P.lookY), 1, 1, C.white); octx.globalAlpha = .7; rct(eXc + 1, ey + 1, 1, 1, C.white); octx.globalAlpha = 1; }
      }
      const by = cy + 6, f = P.beakF;
      rct(cx - 1, by - 1, 3, 1, C.beakD);
      rct(cx - 1, by, 3, 1, C.beak);
      if (f === 1) { rct(cx, by + 1, 1, 1, C.cavity); rct(cx - 1, by + 2, 3, 1, C.beakD); rct(cx, by + 3, 1, 1, C.beakD); }
      else if (f >= 2) { rct(cx - 1, by + 1, 3, 1, C.cavity); rct(cx, by + 2, 1, 1, C.tongue); rct(cx - 1, by + 3, 3, 1, C.beakD); rct(cx, by + 4, 1, 1, C.beakD); }
      else { rct(cx, by + 1, 1, 1, C.beak); }
      octx.globalAlpha = .75; rct(cx - rx + 2, cy + 3, 2, 1, C.blush); rct(cx + rx - 3, cy + 3, 2, 1, C.blush); octx.globalAlpha = 1;
      const st = P.step;
      rct(cx - 4 + (st > 0 ? 1 : 0), cy + ry - 1, 1, 2, C.foot); rct(cx - 5 + (st > 0 ? 1 : 0), cy + ry + 1, 3, 1, C.foot);
      rct(cx + 4 + (st < 0 ? -1 : 0), cy + ry - 1, 1, 2, C.foot); rct(cx + 3 + (st < 0 ? -1 : 0), cy + ry + 1, 3, 1, C.foot);
    }

    // ── main canvas sizing ──
    let W = 0, H = 0; const DPR = Math.min(devicePixelRatio || 1, 2);
    const resize = () => { const r = cv.getBoundingClientRect(); W = r.width; H = r.height; cv.width = W * DPR; cv.height = H * DPR; ctx.setTransform(DPR, 0, 0, DPR, 0, 0); ctx.imageSmoothingEnabled = false; };
    resize(); const ro = new ResizeObserver(resize); ro.observe(cv);

    // ── state ──
    let phase: Phase = "idle";
    const R = { sway: new SOD(2.2, .35, 1.4), look: new SOD(3.5, .7, 0), lookY: new SOD(3.5, .7, 0) };
    const J = { phase: "ground", t: 0, y: 0, vy: 0, power: 0, crouch: 0 };
    const hop = (p: number) => { if (J.phase === "ground" || J.phase === "land") { J.phase = "crouch"; J.t = 0; J.power = p; } };
    const mouth = new MouthDriver();
    let blinkT = -1, nextBlink = 1.4, elapsed = 0, tPrev = 0, eyeWide = 0, eyeWideT = 0, mood = 0, moodT = 0, idleFor = 0, level = 0, beakFrame = 0, beakHold = 0;
    let petX = 54, petTarget = 54, walkPhase = 0, moving = false, wanderTimer = 2, chatterTimer = 0, chatterN = 0, words = 0, doneUntil = 0;
    const sparkle: Array<{ x: number; y: number; vx: number; vy: number; life: number; heart: boolean }> = [];
    const say = (t: string) => { bubble.textContent = t; bubble.classList.toggle("show", !!t); };

    function setPhase(p: Phase) {
      phase = p; idleFor = 0; chatterN = 0; chatterTimer = 0;
      if (p === "listening") { eyeWideT = 1; moodT = 0; petTarget = 54; hop(1); }
      else if (p === "thinking") { eyeWideT = .2; moodT = 0; petTarget = 54; say(chatty(words) ? workLine(words, 0) : ""); }
      else if (p === "done") { moodT = 1; eyeWideT = .4; petTarget = 72; hop(1.3); burst(); say(reactiveLine(words, DEFAULT_TONE, words)); }
      else if (p === "sleepy") { eyeWideT = -1; moodT = 0; say(""); }
      else { eyeWideT = 0; moodT = 0; say(""); }
    }
    function burst() { for (let i = 0; i < 14; i++) { const a = Math.random() * 6.28, sp = 30 + Math.random() * 50; sparkle.push({ x: 0, y: -6, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp - 30, life: 1, heart: i % 3 === 0 }); } }

    // ── events ──
    const unsubs: Array<() => void> = [];
    invoke<AppStatus>("get_status").then((s) => setPhase(s.recording ? "listening" : s.busy ? "thinking" : "idle")).catch(() => {});
    const toIdle = () => { if (Date.now() > doneUntil) setPhase("idle"); };
    listen<AppStatus>("status", (e) => { const s = e.payload; if (s.recording) setPhase("listening"); else if (s.busy) setPhase("thinking"); else toIdle(); }).then((u) => unsubs.push(u));
    listen<boolean>("recording", (e) => { if (e.payload) { setPhase("listening"); mouth.reset(); } }).then((u) => unsubs.push(u));
    listen<number>("audio_level", (e) => { const v = typeof e.payload === "number" ? e.payload : 0; level = Math.max(0, Math.min(1, v)); }).then((u) => unsubs.push(u));
    listen<Transcript>("transcript", (e) => { words = e.payload?.wordCount ?? 0; setPhase("done"); doneUntil = Date.now() + 1600; window.setTimeout(() => { if (phase === "done") setPhase("idle"); }, 1600); }).then((u) => unsubs.push(u));

    // ── loop ──
    let raf = 0;
    function loop(ts: number) {
      const dt = Math.min(.05, (ts - tPrev) / 1000 || 0); tPrev = ts; elapsed += dt; idleFor += dt;
      if (phase === "idle" && idleFor > 10) setPhase("sleepy");
      if (blinkT < 0) { nextBlink -= dt; if (nextBlink <= 0) blinkT = 0; }
      let blink = 0; if (blinkT >= 0) { blinkT += dt; const d = .13; blink = blinkT < d / 2 ? blinkT / (d / 2) : blinkT < d ? 1 - (blinkT - d / 2) / (d / 2) : 0; if (blinkT > d) { blinkT = -1; nextBlink = 2 + Math.random() * 4; } }
      if (phase === "idle") { wanderTimer -= dt; if (wanderTimer <= 0) { petTarget = 20 + Math.random() * 68; wanderTimer = 2 + Math.random() * 3.5; } }
      const active = phase !== "idle" && phase !== "sleepy";
      const dx = petTarget - petX; moving = Math.abs(dx) > 1.2;
      if (moving) { petX += Math.sign(dx) * Math.min(Math.abs(dx), (active ? 70 : 16) * dt); walkPhase += dt * (active ? 18 : 9); }
      let lookTx = 0, lookTy = 0;
      if (phase === "idle" && !moving) lookTx = Math.sin(elapsed * .6) * 1.2; else if (phase === "thinking") { lookTx = Math.sin(elapsed) * 1; lookTy = -1; }
      const look = R.look.update(dt, lookTx), lookY = R.lookY.update(dt, lookTy);
      eyeWide += (eyeWideT - eyeWide) * Math.min(1, dt * 8); mood += (moodT - mood) * Math.min(1, dt * 10);
      const grav = 2600;
      if (J.phase === "crouch") { J.t += dt; J.crouch = Math.min(1, J.t / .11); if (J.t >= .11) { J.phase = "air"; J.vy = -360 * J.power; J.crouch = 0; } }
      else if (J.phase === "air") { J.vy += grav * dt; J.y += J.vy * dt; if (J.y >= 0) { J.y = 0; J.phase = "land"; J.t = 0; J.vy = 0; } }
      else if (J.phase === "land") { J.t += dt; if (J.t > .14) J.phase = "ground"; }
      else if (phase === "idle" && !moving && Math.random() < dt * .25) hop(.6);
      const land = J.phase === "land" ? (1 - J.t / .14) : 0;
      const airStretch = Math.max(-2, Math.min(3, -J.vy / 220));
      const hopPx = (-J.y / 6) - J.crouch * 2;
      const sx = Math.round(-airStretch * .5 + land * 2 + J.crouch * 1.4);
      const sy = Math.round(airStretch * .7 - land * 2.2 - J.crouch * 2);
      const bob = moving ? -Math.round(Math.abs(Math.sin(walkPhase))) : 0;
      const step = moving ? (Math.sin(walkPhase) > 0 ? 1 : -1) : 0;
      const flap = Math.round((moving ? Math.abs(Math.sin(walkPhase)) * 2 : 0) + (phase === "listening" ? Math.abs(Math.sin(elapsed * 14)) * 2 : 0));
      const sway = R.sway.update(dt, Math.sin(elapsed * 3) * 1.2 + look * .6 + (moving ? Math.sin(walkPhase) * 1.5 : 0));
      // `level` is fed by the audio_level event (and returns to 0 when recording stops).
      const rms = (phase === "listening" || phase === "thinking") ? level : 0;
      const beak = mouth.push(rms, dt);
      beakHold -= dt;
      if (phase === "done") beakFrame = 0;
      else if (beakHold <= 0) { let w = beakFrame; if (beakFrame === 0 && beak > .28) w = 1; else if (beakFrame === 1 && beak > .6) w = 2; else if (beakFrame >= 2 && beak < .42) w = 1; else if (beakFrame === 1 && beak < .12) w = 0; if (w !== beakFrame) { beakFrame = w; beakHold = .05; } }
      if (phase === "thinking" && chatty(words)) { chatterTimer -= dt; if (chatterTimer <= 0) { chatterN++; say(workLine(words, chatterN)); chatterTimer = .9 + Math.random() * .5; } }
      for (const p of sparkle) { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += 140 * dt; p.life -= dt * 1.1; }
      for (let i = sparkle.length - 1; i >= 0; i--) if (sparkle[i].life <= 0) sparkle.splice(i, 1);

      drawScene();
      drawChick(petX, GY, { hop: hopPx, sx, sy, bob, step, blink, look, lookY, mood, beakF: beakFrame, sway, flap });

      // to screen
      ctx.clearRect(0, 0, W, H);
      const ww = Math.min(W * .96, 300), wh = ww * PH / PW, wx = (W - ww) / 2, wy = H - wh - 8, scale = ww / PW;
      ctx.save(); rr(ctx, wx, wy, ww, wh, 16); ctx.clip();
      ctx.imageSmoothingEnabled = false; ctx.drawImage(os, wx, wy, ww, wh);
      const pyS = wy + (GY - 13 - hopPx) * scale;
      for (const q of sparkle) { ctx.save(); ctx.globalAlpha = Math.max(0, q.life); ctx.translate(wx + petX * scale + q.x * scale * .4, pyS + q.y * scale * .4); ctx.fillStyle = q.heart ? "#ff6f9c" : "#ffd24a"; ctx.beginPath(); ctx.arc(0, 0, scale * 1.4, 0, 7); ctx.fill(); ctx.restore(); }
      ctx.restore();
      // bubble follows the pet
      bubble.style.left = (wx + petX * scale) + "px";
      bubble.style.top = (wy + (GY - 26) * scale - 4) + "px";
      raf = requestAnimationFrame(loop);
    }
    raf = requestAnimationFrame(loop);

    return () => { cancelAnimationFrame(raf); ro.disconnect(); unsubs.forEach((u) => u()); };
  }, []);

  return (
    <div className="kami-stage">
      <div ref={bubbleRef} className="kami-bubble"></div>
      <canvas ref={canvasRef} className="kami-canvas" aria-hidden />
    </div>
  );
}

function rr(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath();
}
