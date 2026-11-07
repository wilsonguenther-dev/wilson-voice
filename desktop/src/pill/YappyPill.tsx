/**
 * YappyPill — the pixel-chick companion, as an actual PILL.
 * At rest the capsule is a pill of SKY zoomed onto Yappy's tiny face. When you
 * dictate, a pull-back camera pans out so the pixel WORLD (sky + grass) fills the
 * ENTIRE capsule edge-to-edge — no black box-in-a-box. Longer prompts add props:
 * a paragraph → notepad + pencil in the wing; a few paragraphs → a desk + a pixel
 * typewriter + glasses (receptionist); an essay → glasses, filing the whole story.
 * Driven by the real app events (get_status/status/recording/audio_level/transcript),
 * the shared MouthDriver, and the tone data table (rude/friendly/rose).
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { MouthDriver } from "./mouth";
import { drawChick as drawChickArt } from "../sprites/art";
import { drawDeskHD, drawNotepadHD, growPadInk } from "../sprites/props";
import { YAPPY_CHATTER, YAPPY_PHASE_ART } from "./characters/yappy";
import { usePillDrag, reportPillHitbox, extendPillHitbox } from "./drag";
import MeetingBadge from "./MeetingBadge";
import GovernorChip from "./GovernorChip";
import PhaseAnnouncer from "./PhaseAnnouncer";
import { IDLE_MEETING, type MeetingAfterView, type MeetingStatus } from "./meeting";
import { bucketFor } from "./tone";
import { curseAllowed, pickTakeLine, rememberTakeLine, parseMemory, voiceFor, type TakeLineMemory, type VoiceId } from "./voices";
import {
  advanceLive, createLiveState, DEFAULT_STOP_KEY, framePlan, isListening, PHASE_ID, phaseAccessibleName, phaseVisual, progressFraction,
  progressLabel, progressNumeral, resetLive, transcribeLine,
  afterTakeLineAllowed, armGlance, GLANCE_LOOK_SEC, isNewUser, liveLinesLog, parseTakesSeen,
  type ChatTone, type FrameMode, type LivePhase, type LiveProp,
  type TranscribeProgress,
} from "./live";
import {
  GATED_GLYPH, GATED_TITLE, REVEAL_PURCHASE_COMMAND, pillLicenseRoute, splitValue,
  type PillLicense,
} from "./license";
// yap-1106-pill-trial-clickable — the gated pill is the Buy button: its route,
// copy, hover hold and the resting nub's lock come from one pure module.
import {
  GATE_BUY_LABEL, GATE_BUY_TOOLTIP, GATE_CLICK_COMMAND, GATE_LOCK_LABEL, GATE_PRESS_MS, YAPPY_HOP_EVENT, YAPPY_HOP_MS,
  GATE_HITBOX_OWNER_CHIP, GATE_HITBOX_OWNER_LOCK,
  gateChipCopy, gateClickArgs, nubLockVisible, setGateHover, type GateSurface,
} from "./gateClick";
import { PILL_HOVER_OUT_EVENT } from "./hitbox";
// Y5-D — the ONE duration table. Yappy's done-hold used to be a bare 1600 in
// two places; `no_duration_literal_outside_motion_ts` now forbids that shape.
import { DURATION } from "./motion";
// yap24-UI1 — the integer pixel grid: the canvas backing store, the camera blit
// and the dock anchor all come from one pure module, so every art pixel lands
// on whole device pixels at every dock and every backing scale.
import { backingScale, dockCentreX, toDevice } from "./pixelGrid";
// yap-1106-fb-10 — no character is ever cropped: the scene buffer, the pose
// range, the resting capsule and the camera all come from one pure rule.
import { CHICK_DESK_SHIFT, OPEN_CAPSULE, PILL_SCENE, REST_CAPSULE, clampChickPose, cropWarning, pillCamera } from "./characterFit";
// yap-1106-one-pill — the ONE layout (float-main publishes it): Yappy is drawn
// in the character box it places, and the bubble in the bubble box above it.
import { capsuleCentre, currentPillLayout } from "./state";
// yap-1106-pill-sky — the sky is the Home room's time of day (option C), one
// cached backdrop blitted once per redraw (pill/sky.ts).
import { SkyBackdrop, blitSky, skyForced } from "./sky";
// yap-1106-yappy-states-and-motion — the phase transition table, and the
// speech bubble's queue / placement / fixed copy.
import { phaseTransition, publishPhaseTransition } from "./transitions";
import { bubbleCentreX, createBubbleQueue, handsFreeExplainLine, type BubbleQueue } from "./bubble";

interface AppStatus { recording: boolean; busy: boolean; message: string }
// `speechSeconds` = the backend's energy-VAD speech time for the take (db.rs
// TranscriptEntry, camelCase) — what `live.lines` compares the estimate with.
interface Transcript { wordCount: number; text: string; speechSeconds?: number }
type Phase = LivePhase;
type Prop = LiveProp;

// ── tone-aware working chatter: Yappy's DATA (pill/characters/yappy, Y5-K) ──
const WORK = YAPPY_CHATTER;
// live chatter tone tracks the user's configured companion tone (rude/friendly/
// rose, YV27); anything unknown falls back to friendly (`toChatTone`, live.ts).
// The tone is read LIVE off the settings the float already subscribes to.
const chatty = (tone: ChatTone, w: number) => { const o = WORK[tone][bucketFor(w)]; return !!(o && o.length); };
const workLine = (tone: ChatTone, w: number, n: number) => { const o = WORK[tone][bucketFor(w)]; return o && o.length ? o[n % o.length] : ""; };
// yap-1106-voice-modes — take-line memory persistence (see `doneLine` below).
const TAKE_MEMORY_KEY = "yap.voice.recentTakes";
function loadTakeMemory(): TakeLineMemory {
  try { return parseMemory(globalThis.localStorage?.getItem(TAKE_MEMORY_KEY)); }
  catch (e) { console.warn("pill voice_memory load_failed", e); return parseMemory(null); }
}
function saveTakeMemory(m: TakeLineMemory): void {
  try { globalThis.localStorage?.setItem(TAKE_MEMORY_KEY, JSON.stringify(m)); }
  catch (e) { console.warn("pill voice_memory save_failed", e); }
}
// yap-1106-live-reactions-during-take — how many takes this pill has seen, so a
// NEW user (isNewUser) gets the one listening glance. Storage failing costs
// only the glance, never the take.
const TAKES_SEEN_KEY = "yap.live.takesSeen";
function loadTakesSeen(): number {
  try { return parseTakesSeen(globalThis.localStorage?.getItem(TAKES_SEEN_KEY)); }
  catch (e) { console.warn("pill live takes_seen load_failed", e); return 0; }
}
function saveTakesSeen(n: number): void {
  try { globalThis.localStorage?.setItem(TAKES_SEEN_KEY, String(n)); }
  catch (e) { console.warn("pill live takes_seen save_failed", e); }
}
const localDay = (): string => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};
// prop/persona keyed off the transcript word count
const propFor = (w: number): Prop => { const b = bucketFor(w); return b === "medium" ? "pad" : (b === "long" || b === "epic") ? "desk" : "none"; };

// ── LIVE escalation WHILE listening ── lives in live.ts: a PURE state machine
// (tier ladder + repeating chatter, speech measured against the take's own room
// tone) so the "does Yappy still react 4 minutes in?" question is a unit test.
// The canvas below only RENDERS what that machine decides.

class SOD {
  private k1: number; private k2: number; private k3: number; private xp: number; private yv: number; private yd = 0;
  constructor(f: number, z: number, r: number, x0 = 0) { const pf = Math.PI * f; this.k1 = z / pf; this.k2 = 1 / ((2 * pf) * (2 * pf)); this.k3 = r * z / (2 * pf); this.xp = x0; this.yv = x0; }
  update(dt: number, x: number): number { const xd = (x - this.xp) / Math.max(dt, 1e-4); this.xp = x; const k2 = Math.max(this.k2, dt * dt / 2 + dt * this.k1 / 2, dt * this.k1); this.yv += dt * this.yd; this.yd += dt * (x + this.k3 * xd - this.yv - this.k1 * this.yd) / k2; return this.yv; }
}
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/**
 * PERM-C — `gate` is the microphone permission phase, owned by `float-main`:
 * "blocked" (macOS is refusing and will not re-prompt) or "waiting" (the system
 * dialog is up). It outranks every take state, and it is BOTH a canvas phase
 * (so Yappy's accent goes muted) and a DOM overlay (the struck-through mic
 * glyph, which is also the button to the fix).
 */
