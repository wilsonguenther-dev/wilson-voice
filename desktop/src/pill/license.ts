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
 * THREE DIFFERENT NUMBERS, ALL DELIBERATE:
 *   - `PILL_QUIET_FROM_DAYS = 14` (yap24-PILL1) — the pill counts down for the
 *     WHOLE trial. Days 14…8 wear the quiet tone (`trial-quiet`: the numeral at
 *     reduced contrast, no glyph, no purchase action). Before this, the first
 *     week of a trial said nothing at all, which a person reads as "there is no
 *     trial" — the panel may keep a quiet style, it may not keep silence.
 *   - `PILL_SHOW_DAYS = 7` — the last week wears the full `trial` treatment.
 *     It is ambient and cheap to glance at; a "5d" in the corner costs nothing
 *     and is read in passing.
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
 * yap24-PILL1 — how many days out the pill starts counting down at all, in the
 * QUIET tone. Days `PILL_SHOW_DAYS < d <= PILL_QUIET_FROM_DAYS` are
 * `trial-quiet`. A SEPARATE constant, never a repurposed `PILL_SHOW_DAYS`:
 * that one also decides when the menu bar offers to sell. Twinned with
 * `license::PILL_QUIET_FROM_DAYS` in Rust; `license.rs`'s lib tests and
 * `src-tauri/tests/tray_license.rs` both parse this line and fail on drift.
 */
export const PILL_QUIET_FROM_DAYS = 14;

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
export type PillLicenseTone = "trial-quiet" | "trial" | "urgent" | "ended" | "problem";

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

/**
 * Y2-D — the Rust command a `purchase` action reaches, named ONCE.
 *
 * It is not `open_purchase_page`, and the difference is the whole item. That
 * command hands the Payment Link constant to `open(1)`; this one only
 * unminimizes the main window and emits `show_purchase`, so the sheet — which
 * owns the price, the founding code and the keep-forever line — is what a
 * person sees before a checkout. The float webview is the least-trusted window
 * in the app and it never gets to name a URL.
 */
export const REVEAL_PURCHASE_COMMAND = "reveal_purchase_prompt";

/**
 * Y2-D — what a press on a license affordance DOES, decided from the policy and
 * nothing else, so both capsules route identically:
 *
 *   `purchase` → invoke `reveal_purchase_prompt` (main window + the sheet)
 *   `license`  → the re-activate box (Settings → License). Y2-F's `problem`
 *                tone is somebody who already PAID; a price is never shown.
 *   `null`     → nothing at all. A licensed pill (or one with no payload yet)
 *                has no route, so it has no click region either.
 */
export type PillLicenseRoute = "reveal_purchase" | "license_tab" | null;

export function pillLicenseRoute(pill: PillLicense | null | undefined): PillLicenseRoute {
  if (!pill?.show) return null;
  if (pill.action === "purchase") return "reveal_purchase";
  if (pill.action === "license") return "license_tab";
  return null;
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
 *   trial, 14…8 days          → "14d"…"8d", tone `trial-quiet` (yap24-PILL1)
 *   trial, 7…1 days           → "7d"…"1d", tone `trial`
 *   trial, last day (0)       → "1d", tone `urgent` — there is still today,
 *                               so "0d" would be a lie told to hurry someone
 *   trial, more than 14 days  → nothing (a fresh trial never gets here; a
 *                               support grace key is at most 14 too)
 *   license_required          → tone `ended`, no numeral
 *
 * THE PURCHASE ACTION IS TRUTHFUL (yap24-PILL1). `action: "purchase"` only
 * when the backend reports `purchase_open` (Rust `PURCHASE_OPEN`). While
 * checkout is closed the trial tones carry NO action (`"none"`: an inert
 * numeral with a title) and `ended` routes to the License panel, where a key
 * can be entered — never to a Buy that dead-ends.
 *
 * TITLES: "Free trial — N days left" for both running tones, "Trial ends today"
 * on the last day. No price, ever (Y2-D).
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

  // yap24-PILL1 — the backend's switch, never a webview guess. Absent (an
  // older payload) reads as closed.
  const purchaseOpen = status.purchase_open === true;

  if (status.state === "trial") {
    // Borrowed, not recomputed: the backend's day count (rollback floor
    // included), floored at zero.
    const days = daysLeft(status);
    if (days > PILL_QUIET_FROM_DAYS) return PILL_LICENSE_SILENT;
    const action: PillLicenseAction = purchaseOpen ? "purchase" : "none";
    if (days < PILL_URGENT_DAYS) {
      return {
        show: true,
        tone: "urgent",
        glyph: GLYPH_TRIAL,
        value: "1d",
        title: TRIAL_ENDS_TODAY,
        action,
      };
    }
    // `chipFor` renders the numeral ("5d") for a running trial, so the pill and
    // the Settings chip set the same value. The tone and the sentence are the
    // pill's own: its thresholds are not the chip's.
    return {
      show: true,
      tone: days > PILL_SHOW_DAYS ? "trial-quiet" : "trial",
      glyph: GLYPH_TRIAL,
      value: chipFor(status).value,
      title: freeTrialTitle(days),
      action,
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
    action: purchaseOpen ? "purchase" : "license",
  };
}

/** The last-day sentence. No price, no Buy. */
export const TRIAL_ENDS_TODAY = "Trial ends today";

/** "Free trial — 10 days left" / "Free trial — 1 day left". No price, no Buy. */
export function freeTrialTitle(days: number): string {
  return `Free trial — ${trialCountdown(days)}`;
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
  purchase_open: false,
  founding_open: false,
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

/* ───────────────────────── Y2-B — the geometry policy ─────────────────────────
 *
 * Carried from loop/y2-b-pill-trial-countdown-in-both-styles @ 8d85cdf (PR #186,
 * closed) by yap24-PILL1, re-based onto the 14-day policy.
 *
 * THE SIDE DOCK IS THE CONSTRAINT, AND IT IS THE NARROW ONE. On a `left` /
 * `right` dock the capsule sits in a ~30px strip, so the pill shows the VALUE
 * and nothing else — "14d" fits, "14 days left" does not — and the sentence
 * lives in `title`, which a tooltip can make as wide as it likes.
 *
 * Three characters is the budget, and yap24-PILL1 spends all of it: the widest
 * value this policy can now produce is "14d" (`PILL_QUIET_FROM_DAYS`). It is
 * asserted over the whole fortnight in `license.test.ts` rather than left as a
 * comment, because the failure mode is a chip that silently overflows the strip
 * on one dock out of three.
 */
export const PILL_VALUE_MAX_CHARS = 3;

/** Does this value fit the 30px side dock? `null` (no numeral) trivially does. */
export function valueFitsSideDock(value: string | null): boolean {
  return value === null || value.length <= PILL_VALUE_MAX_CHARS;
}

/**
 * Split "5d" into the numeral the eye lands on and its unit, so a pill can set
 * them in two faces: the numeral in Departure Mono, the unit in the body face.
 * `null` when there is no numeral at all (`ended`, `problem`), which is the
 * caller's cue to draw the glyph instead.
 */
export function splitValue(value: string | null): { numeral: string; unit: string } | null {
  if (!value) return null;
  const m = /^(\d+)(.*)$/.exec(value);
  return m ? { numeral: m[1], unit: m[2] } : { numeral: value, unit: "" };
}
