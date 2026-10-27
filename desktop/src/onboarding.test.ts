import { describe, expect, it } from "vitest";
import {
  FAILURE_STEP,
  STEP_ORDER,
  classifyTryItFailure,
  completionFor,
  isFailureStep,
  nextStep,
  progressStep,
  tryItLanded,
  tryItSucceeded,
  unprovenLine,
  type Outcome,
  type Step,
  type TryItFailure,
} from "./onboarding";

const NEXT: Outcome = { kind: "next" };
const BACK: Outcome = { kind: "back" };
const SUCCESS: Outcome = { kind: "success" };
const SKIP: Outcome = { kind: "skip" };
const RETRY: Outcome = { kind: "retry" };
const fail = (failure: TryItFailure): Outcome => ({ kind: "failure", failure });

const ALL_STEPS: Step[] = [
  "welcome",
  "permissions",
  "calibration",
  "try-it",
  "try-it-no-mic",
  "try-it-no-accessibility",
  "try-it-no-model",
  "try-it-paste-refused",
  "done",
];
const ALL_OUTCOMES: Outcome[] = [
  NEXT,
  BACK,
  SUCCESS,
  SKIP,
  RETRY,
  fail("no-mic"),
  fail("no-accessibility"),
  fail("no-model"),
  fail("paste-refused"),
];

