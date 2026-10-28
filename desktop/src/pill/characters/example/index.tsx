/**
 * Y5-K — the PROOF creature. Deliberately minimal: its only job is to show the
 * seam holds — a new module plus one `registerCharacter()` call, with no shell
 * edit. It is NOT registered by the registry (it does not ship as a choice);
 * `characters.test.ts` registers it and mounts it through the real shell.
 *
 * Pixel art on a canvas, smoothing OFF, a limited palette, chunky 8x8 cells.
 */
import { phaseVisual, type LivePhase, type PhaseArtTable } from "../../live";
import { CHARACTER_PHASES, fitSquare, type Character } from "../types";

/** One sprite, three palette slots: 0 clear, 1 body, 2 eye. */
const SPRITE = ["00111100", "01111110", "11211211", "11111111", "11111111", "11111111", "01111110", "00100100"];
const PALETTE: Record<string, string> = { "1": "#7ac74f", "2": "#1d1d2b" };

const motionFor = (p: LivePhase) => {
  const t = phaseVisual(p).tone;
  return t === "busy" ? "work" : t === "live" ? "pulse" : t === "warn" ? "shake" : "still";
};

export const EXAMPLE_PHASE_ART: PhaseArtTable = Object.fromEntries(
  CHARACTER_PHASES.map((p) => [p, { sprite: "blob", motion: motionFor(p) }]),
) as PhaseArtTable;

/** Paint the sprite at 1 canvas px per art px; CSS scales it with `pixelated`. */
export function paintBlob(ctx: CanvasRenderingContext2D): void {
  ctx.imageSmoothingEnabled = false;
  ctx.clearRect(0, 0, 8, 8);
  SPRITE.forEach((row, y) =>
    [...row].forEach((cell, x) => {
      const fill = PALETTE[cell];
      if (!fill) return;
      ctx.fillStyle = fill;
      ctx.fillRect(x, y, 1, 1);
    }),
  );
}

export const example: Character = {
  id: "example",
  label: "Example",
  render: (frame) => {
    const size = fitSquare(frame.box, 24);
    return {
      art: EXAMPLE_PHASE_ART[frame.phase],
      copy: phaseVisual(frame.phase, frame.chordLabel).label,
      size,
      canvas: { imageSmoothingEnabled: false },
      node: (
        <canvas
          className="char-example"
          width={8}
          height={8}
          aria-hidden
          style={{ width: size.w, height: size.h, imageRendering: "pixelated" }}
          ref={(el) => {
            const ctx = el?.getContext("2d");
            if (ctx) paintBlob(ctx);
          }}
        />
      ),
    };
  },
};
