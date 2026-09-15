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
 *   - `PILL_SHOW_DAYS = 7` — the pill starts showing a countdown a full week
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
  storedKeyProblem,
  trialCountdown,
  type LicenseStatus,
} from "../license/status";

/**
 * How many days out the pill starts counting down. Deliberately larger than
 * `TRIAL_WARN_DAYS` (3) — see the header.
 *
 * Y2-E: THIS NUMBER IS SHARED WITH THE MENU BAR. `license::PILL_SHOW_DAYS` in
 * `desktop/src-tauri/src/license.rs` carries the same value under the same
 * name, and `src-tauri/tests/tray_license.rs` parses BOTH files and fails if
 * they drift — numbers AND comparison operators, since `> 7` and `>= 7` are the
 * same constant and a different boundary. If you change it here, change it
 * there; the test will tell you if you forget.
 */
export const PILL_SHOW_DAYS = 7;

/**
 * How few days must remain before the countdown turns URGENT. The rule below is
 * `days < PILL_URGENT_DAYS`, i.e. the last day — `daysLeft` floors at zero, so
 * this is the day on which no whole day is left. Strictly-less is what makes
 * `1` the honest number: on the day the pill reads "1d" there is still a day in
 * it, so that day is not yet urgent.
 *
 * Shared with `license::PILL_URGENT_DAYS` on the Rust side under the same name
 * and held there by the same parsing test. The menu-bar ICON keys off this, and
 * off nothing else — see `license::tray_line`.
 */
export const PILL_URGENT_DAYS = 1;

/**
 * The pill has four voices and no fifth. `licensed` is absent on purpose:
 * a licensed app says NOTHING, so it never needs a tone.
 *
 * Y2-F added `problem`, and it is not a shade of `ended` — it is its opposite.
 * `ended` is addressed to someone who has not paid; `problem` is addressed to
 * someone who HAS, whose key stopped granting anything (revoked, refunded,
 * seat-capped, signed for another plan, or unreadable on this Mac). Collapsing
 * the two is the single most expensive sentence this app can say: it asks a
 * paying customer to buy the thing they already bought.
 */
export type PillLicenseTone = "trial" | "urgent" | "ended" | "problem";

/**
 * Where a press on the pill's license mark goes. This is the machine-checkable
 * half of Y2-D's rule "never show a price to a stored-key holder": the policy,
 * not the component, decides whether a purchase surface is reachable at all,
 * so the rule can be asserted in a unit test instead of eyeballed in a render.
 */
export type PillLicenseAction = "none" | "license" | "purchase";

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
  /**
   * What a press opens. `problem` is ALWAYS `"license"` — a person whose key
   * is stored gets the re-activate box, never the checkout.
   */
  action: PillLicenseAction;
}

/** Is a purchase surface reachable from this result? The Y2-D rule, as a test. */
export function offersPurchase(pill: PillLicense): boolean {
  return pill.show && pill.action === "purchase";
}

/** The silent answer, shared so "show nothing" is one object and one shape. */
export const PILL_LICENSE_SILENT: PillLicense = {
  show: false,
  tone: "trial",
  glyph: "",
  value: null,
  title: "",
  action: "none",
};

const GLYPH_TRIAL = "⧗"; // ⧗ hourglass — a trial that is running out
const GLYPH_ENDED = "⊘"; // ⊘ — new dictation is stopped
const GLYPH_PROBLEM = "⚿"; // ⚿ squared key — the KEY is the thing that is wrong

/**
 * What the pill says when a stored key granted nothing and the backend did not
 * say why (an unreadable or corrupt store: `has_stored_license` is true,
 * `license_problem_message` is null).
 *
 * This string is why the branch below keys on `has_stored_license` and NOT on
 * `storedKeyProblem(status) !== null`. `storedKeyProblem` returns null in
 * exactly this case, so branching on it would drop an unreadable store back
 * into `license_required` — the precise bug Y2-F exists to remove, reintroduced
 * by the function that looks like it prevents it. It carries no figure.
 */
const PROBLEM_FALLBACK =
  "Your license is stored on this Mac but is not being accepted right now. Open License settings to re-enter your key.";