describe("Y6-A onboarding step machine", () => {
  it("the spine ends in try-it before done", () => {
    expect(STEP_ORDER).toEqual(["welcome", "permissions", "calibration", "try-it", "done"]);
  });

  it("happy path: welcome → permissions → calibration → try-it → success → done", () => {
    let s: Step = "welcome";
    s = nextStep(s, NEXT);
    expect(s).toBe("permissions");
    s = nextStep(s, NEXT);
    expect(s).toBe("calibration");
    s = nextStep(s, NEXT);
    expect(s).toBe("try-it");
    // "next" alone never leaves try-it: only a landed take or a skip does.
    expect(nextStep(s, NEXT)).toBe("try-it");
    s = nextStep(s, SUCCESS);
    expect(s).toBe("done");
  });

  it.each([
    ["no-mic", "try-it-no-mic"],
    ["no-accessibility", "try-it-no-accessibility"],
    ["no-model", "try-it-no-model"],
    ["paste-refused", "try-it-paste-refused"],
    ["no-speech", "try-it-no-speech"],
    ["auto-paste-off", "try-it-auto-paste-off"],
  ] as const)("failure %s routes to its own screen %s", (failure, screen) => {
    expect(nextStep("try-it", fail(failure))).toBe(screen);
    expect(FAILURE_STEP[failure]).toBe(screen);
    expect(isFailureStep(screen)).toBe(true);
    expect(progressStep(screen)).toBe("try-it");
    // Retry from the failure screen returns to the field, never to done.
    expect(nextStep(screen, RETRY)).toBe("try-it");
    // A take that lands after the fix is still a success.
    expect(nextStep(screen, SUCCESS)).toBe("done");
  });

  it("the six failure screens are six distinct screens", () => {
    expect(new Set(Object.values(FAILURE_STEP)).size).toBe(6);
  });

  it("skip ends onboarding from any step", () => {
    for (const s of ALL_STEPS) expect(nextStep(s, SKIP)).toBe("done");
  });

  it("back walks the spine", () => {
    expect(nextStep("try-it", BACK)).toBe("calibration");
    expect(nextStep("calibration", BACK)).toBe("permissions");
    expect(nextStep("permissions", BACK)).toBe("welcome");
    expect(nextStep("done", BACK)).toBe("try-it");
  });

  it("done is reachable only through a success or a skip", () => {
    for (const s of ALL_STEPS) {
      if (s === "done") continue;
      for (const o of ALL_OUTCOMES) {
        if (nextStep(s, o) === "done") expect(["success", "skip"]).toContain(o.kind);
      }
    }
  });

  it("onboarded_is_only_set_after_a_success_or_an_explicit_skip", () => {
    for (const o of ALL_OUTCOMES) {
      const c = completionFor(o);
      if (o.kind === "success") expect(c).toEqual({ onboarded: true, dictationProven: true });
      else if (o.kind === "skip") expect(c).toEqual({ onboarded: true, dictationProven: false });
      else expect(c).toBeNull();
    }
  });

  it("classifies the earliest broken stage first", () => {
    const ok = { micAuthorized: true, modelReady: true, heardText: true, autoPaste: true, accessibility: true };
    expect(classifyTryItFailure({ ...ok, micAuthorized: false, modelReady: false, accessibility: false })).toBe("no-mic");
    expect(classifyTryItFailure({ ...ok, modelReady: false, accessibility: false })).toBe("no-model");
    expect(classifyTryItFailure({ ...ok, accessibility: false })).toBe("no-accessibility");
    expect(classifyTryItFailure(ok)).toBe("paste-refused");
  });

  // Fix round 1 (primary BLOCKING 2): a take with NO words never reached the
  // paste stage — a no-speech gate drop, a cancel, a rejected/silent capture,
  // an ASR error — so it must not land on "The text didn't land".
  it("a take that produced no text is never blamed on the paste", () => {
    const ok = { micAuthorized: true, modelReady: true, heardText: true, autoPaste: true, accessibility: true };
    expect(classifyTryItFailure({ ...ok, heardText: false })).toBe("no-speech");
    expect(classifyTryItFailure({ ...ok, heardText: false, accessibility: false })).toBe("no-speech");
    expect(classifyTryItFailure({ ...ok, heardText: false, autoPaste: false })).toBe("no-speech");
    // Earlier stages still win over it.
    expect(classifyTryItFailure({ ...ok, heardText: false, micAuthorized: false })).toBe("no-mic");
    expect(classifyTryItFailure({ ...ok, heardText: false, modelReady: false })).toBe("no-model");
  });

  // Fix round 1 (primary BLOCKING 1): auto-paste OFF is the user's setting,
  // not a refused paste — name it, ahead of the Accessibility grant.
  it("auto-paste off is named as the cause, not the cursor", () => {
    const ok = { micAuthorized: true, modelReady: true, heardText: true, autoPaste: true, accessibility: true };
    expect(classifyTryItFailure({ ...ok, autoPaste: false })).toBe("auto-paste-off");
    expect(classifyTryItFailure({ ...ok, autoPaste: false, accessibility: false })).toBe("auto-paste-off");
  });

  it("a take lands only when the field holds the transcript", () => {
    expect(tryItLanded("Hello  there, Yap.", "hello there, yap.", "")).toBe(true);
    expect(tryItLanded("I typed this", "Hello there", "")).toBe(false);
    expect(tryItLanded("", "Hello", "")).toBe(false);
    expect(tryItLanded("anything", "   ", "")).toBe(false);
  });

  // Fix round 1 (independent BLOCKING 1): text that was ALREADY in the field
  // when the take started proves nothing about this take.
  it("text already in the field before the take does not count as landed", () => {
    // The reviewer's three probes, now with the field's pre-take content.
    expect(tryItLanded("Testing one two three.", "Testing one two three.", "Testing one two three.")).toBe(false);
    expect(tryItLanded("I typed this myself", "I", "I typed this myself")).toBe(false);
    expect(tryItLanded("thank you so much", "Thank you", "thank you so much")).toBe(false);
    // A real paste on top of the hand-pasted copy is a new occurrence: lands.
    expect(
      tryItLanded("Testing one two three. Testing one two three.", "Testing one two three.", "Testing one two three."),
    ).toBe(true);
    // A paste into an empty field still lands.
    expect(tryItLanded("thank you", "Thank you", "")).toBe(true);
  });

  it("an unproven finish raises the reminder until a take pastes", () => {
    expect(unprovenLine({ onboarded: true, dictationProven: false })).toMatch(/hasn't pasted/);
    expect(unprovenLine({ onboarded: true, dictationProven: true })).toBeNull();
    expect(unprovenLine({ onboarded: false, dictationProven: false })).toBeNull();
    expect(unprovenLine(null)).toBeNull();
  });

  // Fix round 1 (primary BLOCKING 1): with auto-paste OFF no take can paste
  // (paste::should_paste requires auto_paste), so the reminder would never clear.
  it("the reminder stays down while the user's auto-paste setting is off", () => {
    expect(unprovenLine({ onboarded: true, dictationProven: false, autoPaste: false })).toBeNull();
    expect(unprovenLine({ onboarded: true, dictationProven: false, autoPaste: true })).toMatch(/hasn't pasted/);
  });

  // Fix round 2 (independent BLOCKING 1): success needs the backend's OWN paste
  // receipt for this take. A copy-only take (want_paste false, so no
  // paste_failed) followed by a manual Cmd-V puts the transcript in the field —
  // that must never read as a proven dictation.
  it("a transcript the user pasted by hand is never a proven dictation", () => {
    const take = { doneOk: true, pasted: false, pasteFailed: false, transcript: "Hello there" };
    expect(tryItSucceeded(take, true, "Hello there", "")).toBe(false);
  });

  it("a take succeeds only with a receipt AND the text in the field", () => {
    const ok = { doneOk: true, pasted: true, pasteFailed: false, transcript: "Hello there" };
    expect(tryItSucceeded(ok, true, "Hello there", "")).toBe(true);
    // Receipt but the text never reached THIS field (pasted somewhere else).
    expect(tryItSucceeded(ok, true, "", "")).toBe(false);
    // Each other gate still fails closed.
    expect(tryItSucceeded({ ...ok, doneOk: false }, true, "Hello there", "")).toBe(false);
    expect(tryItSucceeded({ ...ok, pasteFailed: true }, true, "Hello there", "")).toBe(false);
    expect(tryItSucceeded({ ...ok, transcript: null }, true, "Hello there", "")).toBe(false);
    expect(tryItSucceeded(ok, false, "Hello there", "")).toBe(false);
    // A payload from an older backend with no `pasted` field fails closed.
    expect(tryItSucceeded({ ...ok, pasted: undefined }, true, "Hello there", "")).toBe(false);
  });
});
