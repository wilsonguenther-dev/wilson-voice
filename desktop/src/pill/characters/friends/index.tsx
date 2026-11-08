/**
 * yap-1106-main-character-switch — the friends who can SIT in the pill.
 *
 * companions.html section 04 (approved): Settings → Companion → Who sits in
 * the pill offers Yappy (default), Hoot, Tidy, Gulp and Shelly. Ember, Sal and
 * Mochi are room-only and are never offered (Cinder comes later). A character
 * in the pill needs a pose for all 17 pill states, and the character contract
 * (characters.test.ts) sweeps every one of them for every registered creature.
 *
 * Each friend's art is its approved HD manifest (sprites/friends.ts: idle,
 * blink, a two-frame action), painted at a whole-number scale with smoothing
 * off (sprites/crisp.ts). A phase picks ONE frame, held still: the motion of
 * the phase rides `data-motion` on the shell's capsule (CSS, compositor-only),
 * so a friend in the pill never runs a frame loop of its own.
 *
 * Same voice settings: the shell's tone picks the line, exactly as for Yappy
 * (`phaseCopy`); each friend adds a few lines of its own on top — Hoot is
 * drier, Tidy is precise, Gulp loves new words, Shelly takes it slow.
 *
 * Pure of settings, license, dock and backend, like every creature module.
 */
import { phaseCopy, type ChatTone, type LivePhase, type PhaseArt, type PhaseArtTable } from "../../live";
import { drawFriendFrame, friendManifest, type FrameKind, type FriendManifest } from "../../../sprites/friends";
import { crispPlan, domBuffer } from "../../../sprites/crisp";
import { CHARACTER_PHASES, fitSquare, type Character, type CharacterBox } from "../types";
import { yappy } from "../yappy";

/** The friends a user may put in the pill, in the approved order (after Yappy). */
export const PILL_FRIENDS = ["hoot", "tidy", "gulp", "shelly"] as const;
export type PillFriendId = (typeof PILL_FRIENDS)[number];

/** Which manifest frame, and which motion, each pill state wears. */
export interface FriendPose {
  /** A short pose word; the sprite key is `<friend>-<pose>`. */
  pose: string;
  frame: FrameKind;
  motion: PhaseArt["motion"];
}

/** Every one of the 17 pill states, as a pose. Settled states never "work". */
export const FRIEND_POSES: Readonly<Record<LivePhase, FriendPose>> = {
  idle: { pose: "rest", frame: "idle", motion: "breathe" },
  sleepy: { pose: "asleep", frame: "blink", motion: "still" },
  listening: { pose: "listen", frame: "idle", motion: "pulse" },
  "listening-locked": { pose: "listen", frame: "idle", motion: "pulse" },
  "model-loading": { pose: "stretch", frame: "idle", motion: "breathe" },
  transcribing: { pose: "work", frame: "action", motion: "work" },
  polishing: { pose: "polish", frame: "action", motion: "work" },
  pasting: { pose: "hand-off", frame: "action", motion: "work" },
  thinking: { pose: "think", frame: "blink", motion: "work" },
  done: { pose: "proud", frame: "action", motion: "still" },
  empty: { pose: "confused", frame: "idle", motion: "still" },
  cancelled: { pose: "shrug", frame: "blink", motion: "still" },
  error: { pose: "worried", frame: "idle", motion: "shake" },
  gated: { pose: "locked", frame: "blink", motion: "still" },
  blocked: { pose: "deaf", frame: "blink", motion: "still" },
  waiting: { pose: "ear", frame: "idle", motion: "breathe" },
  blind: { pose: "ear", frame: "idle", motion: "still" },
};

/** The phase art table for one friend (every phase, as DATA). */
export function friendPhaseArt(id: string): PhaseArtTable {
  return Object.fromEntries(
    CHARACTER_PHASES.map((p) => {
      const pose = FRIEND_POSES[p] ?? FRIEND_POSES.idle;
      return [p, { sprite: `${id}-${pose.pose}`, motion: pose.motion }];
    }),
  ) as PhaseArtTable;
}

type OwnLines = Partial<Record<LivePhase, Partial<Record<ChatTone, string>>>>;

/**
 * A few lines of each friend's own, on top of the shared voice. A phase or a
 * tone a friend has no line for uses the shared line (`phaseCopy`), so every
 * state always has words in every voice.
 */
