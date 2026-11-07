// yap-1106-news-duo-delivery — the studio half: Cinder flies to Yappy's desk,
// hands the paper over, the two take turns, reduce motion shows them side by
// side with no flight, and a held delivery keeps her at the back door.
import { describe, expect, it } from "vitest";
import { Raster } from "../../sprites/raster";
import { skyPhase } from "../habitat/habitat";
import {
  CINDER_DOOR,
  CINDER_LAND,
  PAD,
  PAPER_IN_CLAWS,
  PAPER_ON_DESK,
  STUDIO_H,
  STUDIO_W,
  cinderPlace,
  cinderSpot,
  drawMid,
  paperSpot,
  studioAriaLabel,
  type StudioState,
} from "./studio";

const DAY_SKY = { top: "#7ec8ff", bot: "#cdeeff", amb: "#fff2d8", ambA: 0 };

function st(over: Partial<StudioState> = {}): StudioState {
  return {
    phase: skyPhase(new Date(2026, 10, 7, 12, 0)),
    sky: DAY_SKY,
    meetingRecording: false,
    fixesToday: null,
    dictionarySize: null,
    battery: null,
    streakDays: null,
    newsReady: true,
    engineLoading: false,
    modelReady: true,
    screenSharing: false,
    doNotDisturb: false,
    yappy: "rest",
    ...over,
  };
}

function mid(s: StudioState, t = 0): Raster {
  const r = new Raster(STUDIO_W + PAD * 2, STUDIO_H);
  r.translate(PAD, 0);
  drawMid(r, s, t);
  return r;
}

const same = (a: Raster, b: Raster) => Array.from(a.data).join() === Array.from(b.data).join();

describe("yap-1106-news-duo-delivery in the studio", () => {
  it("news_duo_cinder_flies_to_yappys_desk", () => {
    expect(cinderSpot(1)).toEqual({ x: CINDER_LAND.x, y: CINDER_LAND.y });
    // Yappy stands at x 140 behind the desk (x 116..206): she lands over it.
    expect(CINDER_LAND.x).toBeGreaterThanOrEqual(116);
    expect(CINDER_LAND.x).toBeLessThan(206);
    expect(cinderPlace({ cinderFlight: 0.4 }).where).toBe("flying");
    expect(cinderPlace({ cinderFlight: 1, newsDelivered: true })).toEqual({ x: CINDER_LAND.x, y: CINDER_LAND.y, where: "desk" });
    expect(cinderPlace({ newsDelivered: false })).toEqual({ x: CINDER_DOOR.x, y: CINDER_DOOR.y, where: "door" });
  });

  it("news_duo_hands_the_paper_over", () => {
    expect(paperSpot(0)).toEqual({ x: CINDER_LAND.x + PAPER_IN_CLAWS.dx, y: CINDER_LAND.y + PAPER_IN_CLAWS.dy });
    expect(paperSpot(1)).toEqual({ x: PAPER_ON_DESK.x, y: PAPER_ON_DESK.y });
    expect(paperSpot(Number.NaN)).toEqual({ x: PAPER_ON_DESK.x, y: PAPER_ON_DESK.y });
    const m = paperSpot(0.5);
    expect(Number.isInteger(m.x) && Number.isInteger(m.y)).toBe(true);
    const before = mid(st({ newsDelivered: true, cinderFlight: 1, duoHandover: 0 }));
    const after = mid(st({ newsDelivered: true, cinderFlight: 1, duoHandover: 1 }));
    expect(same(before, after)).toBe(false);
    // The paper is on the desk once handed over (its cream face at PAPER_ON_DESK).
    const p = after.px(PAD + PAPER_ON_DESK.x + 3, PAPER_ON_DESK.y);
    expect([p.r, p.g, p.b]).toEqual([0xff, 0xfd, 0xf5]);
  });

  it("news_duo_they_take_turns_talking", () => {
    const base = { newsDelivered: true, cinderFlight: 1, duoHandover: 1 } as const;
    const cinder = mid(st({ ...base, duoSpeaker: "cinder" }), 0);
    const yappy = mid(st({ ...base, duoSpeaker: "yappy" }), 0);
    const nobody = mid(st({ ...base, duoSpeaker: null }), 0);
    expect(same(cinder, yappy)).toBe(false);
    expect(same(yappy, nobody)).toBe(false);
    expect(studioAriaLabel(st({ ...base, duoSpeaker: "cinder" }))).toContain("Cinder is at Yappy's desk, naming the next story.");
    expect(studioAriaLabel(st({ ...base, duoSpeaker: "yappy" }))).toContain("Yappy is explaining the story Cinder brought.");
    expect(studioAriaLabel(st({ ...base }))).toContain("Cinder is at Yappy's desk with the news.");
    expect(studioAriaLabel(st({ cinderFlight: 0.3, newsDelivered: true }))).toContain("Cinder is flying in with the news.");
  });

  it("news_duo_held_for_quiet_waits_at_the_back_door", () => {
    // Home holds a delivery during a meeting, a screen share or DND: not delivered, no flight.
    const s = st({ newsDelivered: false, doNotDisturb: true });
    expect(cinderPlace(s).where).toBe("door");
    expect(studioAriaLabel(s)).toContain("Cinder is at the back door with the news.");
    expect(same(mid(s), mid(st({ newsDelivered: true, cinderFlight: 1, doNotDisturb: true })))).toBe(false);
  });

  it("news_duo_reduce_motion_side_by_side_no_flight", () => {
    // Reduce motion: the room passes cinderFlight 1 and handover 1 at once, so
    // Cinder is drawn at the desk beside Yappy with the paper already down.
    const s = st({ newsDelivered: true, cinderFlight: 1, duoHandover: 1, duoSpeaker: null });
    expect(cinderPlace(s).where).toBe("desk");
    expect(same(mid(s, 0), mid(s, 0))).toBe(true);
  });
});
