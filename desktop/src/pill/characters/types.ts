/**
 * Y5-K — the shell / character seam. ONE interface.
 *
 * A character is `{ id, label, render(frame) }`. `frame` is what the SHELL
 * already computed — the phase, the companion tone, the input level, the BOX it
 * may paint inside, an axis hint and the reduced-motion preference — and
 * nothing else. A character never reads settings, never reads license state,
 * never positions a window and never knows a dock exists beyond the `dock`
 * axis hint. Everything it needs arrives here; everything it owes comes back
 * as a `CharacterPaint`.
 *
 * Adding a creature is a new module under `characters/<id>/` plus one
 * `registerCharacter()` call: no shell file changes, no Rust (the pill-style setting stays
 * a free string in `AppSettings`), and no new dimension in dock-geometry.json.
 */
import type { ComponentType, ReactNode } from "react";
import { ALL_LIVE_PHASES, type ChatTone, type LivePhase, type PhaseArt, type TranscribeProgress } from "../live";
import type { MeetingAfterView, MeetingStatus } from "../meeting";
import type { PillLicense } from "../license";

/** Width and height in CSS px. */
export interface CharacterBox {
  w: number;
  h: number;
}

/** The ONLY thing a character learns about the dock: which way its long axis runs. */
export type DockAxis = "x" | "y";

/** What the shell hands a character, computed once per shell render. */
export interface CharacterFrame {
  phase: LivePhase;
  tone: ChatTone;
  /** Input level 0..1 at the time of the render (the live level rides `--level`). */
  level: number;
  /** The box the shell gives the character, from dock-geometry.json (Y5-I). */
  box: CharacterBox;
  dock: DockAxis;
  reducedMotion: boolean;
  /**
   * yap24-PILL4 — the key that ends a hands-free take, as the user bound it
   * (`AppStatus.chord_label`). Only `listening-locked` copy reads it; absent
   * means the default binding.
   */
  chordLabel?: string;
}

/** What a character owes one frame. */
export interface CharacterPaint {
  /** The phase's sprite + motion key — rides out as `data-sprite` / `data-motion`. */
  art: PhaseArt;
  /** The character's line for this phase in this tone. */
  copy: string;
  /** What it will occupy. Never larger than `frame.box`. */
  size: CharacterBox;
  /** Present iff the character paints this frame to a canvas: smoothing is OFF. */
  canvas: { imageSmoothingEnabled: false } | null;
  /** The creature itself, mounted by the shell in the capsule's body slot. */
  node: ReactNode;
}

/**
 * The props of a LEGACY full-window stage (Yappy's canvas world). Transitional:
 * see `Character.stage`.
 */
export interface CharacterStageProps {
  gate?: LivePhase;
  /** yap24-PILL4 — the bound stop key, for a hands-free take's name and announcement. */
  chordLabel?: string;
  tone?: ChatTone;
  progress?: TranscribeProgress | null;
  meeting?: MeetingStatus;
  meetingAfter?: MeetingAfterView | null;
  onMeetingOpen?: (id: string) => void;
  license?: PillLicense | null;
}

export interface Character {
  id: string;
  label: string;
  /** `null` means "I have nothing for this frame" — the contract test turns that RED. */
  render(frame: CharacterFrame): CharacterPaint | null;
  /**
   * Transitional escape hatch for a character whose world still owns its whole
   * window (Yappy's pull-back camera). The shell mounts it INSTEAD of the
   * capsule, chosen by the presence of this field — never by id. A new creature
   * should not set it.
   */
  stage?: ComponentType<CharacterStageProps>;
}

/** Every tone a character must have copy in (live.ts `ChatTone`). */
export const CHARACTER_TONES: readonly ChatTone[] = ["rude", "friendly", "rose"];

/** Every phase in the `LivePhase` union (live.ts `ALL_LIVE_PHASES`). */
export const CHARACTER_PHASES: readonly LivePhase[] = ALL_LIVE_PHASES;

/** Does `size` fit inside `box`? Non-finite or negative sizes never fit. */
export function fitsBox(size: CharacterBox, box: CharacterBox): boolean {
  const ok = (n: number) => Number.isFinite(n) && n >= 0;
  return ok(size.w) && ok(size.h) && size.w <= box.w && size.h <= box.h;
}

/** The largest whole-px square that fits `box`, capped at `max`. */
export function fitSquare(box: CharacterBox, max = Infinity): CharacterBox {
  const side = Math.max(0, Math.floor(Math.min(box.w, box.h, max)));
  return { w: side, h: side };
}
