/**
 * mouth — turn the live microphone level into a jaw-open amount, so the yap
 * companion's mouth mimics the user's actual speech while dictating.
 *
 * The recorder already exposes a live mic level (see record.rs push_level →
 * audio_level events). Feed that RMS here each frame; get back a smoothed
 * 0..1 jaw-open the renderer maps to a beak/mouth dihedral.
 *
 * Shape of the mapping (from docs/research/origami-yapping-pill.md §2.5):
 *   gate → drop room tone;  gamma → make quiet speech visible;
 *   ASYMMETRIC smoothing → snap open on a syllable, ease closed (a mouth opens
 *   faster than it relaxes);  idle breathing → never a dead, frozen mouth.
 * Smoothing is dt-based (frame-rate independent), so 30fps and 120fps match.
 */

export interface MouthConfig {
  /** RMS at/below this is treated as silence (room tone). 0..1 */
  gate: number;
  /** Perceptual curve on the gated level (<1 lifts quiet speech). */
  gamma: number;
  /** Scale applied to the curved level before clamping to 1. */
  gain: number;
  /** Opening speed (per second). Higher = snappier syllables. */
  attackRate: number;
  /** Closing speed (per second). Lower = softer relax. */
  releaseRate: number;
  /** Idle breathing amplitude when silent (subtle life). 0..1 */
  breath: number;
  /** Idle breathing speed (radians/sec). */
  breathRate: number;
}

export const DEFAULT_MOUTH: MouthConfig = {
  gate: 0.04,
  gamma: 0.6,
  gain: 4.5,
  attackRate: 22,
  releaseRate: 7,
  breath: 0.06,
  breathRate: 2.2,
};

const clamp01 = (v: number): number => (v < 0 ? 0 : v > 1 ? 1 : v);

export class MouthDriver {
  private cfg: MouthConfig;
  private open = 0;
  private phase = 0;

  constructor(cfg: Partial<MouthConfig> = {}) {
    this.cfg = { ...DEFAULT_MOUTH, ...cfg };
  }

  /** Map a raw RMS (0..1) to a smoothed jaw-open (0..1). Call once per frame. */
  push(rms: number, dt: number): number {
    const c = this.cfg;
    this.phase += dt * c.breathRate;

    // gate + perceptual curve → this frame's raw target
    let target = 0;
    if (rms > c.gate) {
      const lifted = Math.pow((rms - c.gate) / (1 - c.gate), c.gamma);
      target = clamp01(lifted * c.gain);
    }

    // asymmetric, dt-based smoothing: fast to open, slow to close
    const rate = target > this.open ? c.attackRate : c.releaseRate;
    const k = 1 - Math.exp(-rate * Math.max(0, dt));
    this.open += (target - this.open) * k;
    this.open = clamp01(this.open);

    // idle breathing only when essentially silent, so a resting mouth still lives
    if (target < 0.02) {
      const breath = (0.5 + 0.5 * Math.sin(this.phase)) * c.breath;
      return Math.max(this.open, breath);
    }
    return this.open;
  }

  /** Snap shut (e.g. when recording stops). */
  reset(): void {
    this.open = 0;
  }

  get value(): number {
    return this.open;
  }
}
