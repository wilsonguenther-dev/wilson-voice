/**
 * YV96 — the one-time meeting-capture notice: its copy, and the two pure rules
 * that decide when it opens.
 *
 * This is the CLOSED-DECISION version of the plan's §8 (finding #13). O1 was
 * resolved by Wilson as "consent liability on the user — one TERMS line + one
 * one-time notice", so what is here is deliberately all there is:
 *
 *   * no reminder toggle,
 *   * no home-state legal setting,
 *   * no gate-vs-nudge branching — it is always a nudge,
 *   * no per-meeting `consent_ack`; one `settings_kv` row app-wide.
 *
 * The copy lives in this module rather than inline in the sheet so the two
 * sentences the item exists to say — you are responsible for the law where you
 * are, and Yap does not announce itself — are unit-testable rather than
 * something a redesign can quietly drop.
 */

/** Mirrors `meetings::MeetingConsent` in Rust. */
export interface MeetingConsent {
  /** True until the notice has been shown once and closed. */
  shouldShow: boolean;
  /** RFC3339 of the first close, or `null`. Never overwritten. */
  acknowledgedAt: string | null;
  /**
   * Always `false`. Read by the sheet so "this is a nudge, not a gate" is a
   * value a future entry point has to actively ignore rather than a convention
   * it can forget.
   */
  blocksRecording: boolean;
}

/**
 * The slice of YV95's `meeting` event this module needs. Typed locally, not
 * imported from `pill/meeting.ts`, so the notice does not acquire a dependency
 * on the recording surface's shape — it only ever asks one question of it.
 */
export interface RecordingSignal {
  recording: boolean;
}

export const CONSENT_NOTICE = {
  eyebrow: "One time only",
  title: "Recording other people is your call",
  paragraphs: [
    "Yap records your Mac's microphone and turns it into a transcript here, on this machine. Nothing is uploaded, and no one else can read it.",
    "Recording laws differ by state and by country. Some places need only your permission, some need everyone's, and a few treat a phone call differently from a room. Following the law where you are is your responsibility — Yap does not know where you are, and it will not decide this for you.",
    "Yap does not announce itself. Nobody in the room or on the call is told that a recording is running unless you tell them. Yap shows you it is recording — the pill and the menu bar stay visible the whole time — but that indicator is on your screen, for you.",
  ],
  /** The one line that names where the durable version of this lives. */
  fine: "This is the same responsibility described in Yap's Terms. You will only see this once.",
  action: "Got it",
} as const;

/**
 * Should the sheet open right now?
 *
 * Called on every `meeting` tick, so it must be safe to ask 3,600 times in an
 * hour: it is a pure read, and the answer flips to `false` for good the moment
 * the close is recorded.
 *
 * `null` — the state before the backend has answered on mount — is `false` on
 * purpose. Guessing "show it" from an unknown state is how a one-time notice
 * becomes a notice that shows twice.
 */
export function shouldOpenNotice(
  consent: MeetingConsent | null,
  signal: RecordingSignal,
): boolean {
  return signal.recording && consent !== null && consent.shouldShow;
}

/**
 * The Settings → Privacy line. The notice is one-time, but re-reading it must
 * always be possible — a one-time notice you cannot find again is a notice the
 * user cannot act on.
 */
export function acknowledgedLabel(consent: MeetingConsent | null): string {
  if (!consent || consent.shouldShow || !consent.acknowledgedAt) {
    return "You have not seen the recording notice yet — it appears the first time you record a meeting.";
  }
  const when = new Date(consent.acknowledgedAt);
  const shown = Number.isNaN(when.getTime())
    ? consent.acknowledgedAt
    : when.toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
      });
  return `Recording notice shown ${shown}. Recording other people stays your call.`;
}
