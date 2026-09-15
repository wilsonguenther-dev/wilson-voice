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

/* ───────────────────────── Y2-B — the geometry policy ─────────────────────────
 *
 * THE SIDE DOCK IS THE CONSTRAINT, AND IT IS THE NARROW ONE. `pill_position` is
 * `bottom | left | right` (lib.rs:408, default "bottom"), and on a side dock the
 * capsule is parked flush against the screen edge in a ~30px-wide strip —
 * reference_wispr_parity_research §4.2 measured Wispr's side-docked bar at 30px
 * and found their fixed-width label is why only the primary listening states
 * rotate: a sentence cannot "crush in a 30px column".
 *
 * So the pill shows the VALUE and nothing else — "5d" fits a 30px column, "5
 * days left" does not — and the whole sentence lives in `title`, which
 * `pillLicense` already returns and which a tooltip can be as wide as it likes.
 *
 * Three characters is the budget: the widest value this policy can EVER produce
 * is "7d" (`PILL_TRIAL_DAYS` is 7 and day 0 renders "1d"), so three leaves one
 * character of headroom for a future unit without re-opening the geometry. It
 * is asserted over the whole fortnight in `license.test.ts` rather than left as
 * a comment, because the failure mode is a chip that silently overflows the
 * strip on one dock out of three.
 */
export const PILL_VALUE_MAX_CHARS = 3;

/** Does this value fit the 30px side dock? `null` (no numeral) trivially does. */
export function valueFitsSideDock(value: string | null): boolean {
  return value === null || value.length <= PILL_VALUE_MAX_CHARS;
}

/**
 * Split "5d" into the numeral the eye lands on and its unit, so the pills can
 * set them in two different faces: the numeral in Departure Mono (the pixel
 * voice every other count in Yap already wears — `chipFor` in status.ts:118
 * says as much, and the component is what actually sets it), the unit in the
 * body face so it reads as a word and not as another digit.
 *
 * Returns `null` when there is no numeral at all (`license_required`), which is
 * the caller's cue to draw the glyph instead.
 */
export function splitValue(value: string | null): { numeral: string; unit: string } | null {
  if (!value) return null;
  const m = /^(\d+)(.*)$/.exec(value);
  return m ? { numeral: m[1], unit: m[2] } : { numeral: value, unit: "" };
}
