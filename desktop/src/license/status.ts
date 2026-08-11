/**
 * YP3 — the license as the interface sees it.
 *
 * Everything here is PURE: the shape the backend serialises, and the small
 * decisions the UI makes from it (what the chip says, when — once — to mention
 * that the trial is nearly up). Keeping them out of the components is what lets
 * `status.test.ts` drive the fortnight, the last three days, expiry, a revoked
 * key and a lifetime license without a running app.
 *
 * The backend half is `desktop/src-tauri/src/license.rs`; the wire shape below
 * is `LicenseStatus` with `Entitlement` flattened into it (`#[serde(tag =
 * "state", rename_all = "snake_case")]`), so `state` is the discriminant.
 */

/** A verified, unrevoked lifetime license is installed on this Mac. */
export type Licensed = {
  state: "licensed";
  plan: string;
  seats: number;
  kid: string;
};

/** No license yet, and the 14-day trial is still running. */
export type Trial = {
  state: "trial";
  days_left: number;
  expires_at_ms: number;
};

/**
 * The trial is over and there is no usable license. Exactly one thing stops
 * here — starting a NEW dictation. `reason` is `trial_expired` when nothing was
 * ever entered, or the refusal code of the key that is stored (`revoked`,
 * `bad_signature`, `wrong_plan`, …).
 */
export type LicenseRequired = {
  state: "license_required";
  reason: string;
};

export type Entitlement = Licensed | Trial | LicenseRequired;

/** The full payload of `license_status` / the `license` event. */
export type LicenseStatus = Entitlement & {
  /** Why a STORED key granted nothing, when one is stored and did not. */
  license_problem: string | null;
  license_problem_message: string | null;
  has_stored_license: boolean;
  trial_days_left: number;
  trial_expires_at_ms: number;
  revocation_checked_at_ms: number | null;
  revoked_count: number;
};

// ─── Pricing ─────────────────────────────────────────────────────────
//
// The money copy, in one place. The Payment Link URL itself is deliberately NOT
// here: it lives in Rust as `license::PAYMENT_LINK_URL` and is reached through
// the `open_purchase_page` command, so the only string that can ever be handed
// to `open(1)` is a compile-time constant. See that constant's doc comment for
// the citation (drivia-forge `docs/YAP-LICENSING.md` § "Live objects":
// plink_1U2yNFBc7RJSrX28KrzOuXwk / price_1U2yN4Bc7RJSrX287I33050v).

/** Full price, one time, forever. */
export const PRICE_LABEL = "$29";
/** Launch promo: coupon z8UZkIH2 applied by promotion code at checkout. */
export const FOUNDING_CODE = "FOUNDING19";
export const FOUNDING_PRICE_LABEL = "$19";
/** How many Macs one key activates (claims `seats`, default 3). */
export const DEFAULT_SEATS = 3;

/**
 * The one promise this whole screen exists to keep. Written once so the
 * Settings card, the purchase prompt and the changelog cannot drift apart.
 */
export const KEEP_FOREVER_LINE =
  "Your notes and history stay yours forever — history, search, exports and settings keep working whether or not you buy.";

// ─── Trial arithmetic ────────────────────────────────────────────────

/**
 * How many days out the single "your trial is nearly up" toast fires.
 *
 * One toast, at three days, and never again — a countdown that reappears every
 * launch is how a good app becomes nagware, and the person it annoys most is
 * the one who already decided to buy.
 */
export const TRIAL_WARN_DAYS = 3;

/** Days-left, floored at zero, for display. */
export function daysLeft(status: LicenseStatus): number {
  return Math.max(0, status.state === "trial" ? status.days_left : 0);
}

/** "3 days left" / "1 day left" / "Last day". */
export function trialCountdown(days: number): string {
  if (days <= 0) return "Last day";
  return days === 1 ? "1 day left" : `${days} days left`;
}

/**
 * Should the trial warning fire for this status, given what was already shown?
 *
 * `lastWarnedFor` is the `trial_expires_at_ms` the warning last fired against —
 * keying on the expiry rather than a boolean means the toast can never fire
 * twice for one trial, and a genuinely new trial (a fresh profile) is not
 * silenced by an old flag.
 */
