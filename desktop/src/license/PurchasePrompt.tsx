import { useEffect, useRef } from "react";
import YappySprite from "./YappySprite";
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
 *  * it offers the two things that actually help — buy, or paste the key you
 *    already own — and nothing else;
 *  * Yappy is here because the companion has been on screen for fourteen days
 *    and this is the moment to be warm, not stern.
 */
export default function PurchasePrompt({
  onBuy,
  onEnterKey,
  onDismiss,
}: {
  onBuy: () => Promise<void> | void;
  /** Jump to Settings → License with the key box in view. */
  onEnterKey: () => void;
  onDismiss: () => void;
}) {
  const buyRef = useRef<HTMLButtonElement | null>(null);

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
