/**
 * YV102 — the "Set up meeting recording" step: its copy, its state, and the one
 * pure rule that decides what the user is told.
 *
 * ## Why this step exists
 *
 * There is no public API to request permission to capture system audio, and
 * none to read whether you have it. AudioCap's own README says so in one line,
 * and plan finding OS-10 builds on it: the TCC alert is a **side effect** of
 * creating and starting a CoreAudio process tap. You cannot ask politely, you
 * cannot check first, and — the part that turns this into an item rather than a
 * footnote — **if the user dismisses or denies it, macOS never asks again.**
 *
 * Left where the plan originally had it, that alert lands at T-0 of the user's
 * first real meeting: mid-Zoom-join, focus stolen (a system alert ignores the
 * pill's non-activating politeness entirely), with no explanation of what is
 * being asked or why. A dismissal there — the reflex response to a surprise
 * dialog while joining a call — is permanent for that install.
 *
 * So Yap provokes it deliberately, from Settings, while the sentence explaining
 * it is already on screen. That is the whole feature. The mechanism underneath
 * is a real 200 ms tap whose audio is discarded.
 *
 * ## The one free trust asset
 *
 * macOS shows its own purple indicator while an app is capturing system audio —
 * distinct from the orange microphone dot. It is a claim Yap does not have to
 * make and cannot fake, so the copy says it out loud. Evidence, not marketing.
 *
 * Copy and state rules live here rather than inline in `App.tsx` so they are
 * unit-tested rather than something a redesign can quietly drop.
 */

/** Mirrors `meetings::SetupVerdict` in Rust (`#[serde(rename_all = "snake_case")]`). */
export type SetupVerdict =
  | "not_run"
  | "ran"
  | "granted"
  | "looks_denied"
  | "unavailable"
  | "failed";

/** Mirrors `meetings::SystemAudioSetup` in Rust. */
export interface SystemAudioSetup {
  /** False until the step has run once on this Mac. */
  hasRun: boolean;
  /** RFC3339 of the LAST run, or `null`. Latest wins — permission can change. */
  lastRunAt: string | null;
  verdict: SetupVerdict;
  /**
   * Always `false`. Mic-only meeting recording runs on the macOS 12 floor and
   * is never gated on this; the field is a value the UI reads so a future
   * surface has to actively ignore it rather than forget a convention.
   */
  blocksRecording: boolean;
}

/**
 * The `pane` argument for `open_privacy_settings`.
 *
 * Verified on the target OS rather than guessed — the plan's candidate anchor
 * (`Privacy_SystemAudio`) does not exist. See `permissions::SYSTEM_AUDIO_PANE`
 * in Rust for the enumeration and the window-title evidence.
 */
export const SYSTEM_AUDIO_PANE = "SystemAudio";

export const SYSTEM_AUDIO_SETUP = {
  title: "Set up meeting recording",
  sub: "One step, once. Yap asks macOS for permission here instead of in the middle of your first call.",
  /**
   * Shown BEFORE the button is pressed, and this ordering is the item: the
   * system alert steals focus the instant the tap starts, so every word that
   * explains it has to already be on screen.
   */
  paragraphs: [
    "Recording the other people on a call means capturing your Mac's own audio output, not just your microphone. macOS treats that as a separate permission and will show you its own alert to confirm it.",
    "Pressing the button below opens a recording for a fifth of a second and throws away everything it hears. That is the only way to ask — macOS has no other request for this permission — and it is why Yap asks now, on a quiet screen, rather than as you join a meeting.",
    "While Yap is capturing system audio, macOS shows a purple indicator in your menu bar for as long as it lasts. That indicator comes from macOS, not from Yap, and no app can turn it off.",
  ],
  /** The sentence that has to be true, and is: macOS only asks once. */
  fine: "macOS asks once. If you dismiss it, it will not ask again — you would have to allow Yap in System Settings by hand.",
  action: "Set up meeting recording",
  actionAgain: "Run the permission check again",
  openSettings: "Open System Audio Recording settings",
} as const;

