import { useEffect, useRef, useState } from "react";
import YappySprite from "./YappySprite";
import { errorText } from "../errors";
import { FOUNDING_CODE, FOUNDING_PRICE_LABEL, PRICE_LABEL } from "./status";

/**
 * YP3 — what a person sees when the fortnight is over and they press the hotkey
 * out of habit.
 *
 * This is a sales moment, not an error, and the shape of it is the whole point:
 *
 *  * it is **dismissible**, and dismissing it returns them to a fully working
 *    app — history, search, exports, settings, the dictionary, the scratchpad.
 *    Nothing behind this sheet is locked. Saying so plainly is the difference
 *    between a purchase and a chargeback;
 *  * it offers the three things that actually help — buy, paste the key you
 *    already own, or (LIC-A) get that key back when the email is gone — and
 *    nothing else. "I already paid" is the single most expensive sentence a
 *    paywall can fail to answer, and until LIC-A the only answer was a support
 *    thread;
 *  * Yappy is here because the companion has been on screen for fourteen days
 *    and this is the moment to be warm, not stern.
 */
export default function PurchasePrompt({
  onBuy,
  onEnterKey,
  onRetrieve,
  onDismiss,
}: {
  onBuy: () => Promise<void> | void;
  /** Jump to Settings → License with the key box in view. */
  onEnterKey: () => void;
  /** LIC-A — ask the issuer for the key that was signed for this address and
   *  activate it in place. Rejects with the backend's `{code, message}`. */
  onRetrieve: (email: string) => Promise<void>;
  onDismiss: () => void;
}) {
  const buyRef = useRef<HTMLButtonElement | null>(null);
  const [retrieving, setRetrieving] = useState<"closed" | "open" | "sending">("closed");
  const [email, setEmail] = useState("");
  const [retrieveError, setRetrieveError] = useState<string | null>(null);

  async function retrieve(e: React.FormEvent) {
    e.preventDefault();
    if (retrieving === "sending") return;
    setRetrieveError(null);
    setRetrieving("sending");
    try {
      await onRetrieve(email);
    } catch (err) {
      // One sentence with a next step, from the backend. A retrieval that
      // fails costs the person nothing: the sheet stays open, the pasted-key
      // route is still right there, and offline verification never involved
      // this call in the first place.
      setRetrieveError(errorText(err));
      setRetrieving("open");
    }
  }

  // Esc closes it, and focus starts on the primary action so the whole sheet is
  // reachable from the keyboard — the person who got here pressed a hotkey.
  useEffect(() => {
    buyRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDismiss();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDismiss]);

  return (
    <div
      className="buy-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="buy-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) onDismiss();
      }}
    >
      <div className="buy-card">
        <YappySprite size={88} className="buy-mascot" />
        <p className="buy-eyebrow">Fourteen days, done</p>
        <h1 id="buy-title">Yap has one more thing to ask</h1>
        <p className="buy-lede">
          Your trial is up, so starting a <em>new</em> dictation is paused. That is the
          only thing that stopped.
        </p>
        <p className="buy-keep">
          Your notes and history stay yours forever — every transcript, every search,
          every export, your dictionary and your settings all keep working, whether or
          not you ever buy.
        </p>

        <div className="buy-actions">
          <button ref={buyRef} type="button" className="primary" onClick={() => onBuy()}>
            Buy Yap — {PRICE_LABEL} once
          </button>
          <button type="button" className="ghost" onClick={onEnterKey}>
            I already have a key
          </button>
        </div>

        {retrieving === "closed" ? (
          <button
            type="button"
            className="ghost buy-retrieve-open"
            onClick={() => setRetrieving("open")}
          >
            I already paid — retrieve my license
          </button>
        ) : (
          <form className="buy-retrieve" onSubmit={retrieve}>
            <label htmlFor="buy-retrieve-email">
              The email address you paid with — we will look up your key and turn Yap
              back on right here.
            </label>
            <div className="buy-retrieve-row">
              <input
                id="buy-retrieve-email"
                type="email"
                autoComplete="email"
                required
                placeholder="you@example.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                disabled={retrieving === "sending"}
              />
              <button type="submit" className="primary" disabled={retrieving === "sending"}>
                {retrieving === "sending" ? "Looking…" : "Retrieve"}
              </button>
            </div>
            {retrieveError && (
              <p className="buy-retrieve-error" role="alert">
                {retrieveError}
              </p>
            )}
          </form>
        )}

        <p className="buy-fine">
          One time, no subscription, no account. Founding price{" "}
          <strong className="license-price">{FOUNDING_PRICE_LABEL}</strong> with{" "}
          <code>{FOUNDING_CODE}</code> at checkout.
        </p>
        <button type="button" className="ghost buy-dismiss" onClick={onDismiss}>
          Not now
        </button>
      </div>
    </div>
  );
}
