/**
 * Y5-K — Yappy, as a CHARACTER: the pixel chick.
 *
 * Her DATA lives here, and both of her surfaces read it: the phase art table,
 * the tone-keyed working chatter and the habitat mood words (home/YappyHouse).
 * One creature, two surfaces, one source.
 *
 * TRANSITIONAL: her pull-back-camera world still owns the whole float window, so
 * she mounts through `stage` (pill/YappyPill.tsx, loaded lazily so the registry
 * never pulls a canvas world into a bundle that does not draw her). Folding that
 * stage's chrome into the shell capsule is the follow-up named in the PR.
 */
import { lazy } from "react";
import { phaseCopy, type ChatTone, type PhaseArtTable } from "../../live";
import type { Bucket } from "../../tone";
import type { Character } from "../types";

/** Yappy's art for EVERY phase, as DATA (lifted from Y5-C's table). */
export const YAPPY_PHASE_ART: PhaseArtTable = {
  idle: { sprite: "face-rest", motion: "breathe" },
  sleepy: { sprite: "face-asleep", motion: "still" },
  listening: { sprite: "world-listen", motion: "pulse" },
  "model-loading": { sprite: "world-stretch", motion: "breathe" },
  transcribing: { sprite: "world-type", motion: "work" },
  polishing: { sprite: "world-polish", motion: "work" },
  pasting: { sprite: "world-hand-off", motion: "work" },
  thinking: { sprite: "world-think", motion: "work" },
  done: { sprite: "face-happy", motion: "still" },
  empty: { sprite: "face-confused", motion: "still" },
  cancelled: { sprite: "face-shrug", motion: "still" },
  error: { sprite: "face-worried", motion: "shake" },
  gated: { sprite: "face-locked", motion: "still" },
  waiting: { sprite: "face-ear", motion: "breathe" },
  blocked: { sprite: "face-deaf", motion: "still" },
  // yap24-PILL3 — fn cannot reach Yappy, but Yappy is fine: the ear face, held
  // still, not the deaf one `blocked` (the MIC) wears.
  blind: { sprite: "face-ear", motion: "still" },
};

/** Tone-aware working chatter, keyed by transcript length bucket. */
export const YAPPY_CHATTER: Record<ChatTone, Partial<Record<Bucket, string[]>>> = {
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

/** The habitat's mood words (YV27): same state, Yappy's voice per tone. */
export type HabitatMood = "listening" | "thinking" | "done" | "nap" | "work" | "idle";
export const YAPPY_HABITAT_MOODS: Record<ChatTone, Record<HabitatMood, string>> = {
  friendly: { listening: "listening", thinking: "thinking", done: "proud ♥", nap: "napping", work: "working", idle: "content" },
  rude: { listening: "all ears", thinking: "hmm…", done: "not bad", nap: "do not disturb", work: "grinding", idle: "whatever" },
  rose: { listening: "listening 🌹", thinking: "thinking of you", done: "adores you 🌹", nap: "dreaming", work: "working for you", idle: "missing you 🌹" },
};

export const yappy: Character = {
  id: "yappy",
  label: "Yappy",
  render: (frame) => ({
    art: YAPPY_PHASE_ART[frame.phase],
    copy: phaseCopy(frame.phase, frame.tone),
    // Her camera fills the box edge to edge — never past it.
    size: { w: frame.box.w, h: frame.box.h },
    canvas: { imageSmoothingEnabled: false },
    node: null,
  }),
  stage: lazy(() => import("../../YappyPill")),
};