/** What the Settings row and the Permissions row both render. */
export interface SetupState {
  /** `ok` paints the green dot, `bad` the red one, `idle` neither. */
  tone: "ok" | "bad" | "idle";
  label: string;
  /** Show the "Open System Audio Recording settings" deep link? */
  showDeepLink: boolean;
  /** Is the step's own button pressable? */
  canRun: boolean;
  actionLabel: string;
}

function runDate(setup: SystemAudioSetup): string {
  if (!setup.lastRunAt) return "";
  const when = new Date(setup.lastRunAt);
  return Number.isNaN(when.getTime())
    ? setup.lastRunAt
    : when.toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
}

/**
 * The one rule. Pure, total, and the only place a verdict becomes a sentence.
 *
 * Read the arms in order and note what is NOT here: no branch turns "quiet" into
 * "denied". `looks_denied` is only ever produced by the Rust discriminator after
 * a tap has run past `syscapture::DENIAL_GRACE` without a single non-zero
 * sample — the one signal that separates a real denial from the CoreAudio bug
 * where a healthy tap goes all-zero for minutes (finding OS-4). A UI that
 * inferred denial from silence would badge that healthy meeting "permission
 * revoked", which is both false and the reason the user stops trusting the
 * badge.
 *
 * `available === false` (macOS below 14.4, YV101's gate) outranks everything:
 * there is no permission to ask for, so offering to ask for it is a lie.
 */
export function setupState(
  setup: SystemAudioSetup | null,
  available: boolean,
  requirement: string,
): SetupState {
  if (!available) {
    return {
      tone: "idle",
      label: `${requirement}. Meeting notes still record your microphone.`,
      showDeepLink: false,
      canRun: false,
      actionLabel: SYSTEM_AUDIO_SETUP.action,
    };
  }
  if (!setup || !setup.hasRun) {
    return {
      tone: "idle",
      label:
        "Yap has not asked for system-audio permission yet. Run this once and the alert arrives here instead of mid-call.",
      showDeepLink: false,
      canRun: true,
      actionLabel: SYSTEM_AUDIO_SETUP.action,
    };
  }
  switch (setup.verdict) {
    case "granted":
      return {
        tone: "ok",
        label: `System audio reached Yap on ${runDate(setup)}. macOS shows a purple menu-bar indicator whenever it is capturing.`,
        showDeepLink: false,
        canRun: true,
        actionLabel: SYSTEM_AUDIO_SETUP.actionAgain,
      };
    case "looks_denied":
      return {
        tone: "bad",
        label:
          "Yap has not received any system audio. macOS has not granted System Audio Recording to Yap, and it will not ask again — allow it in System Settings, then start a new meeting.",
        showDeepLink: true,
        canRun: true,
        actionLabel: SYSTEM_AUDIO_SETUP.actionAgain,
      };
    case "unavailable":
      return {
        tone: "idle",
        label: `${requirement}. Meeting notes still record your microphone.`,
        showDeepLink: false,
        canRun: false,
        actionLabel: SYSTEM_AUDIO_SETUP.action,
      };
    case "failed":
      return {
        tone: "bad",
        label: `The permission check could not start on ${runDate(setup)}. That is a fault, not a refusal — the logs in Privacy & Diagnostics name the CoreAudio call that failed.`,
        showDeepLink: true,
        canRun: true,
        actionLabel: SYSTEM_AUDIO_SETUP.actionAgain,
      };
    default:
      // "ran": the alert has been shown and answered, and 200 ms of a quiet Mac
      // is not evidence of the answer. Saying so is the honest option, and the
      // deep link is offered anyway — it costs nothing and it is the only
      // recovery if the answer was "don't allow".
      return {
        tone: "idle",
        label: `Permission check ran on ${runDate(setup)}. Nothing was playing, so Yap heard nothing — that is expected. Your first meeting will show whether system audio is coming through.`,
        showDeepLink: true,
        canRun: true,
        actionLabel: SYSTEM_AUDIO_SETUP.actionAgain,
      };
  }
}
