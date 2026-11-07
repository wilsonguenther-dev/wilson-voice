// yap-1106-news-home-briefing — Cinder flies in on a new briefing; reduce motion skips the flight.
import { describe, expect, it } from "vitest";
import { CINDER_FLIGHT_MS, CINDER_FROM, CINDER_LAND, cinderFlight, cinderSpot, redrawPlan } from "./studio";

describe("yap-1106-news-home-briefing Cinder's flight", () => {
  it("home_briefing_cinder_flies_in_on_a_new_briefing", () => {
    expect(cinderFlight(1000, 1000, false)).toBe(0);
    expect(cinderFlight(1000, 1000 + CINDER_FLIGHT_MS / 2, false)).toBeCloseTo(0.5);
    expect(cinderFlight(1000, 1000 + CINDER_FLIGHT_MS * 2, false)).toBe(1);
    expect(cinderSpot(0)).toEqual({ x: CINDER_FROM.x, y: CINDER_FROM.y });
    expect(cinderSpot(1)).toEqual({ x: CINDER_LAND.x, y: CINDER_LAND.y });
    const mid = cinderSpot(0.5);
    expect(Number.isInteger(mid.x) && Number.isInteger(mid.y)).toBe(true);
    expect(redrawPlan({ reduceMotion: false, appState: "idle", reacting: false, parallaxSettling: false, meetingRecording: false, cinderFlying: true }).mode).toBe("live");
  });

  it("home_briefing_reduce_motion_skips_the_flight", () => {
    expect(cinderFlight(1000, 1000, true)).toBe(1);
    expect(cinderFlight(null, 5000, false)).toBe(1);
    expect(cinderFlight(9000, 1000, false)).toBe(1);
    expect(redrawPlan({ reduceMotion: true, appState: "idle", reacting: false, parallaxSettling: false, meetingRecording: false, cinderFlying: true }).mode).toBe("still");
  });
});
