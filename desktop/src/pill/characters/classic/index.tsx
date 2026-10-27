/**
 * Y5-K — Classic, as a CHARACTER: the obsidian waveform capsule's creature.
 *
 * Everything that is not the creature — the capsule chrome, the slots, dock
 * geometry, hover hysteresis, the license chip, aria names, the gate note and
 * the determinate progress fill — is the SHELL's (pill/PillShell.tsx). What is
 * left here is what Classic looks like: its art table and its body (the seed +
 * nine-bar waveform at rest and while listening, the check when a take lands).
 */
import type { CSSProperties } from "react";
import { phaseVisual, type PhaseArtTable } from "../../live";
import type { Character } from "../types";

/** Classic's art for EVERY phase, as DATA (lifted from Y5-C's table). */
export const CLASSIC_PHASE_ART: PhaseArtTable = {
  idle: { sprite: "seed", motion: "breathe" },
  sleepy: { sprite: "seed", motion: "still" },
  listening: { sprite: "wave", motion: "pulse" },
  "model-loading": { sprite: "mic", motion: "breathe" },
  transcribing: { sprite: "xscribe", motion: "work" },
  polishing: { sprite: "spark", motion: "work" },
  pasting: { sprite: "caret", motion: "work" },
  thinking: { sprite: "dots", motion: "work" },
  done: { sprite: "check", motion: "still" },
  empty: { sprite: "hush", motion: "still" },
  cancelled: { sprite: "hush", motion: "still" },
  error: { sprite: "alert", motion: "shake" },
  gated: { sprite: "lock", motion: "still" },
  waiting: { sprite: "mic-off", motion: "breathe" },
  blocked: { sprite: "mic-off", motion: "still" },
  // yap24-PILL3 — the resting seed, held still: only the fn key is deaf.
  blind: { sprite: "seed", motion: "still" },
};

const BARS = Array.from({ length: 9 }, (_, i) => i);

export const classic: Character = {
  id: "classic",
  label: "Classic",
  render: (frame) => ({
    art: CLASSIC_PHASE_ART[frame.phase],
    // Classic is the neutral voice: the phase table's own label, in every tone.
    copy: phaseVisual(frame.phase).label,
    size: { w: frame.box.w, h: frame.box.h },
    canvas: null,
    node:
      frame.phase === "done" ? (
        <svg className="check" viewBox="0 0 24 24" aria-hidden><path d="M4 12.5l5 5L20 6.5" /></svg>
      ) : (
        <>
          <span className="seed" aria-hidden />
          <div className="wave" aria-hidden>
            {BARS.map((i) => (
              <i key={i} style={{ ["--i" as string]: String(i) } as CSSProperties} />
            ))}
          </div>
        </>
      ),
  }),
};