export default function YappyPill(
  { gate = "idle", chordLabel = DEFAULT_STOP_KEY, tone = "friendly", voice, curse = false, progress = null, meeting = IDLE_MEETING, meetingAfter = null, onMeetingOpen, license = null }: {
    gate?: LivePhase;
    /** Y5-K — the companion tone, decided by the shell; this stage no longer reads settings. */
    tone?: ChatTone;
    /**
     * yap-1106-voice-modes — Yappy's voice, decided by the shell from the same
     * setting as `tone`. Omitted, it follows `tone` (friendly → Best friend,
     * rude → Badass, rose → Rose).
     */
    voice?: VoiceId;
    /**
     * yap-1106-curse-setting — curse mode as the shell decided it (toggle on,
     * Badass, no meeting / screen-share state). Off by default; re-checked at
     * the line against this capsule's own voice and meeting flag.
     */
    curse?: boolean;
    /**
     * yap24-PILL4 — the key that ends a hands-free take (`AppStatus.chord_label`),
     * decided by the SHELL. Only the accessible name and the announcement read
     * it here; the visible stop hint and lock chip are the shell's.
     */
    chordLabel?: string;
    progress?: TranscribeProgress | null;
    /**
     * YV95 / yap24-NT7 — the meeting, owned by the SHELL (float-main) like
     * `gate` and `progress`: the live status, and the post-stop view already
     * ranked below dictation. This capsule draws them; it decides neither.
     */
    meeting?: MeetingStatus;
    meetingAfter?: MeetingAfterView | null;
    onMeetingOpen?: (id: string) => void;
    /**
     * Y2-A — the license as the pill is allowed to say it, already decided by
     * `pillLicense` in float-main. Y2-D reads its ROUTE (`pillLicenseRoute`) so
     * this face sends a gated press to the SAME surface the classic capsule
     * does. Yappy's capsule is canvas-drawn and grows no DOM upgrade mark here,
     * but where a press GOES must not differ between two styles of one pill.
     */
    license?: PillLicense | null;
  },
) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  // Y5-K — the tone arrives from the shell; the mount-once draw loop reads it
  // through this sink instead of subscribing to settings itself.
  const toneRef = useRef<ChatTone>(tone);
  toneRef.current = tone;
  const toneSink = useRef<(t: ChatTone) => void>(() => {});
  useEffect(() => { toneSink.current(tone); }, [tone]);
  const voiceNow: VoiceId = voice ?? voiceFor(tone);
  const voiceSink = useRef<(v: VoiceId) => void>(() => {});
  const voiceRef = useRef<VoiceId>(voiceNow);
  voiceRef.current = voiceNow;
  useEffect(() => { voiceSink.current(voiceNow); }, [voiceNow]);
  // yap-1106-curse-setting — read by the mount-once draw loop at the line.
  const curseRef = useRef<boolean>(curse);
  curseRef.current = curse;
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  // yap-1106-yappy-states-and-motion — the clamped text lives in its own span
  // so the 2-line clamp's overflow never clips the bubble's tail.
  const bubbleTextRef = useRef<HTMLSpanElement | null>(null);
  // The draw loop reads the gate through this (hands-free pose), and the gate
  // effect speaks through `sayRef` (the queue lives in the mount-once scene).
  const gateRef = useRef<LivePhase>(gate);
  gateRef.current = gate;
  const sayRef = useRef<(t: string) => void>(() => {});
  const prevGateRef = useRef<LivePhase>(gate);
  // The last hands-free explanation, so a `listening` (re)entry that lands
  // after the gate change keeps it on screen instead of blanking the bubble.
  const handsFreeLineRef = useRef<string>("");
  // YV65 — press-and-drag the capsule to re-dock the pill. Yappy's capsule is
  // CANVAS-drawn (the element itself fills the whole transparent window), so its
  // hit-box is published from the draw loop below, not from a DOM rect.
  // yap-wf-01-click-the-pill-to — WF-01: the same gesture is also the CLICK.
  // `usePillDrag` classifies every release (pill/press.ts: under 4 points and
  // under 300 ms) and calls `tap` only for a click, so a drag or a long hold
  // never toggles a take. The handler is assigned below, once the routes it
  // shares with the overlay buttons exist; the hook reads it at call time.
  const tapRef = useRef<() => void>(() => {});
  const drag = usePillDrag({ tap: () => tapRef.current() });
  // PERM-C — set by the draw effect; called when the gate prop changes.
  const applyGateRef = useRef<(g: LivePhase) => void>(() => {});
  // Y3-C — decode progress reaches the mount-once scene the same way the gate
  // does: through a ref, because the canvas loop is not re-created per render.
  const applyProgressRef = useRef<(p: TranscribeProgress | null) => void>(() => {});
  const gateVisual = phaseVisual(gate, chordLabel);
  const blocked = gateVisual.needsPermission;
  // Y2-C — the trial ended and this press was refused. Same "the press produced
  // nothing" shape as `blocked`, different reason and a different destination:
  // the fix is a license, not a macOS grant.
  const gated = gate === "gated";
  const openPermissions = useCallback(() => {
    if (drag.dragged()) return;
    // A blocked capsule is a BUTTON to the fix: raise the main window and land
    // on the Permissions screen (PERM-B), which owns the Settings deep link.
    invoke("show_main").catch(() => {});
    emit("navigate", "permissions").catch(() => {});
  }, [drag]);
  const licenseRoute = pillLicenseRoute(license);
  const openPurchase = useCallback(() => {
    if (drag.dragged()) return;
    // Y2-D — the policy decides, not this component. `reveal_purchase` raises
    // the main window AND the PurchasePrompt sheet (price, founding code,
    // seats, keep-forever line); anything else — Y2-F's `problem` tone, a
    // paying customer with a broken key, or no license payload yet — lands on
    // Settings → License, which is correct under every state. The classic
    // capsule routes identically.
    if (licenseRoute === "reveal_purchase") {
      invoke(REVEAL_PURCHASE_COMMAND).catch(() => {});
      return;
    }
    invoke("show_main").catch(() => {});
    emit("navigate", "settings").catch(() => {});
    emit("settings-tab", "license").catch(() => {});
  }, [drag, licenseRoute]);
  // yap-1106-pill-trial-clickable — a gated click opens the purchase page in the
  // browser (a stored-key problem gets Settings → License instead; the policy
  // in gateClick.ts decides). The pill sends a SURFACE and a TARGET NAME, never
  // a URL: the backend logs `pill gate.click` and owns the destination. If the
  // backend cannot open the browser, the press still lands somewhere useful —
  // the in-app purchase / license surface above.
  //
  // pill.html §13 "Click": the pressed chip sinks for GATE_PRESS_MS and reads
  // "Opening drivia.consulting…" until the refusal settles. The nub's lock has
  // no chip to relabel, so it only opens.
  const [gateOpening, setGateOpening] = useState(false);
  const [gatePressed, setGatePressed] = useState(false);
  const pressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (pressTimer.current !== null) clearTimeout(pressTimer.current);
  }, []);
  const openGatePurchase = useCallback((surface: GateSurface) => {
    if (drag.dragged()) return;
    if (surface !== "nub") {
      setGateOpening(true);
      setGatePressed(true);
      if (pressTimer.current !== null) clearTimeout(pressTimer.current);
      pressTimer.current = setTimeout(() => {
        pressTimer.current = null;
        setGatePressed(false);
      }, GATE_PRESS_MS);
    }
    invoke(GATE_CLICK_COMMAND, gateClickArgs(license, surface)).catch((e) => {
      console.warn("pill gate.click failure=invoke", e);
      setGateOpening(false);
      openPurchase();
    });
  }, [drag, license, openPurchase]);
  // Hover over the gated pill: the chip says "Buy a license", and the refusal
  // holds for as long as the pointer rests on it (float-main owns the timer).
  const [gateHover, setGateHoverLocal] = useState(false);
  const hoverGate = useCallback((on: boolean) => {
    setGateHoverLocal(on);
    setGateHover(on);
  }, []);
  useEffect(() => {
    if (!gated) {
      hoverGate(false);
      setGateOpening(false);
    }
  }, [gated, hoverGate]);
  useEffect(() => {
    // The panel goes click-through the moment the cursor leaves the hitbox, so
    // the webview never sees a pointerleave; the backend says so instead.
    let dead = false;
    let un: (() => void) | null = null;
    listen(PILL_HOVER_OUT_EVENT, () => hoverGate(false))
      .then((u) => (dead ? u() : (un = u)))
      .catch(() => {});
    return () => { dead = true; un?.(); };
  }, [hoverGate]);
  // A license arrived by deep link from the site: Yappy hops once (a CSS class
  // on the stage; reduced motion gets no hop, float.css).
  const [hop, setHop] = useState(false);
  useEffect(() => {
    let dead = false;
    let un: (() => void) | null = null;
    let t: ReturnType<typeof setTimeout> | null = null;
    listen(YAPPY_HOP_EVENT, () => {
      setHop(true);
      if (t !== null) clearTimeout(t);
      t = setTimeout(() => { t = null; setHop(false); }, YAPPY_HOP_MS);
    })
      .then((u) => (dead ? u() : (un = u)))
      .catch(() => {});
    return () => { dead = true; un?.(); if (t !== null) clearTimeout(t); };
  }, []);
  const showLock = nubLockVisible(license, gated, blocked);
  // pill.html §13, frame by frame (refused / hover / pressed).
  const gateChip = gateChipCopy(gateVisual.label, gateHover, gateOpening);
  const gateChipClass = ["kami-blocked gated", gateHover ? "is-hover" : "", gatePressed ? "is-pressed" : ""]
    .filter(Boolean)
    .join(" ");
  // WF-01 — a click on the canvas capsule. A refused pill routes exactly where
  // its overlay button does (licence first when gated, then the mic fix);
  // otherwise the backend toggles a HANDS-FREE take, and on the stop click it
  // hands focus back to the app that had it before the click, then pastes.
  // The backend also owns the busy guard (a click mid-transcription is a no-op)
  // and logs every outcome (`pill click action=…`).
  tapRef.current = () => {
    if (gated) { openGatePurchase("pill"); return; }
    if (blocked) { openPermissions(); return; }
    invoke("pill_click_toggle").catch(() => {
      // A refused start (trial over) already raised its own toast backend-side.
    });
  };
  // …and it is also an INPUT to the frame policy (OS-12 fix 1): a meeting keeps
  // the pill visible for hours, so the canvas parks instead of holding the 10fps
  // ambient tick open for the whole session. The draw loop lives in a mount-once
  // effect, so it reads the flag through a ref rather than closing over a value
  // that was captured before the meeting existed. `wakeRef` is the other half:
  // parking is only safe if something un-parks it when the meeting ends.
  const meetingRef = useRef(false);
  const wakeRef = useRef<() => void>(() => {});
  // Y2-B (8d85cdf) — the draw loop is mounted ONCE, so the license reaches it
  // through a ref. What the canvas takes from it is a POSTURE: on the last day
  // (and once the trial has ended, or a key is in trouble) Yappy's head tufts
  // fold down and his blinks come further apart. `trial` and `trial-quiet`
  // keep him upright — a running trial is not news.
  const droopRef = useRef(false);
  const droop = license?.show === true && license.tone !== "trial" && license.tone !== "trial-quiet";
  useEffect(() => {
    droopRef.current = droop;
    // A settled scene has PARKED its rAF (YV81); wake it so the pose is painted.
    wakeRef.current();
  }, [droop]);
  useEffect(() => {
    const recording = meeting.recording === true;
    const was = meetingRef.current;
    meetingRef.current = recording;
    if (was && !recording) wakeRef.current();   // meeting over → Yappy gets his life back
  }, [meeting.recording]);

  useEffect(() => {
    const cv = canvasRef.current!, bubble = bubbleRef.current!;
    const ctx = cv.getContext("2d")!;
    // prefers-reduced-motion: render ONE calm static frame (resting pill, mouth
    // closed, level 0) instead of the 60fps loop — matches YappyHouse.
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    // ── wide scene buffer: the world fills its full width so it fills the pill ──
    // yap-1106-fb-10 — the world is big enough to cover the widest open capsule at
    // ONE device px per art px, with sky above him for a hop (characterFit PILL_SCENE).
    const PW = PILL_SCENE.w, PH = PILL_SCENE.h, GY = PILL_SCENE.groundY, FX = PILL_SCENE.faceX;   // grass starts at GY
    const os = document.createElement("canvas"); os.width = PW; os.height = PH; const octx = os.getContext("2d")!;
    const C: Record<string, string> = {
      sun: "#fff2b0",
      body: "#ffd24a", light: "#ffe9a0", dark: "#eba21f", outline: "#6b3a2e", cavity: "#7a2e14", belly: "#fff2c4",
      beak: "#ff9f1c", beakD: "#e07a12", eye: "#3a2f36", white: "#ffffff", blush: "#ff89a8", foot: "#f2861a", tongue: "#ff5d7a", sprout: "#e8a41f",
      paper: "#fffdf5", paperL: "#c9c3ad", desk: "#b06a3a", deskD: "#8a4f28", glass: "#2c2a36",
      pencil: "#ffcf3a", pencilT: "#ff8fb0", tw: "#4a4a55", twD: "#33333d", twKey: "#d8d8e2",
    };
    // yap-1106-pill-sky — the sky, sun/moon and grass for the Home room's time of
    // day, painted into its OWN cached surface only when the time of day changes
    // (at most four times a day; the clock is read at most every 30 s). Each
    // redraw is ONE drawImage of it (`blitSky`). A dev/matrix build may pin it
    // with `float.html?sky=dawn|day|dusk|night`; <html data-sky> says which is up.
    const skyCanvas = document.createElement("canvas"); skyCanvas.width = PW; skyCanvas.height = PH;
    const forcedSky = (import.meta.env.DEV || __YAP_PILL_MATRIX__) ? skyForced(new URLSearchParams(window.location.search).get("sky")) : null;
    const sky = new SkyBackdrop<HTMLCanvasElement>(
      { image: skyCanvas, ctx: skyCanvas.getContext("2d")! }, PW, PH, GY, () => new Date(), forcedSky,
      (look, builds) => {
        document.documentElement.dataset.sky = look.time;
        console.info(`pill sky_change time=${look.time} builds=${builds} forced=${forcedSky ? 1 : 0}`);
      },
    );
    // the capsule's own fill follows the same sky; one gradient, rebuilt only
    // when the time of day or the capsule's box changes
    let capGrad: CanvasGradient | null = null, capGradKey = "";

    interface SP { hop: number; sx: number; sy: number; blink: number; mood: number; beakF: number; sway: number; flap: number; world: number; prop: Prop; propIn: number; look: "down" | "fwd"; glasses: boolean; pencil: boolean; droop: boolean; handsFree: boolean; ink: number; elapsed: number }

    // the chick — front wing holds a pencil when noting / at the desk
    // FB-09 — the chick is drawn by sprites/art.ts so the snapshot test
    // (sprites/art.snapshot.test.ts) pixel-checks the code the pill runs.
    function drawChick(cx: number, feetY: number, A: SP) { drawChickArt(octx, cx, feetY, A); }

    function drawScene(A: SP) {
      octx.clearRect(0, 0, PW, PH);
      const w = A.world;
      // yap-1106-pill-sky — the whole backdrop in ONE blit (no drifting clouds:
      // nothing in the sky moves between time-of-day changes).
      blitSky<HTMLCanvasElement>(octx, sky, performance.now(), Math.max(0.8, w));
      const cx = FX - (A.prop === "desk" ? CHICK_DESK_SHIFT : 0), feet = GY;
      // notepad slides in from the side when noting — yap-1106-pill-props-detail:
      // the approved detail pass (clip, margin line, handwriting that grows).
      if (A.prop === "pad") drawNotepadHD(octx, cx, GY, A.propIn, C.paper, A.ink);
      // yap-1106-fb-10 — the pose is clamped into the range his bounding box promises.
      drawChick(cx, feet, clampChickPose(A));
      // receptionist desk + a little pixel TYPEWRITER — yap-1106-pill-props-detail:
      // wood grain + edge highlight, carriage, return levers, keys that light in
      // sequence, a bouncing page, and a mug + pencils on the left.
      if (A.prop === "desk") drawDeskHD(octx, cx, GY, PW, A.propIn, A.elapsed, C.paper);
    }

    // yap24-UI1 — the backing store is WHOLE device px of the CSS box at the REAL
    // backing scale (no clamp: a store smaller than the device grid is upscaled by
    // a non-integer ratio). The DPR is re-read every frame, so dragging the pill to
    // a display with another backing scale re-grids it without a resize event.
    let W = 0, H = 0, DPR = backingScale(devicePixelRatio);
    // yap-1106-fb-10 — one log line per mount if the camera ever could not keep him whole.
    let warnedCrop = false;
    const resize = () => {
      DPR = backingScale(devicePixelRatio);
      const r = cv.getBoundingClientRect(); W = r.width; H = r.height;
      cv.width = toDevice(W, DPR); cv.height = toDevice(H, DPR);
      ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
    };
    resize(); const ro = new ResizeObserver(resize); ro.observe(cv);

    let phase: Phase = "idle";
    const R = { sway: new SOD(2.2, .35, 1.4), open: new SOD(3.4, .7, 1.4), prop: new SOD(3, .7, 1.2) };
    const J = { phase: "ground", t: 0, y: 0, vy: 0, power: 0, crouch: 0 };
    const hop = (p: number) => { if (J.phase === "ground" || J.phase === "land") { J.phase = "crouch"; J.t = 0; J.power = p; } };
    const mouth = new MouthDriver();
    let blinkT = -1, nextBlink = 1.4, elapsed = 0, tPrev = 0, mood = 0, moodT = 0, idleFor = 0, level = 0, beakFrame = 0, beakHold = 0;
    let chatterTimer = 0, words = 0, doneUntil = 0, openV = 0, propV = 0;
    // YV71 — the live commentary state machine (live.ts) owns the tier ladder,
    // the speech estimate and the chatter schedule for the whole take.
    const live = createLiveState();
    let liveProp: Prop = "none";
    // yap-1106-live-reactions-during-take — the take's line tally. `afterLines`
    // caps the post-release workLine chatter (AFTER_TAKE_MAX_LINES); `takeOpen`
    // makes the `live.lines` telemetry fire exactly once per take; `glanceT` is
    // the remaining eyes-forward time of a new user's listening glance.
    let afterLines = 0, takeOpen = false, glanceT = 0;
    // yap-1106-pill-props-detail — how much of the notepad page is written (0..1):
    // it grows while he notes during a take and clears once the pad has gone.
    // `shownProp` makes the `pill prop_change` line fire once per change, not per frame.
    let padInk = 0, shownProp: Prop = "none";
    let takesSeen = loadTakesSeen();
    const logLiveLines = (vadSec: number | null) => {
      if (!takeOpen) return;
      takeOpen = false;
      console.info(liveLinesLog({ during: live.lines, after: afterLines, voicedSec: live.voicedT, vadSec }));
    };
    const afterLine = (): string => {
      if (!chatty(chatTone, words) || !afterTakeLineAllowed(afterLines)) return "";
      const line = workLine(chatTone, words, afterLines);
      afterLines++;
      return line;
    };
    // YV27 — live companion tone, read off settings (not a hardcoded constant).
    // Seeded from get_settings + kept current via the "settings" event the float
    // already emits on save, so a tone change takes effect without a remount.
    let chatTone: ChatTone = toneRef.current;
    toneSink.current = (t) => { chatTone = t; };
    // yap-1106-voice-modes — the voice and the take-line memory. The memory is
    // the last 20 finished-take lines (no repeats inside it), kept in this
    // webview's localStorage so it survives a restart; storage failing only
    // costs the memory, never the line.
    let liveVoice: VoiceId = voiceRef.current;
    voiceSink.current = (v) => { liveVoice = v; };
    let takeMemory: TakeLineMemory = loadTakeMemory();
    let takeCount = takeMemory.recent.length;
    const doneLine = (w: number): string => {
      const today = localDay();
      // Belt and braces: the shell already gated `curse`; the line re-checks
      // this capsule's live voice and meeting flag so a stale prop can never
      // swear in Professional or during a meeting.
      const curseNow = curseAllowed({ enabled: curseRef.current, voice: liveVoice, meeting: meetingRef.current, screenShared: false });
      const picked = pickTakeLine(w, liveVoice, takeMemory, { nonce: takeCount, today, curse: curseNow, rand: Math.random });
      takeCount++;
      takeMemory = rememberTakeLine(takeMemory, picked, today);
      saveTakeMemory(takeMemory);
      console.info(`pill voice_line voice=${liveVoice} band=${picked.band} source=${picked.source} curse=${curseNow} recent=${takeMemory.recent.length}`);
      return picked.text;
    };
    // yap-1106-yappy-states-and-motion — ONE bubble at a time: a new line waits
    // for the old one to leave (bubble.ts), timings from DURATION.
    const bubbleText = bubbleTextRef.current ?? bubble;
    const bubbleQ: BubbleQueue = createBubbleQueue(
      {
        setText: (t) => { bubbleText.textContent = t; },
        setShown: (on) => { bubble.classList.toggle("show", on); },
      },
      { after: (ms, fn) => window.setTimeout(fn, ms), cancel: (id) => window.clearTimeout(id) },
      { reduceMotion: reduce },
    );
    const say = (t: string) => bubbleQ.say(t);
    sayRef.current = say;

    // PERM-C — a Record over the WHOLE phase union, so a variant added to
    // `LivePhase` without an accent here is a compile error, never a blank pill.
    // The placeholder phases borrow the nearest shipped accent until the item
    // that owns them draws its own scene; the two refusal phases are grey on
    // purpose — a muted capsule is the whole visual point.
    const ACC: Record<Phase, string> = {
      idle: "351 95% 71%", listening: "351 95% 71%", "listening-locked": "351 95% 71%", thinking: "38 92% 55%",
      done: "152 69% 52%", sleepy: "230 20% 60%",
      blocked: "0 0% 55%", waiting: "0 0% 55%", gated: "38 92% 55%",
      transcribing: "38 92% 55%", polishing: "38 92% 55%", pasting: "38 92% 55%",
      error: "0 72% 58%", "model-loading": "38 92% 55%", empty: "230 20% 60%",
      // Y3-D — the SAME muted slate the two calm non-take phases use. A
      // cancelled take is not a failure, so it deliberately does not borrow the
      // red `error` accent it would be easiest to reach for.
      cancelled: "230 20% 60%",
      // yap24-PILL3 — the calm slate: fn is blocked, dictation is not.
      blind: "230 20% 60%",
    };
    // listening → prop comes from the live state machine (escalates with actual
    // speech); thinking/done → keyed off the transcript word count
    const wantPropFor = (p: Phase, w: number): Prop =>
      p === "listening" ? liveProp
        : (p === "thinking" || p === "done") ? propFor(w)
          : "none";

    function setPhase(p: Phase) {
      // YV71 — re-entering the phase we are ALREADY in is not a restart. Every
      // `status` emit while recording lands here (the YV43 secure-input watchdog
      // edge, the hands-free toggle), and each one used to rewind the live
      // ladder to `quick` and blank the bubble mid-sentence. `done` is exempt:
      // a fresh transcript re-fires its celebration.
      if (p === phase && p !== "done") return;
      // A take that ends WITHOUT a transcript (cancelled, empty, failed) still
      // reports its live.lines — with no VAD time to compare against.
      if (p === "listening" || (p !== "thinking" && p !== "transcribing" && p !== "done")) logLiveLines(null);
      phase = p; idleFor = 0; chatterTimer = 0; liveProp = "none"; glanceT = 0;
      wake();
      if (p === "listening") {
        moodT = 0; hop(1); say(gateRef.current === "listening-locked" ? handsFreeLineRef.current : ""); resetLive(live);
        if (isNewUser(takesSeen)) armGlance(live);
        takesSeen++; saveTakesSeen(takesSeen);
        afterLines = 0; takeOpen = true;
      }
      else if (p === "thinking") { moodT = 0; say(afterLine()); }
      else if (p === "done") { moodT = 1; hop(1.2); burst(); say(doneLine(words)); }
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
    const toIdle = () => { if (Date.now() > doneUntil) setPhase("idle"); };
    listen<AppStatus>("status", (e) => { const s = e.payload; if (s.recording) setPhase("listening"); else if (s.busy) setPhase("thinking"); else toIdle(); }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<boolean>("recording", (e) => { if (e.payload) { setPhase("listening"); mouth.reset(); } }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<number>("audio_level", (e) => { const v = typeof e.payload === "number" ? e.payload : 0; level = Math.max(0, Math.min(1, v)); if (level >= 0.02) wake(); }, { target: "float" }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV81 — the pill window's own visibility. Two sources, because neither is
    // complete on its own: `pill_visible` is the panel being ordered out/in by
    // the backend (float_pill::set_shown), and `visibilitychange` is the webview
    // reporting itself occluded/minimised. Hidden, the loop parks entirely —
    // nothing renders for a window nobody can see; visible, it wakes.
    let shown = true, hidden = document.hidden;
    const onVisibility = () => {
      const was = hidden;
      hidden = document.hidden || !shown;
      if (was && !hidden) wake();
    };
    listen<boolean>("pill_visible", (e) => { shown = e.payload !== false; onVisibility(); }).then((u) => (dead ? u() : unsubs.push(u)));
    document.addEventListener("visibilitychange", onVisibility);
    listen<Transcript>("transcript", (e) => {
      words = e.payload?.wordCount ?? 0;
      const vad = e.payload?.speechSeconds;
      logLiveLines(typeof vad === "number" && Number.isFinite(vad) ? vad : null);
      setPhase("done"); doneUntil = Date.now() + DURATION.doneHold; window.setTimeout(() => { if (phase === "done") setPhase("idle"); }, DURATION.doneHold); }).then((u) => (dead ? u() : unsubs.push(u)));

    function rr(x: number, y: number, w: number, h: number, r: number) { ctx.beginPath(); ctx.moveTo(x + r, y); ctx.arcTo(x + w, y, x + w, y + h, r); ctx.arcTo(x + w, y + h, x, y + h, r); ctx.arcTo(x, y + h, x, y, r); ctx.arcTo(x, y, x + w, y, r); ctx.closePath(); }

    let raf = 0, tick = 0, lastDraw = 0;
    // YV81 — is every transient finished, so that what is LEFT to draw is only
    // ambient life (breathing, sway, drifting clouds, the next blink)? The
    // capsule shut, no prop, no sparkle, both feet on the ground and the mood
    // caught up. Springs are asymptotic, so "at rest" is a threshold, not zero.
    // `blinkT < 0` = no blink in flight. It matters now that a park can happen
    // while the pill is ON SCREEN (a meeting): a frame frozen 60ms into a blink
    // would hold a half-shut eye for the rest of the meeting. Under YV81's
    // parks this only ever delayed the ambient tick by ~130ms.
    const settledNow = () =>
      openV < 0.01 && propV < 0.01 && sparkle.length === 0 && blinkT < 0
      && J.phase === "ground" && Math.abs(moodT - mood) < 0.01;
    // The schedule comes from `framePlan` (live.ts, unit-tested): FULL rate the
    // moment a take is live, ~18fps while an idle scene is still settling, then
    // a parked rAF with ambient life on a 10fps timer once it has — and nothing
    // at all while the pill is hidden, under reduced motion, or holding a
    // settled pose through a meeting (OS-12). Never a park while a take is live,
    // where the mouth follows the mic and the commentary runs on a schedule that
    // needs frames to advance — dictation DURING a meeting animates normally,
    // because the live-take branch is answered before anything can park.
    function loop(ts: number) {
      const plan = framePlan(phase, {
        level,
        busyVisuals: sparkle.length > 0 || J.phase !== "ground",
        reduceMotion: reduce,
        settled: settledNow(),
        hidden,
        meetingRecording: meetingRef.current,
      });
      const mode: FrameMode = plan.mode;
      // schedule the next wake FIRST, so nothing below can strand the loop
      raf = 0; tick = 0;
      if (mode === "raf") raf = requestAnimationFrame(loop);
      else if (mode === "ambient") tick = window.setTimeout(() => loop(performance.now()), plan.intervalMs);
      if (mode === "raf" && plan.intervalMs > 0 && lastDraw && ts - lastDraw < plan.intervalMs) return;   // skip this redraw, keep the rAF alive
      lastDraw = ts;
      // dt drives the animation (clamped for stability); wallDt drives the live
      // state machine, so a throttled frame still advances the take's clock. The
      // clamp rides the schedule: on the ambient tick a whole frame IS 100ms, and
      // clamping that to 50 would run Yappy's breathing at half speed (the springs
      // it protects are settled by definition before that tick can start).
      const wallDt = Math.min(.25, (ts - tPrev) / 1000 || 0);
      const dt = Math.min(mode === "ambient" ? .12 : .05, wallDt); tPrev = ts; elapsed += dt; idleFor += dt;
      if (phase === "idle" && idleFor > 12) setPhase("sleepy");
      if (blinkT < 0) { nextBlink -= dt; if (nextBlink <= 0) blinkT = 0; }
      let blink = 0; if (blinkT >= 0) { blinkT += dt; const d = .13; blink = blinkT < d / 2 ? blinkT / (d / 2) : blinkT < d ? 1 - (blinkT - d / 2) / (d / 2) : 0; if (blinkT > d) { blinkT = -1; nextBlink = (2 + Math.random() * 4) * (droopRef.current ? 1.6 : 1); } }
      const active = phase === "listening" || phase === "thinking" || phase === "done";
      openV = R.open.update(dt, active ? 1 : 0); openV = Math.max(0, Math.min(1, openV));
      // live escalation WHILE talking — the pure machine decides the tier, the
      // prop and WHEN Yappy says something (it keeps reacting for the whole take,
      // measuring speech against this take's own room tone); the canvas renders it.
      if (phase === "listening") {
        const f = advanceLive(live, wallDt, level, chatTone, undefined, liveVoice);
        liveProp = f.prop;
        if (f.say) say(f.say);
        // a new user's one listening glance: eyes up at you, a small hop
        if (f.glance) { glanceT = GLANCE_LOOK_SEC; hop(.8); }
      }
      if (glanceT > 0) glanceT = Math.max(0, glanceT - dt);
      const wantProp = wantPropFor(phase, words);
      propV = R.prop.update(dt, wantProp !== "none" ? 1 : 0); propV = Math.max(0, Math.min(1, propV));
      if (wantProp === "pad" && phase === "listening") padInk = growPadInk(padInk, dt);
      else if (wantProp !== "pad" && propV < 0.01) padInk = 0;
      if (wantProp !== shownProp) { console.info(`pill prop_change from=${shownProp} to=${wantProp} detail=hd`); shownProp = wantProp; }
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
      // after release the chatter is CAPPED (AFTER_TAKE_MAX_LINES): the bubble keeps
      // its last line instead of rotating every second while the take decodes.
      if (phase === "thinking" && chatty(chatTone, words) && afterTakeLineAllowed(afterLines)) { chatterTimer -= dt; if (chatterTimer <= 0) { say(afterLine()); chatterTimer = .9 + Math.random() * .5; } }
      for (const p of sparkle) { p.x += p.vx * dt; p.y += p.vy * dt; p.vy += 120 * dt; p.life -= dt * 1.2; }
      for (let i = sparkle.length - 1; i >= 0; i--) if (sparkle[i].life <= 0) sparkle.splice(i, 1);

      const lookMode: "down" | "fwd" = glanceT > 0 ? "fwd" : (wantProp !== "none" && propV > .5) ? "down" : "fwd";
      // glasses ride the desk prop (desk/essay tiers), pencil rides the notepad (notes tier) — for both live + transcript tiers
      const glasses = wantProp === "desk" && propV > .5 && phase !== "idle";
      const pencil = wantProp === "pad" && propV > .4 && phase !== "idle";
      drawScene({ hop: hopPx, sx, sy, blink, mood, beakF: beakFrame, sway, flap, world: openV, prop: wantProp, propIn: propV, look: lookMode, glasses, pencil, droop: droopRef.current, handsFree: gateRef.current === "listening-locked", ink: padInk, elapsed });

      // ── the capsule + pull-back camera (world fills the whole pill) ──
      if (backingScale(devicePixelRatio) !== DPR) resize();   // moved to another display
      ctx.clearRect(0, 0, W, H);
      const tierOpen = (wantProp === "desk") ? 1.16 : (wantProp === "pad") ? 1.05 : 1;
      // bigger open capsule so the chick + grass read larger
      // yap-1106-fb-10 — the resting capsule holds a WHOLE character (characterFit REST_CAPSULE).
      const capW = lerp(REST_CAPSULE.w, OPEN_CAPSULE.w * tierOpen, openV), capH = lerp(REST_CAPSULE.h, OPEN_CAPSULE.h * tierOpen, openV);
      // YV53 — dock edge (read live off <html data-dock>, set by float-main): the
      // window is parked flush to that screen edge, so the capsule hugs it instead
      // of sitting mid-window. ONLY the anchor moves — capsule size and the camera
      // below are untouched, so the world keeps its aspect (never letterboxed).
      // yap-1106-one-pill — the anchor is the published layout's character box
      // (pill/state.ts), the same box the clock, hint and bubble are placed
      // against and the panel is sized around. Before the first layout lands
      // (one frame at mount) the YV53 dock anchor stands in.
      const lay = currentPillLayout();
      const cc = lay ? capsuleCentre(lay, capW) : null;
      const cx = cc ? cc.x : dockCentreX(document.documentElement.dataset.dock, W, capW);
      const cy = cc ? cc.y : H * 0.56;
      const x0 = cx - capW / 2, y0 = cy - capH / 2, acc = ACC[phase];
      // YV65 — the capsule IS the grab handle: publish exactly where it was just
      // drawn (canvas coords are logical points here — the DPR lives in the
      // context transform) so the panel only takes the cursor over the pill.
      reportPillHitbox(x0, y0, capW, capH);
      // ambient glow when active
      if (openV > .05) { ctx.save(); ctx.shadowColor = `hsla(${acc}, ${.5 * openV})`; ctx.shadowBlur = 22 * openV; rr(x0, y0, capW, capH, capH / 2); ctx.fillStyle = "rgba(0,0,0,0.001)"; ctx.fill(); ctx.restore(); }
      // the capsule fill is the same time-of-day sky (not obsidian)
      const look = sky.look, gKey = `${look ? look.time : "-"}|${y0}|${capH}`;
      if (!capGrad || gKey !== capGradKey) {
        capGrad = ctx.createLinearGradient(0, y0, 0, y0 + capH);
        capGrad.addColorStop(0, look ? look.top : "#8fd6ff"); capGrad.addColorStop(1, look ? look.bot : "#c6ecff");
        capGradKey = gKey;
      }
      ctx.fillStyle = capGrad; rr(x0, y0, capW, capH, capH / 2); ctx.fill();
      ctx.save(); rr(x0 + 1, y0 + 1, capW - 2, capH - 2, (capH - 2) / 2); ctx.clip(); ctx.imageSmoothingEnabled = false;
      // CAMERA (yap-1106-fb-10): centred on Yappy's whole bounding box — tufts,
      // wings, tail, a hop — at the largest WHOLE number of device px per art px
      // that keeps all of it inside the capsule's rounded clip (never zoomed onto
      // his face, which cropped his head at rest). The leftover margin is more of
      // the world at the same cell, never a stretch; the buffer covers every
      // capsule at that cell, so it is never letterboxed. Drawn under an identity
      // transform (the clip above is already in device space).
      const cam = pillCamera({ x: x0, y: y0, w: capW, h: capH }, DPR);
      if (!cam.fits && !warnedCrop) { warnedCrop = true; console.warn(cropWarning("yappy", { w: capW, h: capH }, DPR)); }
      const blit = cam.blit;
      ctx.save(); ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.drawImage(os, blit.src.x, blit.src.y, blit.src.w, blit.src.h, blit.dst.x, blit.dst.y, blit.dst.w, blit.dst.h);
      // sparkles (done) inside the pill — 2 CSS px squares, snapped to whole device px
      const sp = Math.max(1, Math.round(2 * DPR));
      for (const q of sparkle) { ctx.globalAlpha = Math.max(0, q.life); ctx.fillStyle = q.heart ? "#ff6f9c" : "#ffd24a"; ctx.fillRect(Math.round((cx + q.x - 1) * DPR), Math.round((cy + q.y - 1) * DPR), sp, sp); }
      ctx.restore();
      ctx.restore();
      ctx.lineWidth = 1; ctx.strokeStyle = openV > .05 ? `hsla(${acc}, ${.55})` : "rgba(255,255,255,0.12)"; rr(x0 + .5, y0 + .5, capW - 1, capH - 1, capH / 2); ctx.stroke();

      // speech bubble above the capsule — centred on it, but kept inside the
      // window so a docked (edge-hugging) capsule doesn't push it off-screen.
      const bw = bubble.classList.contains("show") ? bubble.offsetWidth : 0;
      if (lay) {
        // yap-1106-one-pill — kept inside its own reserved row: never over the
        // character, the hint or the clock, and never outside the panel.
        const bb = lay.bubble;
        bubble.style.left = bubbleCentreX(cx, bw, bb, document.documentElement.dataset.dock) + "px";
        bubble.style.top = (bb.y + bb.h) + "px";
      } else {
        bubble.style.left = bubbleCentreX(cx, bw, { x: 4, w: W - 8 }, document.documentElement.dataset.dock) + "px";
        bubble.style.top = (y0 - 10) + "px";
      }
    }
    // A parked loop paints one calm frame and stops scheduling; anything that
    // gives it something to draw again — a phase change (a take starting), the
    // pill coming back on screen, a live mic level — wakes it, so it is never
    // asleep while recording. Re-entrant by design: `wake` is also reached from
    // inside a frame (setPhase), where a wake is already scheduled.
    function wake() { if (raf || tick) return; tPrev = 0; lastDraw = 0; raf = requestAnimationFrame(loop); }
    // The meeting effect above holds the only reference that can un-park a loop
    // parked BY a meeting: nothing else fires when a meeting ends.
    wakeRef.current = wake;
    // PERM-C — the permission phase arrives as a PROP (float-main owns it) while
    // the scene lives in this mount-once effect, so it is applied through a ref,
    // the same way the meeting flag is. Clearing only ever unwinds a permission
    // phase: it must never yank a live take back to idle.
    applyGateRef.current = (g: LivePhase) => {
      // Y2-C — `gated` joins the two refusal phases here: it is the same kind of
      // thing (a press that produced no take) and it unwinds the same way, so
      // the scene must be able to enter AND leave it. Clearing still only ever
      // unwinds a refusal — it must never yank a live take back to idle.
      if (g === "blocked" || g === "waiting" || g === "gated") setPhase(g);
      else if (phase === "blocked" || phase === "waiting" || phase === "gated") setPhase("idle");
    };
    // Y3-C — every accepted chunk. `setPhase` is a no-op on re-entry (YV71), so
    // the LINE is said here rather than in setPhase: the point of the phase is
    // that Yappy's commentary escalates on the words REALLY decoded so far, not
    // on `wordsFromVoiced`'s stopwatch estimate. A permission refusal still
    // outranks it, and a live take is never yanked out from under the mic.
    applyProgressRef.current = (p: TranscribeProgress | null) => {
      if (phase === "blocked" || phase === "waiting" || phase === "gated" || phase === "listening") return;
      if (!p) {
        if (phase === "transcribing") setPhase("thinking");
        return;
      }
      setPhase("transcribing");
      words = p.wordsSoFar;
      say(transcribeLine(chatTone, p));
      wake();
    };
    raf = requestAnimationFrame(loop);
    return () => {
      dead = true;
      wakeRef.current = () => {};
      cancelAnimationFrame(raf); window.clearTimeout(tick);
      document.removeEventListener("visibilitychange", onVisibility);
      applyGateRef.current = () => {};
      sayRef.current = () => {};
      bubbleQ.dispose();
      ro.disconnect(); unsubs.forEach((u) => u());
    };
  }, []);

  // PERM-C — drive the canvas phase from the gate prop.
  useEffect(() => { applyGateRef.current(gate); }, [gate]);
  // yap-1106-yappy-states-and-motion — every gate change runs ONE row of the
  // transition table (published as --phase-dur/--phase-ease/--phase-delay on
  // the stage), and entering hands-free says what hands-free means.
  useEffect(() => {
    const from = prevGateRef.current;
    prevGateRef.current = gate;
    if (from === gate) return;
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const t = phaseTransition(from, gate, reduceMotion);
    const stage = stageRef.current;
    if (stage) publishPhaseTransition(stage, t);
    console.info(`pill phase_transition from=${PHASE_ID[from]} to=${PHASE_ID[gate]} dur_ms=${t.durationMs} delay_ms=${t.delayMs} curve=${t.curve} reduce=${reduceMotion ? 1 : 0}`);
    if (gate === "listening-locked") {
      const line = handsFreeExplainLine(toneRef.current, chordLabel.trim() || DEFAULT_STOP_KEY);
      handsFreeLineRef.current = line;
      sayRef.current(line);
      console.info(`pill hands_free_explain tone=${toneRef.current} chars=${line.length}`);
    }
  }, [gate, chordLabel]);
  // Y3-C — and the transcribing phase from the progress prop (float-main owns it).
  useEffect(() => { applyProgressRef.current(progress); }, [progress]);

  // Y5-C — Yappy's own art for the phase, from her own table. Total record, so
  // a new phase cannot reach the canvas without a pose to draw.
  const art = YAPPY_PHASE_ART[gate];

  // yap24-NT7 fix — the meeting badge ("Notes ready — Open", the failure, the
  // recording stop button) sits in the layout's clock slot (yap-1106-one-pill:
  // beside or under Yappy, never pinned to the window's bottom edge), OUTSIDE the
  // canvas capsule this pill reports as its hitbox, so on a click-through panel
  // its clicks fell through to the app underneath. While a badge is mounted its
  // rect is unioned into the hitbox; a childList observer follows it as it
  // swaps element (recording → processing → notes-ready), a ResizeObserver as
  // its label grows, and unmount hands the hitbox back to the capsule alone.
  const stageRef = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    let badge: HTMLElement | null = null;
    const push = () => {
      if (!badge || !badge.isConnected) {
        extendPillHitbox(null);
        return;
      }
      const r = badge.getBoundingClientRect();
      extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height });
    };
    const ro = new ResizeObserver(push);
    const track = () => {
      const next = stage.querySelector<HTMLElement>(":scope > .meeting-badge");
      if (next !== badge) {
        if (badge) ro.unobserve(badge);
        badge = next;
        if (badge) ro.observe(badge);
      }
      push();
    };
    track();
    const mo = new MutationObserver(track);
    mo.observe(stage, { childList: true });
    return () => {
      mo.disconnect();
      ro.disconnect();
      extendPillHitbox(null);
    };
  }, []);
  // yap-1106-pill-trial-clickable — the gated chip (now wider: "Trial ended" +
  // its "Get Yap" button) and the resting nub's lock can sit OUTSIDE the canvas
  // capsule this pill reports as its hitbox; on the click-through panel their
  // clicks would fall through to the app underneath. Each one, while mounted,
  // is unioned into the hitbox under its own owner (the badge above keeps the
  // default owner), and unmount hands that share back.
  useEffect(() => {
    const stage = stageRef.current;
    if (!stage) return;
    const owners: ReadonlyArray<readonly [string, string]> = [
      [GATE_HITBOX_OWNER_CHIP, ".kami-blocked.gated"],
      [GATE_HITBOX_OWNER_LOCK, ".kami-gate-lock"],
    ];
    const push = () => {
      for (const [owner, sel] of owners) {
        const el = stage.querySelector<HTMLElement>(sel);
        if (!el || !el.isConnected) {
          extendPillHitbox(null, owner);
          continue;
        }
        const r = el.getBoundingClientRect();
        extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, owner);
      }
    };
    const ro = new ResizeObserver(push);
    for (const [, sel] of owners) {
      const el = stage.querySelector<HTMLElement>(sel);
      if (el) ro.observe(el);
    }
    push();
    return () => {
      ro.disconnect();
      for (const [owner] of owners) extendPillHitbox(null, owner);
    };
  }, [gated, showLock, gateChip.lead, gateChip.cta]);

  return (
    <div
      ref={stageRef}
      className={hop ? "kami-stage kami-hop" : "kami-stage"}
      data-phase={PHASE_ID[gate]}
      onPointerEnter={gated ? () => hoverGate(true) : undefined}
      onPointerLeave={gated ? () => hoverGate(false) : undefined}
      data-sprite={art.sprite}
      data-motion={art.motion}
      // Y5-J — the canvas is aria-hidden pixels, so the stage itself carries
      // the phase's name; the live region below announces its changes.
      role="group"
      aria-label={phaseAccessibleName(gate, progress, chordLabel)}
    >
      <PhaseAnnouncer phase={gate} chordLabel={chordLabel} />
      <div ref={bubbleRef} className="kami-bubble"><span ref={bubbleTextRef} className="kami-bubble-text"></span></div>
      {/* Y3-C — the determinate fill and its numeral ride ABOVE the canvas as
          DOM: the bar is a compositor transform and the numeral is text in
          Departure Mono, so neither costs the draw loop a repaint. Never a
          spinner — the count is knowable, so it is stated. */}
      {progress ? (
        <div
          className="kami-progress"
          // Y5-J — a progressbar, not a second live region: `status` is
          // implicitly polite, so every chunk tick was read aloud on top of
          // the PhaseAnnouncer. The count is a value to inspect, not news.
          role="progressbar"
          aria-label={progressLabel(progress)}
          aria-valuemin={0}
          aria-valuemax={progress.of}
          aria-valuenow={progress.chunk}
          style={{ ["--progress" as string]: progressFraction(progress).toFixed(4) } as React.CSSProperties}
        >
          <span className="kami-progress-track" aria-hidden><i /></span>
          <span className="kami-progress-count" aria-hidden>{progressNumeral(progress)}</span>
        </div>
      ) : null}
      {/* Y2-B (8d85cdf) + yap24-PILL1 — the trial numeral, held in Yappy's
          wing, for the WHOLE trial (14…8 in the quiet tone). DOM, not canvas,
          so it survives every frame policy including reduced motion. Inert: a
          purchase route (when checkout opens) is the gated capsule's job.
          Only the value is here; the sentence is the title. */}
      {/* yap-1106-pill-trial-clickable — after the refusal settles, the
          resting nub keeps a small LOCK that opens the same page. */}
      {showLock ? (
        <button
          type="button"
          className="kami-license kami-license-ended kami-gate-lock"
          aria-label={GATE_LOCK_LABEL}
          title={GATE_BUY_TOOLTIP}
          onClick={() => openGatePurchase("nub")}
        >
          <svg viewBox="0 0 12 12" aria-hidden>
            <path d="M3.5 5.5V4a2.5 2.5 0 0 1 5 0v1.5" />
            <rect x="2.5" y="5.5" width="7" height="5" rx="1" />
          </svg>
        </button>
      ) : null}
      {license?.show && !gated && !blocked && !showLock ? (
        <div className={`kami-license kami-license-${license.tone}`} title={license.title} role="note" aria-label={license.title}>
          {(() => {
            const sp = splitValue(license.value);
            return sp ? (
              <>
                <span className="kami-license-num">{sp.numeral}</span>
                {sp.unit ? <span className="kami-license-unit">{sp.unit}</span> : null}
              </>
            ) : (
              <span className="kami-license-glyph" aria-hidden>{license.glyph}</span>
            );
          })()}
        </div>
      ) : null}
      <canvas ref={canvasRef} className="kami-canvas" aria-hidden {...drag.handlers} />
      {/* YV95 — the recording state is a DOM overlay, not a canvas layer, so
          the pulse is a compositor animation and the clock is text the backend
          already rendered. That is only half of OS-12 fix (1): the canvas loop
          is what actually has to stop, which is `meetingRecording` in the frame
          policy above (live.ts) — an overlay that costs nothing does not park a
          loop that was still redrawing behind it 10 times a second. */}
      {gated && (
        <button
          type="button"
          className={gateChipClass}
          // The headline, what a click does, where it goes, and the why.
          aria-label={`${gateVisual.label}. ${GATE_BUY_LABEL}. ${GATE_BUY_TOOLTIP}. ${GATED_TITLE}`}
          title={GATE_BUY_TOOLTIP}
          onClick={() => openGatePurchase("chip")}
          onFocus={() => hoverGate(true)}
          onBlur={() => hoverGate(false)}
        >
          {/* The SAME ⊘ the ended license chip wears — one mark for one state. */}
          <i className="kami-gated-glyph" aria-hidden>{GATED_GLYPH}</i>
          {/* pill.html §13: "Trial ended" + a "Get Yap" button; hover (or focus)
              fills it as "Buy a license" and the tooltip says where it goes;
              pressed, the pill reads "Opening drivia.consulting…". The words
              are in the button's accessible name already, so these are hidden
              from assistive tech to keep it one announcement. */}
          <span>{gateChip.lead}</span>
          {gateChip.cta !== null ? (
            <span className={gateChip.ctaFilled ? "kami-gate-cta is-filled" : "kami-gate-cta"} aria-hidden>
              {gateChip.cta}
            </span>
          ) : null}
          {gateChip.tooltip !== null ? (
            <span className="kami-gate-tip" role="tooltip" aria-hidden>{gateChip.tooltip}</span>
          ) : null}
        </button>
      )}
      {blocked && (
        <button
          type="button"
          className={`kami-blocked ${gate}`}
          aria-label={gateVisual.label}
          title={gateVisual.label}
          onClick={openPermissions}
        >
          {/* A mic with a slash through it — the one glyph that reads as
              "cannot hear you" without a word of copy. */}
          <svg viewBox="0 0 24 24" aria-hidden>
            <path d="M12 3a3 3 0 0 1 3 3v6a3 3 0 0 1-3 3 3 3 0 0 1-3-3V6a3 3 0 0 1 3-3z" />
            <path d="M5 11a7 7 0 0 0 14 0" />
            <path d="M12 18v3" />
            <path className="slash" d="M3 3l18 18" />
          </svg>
          <span>{gate === "waiting" ? "Allow the mic…" : "Mic blocked"}</span>
        </button>
      )}
      <MeetingBadge status={meeting} after={meetingAfter} onOpen={onMeetingOpen} dictating={isListening(gate)} />
      <GovernorChip />
    </div>
  );
}
