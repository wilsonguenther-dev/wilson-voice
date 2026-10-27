// yap-notetaker-detect — the pill expands with "Record this meeting?".
// Mounted by float-main.tsx (float.html) beside whichever capsule is drawn.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { holdPillHitbox } from "./drag";
import {
  MEETING_OFFER_CLEAR_EVENT,
  MEETING_OFFER_EVENT,
  MEETING_OFFER_RESPOND,
  isMeetingOffer,
  offerLines,
  respondArgs,
  type MeetingOffer,
  type OfferChoice,
} from "./meetingOffer";

/** Send an answer to Rust. The ONLY way an offer becomes a recording. */
export function respondToOffer(
  offer: MeetingOffer,
  choice: OfferChoice,
  call: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke,
): Promise<unknown> {
  return call(MEETING_OFFER_RESPOND, respondArgs(offer, choice));
}

/** Presentational: no hooks, so a test can render it and press its buttons. */
export function MeetingOfferView({
  offer,
  error,
  onChoose,
}: {
  offer: MeetingOffer;
  error?: string | null;
  onChoose: (choice: OfferChoice) => void;
}) {
  const { headline, title, consent } = offerLines(offer);
  return (
    <div
      className="meeting-offer"
      role="dialog"
      aria-label={headline}
      data-meeting-offer={offer.source}
    >
      <p className="meeting-offer-headline">{headline}</p>
      {title && <p className="meeting-offer-title">{title}</p>}
      <p className="meeting-offer-consent">{consent}</p>
      {error && (
        <p className="meeting-offer-error" role="alert">
          {error}
        </p>
      )}
      <div className="meeting-offer-actions">
        <button type="button" className="meeting-offer-record" onClick={() => onChoose("record")}>
          Record
        </button>
        <button type="button" className="meeting-offer-snooze" onClick={() => onChoose("snooze")}>
          Not now
        </button>
      </div>
    </div>
  );
}

export default function MeetingOfferCard() {
  const [offer, setOffer] = useState<MeetingOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement | null>(null);

  const show = useCallback((o: unknown) => {
    if (!isMeetingOffer(o)) return;
    setOffer(o);
    setError(null);
  }, []);

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<MeetingOffer>(MEETING_OFFER_EVENT, (e) => show(e.payload)).then(push);
    listen(MEETING_OFFER_CLEAR_EVENT, () => setOffer(null)).then(push);
    // A pill webview that mounted after the emit still gets the offer.
    invoke<MeetingOffer | null>("pending_meeting_offer")
      .then((o) => { if (!dead) show(o); })
      .catch(() => {});
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, [show]);

  useEffect(() => {
    if (!offer) return;
    // The float window is click-through outside the reported hitbox; while
    // the offer is up it IS the hitbox, so Record and Not now can be clicked.
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height });
    }
    return () => holdPillHitbox(null);
  }, [offer]);

  if (!offer) return null;
  const choose = (choice: OfferChoice) => {
    respondToOffer(offer, choice)
      .then(() => setOffer(null))
      .catch((e) => setError(String(e)));
  };
  return (
    <div ref={ref} className="meeting-offer-wrap">
      <MeetingOfferView offer={offer} error={error} onChoose={choose} />
    </div>
  );
}