/**
 * The whole display policy for the pill, given the backend's license payload.
 *
 *   licensed                  → nothing, ever, whatever the trial fields say
 *   stored key, not licensed  → tone `problem` — Y2-F, see below
 *   trial, more than 7 days   → nothing (ambient silence)
 *   trial, 7…1 days           → "7d"…"1d", tone `trial`
 *   trial, last day (0)       → "1d", tone `urgent` — there is still today,
 *                               so "0d" would be a lie told to hurry someone
 *   license_required          → tone `ended`, no numeral
 *
 * `null` (nothing has arrived from the backend yet) is silence, not an
 * assumption: the pill must not flash a countdown during boot.
 *
 * ORDER IS THE DESIGN. The `problem` test sits directly under `licensed` and
 * ABOVE the trial arithmetic, so it wins over every other non-licensed row. A
 * broken stored key during a still-running trial is still a broken key: the
 * person paid, and telling them "5 days left" would hide the one fact they can
 * act on. `licensed` stays first because a working key outranks everything —
 * `has_stored_license` is true for every licensed install too, and testing it
 * first would silence the entire app.
 */
export function pillLicense(status: LicenseStatus | null | undefined): PillLicense {
  if (!status) return PILL_LICENSE_SILENT;

  if (status.state === "licensed") return PILL_LICENSE_SILENT;

  // Y2-F — a key IS stored and it granted nothing. This is a paying customer
  // with a broken key, not a visitor whose trial lapsed, and the two must not
  // look alike. Copy comes from the backend's own diagnosis; the purchase
  // sentence is never reachable from here.
  if (status.has_stored_license) {
    return {
      show: true,
      tone: "problem",
      glyph: GLYPH_PROBLEM,
      value: null,
      title: storedKeyProblem(status) ?? PROBLEM_FALLBACK,
      action: "license",
    };
  }

  if (status.state === "trial") {
    // Borrowed, not recomputed: the backend's day count, floored at zero.
    const days = daysLeft(status);
    if (days > PILL_SHOW_DAYS) return PILL_LICENSE_SILENT;
    if (days < PILL_URGENT_DAYS) {
      return {
        show: true,
        tone: "urgent",
        glyph: GLYPH_TRIAL,
        value: "1d",
        title: `${trialCountdown(0)} of your Yap trial.`,
        action: "purchase",
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
      action: "purchase",
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
    action: "purchase",
  };
}

/**
 * Y2-C — the two sentences the `gated` PILL PHASE says after a refused press.
 *
 * BORROWED, NEVER REWRITTEN. Both strings are read out of `statusCopy`'s
 * `license_required` branch at module load, so the pill's refused-press copy and
 * the Settings → License card are physically the same two strings: editing the
 * card edits the pill, and there is no second sentence to drift.
 *
 * The status object below exists only to ask `statusCopy` a question it already
 * answers. It is NOT a license: nothing reads it, nothing renders it, and it
 * never reaches the backend — `reason` is the one the gate actually emits when
 * a trial simply ran out (`license.rs`), so the branch taken here is the branch
 * a real refusal takes.
 */
const ENDED_FOR_COPY: LicenseStatus = {
  state: "license_required",
  reason: "trial_expired",
  license_problem: null,
  license_problem_message: null,
  has_stored_license: false,
  trial_days_left: 0,
  trial_expires_at_ms: 0,
  revocation_checked_at_ms: null,
  revoked_count: 0,
};

/** "Dictation is paused" — the headline the capsule wears. */
export const GATED_HEADLINE = statusCopy(ENDED_FOR_COPY).headline;
// The supporting line. NOT exported: the pill never shows it on its own, only
// as the second half of `GATED_TITLE`, so exporting it would be a symbol with
// no consumer.
const GATED_BODY = statusCopy(ENDED_FOR_COPY).body;
/** Headline + body as one hover string: what a refused press explains. */
export const GATED_TITLE = `${GATED_HEADLINE} ${GATED_BODY}`;
/** The mark a gated capsule wears — the same ⊘ the ended chip already uses. */
export const GATED_GLYPH = GLYPH_ENDED;
