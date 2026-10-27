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

  it("the four failure screens are four distinct screens", () => {
    expect(new Set(Object.values(FAILURE_STEP)).size).toBe(4);
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
    const ok = { micAuthorized: true, modelReady: true, accessibility: true };
    expect(classifyTryItFailure({ ...ok, micAuthorized: false, modelReady: false, accessibility: false })).toBe("no-mic");
    expect(classifyTryItFailure({ ...ok, modelReady: false, accessibility: false })).toBe("no-model");
    expect(classifyTryItFailure({ ...ok, accessibility: false })).toBe("no-accessibility");
    expect(classifyTryItFailure(ok)).toBe("paste-refused");
  });

  it("a take lands only when the field holds the transcript", () => {
    expect(tryItLanded("Hello  there, Yap.", "hello there, yap.")).toBe(true);
    expect(tryItLanded("I typed this", "Hello there")).toBe(false);
    expect(tryItLanded("", "Hello")).toBe(false);
    expect(tryItLanded("anything", "   ")).toBe(false);
  });

  it("an unproven finish raises the reminder until a take pastes", () => {
    expect(unprovenLine({ onboarded: true, dictationProven: false })).toMatch(/hasn't pasted/);
    expect(unprovenLine({ onboarded: true, dictationProven: true })).toBeNull();
    expect(unprovenLine({ onboarded: false, dictationProven: false })).toBeNull();
    expect(unprovenLine(null)).toBeNull();
  });
});
