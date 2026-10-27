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
  /** Six dead ends, six screens, each with the one button that fixes it. */
  | "try-it-no-mic"
  | "try-it-no-accessibility"
  | "try-it-no-model"
  | "try-it-paste-refused"
  /** Fix round 1: the take produced no words at all (no speech, cancel, ASR error). */
  | "try-it-no-speech"
  /** Fix round 1: the user's own "Paste my words automatically" setting is off. */
  | "try-it-auto-paste-off"
  | "done";

/** Why a try-it take did not land in the field. */
export type TryItFailure =
  | "no-mic"
  | "no-accessibility"
  | "no-model"
  | "paste-refused"
  | "no-speech"
  | "auto-paste-off";

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

/** The failure screens, keyed by the failure that leads to each. */
export const FAILURE_STEP: Record<TryItFailure, Step> = {
  "no-mic": "try-it-no-mic",
  "no-accessibility": "try-it-no-accessibility",
  "no-model": "try-it-no-model",
  "paste-refused": "try-it-paste-refused",
  "no-speech": "try-it-no-speech",
  "auto-paste-off": "try-it-auto-paste-off",
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
    case "try-it-no-speech":
    case "try-it-auto-paste-off":
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
 *
 * Fix round 1 — `heardText` is whether THIS take produced a transcript at all.
 * A take with no words (a fumbled tap the no-speech gate drops, a cancel, a
 * rejected or silent capture, an ASR error) never reached the paste stage, so
 * it must never be blamed on the paste. `autoPaste` is the user's own
 * "Paste my words automatically" setting: off, a take is copy-only by design
 * and the paste stage is the setting, not the cursor or a grant.
 */
export function classifyTryItFailure(input: {
  micAuthorized: boolean;
  modelReady: boolean;
  heardText: boolean;
  autoPaste: boolean;
  accessibility: boolean;
}): TryItFailure {
  if (!input.micAuthorized) return "no-mic";
  if (!input.modelReady) return "no-model";
  if (!input.heardText) return "no-speech";
  if (!input.autoPaste) return "auto-paste-off";
  if (!input.accessibility) return "no-accessibility";
  return "paste-refused";
}

function normalize(s: string): string {
  return s.replace(/\s+/g, " ").trim().toLowerCase();
}

function occurrences(haystack: string, needle: string): number {
  let n = 0;
  for (let i = haystack.indexOf(needle); i !== -1; i = haystack.indexOf(needle, i + needle.length)) n++;
  return n;
}

/**
 * Did the dictated text actually ARRIVE in the try-it field DURING THIS TAKE?
 * The backend pastes with a real ⌘V; this compares what the field holds now
 * against what it held when the take started (`baseline`). Whitespace and
 * case are not evidence either way.
 *
 * Fix round 1 — a plain "field contains the transcript" check failed OPEN: a
 * phrase the user had pasted by hand (or a one-word transcript that was any
 * substring of earlier text) read as a landed take. The transcript must now
 * occur MORE times than it did in the baseline — text that was already there
 * proves nothing about this take.
 */
export function tryItLanded(fieldValue: string, transcript: string, baseline: string): boolean {
  const want = normalize(transcript);
  if (!want) return false;
  return occurrences(normalize(fieldValue), want) > occurrences(normalize(baseline), want);
}

/**
 * The main window's reminder after an unproven finish. `null` once a take has
 * pasted (or while onboarding itself is still on screen).
 *
 * Fix round 1 — also `null` while the user's own "Paste my words
 * automatically" setting is OFF: no take can ever paste then (the backend's
 * `should_paste` requires `auto_paste`), so a "hasn't pasted yet" line would
 * never clear and would blame Yap for the user's choice.
 */
export function unprovenLine(settings: {
  onboarded?: boolean;
  dictationProven?: boolean;
  autoPaste?: boolean;
} | null | undefined): string | null {
  if (!settings?.onboarded || settings.dictationProven) return null;
  if (settings.autoPaste === false) return null;
  return "Dictation hasn't pasted yet — try it once to confirm Yap works.";
}
