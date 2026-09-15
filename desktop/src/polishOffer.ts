// SEC-C (onboarding half) — whether first-run says anything about the polish
// model, and what.
//
// The install path itself landed with the rest of SEC-C: `list_polish_models`,
// `download_polish_model`, `clear_polish_model`, the digest-verified download
// and `PolishModelPicker` all exist. What did not exist was any way for a NEW
// user to learn the LLM formatting stage is there at all — `STEP_ORDER` never
// mentioned it, so the only surface that offered it was Settings → Advanced,
// which a first-run user has no reason to open. An optional feature nobody is
// told about is indistinguishable from one that was never built.
//
// Telling them is not the same as starting a 1.1 GB download for them. The item
// is explicit: offering it is right, forcing it is not. So this module decides
// what the last onboarding step SAYS, and every path that can start bytes
// moving still needs a human to press a button in `PolishModelPicker`.
//
// It is pure on purpose — the same shape as `micStatus.ts` and `permission.ts`
// — because the interesting states (a catalog that has not loaded, a speech
// model still coming down, a polish download already in flight) are states you
// cannot conjure on demand in a running app.

export type PolishOfferKind =
  /** Nothing truthful to say yet — no row, no heading, no empty skeleton. */
  | "hidden"
  /** Worth mentioning, but not worth racing the download dictation needs. */
  | "deferred"
  /** The optional download, with its size, offered behind a button. */
  | "offer"
  /** The user asked for it and it is coming down. */
  | "downloading"
  /** Installed, verified and selected — the LLM stage can actually run. */
  | "active";

export interface PolishOfferInput {
  /** `ModelSetup.ready` — a speech model is downloaded AND selected. */
  speechModelReady: boolean;
  /** How many polish entries the catalog returned (0 = not loaded, or none). */
  catalogSize: number;
  /** `PolishModelSetup.active` — a polish model is on disk AND selected. */
  polishActive: boolean;
  /** `PolishModelSetup.downloading` — catalog id in flight, or null. */
  polishDownloading: string | null;
}

export interface PolishOffer {
  kind: PolishOfferKind;
  title: string;
  /** One sentence. Says what it changes and that it is optional. */
  body: string;
  /**
   * Whether the download control may be rendered at all. This is the whole
   * guarantee: in a state where the answer is `false`, first run has no button
   * that can start a 1.1 GB download, by construction rather than by hoping the
   * user reads first.
   */
  showPicker: boolean;
}

/**
 * The precedence is deliberate and the order is the argument:
 *
 *  1. A download already in flight outranks everything. The user pressed the
 *     button; yanking the progress bar out from under them because some other
 *     condition changed is how a 1.1 GB transfer becomes invisible.
 *  2. An installed model outranks the deferral below — it is already on disk,
 *     so there is nothing left to race, and "Turn off" must stay reachable.
 *  3. An unloaded catalog says NOTHING. A heading over an empty list is a
 *     feature that looks broken on the one screen that sets first impressions.
 *  4. While the SPEECH model is still downloading, mention it but offer no
 *     button. Dictation cannot run at all without the speech model; starting a
 *     second, larger download beside it makes the one thing the user actually
 *     needs arrive later, on exactly the connection least able to afford it.
 */
export function describePolishOffer(input: PolishOfferInput): PolishOffer {
  if (input.polishDownloading !== null) {
    return {
      kind: "downloading",
      title: "Getting the formatting model",
      body: "This keeps going in the background — you can finish setup now and start dictating as soon as the speech model is ready.",
      showPicker: true,
    };
  }

  if (input.polishActive) {
    return {
      kind: "active",
      title: "Formatting model installed",
      body: "Yap will clean up punctuation, capitalisation and paragraphs on top of the rules it already applies. You can turn it off any time.",
      showPicker: true,
    };
  }

  if (input.catalogSize === 0) {
    return {
      kind: "hidden",
      title: "",
      body: "",
      showPicker: false,
    };
  }

  if (!input.speechModelReady) {
    return {
      kind: "deferred",
      title: "One optional extra, later",
      body: "Yap can also run a local language model to tidy punctuation and paragraphs. It is a separate, larger download — add it from Settings once your speech model has finished.",
      showPicker: false,
    };
  }

  return {
    kind: "offer",
    title: "Want tidier text? (optional)",
    body: "A local language model cleans up punctuation, capitalisation and paragraph breaks. It is a large one-time download and it never leaves this Mac. Skip it and Yap works exactly as it does now.",
    showPicker: true,
  };
}

/** Every state this module can produce — used by its test to sweep them all. */
export const POLISH_OFFER_KINDS: PolishOfferKind[] = [
  "hidden",
  "deferred",
  "offer",
  "downloading",
  "active",
];
