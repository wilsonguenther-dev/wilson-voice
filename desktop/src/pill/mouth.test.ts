import { describe, expect, it } from "vitest";
import { DEFAULT_MOUTH, MouthDriver } from "./mouth";

const FRAME = 1 / 60;

/** Push the same level for `seconds` at 60fps; return the last jaw-open. */
function hold(d: MouthDriver, rms: number, seconds: number, dt = FRAME): number {
  let v = 0;
  for (let t = 0; t < seconds; t += dt) v = d.push(rms, dt);
  return v;
}

describe("MouthDriver (pill/mouth)", () => {
  it("treats room tone at or under the gate as silence: only the idle breath shows", () => {
    const d = new MouthDriver({ breath: 0 });
    expect(hold(d, DEFAULT_MOUTH.gate, 1)).toBe(0);
    expect(d.value).toBe(0);
  });

  it("opens on speech and never leaves 0..1, however loud the input", () => {
    const d = new MouthDriver();
    const v = hold(d, 5, 0.5);
    expect(v).toBeGreaterThan(0.95);
    expect(v).toBeLessThanOrEqual(1);
  });

  it("opens faster than it closes (asymmetric attack/release)", () => {
    const d = new MouthDriver({ breath: 0 });
    const opened = d.push(0.5, 0.05);
    hold(d, 0.5, 1);
    const top = d.value;
    const closed = top - d.push(0, 0.05);
    expect(opened).toBeGreaterThan(closed);
  });

  it("is frame-rate independent: 30fps and 120fps land on the same jaw", () => {
    const a = hold(new MouthDriver({ breath: 0 }), 0.2, 0.2, 1 / 30);
    const b = hold(new MouthDriver({ breath: 0 }), 0.2, 0.2, 1 / 120);
    expect(Math.abs(a - b)).toBeLessThan(0.05);
  });

  it("lifts quiet speech with gamma < 1, so a soft voice still moves the mouth", () => {
    const linear = hold(new MouthDriver({ gamma: 1, breath: 0 }), 0.06, 1);
    const lifted = hold(new MouthDriver({ breath: 0 }), 0.06, 1);
    expect(lifted).toBeGreaterThan(linear);
  });

  it("breathes when silent instead of freezing, within the breath amplitude", () => {
    const d = new MouthDriver();
    const samples = Array.from({ length: 180 }, () => d.push(0, FRAME));
    expect(Math.max(...samples)).toBeGreaterThan(0);
    expect(Math.max(...samples)).toBeLessThanOrEqual(DEFAULT_MOUTH.breath + 1e-9);
  });

  it("ignores a negative dt rather than running the smoothing backwards", () => {
    const d = new MouthDriver({ breath: 0 });
    hold(d, 0.5, 0.5);
    const before = d.value;
    d.push(0, -1);
    expect(d.value).toBe(before);
  });

  it("snaps shut on reset", () => {
    const d = new MouthDriver();
    hold(d, 0.5, 0.5);
    d.reset();
    expect(d.value).toBe(0);
  });
});
