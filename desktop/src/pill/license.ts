/**
 * Y2-A — what the PILL is allowed to say about the license, as one pure
 * function.
 *
 * The pill is ambient: it sits over every app you own, all day, and it is the
 * one surface a person never asked to look at. So the decision "does the pill
 * mention the trial at all, and how loudly" is a POLICY, and policy belongs in
 * one place. `pillLicense` is that place. Y2-B (classic), Y2-C (yappy) and Y2-D
 * (the purchase surface) RENDER this result; none of them re-decides it, and
 * none of them reaches for `days_left` on its own.
 *
 * TWO DIFFERENT NUMBERS, BOTH DELIBERATE:
 *   - `PILL_TRIAL_DAYS = 7` — the pill starts showing a countdown a full week
 *     out. It is ambient and cheap to glance at; a quiet "5d" in the corner
 *     costs nothing and is read in passing.
 *   - `TRIAL_WARN_DAYS = 3` (in `../license/status`) — the ONE-SHOT toast. That
 *     one interrupts, so it fires later and exactly once. It is untouched by
 *     this file.
 * Seven is not three because a glance is not an interruption. If the two are
 * ever unified, unify them deliberately — the difference is the design.
 *
 * ARITHMETIC IS BORROWED, NEVER RE-DERIVED. `daysLeft`, `trialCountdown`,
 * `chipFor` and `statusCopy` come from `../license/status`, whose own doc
 * comment says it exists so the card, the prompt and the changelog "cannot
 * drift apart". The pill is simply the fourth consumer. There is no second copy
 * of the trial arithmetic here and there is no second clock: the backend owns
 * the clock, including the rollback floor (`license.rs:473-520`), and
 * `days_left` arrives already computed on every `license` / `license_required`
 * event.
 *
 * NO PRICE, EVER. The money copy is the purchase surface's job (Y2-D). No file
 * under `desktop/src/pill/` imports the price-label constant that
 * `../license/status` exports, which is why the ended-trial copy below is taken
 * from the branches of `chipFor` / `statusCopy` that carry no figure. That rule
 * is enforced by a plain `git grep` of this directory for the constant's name,
 * so do not spell that name out here either: to a grep, a mention in a comment
 * and an import look the same.
 */
import {
  chipFor,
  daysLeft,
  statusCopy,
  trialCountdown,
  type LicenseStatus,
} from "../license/status";

/**
 * How many days out the pill starts counting down. Deliberately larger than
 * `TRIAL_WARN_DAYS` (3) — see the header.
 */
export const PILL_TRIAL_DAYS = 7;

/**
 * The pill has three voices and no fourth. `licensed` is absent on purpose:
 * a licensed app says NOTHING, so it never needs a tone.
 */
export type PillLicenseTone = "trial" | "urgent" | "ended";

export interface PillLicense {
  /** Draw anything at all? When false, every other field is meaningless. */
  show: boolean;
  tone: PillLicenseTone;
  /** A single monochrome mark; the pill faces draw it in their own type. */
  glyph: string;
  /** The numeral the eye lands on ("5d"), or null when there is no number. */
  value: string | null;
  /** Hover / accessible description. Never contains a price. */
  title: string;
}

/** The silent answer, shared so "show nothing" is one object and one shape. */
export const PILL_LICENSE_SILENT: PillLicense = {
  show: false,
  tone: "trial",
  glyph: "",
  value: null,
  title: "",
};

const GLYPH_TRIAL = "⧗"; // ⧗ hourglass — a trial that is running out
const GLYPH_ENDED = "⊘"; // ⊘ — new dictation is stopped

/**
 * The whole display policy for the pill, given the backend's license payload.
 *
 *   licensed                  → nothing, ever, whatever the trial fields say
 *   trial, more than 7 days   → nothing (ambient silence)
 *   trial, 7…1 days           → "7d"…"1d", tone `trial`
 *   trial, last day (0)       → "1d", tone `urgent` — there is still today,
 *                               so "0d" would be a lie told to hurry someone
 *   license_required          → tone `ended`, no numeral
 *
 * `null` (nothing has arrived from the backend yet) is silence, not an
 * assumption: the pill must not flash a countdown during boot.
 */
export function pillLicense(status: LicenseStatus | null | undefined): PillLicense {
  if (!status) return PILL_LICENSE_SILENT;

  if (status.state === "licensed") return PILL_LICENSE_SILENT;

  if (status.state === "trial") {
    // Borrowed, not recomputed: the backend's day count, floored at zero.
    const days = daysLeft(status);
    if (days > PILL_TRIAL_DAYS) return PILL_LICENSE_SILENT;
    if (days <= 0) {
      return {
        show: true,
        tone: "urgent",
        glyph: GLYPH_TRIAL,
        value: "1d",
        title: `${trialCountdown(0)} of your Yap trial.`,
      };
    }
    // `chipFor` already renders the numeral and the sentence for a running
    // trial; the pill shows the SAME string the Settings chip does. Only the
    // tone is the pill's own, because the pill's urgency threshold is not the
    // chip's.
    const chip = chipFor(status);
    return {
      show: true,
      tone: "trial",
      glyph: GLYPH_TRIAL,
      value: chip.value,
      title: chip.title,
    };
  }

  // license_required. `statusCopy`'s headline for this state is the one line
  // that says what actually stopped, and it carries no figure.
  return {
    show: true,
    tone: "ended",
    glyph: GLYPH_ENDED,
    value: null,
    title: statusCopy(status).headline,
  };
}
