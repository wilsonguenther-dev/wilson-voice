// WF-12 — the pill shows "Sounded like Spanish — Add Spanish?" after a take
// heard in a language the user has not picked. Mounted by float-main.tsx
// beside the meeting offer. Add / Not now / Esc; unanswered, it leaves on its
// own after LANGUAGE_OFFER_TTL_MS.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { holdPillHitbox } from "./drag";
import {
  LANGUAGE_OFFER_EVENT,
  LANGUAGE_OFFER_RESPOND,
  LANGUAGE_OFFER_TTL_MS,
  isLanguageOffer,
  languageOfferArgs,
  languageOfferLines,
  type LanguageOffer,
  type LanguageOfferChoice,
} from "./languageOffer";

/** Send an answer to Rust. The ONLY way an offer changes the language set. */
export function respondToLanguageOffer(
  offer: LanguageOffer,
  choice: LanguageOfferChoice,
  call: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke,
): Promise<unknown> {
  return call(LANGUAGE_OFFER_RESPOND, languageOfferArgs(offer, choice));
}

/** Presentational: no hooks, so a test can render it and press its buttons. */
export function LanguageOfferView({
  offer,
  error,
  busy,
  onChoose,
}: {
  offer: LanguageOffer;
  error?: string | null;
  busy?: boolean;
  onChoose: (choice: LanguageOfferChoice) => void;
}) {
  const { headline, add, notNow } = languageOfferLines(offer);
  return (
    <div
      className="language-offer"
      role="dialog"
      aria-label={headline}
      data-language-offer={offer.code}
    >
      <p className="language-offer-headline">{headline}</p>
      {error && (
        <p className="language-offer-error" role="alert">
          {error}
        </p>
      )}
      <div className="language-offer-actions">
        <button
          type="button"
          className="language-offer-add"
          disabled={busy}
          onClick={() => onChoose("add")}
        >
          {add}
        </button>
        <button
          type="button"
          className="language-offer-later"
          disabled={busy}
          onClick={() => onChoose("notNow")}
        >
          {notNow}
        </button>
      </div>
    </div>
  );
}

export default function LanguageOfferCard({
  forced = null,
}: {
  /** A dev-only matrix scene's offer (`forcedLanguageOffer`); `null` in a release. */
  forced?: LanguageOffer | null;
} = {}) {
  const [offer, setOffer] = useState<LanguageOffer | null>(forced);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  const show = useCallback((o: unknown) => {
    if (!isLanguageOffer(o)) return;
    setOffer(o);
    setError(null);
    setBusy(false);
  }, []);

  useEffect(() => {
    let dead = false;
    let unlisten: (() => void) | null = null;
    listen<LanguageOffer>(LANGUAGE_OFFER_EVENT, (e) => show(e.payload)).then((u) => {
      if (dead) u();
      else unlisten = u;
    });
    return () => {
      dead = true;
      unlisten?.();
    };
  }, [show]);

  const choose = useCallback(
    (choice: LanguageOfferChoice) => {
      if (!offer) return;
      setBusy(true);
      respondToLanguageOffer(offer, choice)
        .then(() => setOffer(null))
        .catch((e) => {
          setBusy(false);
          setError(String(e));
        });
    },
    [offer],
  );

  useEffect(() => {
    if (!offer) return;
    // The float window is click-through outside the reported hitbox; while
    // the card is up it IS the hitbox, so Add and Not now can be clicked.
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "language-offer");
    }
    // An error stays up until answered; a quiet card leaves on its own (a
    // forced matrix scene stays, so the photo is of the card).
    const ttl = error || forced ? null : window.setTimeout(() => setOffer(null), LANGUAGE_OFFER_TTL_MS);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") choose("notNow");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      if (ttl !== null) window.clearTimeout(ttl);
      window.removeEventListener("keydown", onKey);
      holdPillHitbox(null, "language-offer");
    };
  }, [offer, error, forced, choose]);

  if (!offer) return null;
  return (
    <div ref={ref} className="language-offer-wrap">
      <LanguageOfferView offer={offer} error={error} busy={busy} onChoose={choose} />
    </div>
  );
}
