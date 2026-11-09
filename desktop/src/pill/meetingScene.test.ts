/**
 * yap-1106-pill-rec-headphones-hoot-tape (+ FB-05) — the meeting scene, on a
 * fake clock. Every case drives `elapsedS` exactly the way the backend's 1 Hz
 * `meeting` emit does; no timers, no canvas.
 */
import { describe, expect, it } from "vitest";
import { CHICK_DETAIL, drawChick, type ChickPose } from "../sprites/art";
import { Raster } from "../sprites/raster";
import {
  COMPANION_VISIT_CAP_MS,
  GLANCE_EVERY_S,
  isGlancing,
  MEETING_IDLE_REDRAW_MS,
  meetingFrameKey,
  meetingRedraws,
  meetingScene,
  moonPosition,
  REC_LIGHT_PULSE_MS,
  SAVED_HOLD_MS,
  SAVED_LABEL,
  SCENE_H,
  SCENE_LAYOUT,
  SCENE_W,
  sceneTimeLabel,
  skyPhase,
  stopBeatUntil,
  sunPosition,
  visitCapMs,
} from "./meetingScene";

const MIN = 60;

describe("the sun meeting clock, fake clock at 0, 1, 10, 60 and 61 min", () => {
  it("0 min: day, the sun rising at the left, the time 0:00 under it, no glance", () => {
    const m = meetingScene({ elapsedS: 0 });
    expect(m.sky).toBe("day");
    expect(m.sun).not.toBeNull();
    expect(m.sun!.x).toBeLessThan(SCENE_W / 4);
    expect(m.moon).toBeNull();
    expect(m.pose).toBe("listening");
    expect(m.timeLabel).toBe("0:00");
    expect(m.timeAnchor.x).toBe(m.sun!.x);
    expect(m.timeAnchor.y).toBeGreaterThan(m.sun!.y);
  });

  it("1 min: the sun has moved one minute's worth, the time reads 1:00", () => {
    const a = meetingScene({ elapsedS: 0 }).sun!;
    const b = meetingScene({ elapsedS: 1 * MIN }).sun!;
    expect(b.x).toBeGreaterThanOrEqual(a.x);
    expect(meetingScene({ elapsedS: 1 * MIN }).timeLabel).toBe("1:00");
    // and it does NOT move between minutes (one redraw a minute)
    expect(sunPosition(1 * MIN + 59)).toEqual(sunPosition(1 * MIN));
    expect(isGlancing(1 * MIN)).toBe(false);
  });

  it("10 min: Yappy glances up at the sun for exactly 1 s", () => {
    expect(meetingScene({ elapsedS: 10 * MIN }).pose).toBe("glance");
    expect(meetingScene({ elapsedS: 10 * MIN + 1 }).pose).toBe("listening");
    expect(meetingScene({ elapsedS: 10 * MIN - 1 }).pose).toBe("listening");
    expect(isGlancing(20 * MIN)).toBe(true);
    expect(isGlancing(GLANCE_EVERY_S * 5)).toBe(true);
  });

  it("10 min while dictating: no glance, Yappy keeps the notepad", () => {
    const m = meetingScene({ elapsedS: 10 * MIN, dictating: true });
    expect(m.pose).toBe("notepad");
    expect(isGlancing(10 * MIN, { dictating: true })).toBe(false);
  });

  it("60 min: the sky turns dusk, the sun has set, the moon rises", () => {
    expect(skyPhase(60 * MIN - 1)).toBe("day");
    const m = meetingScene({ elapsedS: 60 * MIN });
    expect(m.sky).toBe("dusk");
    expect(m.sun).toBeNull();
    expect(m.moon).not.toBeNull();
    expect(m.timeLabel).toBe("1:00:00");
    expect(m.timeAnchor.x).toBe(m.moon!.x);
  });

  it("61 min: the moon is crossing in hour two", () => {
    const at60 = moonPosition(60 * MIN)!;
    const at61 = moonPosition(61 * MIN)!;
    const at119 = moonPosition(119 * MIN)!;
    expect(at61.x).toBeGreaterThanOrEqual(at60.x);
    expect(at119.x).toBeGreaterThan(at61.x);
    expect(meetingScene({ elapsedS: 61 * MIN }).sky).toBe("dusk");
    expect(sceneTimeLabel(61 * MIN + 5)).toBe("1:01:05");
  });

  it("before 1 h there is no moon; garbage elapsed reads as 0", () => {
    expect(moonPosition(59 * MIN)).toBeNull();
    expect(sceneTimeLabel(Number.NaN)).toBe("0:00");
    expect(sceneTimeLabel(-5)).toBe("0:00");
    expect(meetingScene({ elapsedS: Number.POSITIVE_INFINITY }).timeLabel).toBe("0:00");
  });
});

