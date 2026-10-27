// Y6-A — the onboarding step machine, extracted from Onboarding.tsx so the
// routing is testable without a webview.
//
// Before Y6-A the flow was welcome → permissions → calibration → done, and
// calibration only RECORDED a sample: nothing ever pasted. A user could finish
// setup without once seeing Yap's one trick work, and the four things that can
// break it — the microphone grant, the Accessibility grant, a missing speech
// model, a refused paste — each failed later, separately, with no context.
//
// Now the last real step is TRY IT: the user clicks into a field inside Yap's
// own window, dictates, and watches the text arrive. Success is the only road
// to "done" that carries proof; "Skip for now" is the other, and it is honest
// about it (`completionFor` returns `dictationProven: false`, which keeps the
// main window's health row raised until a take actually pastes).

/** Every screen the onboarding overlay can show. */
export type Step =
  | "welcome"
  | "permissions"
  | "calibration"
  | "try-it"
  /** Four dead ends, four screens, each with the one button that fixes it. */
  | "try-it-no-mic"
  | "try-it-no-accessibility"
  | "try-it-no-model"
  | "try-it-paste-refused"
  | "done";

/** Why a try-it take did not land in the field. */
export type TryItFailure =
  | "no-mic"
  | "no-accessibility"
  | "no-model"
  | "paste-refused";

/** What just happened on the current step. */
export type Outcome =
  | { kind: "next" }
  | { kind: "back" }
  /** The dictated text arrived in the try-it field. */
  | { kind: "success" }
  | { kind: "failure"; failure: TryItFailure }
  /** From a failure screen: go back to the try-it field and take again. */
  | { kind: "retry" }
  /** "Skip for now" — leaves onboarding without proof that dictation works. */
  | { kind: "skip" };

/** The linear spine the progress dots draw. Failure screens sit on "try-it". */
export const STEP_ORDER: readonly Step[] = [
  "welcome",
  "permissions",
  "calibration",
  "try-it",
  "done",
];

/** The four failure screens, keyed by the failure that leads to each. */
export const FAILURE_STEP: Record<TryItFailure, Step> = {
  "no-mic": "try-it-no-mic",
  "no-accessibility": "try-it-no-accessibility",
  "no-model": "try-it-no-model",
  "paste-refused": "try-it-paste-refused",
};

export function isFailureStep(step: Step): boolean {
  return (Object.values(FAILURE_STEP) as Step[]).includes(step);
}

/** Which progress dot a step lights: failure screens belong to "try-it". */
export function progressStep(step: Step): Step {
  return isFailureStep(step) ? "try-it" : step;
}

/**
 * The step machine. A pure function of (step, outcome): any pairing that makes
 * no sense for the step (a "success" on the welcome screen, say) leaves the
 * step where it is rather than jumping somewhere surprising.
 */
export function nextStep(step: Step, outcome: Outcome): Step {
  // Skip is honest everywhere: it ends onboarding, unproven.
  if (outcome.kind === "skip") return "done";
  switch (step) {
    case "welcome":
      return outcome.kind === "next" ? "permissions" : step;
    case "permissions":
      if (outcome.kind === "next") return "calibration";
      if (outcome.kind === "back") return "welcome";
      return step;
    case "calibration":
      if (outcome.kind === "next") return "try-it";
      if (outcome.kind === "back") return "permissions";
      return step;
    case "try-it":
      if (outcome.kind === "success") return "done";
      if (outcome.kind === "failure") return FAILURE_STEP[outcome.failure];
      if (outcome.kind === "back") return "calibration";
      return step;
    case "try-it-no-mic":
    case "try-it-no-accessibility":
    case "try-it-no-model":
    case "try-it-paste-refused":
      if (outcome.kind === "retry" || outcome.kind === "back") return "try-it";
      // A take that lands while a failure screen is up (the user fixed it and
      // dictated straight away) is still a success.
      if (outcome.kind === "success") return "done";
      if (outcome.kind === "failure") return FAILURE_STEP[outcome.failure];
      return step;
    case "done":
      return outcome.kind === "back" ? "try-it" : step;
  }
}

/**
 * What finishing onboarding writes to settings, or `null` when this outcome
 * must NOT finish it. `onboarded: true` exists only on the far side of a
 * success (proven) or an explicit skip (unproven) — never on mount, never on
 * a failure, never on plain navigation.
 */
export interface Completion {
  onboarded: true;
  dictationProven: boolean;
}

export function completionFor(outcome: Outcome): Completion | null {
  if (outcome.kind === "success") return { onboarded: true, dictationProven: true };
  if (outcome.kind === "skip") return { onboarded: true, dictationProven: false };
  return null;
}

/**
 * Name the stage that broke a try-it take, earliest stage first: no audio
 * beats no transcript beats no paste. `micAuthorized` is PERM-A/B's real
 * AVCaptureDevice reading, `accessibility` PERM-E's grant row.
 */
export function classifyTryItFailure(input: {
  micAuthorized: boolean;
  modelReady: boolean;
  accessibility: boolean;
}): TryItFailure {
  if (!input.micAuthorized) return "no-mic";
  if (!input.modelReady) return "no-model";
  if (!input.accessibility) return "no-accessibility";
  return "paste-refused";
}

function normalize(s: string): string {
  return s.replace(/\s+/g, " ").trim().toLowerCase();
}

/**
 * Did the dictated text actually ARRIVE in the try-it field? The backend
 * pastes with a real ⌘V; this compares what the field holds against the
 * transcript it announced. Whitespace and case are not evidence either way.
 */
export function tryItLanded(fieldValue: string, transcript: string): boolean {
  const want = normalize(transcript);
  if (!want) return false;
  return normalize(fieldValue).includes(want);
}

/**
 * The main window's reminder after an unproven finish. `null` once a take has
 * pasted (or while onboarding itself is still on screen).
 */
export function unprovenLine(settings: {
  onboarded?: boolean;
  dictationProven?: boolean;
} | null | undefined): string | null {
  if (!settings?.onboarded || settings.dictationProven) return null;
  return "Dictation hasn't pasted yet — try it once to confirm Yap works.";
}
