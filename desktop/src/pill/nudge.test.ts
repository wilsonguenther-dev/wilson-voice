import { describe, expect, it } from "vitest";
import {
  FLUENT_AFTER_TAKES,
  MAX_NUDGES_PER_SESSION,
  NUDGE_IDS,
  NUDGE_WINDOW_DAYS,
  daysSince,
  isTakeContext,
  nextNudge,
  nudge,
  type NudgeContext,
  type NudgeShown,
  type TakeContextPayload,
} from "./nudge";
import { NUDGE_LINES } from "./tone";
import { PHASE_COPY, fitsSideDockStrip, type ChatTone } from "./live";
import nudgeRs from "../../src-tauri/src/nudge.rs?raw";
import libRs from "../../src-tauri/src/lib.rs?raw";

const TONES: readonly ChatTone[] = ["rude", "friendly", "rose"];

const noTarget: TakeContextPayload = { textFieldFocused: false, appCategory: "plain", pasted: false };
const inField: TakeContextPayload = { textFieldFocused: true, appCategory: "chat", pasted: true };
const inEditor: TakeContextPayload = { textFieldFocused: true, appCategory: "code", pasted: true };

const ctx = (over: Partial<NudgeContext> = {}): NudgeContext => ({
  takeLive: false,
  lastTake: inField,
  takesSoFar: 1,
  daysSinceInstall: 0,
  unusedFeatures: [],
  ...over,
});
const fresh = (): NudgeShown => ({ thisSession: 0, ever: new Set() });

/** Drive the engine the way NudgeRow does: show, then record. */
function show(c: NudgeContext, s: NudgeShown): { id: string | null; shown: NudgeShown } {
  const n = nextNudge(c, s);
  if (!n) return { id: null, shown: s };
  return { id: n.id, shown: { thisSession: s.thisSession + 1, ever: new Set([...s.ever, n.id]) } };
}

// Anything that reads as commerce. Deliberately broad: a false positive costs a
// copy edit, a false negative ships a nag.
const MONEY = /[$£€¥]|\b(pay|paid|price|pricing|cost|buy|purchase|upgrade|subscribe|subscription|trial|licen[cs]e|plan|pro|premium|unlock|discount|deal|free)\b/i;

