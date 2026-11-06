/**
 * yap-call-1 — the call offer's wire shape, copy and pure rules. Rust owns
 * every decision (`call_offer.rs`): the offer window only draws the view it
 * is handed and reports the answer. Nothing here starts or stops anything —
 * Record goes through `accept_call_offer` with the live offer id, and the
 * backend rejects any other id.
 */
import { CONSENT_PILL_LINE } from "../meetings/consent";
import { recordLabel } from "./meeting";

/** Rust `call_offer::CALL_OFFER_EVENT`. */
export const CALL_OFFER_EVENT = "call_offer";
/** Rust `accept_call_offer` — [Record] on the offer and on the chip. */
export const ACCEPT_CALL_OFFER = "accept_call_offer";
/** Rust `call_offer_choose` — Not now / Never / Stop / Keep going. */
export const CALL_OFFER_CHOOSE = "call_offer_choose";
/** Rust `call_offer_state` — the view for a webview that mounted late. */
export const CALL_OFFER_STATE = "call_offer_state";
/** Rust `call_offer::OFFER_COUNTDOWN_MS`. Expiry == Not now, never Record. */
export const COUNTDOWN_MS = 10_000;

export type CallOfferPhase = "idle" | "offering" | "chip" | "recording" | "end_prompt";
export type CallOfferChoice = "not_now" | "never" | "stop" | "keep_going";

/** Mirrors `call_offer::CallOfferView` (camelCase on the wire). */
export interface CallOfferView {
  phase: CallOfferPhase;
  offerId: string | null;
  appName: string | null;
  bundleId: string | null;
  copy: string | null;
  siteLine: string | null;
  yourSideOnly: boolean;
  countdownMsLeft: number;
}

const PHASES: readonly CallOfferPhase[] = ["idle", "offering", "chip", "recording", "end_prompt"];

export function isCallOfferView(v: unknown): v is CallOfferView {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    typeof o.phase === "string" &&
    (PHASES as readonly string[]).includes(o.phase) &&
    (o.offerId === null || typeof o.offerId === "string") &&
    typeof o.yourSideOnly === "boolean" &&
    typeof o.countdownMsLeft === "number"
  );
}

/** What the window draws. `elapsedMs` is how long THIS offer has been up
 * locally, so the chip appears at 10 s even before the backend's answer. */
export type CallSurface = "offer" | "chip" | "ended" | null;

export function surfaceAt(view: CallOfferView | null, elapsedMs: number): CallSurface {
  if (!view || !view.offerId) return null;
  switch (view.phase) {
    case "offering":
      return elapsedMs >= Math.min(COUNTDOWN_MS, view.countdownMsLeft || COUNTDOWN_MS)
        ? "chip"
        : "offer";
    case "chip":
      return "chip";
    case "end_prompt":
      return "ended";
    default:
      return null;
  }
}

/** Whole seconds left on the ring (10 … 1, then 0). */
export function secondsLeft(view: CallOfferView, elapsedMs: number): number {
  const left = Math.max(0, (view.countdownMsLeft || COUNTDOWN_MS) - elapsedMs);
  return Math.ceil(left / 1000);
}

/** The offer's lines: the headline from `offer_copy()`, the site line, and the
 * ONE consent line. */
export function callOfferLines(view: CallOfferView): {
  headline: string;
  site: string | null;
  consent: string;
} {
  return {
    headline: view.copy ?? `${view.appName ?? "Browser"} call — record it?`,
    site: view.siteLine,
    consent: CONSENT_PILL_LINE,
  };
}

/** "Record Zoom call" — the chip that stays for the life of the call. */
export function chipLabel(view: CallOfferView): string {
  return recordLabel({ recording: false, elapsedSeconds: 0, captureAvailable: true }, view.appName);
}

export function neverLabel(view: CallOfferView): string {
  return `Never for ${view.appName ?? "this app"}`;
}

/** The meeting badge's app name for a recording that started from an offer:
 * "Zoom", or "FaceTime · your side". `null` for any other recording. */
export function badgeCallLabel(view: CallOfferView | null): string | null {
  if (!view || !view.appName) return null;
  if (view.phase !== "recording" && view.phase !== "end_prompt") return null;
  return view.yourSideOnly ? `${view.appName} · your side` : view.appName;
}

export function acceptArgs(view: CallOfferView) {
  return { offerId: view.offerId };
}

export function chooseArgs(view: CallOfferView, choice: CallOfferChoice) {
  return { offerId: view.offerId, choice };
}