export const FRIEND_LINES: Readonly<Record<PillFriendId, OwnLines>> = {
  hoot: {
    listening: { friendly: "go on. I'm listening.", rude: "go on. I'm listening.", rose: "go on, I'm listening 🌹" },
    done: { friendly: "noted.", rude: "noted. obviously." },
    thinking: { friendly: "hm. thinking.", rude: "give me a second." },
    sleepy: { friendly: "resting my eyes.", rude: "not now." },
  },
  tidy: {
    listening: { friendly: "listening. every word.", rude: "go. I'll fix it after." },
    polishing: { friendly: "tidying the commas.", rude: "cleaning up after you." },
    done: { friendly: "neat and filed.", rude: "there. clean." },
    empty: { friendly: "nothing to tidy yet.", rude: "nothing. at all." },
  },
  gulp: {
    listening: { friendly: "ooh, new words?", rude: "feed me words." },
    transcribing: { friendly: "munching your words…", rude: "chewing…" },
    done: { friendly: "delicious words.", rude: "tasty enough." },
    empty: { friendly: "still hungry.", rude: "that was nothing." },
  },
  shelly: {
    listening: { friendly: "slow and steady. go on.", rude: "take your time. or don't." },
    "model-loading": { friendly: "warming up, slowly…", rude: "I'm coming, I'm coming." },
    done: { friendly: "got there.", rude: "made it." },
    sleepy: { friendly: "tucked in.", rude: "in my shell." },
  },
};

/** A friend's line for a phase in a voice: its own if it has one, else the shared one. */
export function friendLine(id: PillFriendId, phase: LivePhase, tone: ChatTone): string {
  const own = FRIEND_LINES[id]?.[phase]?.[tone];
  return own && own.trim() ? own : phaseCopy(phase, tone);
}

/** The largest art box a friend takes in the pill (its manifest is at most 46 x 40). */
const MAX_SIDE = 40;

/** Paint one held frame of `m` into `canvas` at a whole-number scale, smoothing off. */
function paintFriend(canvas: HTMLCanvasElement, m: FriendManifest, kind: FrameKind, size: CharacterBox): void {
  try {
    const plan = crispPlan(size.w, size.h, m.w, m.h, globalThis.devicePixelRatio);
    if (canvas.width !== plan.backingW) canvas.width = plan.backingW;
    if (canvas.height !== plan.backingH) canvas.height = plan.backingH;
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      console.warn(`pill character_paint_failed friend=${m.id} reason=no-2d-context`);
      return;
    }
    const buf = domBuffer(m.w, m.h);
    // The action pose is its first frame (a held frame reads the same under reduce motion).
    drawFriendFrame(buf.ctx, m.id, kind, 0);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, plan.backingW, plan.backingH);
    ctx.drawImage(buf.image, 0, 0, m.w, m.h, plan.dx, plan.dy, m.w * plan.scale, m.h * plan.scale);
  } catch (e) {
    console.warn(`pill character_paint_failed friend=${m.id} reason=${e instanceof Error ? e.name : "unknown"}`);
  }
}

/** Build the pill character for one friend. */
export function friendCharacter(id: PillFriendId): Character {
  const m = friendManifest(id);
  if (!m) throw new Error(`friendCharacter: "${id}" has no manifest`);
  const art = friendPhaseArt(id);
  return {
    id,
    label: m.label,
    // Batch 10 bounce fix: exactly ONE pill surface. A friend sits in the same
    // one-pill stage as Yappy (sky, layout-sized window, top-center meeting dock,
    // timer inside the pill); the stage paints it via `occupant` (pill/occupant.ts).
    // `render` stays the art/copy contract and the shell body's fallback.
    stage: yappy.stage,
    render: (frame) => {
      const pose = FRIEND_POSES[frame.phase] ?? FRIEND_POSES.idle;
      // Never past the box; an empty box paints an empty (0 x 0) canvas.
      const size = fitSquare(frame.box, MAX_SIDE);
      const kind: FrameKind = pose.frame;
      return {
        art: art[frame.phase] ?? art.idle,
        copy: friendLine(id, frame.phase, frame.tone),
        size,
        canvas: { imageSmoothingEnabled: false },
        node: (
          <canvas
            className="char-friend"
            data-friend={id}
            data-pose={pose.pose}
            aria-hidden
            width={1}
            height={1}
            style={{ width: size.w, height: size.h, imageRendering: "pixelated" }}
            ref={(el) => {
              if (el && size.w > 0 && size.h > 0) paintFriend(el, m, kind, size);
            }}
          />
        ),
      };
    },
  };
}

export const hoot = friendCharacter("hoot");
export const tidy = friendCharacter("tidy");
export const gulp = friendCharacter("gulp");
export const shelly = friendCharacter("shelly");
