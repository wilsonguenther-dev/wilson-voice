import { describe, expect, it } from "vitest";
import {
  VIEW_IDS,
  VIEW_STATES,
  type ViewId,
  hasEmptyState,
  isEmptyData,
  loadingLabel,
  viewErrorText,
  viewState,
} from "./viewState";

/**
 * Y5-B — the 3x7 matrix.
 *
 * The point of extracting `viewState` was that these assertions must not
 * require rendering App.tsx. So this file walks all seven views against the
 * function, and the only thing it needs from the views themselves is the
 * shape of the payload each one holds.
 */

/** What each view actually passes to `viewState` as its `data` argument. */
const PAYLOAD: Record<ViewId, { empty: unknown; ready: unknown }> = {
  // `history` — TranscriptEntry[]
  home: { empty: [], ready: [{ id: "t1" }] },
  // `perms.rows` — always seven rows; an empty report is a failed read.
  permissions: { empty: [], ready: [{ id: "mic" }] },
  // `meetings` — Meeting[]
  meetings: { empty: [], ready: [{ id: "m1" }] },
  // Insights passes a COUNT: `insights.totalSessions`. Zero takes is the
  // blank-screen case the item names by hand.
  insights: { empty: 0, ready: 42 },
  // `dictionary` — DictEntry[]
  dictionary: { empty: [], ready: [{ id: "d1" }] },
  // `scratch` — ScratchNote[]
  scratchpad: { empty: [], ready: [{ id: "n1" }] },
  // `settings` — a single AppSettings object; it is never "empty".
  settings: { empty: { hotkeyLabel: "fn" }, ready: { hotkeyLabel: "fn" } },
};

describe("viewState — the 3x7 matrix", () => {
  for (const view of VIEW_IDS) {
    describe(view, () => {
      it("is loading while its data has not arrived", () => {
        expect(viewState(null, false, null)).toBe("loading");
      });

      it("is loading while a refetch is explicitly in flight", () => {
        expect(viewState(PAYLOAD[view].ready, true, null)).toBe("loading");
      });

      it("is error when the read failed, even over stale data", () => {
        expect(viewState(PAYLOAD[view].ready, false, "disk is full")).toBe(
          "error",
        );
        // The lie this closes: a failed refetch under an empty list used to
        // render as "you have nothing", because loadMeetings swallowed the
        // rejection into console.error.
        expect(viewState(PAYLOAD[view].empty, false, "disk is full")).toBe(
          "error",
        );
      });

      it("error outranks loading", () => {
        expect(viewState(null, true, "boom")).toBe("error");
      });

      it("is ready when it has something to show", () => {
        expect(viewState(PAYLOAD[view].ready, false, null)).toBe("ready");
      });

      it("names what failed in the user's terms, not a code", () => {
        const text = viewErrorText(view);
        expect(text.startsWith("Yap could not")).toBe(true);
        expect(text.endsWith(".")).toBe(true);
        // Copy rules: sentence case, no exclamation marks.
        expect(text).not.toContain("!");
        // Never an engine code in the headline.
        expect(text).not.toMatch(/SQLITE|errno|0x[0-9a-f]+/i);
      });

      it("reaches exactly the states it enumerates", () => {
        const states = VIEW_STATES[view];
        expect(states).toContain("loading");
        expect(states).toContain("error");
        expect(states).toContain("ready");

        const settled = viewState(PAYLOAD[view].empty, false, null);
        if (hasEmptyState(view)) {
          expect(settled).toBe("empty");
        } else {
          // PANEL 2026-09-12: permissions and settings have no empty state.
          // Their settled state is "ready" — "nothing to fix here" — and a
          // permissions report with zero rows is an error, not an empty state.
          expect(states).not.toContain("empty");
          expect(settled === "ready" || settled === "empty").toBe(true);
        }
      });
    });
  }

  it("covers all seven views and no more", () => {
    expect(VIEW_IDS).toHaveLength(7);
    expect(Object.keys(VIEW_STATES).sort()).toEqual([...VIEW_IDS].sort());
  });

  it("gives the five list views an empty state and the other two none", () => {
    const withEmpty = VIEW_IDS.filter(hasEmptyState);
    expect(withEmpty.sort()).toEqual(
      ["dictionary", "home", "insights", "meetings", "scratchpad"].sort(),
    );
    expect(hasEmptyState("permissions")).toBe(false);
    expect(hasEmptyState("settings")).toBe(false);
  });
});

describe("insights — the named blank-page case", () => {
  it("zero takes is EMPTY, never a blank screen", () => {
    // App.tsx used to render `nav === "insights" && insights && <Insights/>`
    // and Insights.tsx opened with `if (!insights) return null`. Both of
    // those produced a heading over nothing. Neither state is reachable now:
    // null is loading, zero is empty.
    expect(viewState(null, false, null)).toBe("loading");
    expect(viewState(0, false, null)).toBe("empty");
    expect(viewState(1, false, null)).toBe("ready");
  });

  it("an insights read that failed is an error, not zero takes", () => {
    expect(viewState(0, false, "db locked")).toBe("error");
    expect(viewErrorText("insights")).toBe(
      "Yap could not read your dictation stats.",
    );
  });
});

describe("isEmptyData", () => {
  it("treats a list by length", () => {
    expect(isEmptyData([])).toBe(true);
    expect(isEmptyData([1])).toBe(false);
  });

  it("treats a count by value", () => {
    expect(isEmptyData(0)).toBe(true);
    expect(isEmptyData(-1)).toBe(true);
    expect(isEmptyData(NaN)).toBe(true);
    expect(isEmptyData(3)).toBe(false);
  });

  it("treats a string by its trimmed length", () => {
    expect(isEmptyData("")).toBe(true);
    expect(isEmptyData("   ")).toBe(true);
    expect(isEmptyData("x")).toBe(false);
  });

  it("does not call a populated object empty", () => {
    expect(isEmptyData({ hotkeyLabel: "fn" })).toBe(false);
  });

  it("handles Map and Set", () => {
    expect(isEmptyData(new Map())).toBe(true);
    expect(isEmptyData(new Set([1]))).toBe(false);
  });
});

describe("viewState error argument shapes", () => {
  it("an empty string is not an error", () => {
    expect(viewState([1], false, "")).toBe("ready");
  });

  it("a structured CommandError is an error", () => {
    expect(viewState([1], false, { code: "db", message: "locked" })).toBe(
      "error",
    );
  });

  it("null and undefined are not errors", () => {
    expect(viewState([1], false, null)).toBe("ready");
    expect(viewState([1], false, undefined)).toBe("ready");
    expect(viewState([1])).toBe("ready");
  });
});

describe("loadingLabel", () => {
  it("is determinate when the count is knowable", () => {
    expect(loadingLabel("takes", 1280)).toBe("Reading 1,280 takes…");
  });

  it("is calm and indeterminate when it is not", () => {
    expect(loadingLabel("history", null)).toBe("Opening your history…");
    expect(loadingLabel("history")).toBe("Opening your history…");
    expect(loadingLabel("history", 0)).toBe("Opening your history…");
  });

  it("never shouts", () => {
    expect(loadingLabel("notes", 3)).not.toContain("!");
  });
});
