/**
 * YappyPill — the pixel-chick companion, as an actual PILL.
 * At rest it's a small obsidian capsule showing just Yappy's face (eyes + beak).
 * When you dictate, the pill OPENS (expands) to reveal the whole chick, its beak
 * tracks your live mic level, it chatters on long prompts, and yaps a word-count
 * line — then folds back to the little face. Compact, not a big world box.
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
  long: ["formulating…", "almost there…"],
  epic: ["formulating…", "that's an essay…", "hang on…"],
};
const chatty = (w: number) => { const b = bucketFor(w); return b === "medium" || b === "long" || b === "epic"; };
const workLine = (w: number, n: number) => { const o = WORK[bucketFor(w)] || WORK.medium; return o[n % o.length]; };

class SOD {
  private k1: number; private k2: number; private k3: number; private xp: number; private yv: number; private yd = 0;
  constructor(f: number, z: number, r: number, x0 = 0) { const pf = Math.PI * f; this.k1 = z / pf; this.k2 = 1 / ((2 * pf) * (2 * pf)); this.k3 = r * z / (2 * pf); this.xp = x0; this.yv = x0; }
  update(dt: number, x: number): number { const xd = (x - this.xp) / Math.max(dt, 1e-4); this.xp = x; const k2 = Math.max(this.k2, dt * dt / 2 + dt * this.k1 / 2, dt * this.k1); this.yv += dt * this.yd; this.yd += dt * (x + this.k3 * xd - this.yv - this.k1 * this.yd) / k2; return this.yv; }
}
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

export default function YappyPill() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const cv = canvasRef.current!, bubble = bubbleRef.current!;
    const ctx = cv.getContext("2d")!;

    // ── tiny chick buffer (face-forward: eyes at ~y15, so a short capsule reveals only the face) ──
    const PW = 32, PH = 34, FEET = 30;
    const os = document.createElement("canvas"); os.width = PW; os.height = PH; const octx = os.getContext("2d")!;
    const C: Record<string, string> = {
      body: "#ffd24a", light: "#ffe9a0", dark: "#eba21f", outline: "#6b3a2e", cavity: "#7a2e14",
      belly: "#fff2c4", beak: "#ff9f1c", beakD: "#e07a12", eye: "#3a2f36", white: "#ffffff", blush: "#ff89a8", foot: "#f2861a", tongue: "#ff5d7a", sprout: "#e8a41f",
    };
    const ell = (cx: number, cy: number, rx: number, ry: number, col: string) => { octx.fillStyle = col; for (let y = Math.floor(cy - ry); y <= Math.ceil(cy + ry); y++) { const t = (y + .5 - cy) / ry; if (Math.abs(t) > 1) continue; const hw = rx * Math.sqrt(1 - t * t); octx.fillRect(Math.round(cx - hw), y, Math.max(1, Math.round(2 * hw)), 1); } };
    const rct = (x: number, y: number, w: number, h: number, col: string) => { octx.fillStyle = col; octx.fillRect(Math.round(x), Math.round(y), w, h); };

    interface CP { hop: number; sx: number; sy: number; blink: number; look: number; mood: number; beakF: number; sway: number; flap: number }
    function drawChick(P: CP) {
      octx.clearRect(0, 0, PW, PH);
      const cx = 16, cy = FEET - 13 - P.hop;
      const rx = 12 + P.sx, ry = 12 + P.sy;
      ell(cx + rx - 1, cy + 2, 5, 4, C.outline); ell(cx + rx - 1, cy + 2, 4, 3, C.dark);            // tail
      ell(cx + rx - 2, cy + 1 - P.flap, 3, 5, C.outline); ell(cx + rx - 2, cy + 1 - P.flap, 2, 4, C.dark); // back wing
      ell(cx, cy, rx + 1, ry + 1, C.outline);
      ell(cx, cy, rx, ry, C.body);
      ell(cx - 3, cy - 4, rx - 4, ry - 5, C.light);
      ell(cx, cy + 5, rx - 5, ry - 5, C.belly);
      const sw = Math.round(P.sway); rct(cx - 1 + sw, cy - ry - 2, 1, 3, C.sprout); rct(cx + 1 + sw, cy - ry - 3, 1, 4, C.sprout);
      ell(cx - rx + 2, cy + 1 - P.flap, 3, 5, C.outline); ell(cx - rx + 2, cy + 1 - P.flap, 2, 4, C.body); // front wing
      const ex = 5.5, ey = cy + 1;
      for (const s of [-1, 1]) { const eXc = cx + s * ex;
        if (P.mood > .55) { rct(eXc - 2, ey - 1, 1, 1, C.eye); rct(eXc - 1, ey - 2, 1, 1, C.eye); rct(eXc, ey - 2, 1, 1, C.eye); rct(eXc + 1, ey - 1, 1, 1, C.eye); }
        else if (P.blink > .5 || P.mood < -.5) { rct(eXc - 2, ey + (P.mood < -.5 ? 1 : 0), 4, 1, C.eye); }
        else { ell(eXc, ey, 2.3, 2.9, C.eye); rct(eXc - 1 + Math.round(P.look), ey - 2, 1, 1, C.white); octx.globalAlpha = .7; rct(eXc + 1, ey + 1, 1, 1, C.white); octx.globalAlpha = 1; }
      }
      const by = cy + 6, f = P.beakF;
      rct(cx - 1, by - 1, 3, 1, C.beakD);
      rct(cx - 1, by, 3, 1, C.beak);
      if (f === 1) { rct(cx, by + 1, 1, 1, C.cavity); rct(cx - 1, by + 2, 3, 1, C.beakD); }
      else if (f >= 2) { rct(cx - 1, by + 1, 3, 1, C.cavity); rct(cx, by + 2, 1, 1, C.tongue); rct(cx - 1, by + 3, 3, 1, C.beakD); }
      else { rct(cx, by + 1, 1, 1, C.beak); }
      octx.globalAlpha = .7; rct(cx - rx + 2, cy + 3, 2, 1, C.blush); rct(cx + rx - 3, cy + 3, 2, 1, C.blush); octx.globalAlpha = 1;
      rct(cx - 3, cy + ry - 1, 1, 2, C.foot); rct(cx + 3, cy + ry - 1, 1, 2, C.foot);
    }

    let W = 0, H = 0; const DPR = Math.min(devicePixelRatio || 1, 2);
    const resize = () => { const r = cv.getBoundingClientRect(); W = r.width; H = r.height; cv.width = W * DPR; cv.height = H * DPR; ctx.setTransform(DPR, 0, 0, DPR, 0, 0); };
    resize(); const ro = new ResizeObserver(resize); ro.observe(cv);

    let phase: Phase = "idle";
    const R = { sway: new SOD(2.2, .35, 1.4), look: new SOD(3.5, .7, 0), open: new SOD(3.4, .7, 1.4) };
    const J = { phase: "ground", t: 0, y: 0, vy: 0, power: 0, crouch: 0 };
    const hop = (p: number) => { if (J.phase === "ground" || J.phase === "land") { J.phase = "crouch"; J.t = 0; J.power = p; } };
    const mouth = new MouthDriver();
    let blinkT = -1, nextBlink = 1.4, elapsed = 0, tPrev = 0, mood = 0, moodT = 0, idleFor = 0, level = 0, beakFrame = 0, beakHold = 0;
    let chatterTimer = 0, chatterN = 0, words = 0, doneUntil = 0, openV = 0;
    const say = (t: string) => { bubble.textContent = t; bubble.classList.toggle("show", !!t); };

    const ACC: Record<Phase, string> = { idle: "351 95% 71%", listening: "351 95% 71%", thinking: "38 92% 55%", done: "152 69% 52%", sleepy: "230 20% 60%" };

    function setPhase(p: Phase) {
      phase = p; idleFor = 0; chatterN = 0; chatterTimer = 0;
      if (p === "listening") { moodT = 0; hop(1); }
      else if (p === "thinking") { moodT = 0; say(chatty(words) ? workLine(words, 0) : ""); }
      else if (p === "done") { moodT = 1; hop(1.2); burst(); say(reactiveLine(words, DEFAULT_TONE, words)); }
      else { moodT = 0; say(""); }
    }
    const sparkle: Array<{ x: number; y: number; vx: number; vy: number; life: number; heart: boolean }> = [];
    function burst() { for (let i = 0; i < 10; i++) { const a = Math.random() * 6.28, sp = 26 + Math.random() * 40; sparkle.push({ x: 0, y: -4, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp - 24, life: 1, heart: i % 3 === 0 }); } }

    const unsubs: Array<() => void> = [];
    invoke<AppStatus>("get_status").then((s) => setPhase(s.recording ? "listening" : s.busy ? "thinking" : "idle")).catch(() => {});
    const toIdle = () => { if (Date.now() > doneUntil) setPhase("idle"); };
    listen<AppStatus>("status", (e) => { const s = e.payload; if (s.recording) setPhase("listening"); else if (s.busy) setPhase("thinking"); else toIdle(); }).then((u) => unsubs.push(u));
    listen<boolean>("recording", (e) => { if (e.payload) { setPhase("listening"); mouth.reset(); } }).then((u) => unsubs.push(u));
    listen<number>("audio_level", (e) => { const v = typeof e.payload === "number" ? e.payload : 0; level = Math.max(0, Math.min(1, v)); }).then((u) => unsubs.push(u));
    listen<Transcript>("transcript", (e) => { words = e.payload?.wordCount ?? 0; setPhase("done"); doneUntil = Date.now() + 1600; window.setTimeout(() => { if (phase === "done") setPhase("idle"); }, 1600); }).then((u) => unsubs.push(u));

    function rr(x: number, y: number, w: number, h: number, r: number) { ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath(); }

    let raf = 0;
    function loop(ts: number) {
      const dt = Math.min(.05, (ts - tPrev) / 1000 || 0); tPrev = ts; elapsed += dt; idleFor += dt;
      if (phase === "idle" && idleFor > 12) setPhase("sleepy");
      if (blinkT < 0) { nextBlink -= dt; if (nextBlink <= 0) blinkT = 0; }
      let blink = 0; if (blinkT >= 0) { blinkT += dt; const d = .13; blink = blinkT < d / 2 ? blinkT / (d / 2) : blinkT < d ? 1 - (blinkT - d / 2) / (d / 2) : 0; if (blinkT > d) { blinkT = -1; nextBlink = 2 + Math.random() * 4; } }
      const active = phase === "listening" || phase === "thinking" || phase === "done";
      openV = R.open.update(dt, active ? 1 : 0); openV = Math.max(0, Math.min(1, openV));
      mood += (moodT - mood) * Math.min(1, dt * 10);
      const look = R.look.update(dt, phase === "thinking" ? Math.sin(elapsed) * 1 : phase === "idle" ? Math.sin(elapsed * .6) * .8 : 0);
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
      const sway = R.sway.update(dt, Math.sin(elapsed * 3) * 1 + look * .5);
      const rms = (phase === "listening" || phase === "thinking") ? level : 0;
      const beak = mouth.push(rms, dt);
      beakHold -= dt;
      if (phase === "done") beakFrame = 0;
      else if (beakHold <= 0) { let w = beakFrame; if (beakFrame === 0 && beak > .28) w = 1; else if (beakFrame === 1 && beak > .6) w = 2; else if (beakFrame >= 2 && beak < .42) w = 1; else if (beakFrame === 1 && beak < .12) w = 0; if (w !== beakFrame) { beakFrame = w; beakHold = .05; } }
      if (phase === "thinking" && chatty(words)) { chatterTimer -= dt; if (chatterTimer <= 0) { chatterN++; say(workLine(words, chatterN)); chatterTimer = .9 + Math.random() * .5; } }
      for (const p of sparkle) { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += 120 * dt; p.life -= dt * 1.2; }
      for (let i = sparkle.length - 1; i >= 0; i--) if (sparkle[i].life <= 0) sparkle.splice(i, 1);

      drawChick({ hop: hopPx, sx, sy, blink, look, mood, beakF: beakFrame, sway, flap });

      // ── the pill capsule (opens with `openV`) ──
      ctx.clearRect(0, 0, W, H);
      const cx = W / 2, cy = H * 0.58;
      const capW = lerp(64, 210, openV), capH = lerp(20, 46, openV);
      const x0 = cx - capW / 2, y0 = cy - capH / 2;
      const acc = ACC[phase];
      // ambient glow when active
      if (openV > 0.05) { ctx.save(); ctx.shadowColor = `hsla(${acc}, ${.5 * openV})`; ctx.shadowBlur = 22 * openV; rr(x0, y0, capW, capH, capH / 2); ctx.fillStyle = "rgba(0,0,0,0.001)"; ctx.fill(); ctx.restore(); }
      // capsule body (obsidian, like the classic pill)
      const g = ctx.createLinearGradient(0, y0, 0, y0 + capH); g.addColorStop(0, "#17181c"); g.addColorStop(1, "#0c0c0f");
      ctx.fillStyle = g; rr(x0, y0, capW, capH, capH / 2); ctx.fill();
      ctx.lineWidth = 1; ctx.strokeStyle = openV > .05 ? `hsla(${acc}, ${.5})` : "rgba(255,255,255,0.12)"; rr(x0 + .5, y0 + .5, capW - 1, capH - 1, capH / 2); ctx.stroke();
      // clip to capsule, draw the chick (face-centered → rest shows only the face)
      ctx.save(); rr(x0 + 1, y0 + 1, capW - 2, capH - 2, (capH - 2) / 2); ctx.clip();
      ctx.imageSmoothingEnabled = false;
      const scale = 1.5, chW = PW * scale, chH = PH * scale;
      // face (buffer y≈15) sits at capsule centre
      ctx.drawImage(os, Math.round(cx - chW / 2), Math.round(cy - 15 * scale), Math.round(chW), Math.round(chH));
      // sparkles (done) inside the pill
      for (const q of sparkle) { ctx.save(); ctx.globalAlpha = Math.max(0, q.life); ctx.translate(cx + q.x, cy + q.y); ctx.fillStyle = q.heart ? "#ff6f9c" : "#ffd24a"; ctx.fillRect(-1, -1, 2, 2); ctx.restore(); }
      ctx.restore();

      // speech bubble above the capsule
      bubble.style.left = cx + "px";
      bubble.style.top = (y0 - 12) + "px";
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