export function shouldWarnTrial(
  status: LicenseStatus,
  lastWarnedFor: number | null,
): boolean {
  if (status.state !== "trial") return false;
  if (status.days_left > TRIAL_WARN_DAYS) return false;
  return lastWarnedFor !== status.expires_at_ms;
}

/** The sentence that single toast carries. */
export function trialWarningText(days: number): string {
  return days <= 0
    ? "Last day of your Yap trial — a one-time $29 license keeps dictation on. Everything you've written stays yours either way."
    : `${trialCountdown(days)} on your Yap trial. A one-time $29 license keeps dictation on; everything you've written stays yours either way.`;
}

// ─── The chip ────────────────────────────────────────────────────────

export type ChipTone = "trial" | "urgent" | "licensed" | "ended";

/**
 * The always-on status chip beside the app's other header state. Short enough
 * to sit next to the recording pill; the numeral is what the eye lands on, so
 * the component sets it in Departure Mono.
 */
export function chipFor(status: LicenseStatus): {
  tone: ChipTone;
  label: string;
  /** The part set in the pixel numeral face, if any. */
  value: string | null;
  title: string;
} {
  switch (status.state) {
    case "licensed":
      return {
        tone: "licensed",
        label: "Licensed",
        value: null,
        title: "Yap is licensed on this Mac — lifetime, no renewal.",
      };
    case "trial": {
      const d = daysLeft(status);
      return {
        tone: d <= TRIAL_WARN_DAYS ? "urgent" : "trial",
        label: "Trial",
        value: `${d}d`,
        title: `${trialCountdown(d)} of your 14-day Yap trial.`,
      };
    }
    default:
      return {
        tone: "ended",
        label: "Trial ended",
        value: null,
        title:
          "Your trial has ended. New dictation needs a license; everything already written stays.",
      };
  }
}

// ─── The status card ─────────────────────────────────────────────────

/**
 * Headline + supporting line for the Settings → License card. Three states, in
 * the app's own voice: a running trial, a lifetime license, and the end of the
 * trial — which is a sales moment, not an error, and reads like one.
 */
export function statusCopy(status: LicenseStatus): {
  eyebrow: string;
  headline: string;
  body: string;
} {
  switch (status.state) {
    case "licensed":
      return {
        eyebrow: "Licensed",
        headline: "Yap is yours — lifetime",
        body: `One-time license, no renewal, no account. This key activates ${status.seats || DEFAULT_SEATS} Macs; remove it here to move a seat to another one.`,
      };
    case "trial": {
      const d = daysLeft(status);
      return {
        eyebrow: "Trial",
        headline: d <= 0 ? "Last day of your trial" : `${trialCountdown(d)}`,
        body:
          d <= TRIAL_WARN_DAYS
            ? `The whole app, nothing held back, until your 14 days are up. After that only NEW dictation stops — ${PRICE_LABEL} once turns it back on.`
            : "The whole app, nothing held back, for 14 days. No card, no account, nothing to cancel.",
      };
    }
    default:
      return {
        eyebrow: "Trial ended",
        headline: "Dictation is paused",
        body: "Everything you have already written is still here and still exportable. A one-time license turns the microphone back on.",
      };
  }
}

/**
 * What went wrong with a key that IS stored but granted nothing. `null` when
 * there is no stored key or the stored key is fine — the card only ever shows
 * this when there is something a person can act on.
 */
export function storedKeyProblem(status: LicenseStatus): string | null {
  if (!status.has_stored_license) return null;
  if (status.state === "licensed") return null;
  return status.license_problem_message ?? null;
}

/**
 * The human sentence for a rejected activation, given whatever `invoke` threw.
 *
 * The backend already writes a next step into every `VerifyError`, so this only
 * has to cover the two things it cannot: an empty box, and a rejection that
 * arrived as something other than a `{code, message}`.
 */
export function activationError(e: unknown): string {
  const message =
    typeof e === "object" && e !== null && typeof (e as { message?: unknown }).message === "string"
      ? (e as { message: string }).message
      : String(e ?? "");
  return (
    message.trim() ||
    "That key could not be read. Paste the whole line from your purchase email."
  );
}
