/** yap-call-1 — the call offer's pure rules (surface, countdown, labels). */
import { describe, expect, it } from "vitest";
import { CONSENT_PILL_LINE } from "../meetings/consent";
import { recordLabel } from "./meeting";
import {
  COUNTDOWN_MS,
  acceptArgs,
  badgeCallLabel,
  callOfferLines,
  chipLabel,
  chooseArgs,
  isCallOfferView,
  neverLabel,
  secondsLeft,
  surfaceAt,
  type CallOfferView,
} from "./callOfferModel";

const V: CallOfferView = {
  phase: "offering",
  offerId: "app:us.zoom.xos:1#1",
  appName: "Zoom",
  bundleId: "us.zoom.xos",
  copy: "Zoom call — record it?",
  siteLine: null,
  yourSideOnly: false,
  countdownMsLeft: COUNTDOWN_MS,
};

describe("callOfferModel", () => {
  it("validates the wire shape", () => {
    expect(isCallOfferView(V)).toBe(true);
    expect(isCallOfferView(null)).toBe(false);
    expect(isCallOfferView({ ...V, phase: "auto" })).toBe(false);
    expect(isCallOfferView({ ...V, offerId: 3 })).toBe(false);
  });

  it("offer for 10 s, then the chip; ended -> ended; idle/recording -> nothing", () => {
    expect(surfaceAt(V, 0)).toBe("offer");
    expect(surfaceAt(V, COUNTDOWN_MS - 1)).toBe("offer");
    expect(surfaceAt(V, COUNTDOWN_MS)).toBe("chip");
    expect(surfaceAt({ ...V, countdownMsLeft: 0 }, COUNTDOWN_MS)).toBe("chip");
    expect(surfaceAt({ ...V, phase: "chip" }, 0)).toBe("chip");
    expect(surfaceAt({ ...V, phase: "end_prompt" }, 0)).toBe("ended");
    expect(surfaceAt({ ...V, phase: "idle" }, 0)).toBeNull();
    expect(surfaceAt({ ...V, phase: "recording" }, 0)).toBeNull();
    expect(surfaceAt(null, 0)).toBeNull();
  });

  it("seconds left on the ring", () => {
    expect(secondsLeft(V, 0)).toBe(10);
    expect(secondsLeft(V, 9_001)).toBe(1);
    expect(secondsLeft(V, 12_000)).toBe(0);
    expect(secondsLeft({ ...V, countdownMsLeft: 0 }, 0)).toBe(10);
  });

  it("lines, labels and args", () => {
    expect(callOfferLines(V)).toEqual({ headline: "Zoom call — record it?", site: null, consent: CONSENT_PILL_LINE });
    expect(callOfferLines({ ...V, copy: null, appName: null }).headline).toBe("Browser call — record it?");
    expect(chipLabel(V)).toBe("Record Zoom call");
    expect(recordLabel({ recording: false, elapsedSeconds: 0, captureAvailable: true }, null)).toBe(
      "Record this meeting",
    );
    expect(neverLabel(V)).toBe("Never for Zoom");
    expect(neverLabel({ ...V, appName: null })).toBe("Never for this app");
    expect(acceptArgs(V)).toEqual({ offerId: V.offerId });
    expect(chooseArgs(V, "not_now")).toEqual({ offerId: V.offerId, choice: "not_now" });
  });

  it("the badge names the app while an offer-started recording runs; FaceTime says your side", () => {
    expect(badgeCallLabel({ ...V, phase: "recording" })).toBe("Zoom");
    expect(badgeCallLabel({ ...V, phase: "end_prompt", appName: "FaceTime", yourSideOnly: true })).toBe(
      "FaceTime · your side",
    );
    expect(badgeCallLabel(V)).toBeNull();
    expect(badgeCallLabel({ ...V, phase: "recording", appName: null })).toBeNull();
    expect(badgeCallLabel(null)).toBeNull();
  });
});
