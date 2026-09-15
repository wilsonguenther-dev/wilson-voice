import { describe, expect, it } from "vitest";
import {
  describePolishOffer,
  POLISH_OFFER_KINDS,
  type PolishOfferInput,
} from "./polishOffer";

const base: PolishOfferInput = {
  speechModelReady: true,
  catalogSize: 2,
  polishActive: false,
  polishDownloading: null,
};

describe("describePolishOffer", () => {
  it("offers the optional download once the speech model is ready", () => {
    const o = describePolishOffer(base);
    expect(o.kind).toBe("offer");
    expect(o.showPicker).toBe(true);
    // The three facts a 1.1 GB offer is a trap without.
    expect(o.title.toLowerCase()).toContain("optional");
    expect(o.body.toLowerCase()).toContain("download");
    expect(o.body.toLowerCase()).toContain("skip");
  });

  it("an empty catalog says nothing rather than an empty row", () => {
    const o = describePolishOffer({ ...base, catalogSize: 0 });
    expect(o.kind).toBe("hidden");
    expect(o.showPicker).toBe(false);
    expect(o.title).toBe("");
  });

  it("the offer waits while the speech model is still downloading", () => {
    const o = describePolishOffer({ ...base, speechModelReady: false });
    expect(o.kind).toBe("deferred");
    // The whole point: no button exists that could race the download the user
    // actually needs.
    expect(o.showPicker).toBe(false);
    expect(o.body.toLowerCase()).toContain("settings");
  });

  it("an installed polish model reports itself active and stays turn-off-able", () => {
    const o = describePolishOffer({ ...base, polishActive: true });
    expect(o.kind).toBe("active");
    expect(o.showPicker).toBe(true);
  });

  it("an installed model outranks the speech-model deferral", () => {
    const o = describePolishOffer({
      ...base,
      polishActive: true,
      speechModelReady: false,
    });
    expect(o.kind).toBe("active");
    expect(o.showPicker).toBe(true);
  });

  it("a download in flight keeps its progress in every other state", () => {
    for (const speechModelReady of [true, false]) {
      for (const polishActive of [true, false]) {
        for (const catalogSize of [0, 2]) {
          const o = describePolishOffer({
            speechModelReady,
            polishActive,
            catalogSize,
            polishDownloading: "qwen2.5-1.5b-instruct-q4_k_m",
          });
          expect(o.kind).toBe("downloading");
          expect(o.showPicker).toBe(true);
        }
      }
    }
  });

  it("no state onboarding can reach starts a download on its own", () => {
    // SEC-C: "Do NOT download a model during onboarding by default." This
    // module can never ASK for bytes — it only decides whether a button the
    // user must press is allowed on screen, and it forbids even that button
    // until the speech model is done or the user already started something.
    const seen = new Set<string>();
    for (const speechModelReady of [true, false]) {
      for (const polishActive of [true, false]) {
        for (const catalogSize of [0, 2]) {
          for (const polishDownloading of [null, "qwen2.5-1.5b-instruct-q4_k_m"]) {
            const o = describePolishOffer({
              speechModelReady,
              polishActive,
              catalogSize,
              polishDownloading,
            });
            seen.add(o.kind);
            expect(POLISH_OFFER_KINDS).toContain(o.kind);
            expect(Object.keys(o).sort()).toEqual([
              "body",
              "kind",
              "showPicker",
              "title",
            ]);
            if (o.kind === "hidden" || o.kind === "deferred") {
              expect(o.showPicker).toBe(false);
            }
            if (o.showPicker) expect(o.title.length).toBeGreaterThan(0);
          }
        }
      }
    }
    // Every kind the module declares is actually reachable — a state nothing
    // can produce is dead copy.
    expect([...seen].sort()).toEqual([...POLISH_OFFER_KINDS].sort());
  });
});