describe("redraws: one a minute idle, plus the glance frames", () => {
  it("an idle hour repaints once per minute plus the glance frames", () => {
    // Minutes 0..59 = 60 repaints. Each glance (10..50 min) STARTS on its
    // minute's own repaint and adds one more when it ends a second later.
    expect(meetingRedraws(0, 60 * MIN - 1)).toBe(60 + 5);
    expect(meetingRedraws(10 * MIN - 1, 10 * MIN + 1)).toBe(3);
    expect(MEETING_IDLE_REDRAW_MS).toBe(60_000);
  });

  it("inside one minute the frame key never changes (no glance there)", () => {
    const k = meetingFrameKey({ elapsedS: 3 * MIN });
    for (let s = 3 * MIN; s < 4 * MIN; s++) expect(meetingFrameKey({ elapsedS: s })).toBe(k);
  });

  it("reduce motion: no glance, the time still updates every minute", () => {
    expect(meetingScene({ elapsedS: 10 * MIN, reduceMotion: true }).pose).toBe("listening");
    expect(meetingRedraws(0, 60 * MIN - 1, { reduceMotion: true })).toBe(60);
    expect(meetingScene({ elapsedS: 10 * MIN, reduceMotion: true }).timeLabel).toBe("10:00");
    const m = meetingScene({ elapsedS: 5, reduceMotion: true });
    expect(m.recLight.pulsing).toBe(false);
    expect(m.recLight.on).toBe(true);
    expect(m.reelsTurning).toBe(false);
  });
});

describe("the recording light, Hoot and the tape", () => {
  it("the right ear cup is the recording light, pulsing about every 1.6 s", () => {
    expect(REC_LIGHT_PULSE_MS).toBe(1600);
    const m = meetingScene({ elapsedS: 42 });
    expect(m.recLight).toEqual({ pulseMs: 1600, pulsing: true, on: true });
    // the cup sits on Yappy's RIGHT (screen right of his centre)
    expect(SCENE_LAYOUT.cup.x).toBeGreaterThan(SCENE_LAYOUT.yappy.cx);
  });

  it("Hoot sits to Yappy's left at the tape machine, and the reels turn while recording", () => {
    expect(SCENE_LAYOUT.hoot.x + SCENE_LAYOUT.hoot.size).toBeLessThanOrEqual(SCENE_LAYOUT.tape.x + 1);
    expect(SCENE_LAYOUT.tape.x + SCENE_LAYOUT.tape.w).toBeLessThan(SCENE_LAYOUT.yappy.cx - 11);
    const m = meetingScene({ elapsedS: 30 });
    expect(m.hoot.present).toBe(true);
    expect(m.reelsTurning).toBe(true);
  });

  it("dictating mid-meeting gives Yappy the notepad and keeps the tape running", () => {
    const m = meetingScene({ elapsedS: 12 * MIN, dictating: true });
    expect(m.pose).toBe("notepad");
    expect(m.reelsTurning).toBe(true);
    expect(m.recLight.pulsing).toBe(true);
    expect(meetingFrameKey({ elapsedS: 12 * MIN, dictating: true })).not.toBe(meetingFrameKey({ elapsedS: 12 * MIN }));
  });

  it("Hoot is exempt from the 4-second visit cap for meetings only", () => {
    expect(COMPANION_VISIT_CAP_MS).toBe(4000);
    expect(visitCapMs("hoot", { meeting: true })).toBe(Infinity);
    expect(visitCapMs("hoot", { meeting: false })).toBe(4000);
    expect(visitCapMs("shelly", { meeting: true })).toBe(4000);
    expect(meetingScene({ elapsedS: 3 * 3600 }).hoot.visitCapMs).toBe(Infinity);
  });

  it("on stop Hoot hands over the tape and the pill shows Saved", () => {
    expect(stopBeatUntil(true, false, 1000)).toBe(1000 + SAVED_HOLD_MS);
    expect(stopBeatUntil(false, false, 1000)).toBeNull();
    expect(stopBeatUntil(true, true, 1000)).toBeNull();
    expect(stopBeatUntil(false, true, 1000)).toBeNull();
    expect(SAVED_LABEL).toBe("Saved");
    const m = meetingScene({ elapsedS: 25 * MIN, handover: true });
    expect(m.hoot.handingTape).toBe(true);
    expect(m.reelsTurning).toBe(false);
    expect(m.recLight.on).toBe(false);
    expect(meetingFrameKey({ elapsedS: 25 * MIN, handover: true })).toContain("|h");
  });
});

describe("yap-1106-yappy-detail-pass — the detail-pass Yappy stands whole in the meeting scene", () => {
  const REST: ChickPose = { hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0, look: "fwd", glasses: false, pencil: false, droop: false };
  for (const pose of [REST, { ...REST, look: "down" as const, pencil: true }]) {
    it(`meeting_yappy_fits_scene_buffer pencil=${pose.pencil}: toes and shadow inside ${SCENE_W}x${SCENE_H}`, () => {
      const { cx, feetY } = SCENE_LAYOUT.yappy;
      // a taller raster, so anything that would fall off the real buffer is visible here
      const r = new Raster(SCENE_W, SCENE_H + 12);
      drawChick(r, cx, feetY, pose);
      let below = 0, toes = 0, top = Infinity;
      for (let y = 0; y < r.height; y++) for (let x = 0; x < r.width; x++) {
        const p = r.px(x, y);
        if (p.a === 0) continue;
        top = Math.min(top, y);
        if (y >= SCENE_H) below++;
        const hex = "#" + [p.r, p.g, p.b].map((v) => v.toString(16).padStart(2, "0")).join("");
        if (p.a === 255 && hex === CHICK_DETAIL.toe && y < SCENE_H) toes++;
      }
      expect(below, "no pixel of Yappy or his shadow below the scene buffer").toBe(0);
      expect(toes, "both three-pixel toes are on screen").toBe(6);
      expect(top, "his sprout is on screen too").toBeGreaterThanOrEqual(0);
      expect(r.subpixel).toBe(0);
    });
  }
});
