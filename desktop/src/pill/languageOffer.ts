/**
 * WF-12 — "Speaking a different language?": the pill's offer to add a
 * language it heard that is not in the user's set. Its wire shape, its copy,
 * and the one call each answer makes. Pure, so vitest pins it.
 *
 * Rust owns the decision (`languages::take_offer`): a language is offered at
 * most once per `LANGUAGE_OFFER_QUIET_DAYS` days, whatever the answer, and Not
 * now restarts that window from the click. The pill only shows what it is
 * handed and reports the answer through `language_offer_respond`; Add goes
 * through the same language writer as Settings and the bar's picker, so both
 * update off the `settings` event it broadcasts.
 */

/** Rust `languages::LANGUAGE_OFFER_EVENT`. */
export const LANGUAGE_OFFER_EVENT = "language_offer";
/** The command both buttons call. */
export const LANGUAGE_OFFER_RESPOND = "language_offer_respond";
/** Rust `languages::LANGUAGE_OFFER_QUIET_DAYS`. */
export const LANGUAGE_OFFER_QUIET_DAYS = 7;
/**
 * An unanswered card leaves on its own, handing the hitbox back to the pill.
 * Leaving is not an answer: the offer was already counted when it was shown,
 * so it still will not come back inside the quiet window.
 */
export const LANGUAGE_OFFER_TTL_MS = 12_000;

/** Mirrors `languages::LanguageOffer` (camelCase on the wire). */
export interface LanguageOffer {
  code: string;
  name: string;
  headline: string;
}

export type LanguageOfferChoice = "add" | "notNow";

export function isLanguageOffer(v: unknown): v is LanguageOffer {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.code === "string" &&
    /^[a-z]{2}$/.test(o.code) &&
    typeof o.name === "string" &&
    o.name.length > 0 &&
    typeof o.headline === "string" &&
    o.headline.length > 0
  );
}

/** The card's copy. Rust builds the headline; the buttons are fixed. */
export function languageOfferLines(offer: LanguageOffer): {
  headline: string;
  add: string;
  notNow: string;
} {
  return { headline: offer.headline, add: "Add", notNow: "Not now" };
}

/** The invoke() arguments for an answer (Rust `language_offer_respond`). */
export function languageOfferArgs(offer: LanguageOffer, choice: LanguageOfferChoice) {
  return { code: offer.code, add: choice === "add" };
}

/** The offer the dev-only matrix scene draws (`float.html?phase=idle&langOffer=1`). */
export const SAMPLE_LANGUAGE_OFFER: LanguageOffer = {
  code: "es",
  name: "Spanish",
  headline: "Sounded like Spanish — Add Spanish?",
};

/**
 * The forced matrix scene's offer, or `null`. float-main only calls this when
 * its DEV-ONLY `FORCED` handle is compiled in (scripts/pill-matrix.mjs), so the
 * released float window ignores the query string.
 */
export function forcedLanguageOffer(search: string): LanguageOffer | null {
  return new URLSearchParams(search).get("langOffer") === "1" ? SAMPLE_LANGUAGE_OFFER : null;
}