describe("Y8-D coaching nudges — the caps are the feature", () => {
  it("the caps are named constants with the values the item promises", () => {
    expect(MAX_NUDGES_PER_SESSION).toBe(1);
    expect(NUDGE_WINDOW_DAYS).toBe(7);
    expect(FLUENT_AFTER_TAKES).toBeGreaterThan(0);
  });

  it("at_most_one_nudge_per_session", () => {
    // Two conditions hold at once (no target AND meetings unused): only one shows.
    let s = fresh();
    const c = ctx({ lastTake: noTarget, takesSoFar: 5, unusedFeatures: ["meetings"] });
    const first = show(c, s);
    expect(first.id).toBe("no-text-field");
    s = first.shown;
    // The very next take, in the same session, satisfies a different condition.
    expect(nextNudge(ctx({ lastTake: inEditor, takesSoFar: 6, unusedFeatures: ["meetings"] }), s)).toBeNull();
    // A NEW session (counter reset, ledger kept) may teach the next lesson.
    const nextSession = { thisSession: 0, ever: s.ever };
    expect(nextNudge(ctx({ lastTake: inEditor, takesSoFar: 6 }), nextSession)?.id).toBe("code-editor");
  });

  it("never_fires_twice_for_one_condition", () => {
    let ever = new Set<string>();
    for (let session = 0; session < 5; session++) {
      const r = show(ctx({ lastTake: noTarget }), { thisSession: 0, ever });
      if (session === 0) expect(r.id).toBe("no-text-field");
      else expect(r.id).toBeNull();
      ever = new Set(r.shown.ever);
    }
    // And for every condition, not just the first.
    for (const id of NUDGE_IDS) {
      const once = new Set<string>([id]);
      const c = ctx({
        lastTake: id === "no-text-field" ? noTarget : id === "code-editor" ? inEditor : inField,
        takesSoFar: 5,
        unusedFeatures: id === "meetings" ? ["meetings"] : [],
      });
      expect(nextNudge(c, fresh())?.id).toBe(id);
      expect(nextNudge(c, { thisSession: 0, ever: once })).toBeNull();
    }
  });

  it("silent_after_the_caps: a week old, or fluent", () => {
    const c = ctx({ lastTake: noTarget });
    expect(nextNudge(c, fresh())?.id).toBe("no-text-field");
    expect(nextNudge({ ...c, daysSinceInstall: NUDGE_WINDOW_DAYS }, fresh())).toBeNull();
    expect(nextNudge({ ...c, daysSinceInstall: NUDGE_WINDOW_DAYS - 0.01 }, fresh())?.id).toBe("no-text-field");
    expect(nextNudge({ ...c, takesSoFar: FLUENT_AFTER_TAKES }, fresh())).toBeNull();
    expect(nextNudge({ ...c, takesSoFar: FLUENT_AFTER_TAKES - 1 }, fresh())?.id).toBe("no-text-field");
  });

  it("the_no_focus_nudge_fires_on_the_first_no_target_take", () => {
    // A brand-new install: the first take this session had nowhere to land.
    const n = nextNudge(ctx({ lastTake: noTarget, takesSoFar: 1 }), fresh());
    expect(n?.id).toBe("no-text-field");
    for (const t of TONES) expect(n?.copy[t]).toMatch(/text box/);
    // It outranks every other lesson that holds at the same moment.
    const crowded = ctx({ lastTake: { ...noTarget, appCategory: "code" }, takesSoFar: 5, unusedFeatures: ["meetings"] });
    expect(nextNudge(crowded, fresh())?.id).toBe("no-text-field");
    // A take that DID land in a field never earns it.
    expect(nextNudge(ctx({ lastTake: inField }), fresh())).toBeNull();
  });

  it("no_nudge_mentions_money", () => {
    // Prove the pattern can match the shape we fear: the gated phase's rude line
    // IS a money line, and the detector must catch it.
    expect(MONEY.test(PHASE_COPY.gated.rude)).toBe(true);
    expect(MONEY.test("Upgrade to Pro for $8/mo")).toBe(true);
    for (const id of NUDGE_IDS) {
      for (const t of TONES) {
        expect(NUDGE_LINES[id][t], `${id}/${t}`).not.toMatch(MONEY);
      }
    }
  });

  it("every_nudge_has_copy_in_all_three_tones", () => {
    expect(Object.keys(NUDGE_LINES).sort()).toEqual([...NUDGE_IDS].sort());
    for (const id of NUDGE_IDS) {
      const n = nudge(id);
      for (const t of TONES) {
        expect(n.copy[t].trim().length, `${id}/${t}`).toBeGreaterThan(0);
        // It shares the strip's budget: one line that fits a side dock.
        expect(fitsSideDockStrip(n.copy[t]), `${id}/${t} fits the side-dock strip`).toBe(true);
        // No key is named: every chord is rebindable and the default is fn⌃.
        expect(n.copy[t], `${id}/${t}`).not.toMatch(/\bfn\b|⌃|⌘|⌥|hold /i);
      }
      // Three tones are three voices, not one line copied three times.
      expect(new Set(TONES.map((t) => n.copy[t])).size).toBe(3);
    }
  });

  it("none_fires_during_a_take", () => {
    for (const lastTake of [noTarget, inEditor, inField]) {
      const c = ctx({ takeLive: true, lastTake, takesSoFar: 5, unusedFeatures: ["meetings"] });
      expect(nextNudge(c, fresh())).toBeNull();
    }
  });

  it("nothing fires before this session has had a take", () => {
    expect(nextNudge(ctx({ lastTake: null, takesSoFar: 5, unusedFeatures: ["meetings"] }), fresh())).toBeNull();
  });

  it("the meetings lesson waits for familiarity and stops once a meeting exists", () => {
    expect(nextNudge(ctx({ takesSoFar: 1, unusedFeatures: ["meetings"] }), fresh())).toBeNull();
    expect(nextNudge(ctx({ takesSoFar: 3, unusedFeatures: ["meetings"] }), fresh())?.id).toBe("meetings");
    expect(nextNudge(ctx({ takesSoFar: 3, unusedFeatures: [] }), fresh())).toBeNull();
  });

  it("daysSince and isTakeContext are total", () => {
    expect(daysSince(0, 1e12)).toBe(0);
    expect(daysSince(1_000, 1_000 + 2 * 86_400_000)).toBe(2);
    expect(daysSince(5_000, 1_000)).toBe(0);
    expect(isTakeContext(noTarget)).toBe(true);
    expect(isTakeContext({ textFieldFocused: "no" })).toBe(false);
    expect(isTakeContext(null)).toBe(false);
  });

  it("the event name and payload fields match nudge.rs (no compiler checks this seam)", () => {
    const rs = nudgeRs;
    expect(rs).toMatch(/pub const TAKE_CONTEXT_EVENT: &str = "take_context";/);
    expect(rs).toMatch(/#\[serde\(rename_all = "camelCase"\)\]\s*pub struct TakeContext/);
    for (const f of ["text_field_focused", "app_category", "pasted"]) expect(rs).toContain(`pub ${f}:`);
    const lib = libRs;
    expect(lib).toMatch(/nudge::TAKE_CONTEXT_EVENT/);
    expect(lib).toMatch(/generate_handler!\[[\s\S]*\bnudge_ledger,[\s\S]*\bnudge_mark_shown,/);
  });
});
