// yap-call-1 — the call offer: "Zoom call — record it?" with ONE consent line
// and a 10 s ring, then ONE one-click chip "Record Zoom call" for as long as
// the call lives, Never for <App>, and "Call ended — stop recording?".
//
// It renders in its OWN window (label "call-offer", call-offer.html) because
// that window is content-protected — hidden from screen capture and screen
// share — and the pill's recording dot must never be. Rust owns every
// decision (call_offer.rs); this file draws the view and reports answers.
// Nothing here starts or stops a recording on its own: expiry is Not now.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ACCEPT_CALL_OFFER,
  CALL_OFFER_CHOOSE,
  CALL_OFFER_EVENT,
  CALL_OFFER_STATE,
  COUNTDOWN_MS,
  acceptArgs,
  callOfferLines,
  chipLabel,
  chooseArgs,
  isCallOfferView,
  neverLabel,
  secondsLeft,
  surfaceAt,
  type CallOfferChoice,
  type CallOfferView,
} from "./callOfferModel";

export const CALL_ENDED_HEADLINE = "Call ended — stop recording?";

type Call = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;

/** [Record] on the offer or the chip — the live offer id, checked by Rust. */
export function acceptOffer(view: CallOfferView, call: Call = invoke): Promise<unknown> {
  return call(ACCEPT_CALL_OFFER, acceptArgs(view));
}

/** Not now / Never / Stop / Keep going. */
export function chooseOffer(
  view: CallOfferView,
  choice: CallOfferChoice,
  call: Call = invoke,
): Promise<unknown> {
  return call(CALL_OFFER_CHOOSE, chooseArgs(view, choice));
}

/** Presentational: no hooks, so a test can render it and press its buttons. */
export function CallOfferSurface({
  view,
  elapsedMs,
  error,
  onRecord,
  onChoose,
}: {
  view: CallOfferView;
  elapsedMs: number;
  error?: string | null;
  onRecord: () => void;
  onChoose: (choice: CallOfferChoice) => void;
}) {
  const surface = surfaceAt(view, elapsedMs);
  if (!surface) return null;
  const err = error ? (
    <p className="call-offer-error" role="alert">
      {error}
    </p>
  ) : null;

  if (surface === "ended") {
    return (
      <div className="call-offer is-ended" role="dialog" aria-label={CALL_ENDED_HEADLINE} data-call-offer="ended">
        <p className="call-offer-headline">{CALL_ENDED_HEADLINE}</p>
        {err}
        <div className="call-offer-actions">
          <button type="button" className="call-offer-stop" onClick={() => onChoose("stop")}>
            Stop
          </button>
          <button type="button" className="call-offer-keep" onClick={() => onChoose("keep_going")}>
            Keep going
          </button>
        </div>
      </div>
    );
  }

  if (surface === "chip") {
    // After expiry / Not now: ONE quiet chip, no sound, no second banner.
    return (
      <div className="call-offer is-chip" data-call-offer="chip">
        <button type="button" className="call-offer-chip" onClick={onRecord}>
          <span className="call-offer-chip-dot" aria-hidden />
          {chipLabel(view)}
        </button>
        {err}
      </div>
    );
  }

  const { headline, site, consent } = callOfferLines(view);
  const secs = secondsLeft(view, elapsedMs);
  const total = view.countdownMsLeft || COUNTDOWN_MS;
  const frac = Math.max(0, Math.min(1, 1 - elapsedMs / total));
  return (
    <div className="call-offer is-offer" role="dialog" aria-label={headline} data-call-offer="offer">
      <div className="call-offer-top">
        <p className="call-offer-headline">{headline}</p>
        <svg
          className="call-offer-ring"
          viewBox="0 0 24 24"
          role="timer"
          aria-label={`${secs} seconds`}
          data-seconds={secs}
        >
          <circle cx="12" cy="12" r="10" pathLength={1} className="call-offer-ring-track" />
          <circle
            cx="12"
            cy="12"
            r="10"
            pathLength={1}
            className="call-offer-ring-fill"
            strokeDasharray={`${frac} 1`}
            style={{ animationDuration: `${Math.max(0, total - elapsedMs)}ms` }}
          />
        </svg>
      </div>
      {site && <p className="call-offer-site">{site}</p>}
      <p className="call-offer-consent">{consent}</p>
      {err}
      <div className="call-offer-actions">
        <button type="button" className="call-offer-record" onClick={onRecord}>
          Record
        </button>
        <button type="button" className="call-offer-notnow" onClick={() => onChoose("not_now")}>
          Not now
        </button>
        <button type="button" className="call-offer-never" onClick={() => onChoose("never")}>
          {neverLabel(view)}
        </button>
      </div>
    </div>
  );
}

/** The offer window's root (call-offer-main.tsx). */
export default function CallOffer() {
  const [view, setView] = useState<CallOfferView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [elapsedMs, setElapsedMs] = useState(0);
  const shownAt = useRef<{ id: string | null; at: number }>({ id: null, at: 0 });
  const expired = useRef<string | null>(null);

  const show = useCallback((v: unknown) => {
    if (!isCallOfferView(v)) return;
    if (v.offerId !== shownAt.current.id) {
      shownAt.current = { id: v.offerId, at: Date.now() };
      setElapsedMs(0);
      setError(null);
    }
    setView(v);
  }, []);

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<CallOfferView>(CALL_OFFER_EVENT, (e) => show(e.payload)).then(push);
    invoke<CallOfferView>(CALL_OFFER_STATE)
      .then((v) => { if (!dead) show(v); })
      .catch(() => {});
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, [show]);

  // The ring is a CSS animation (call-offer.css); the ONLY JS timer is one
  // setTimeout at the end of the 10 s, cleared the moment the offer changes.
  // No backend polling here.
  useEffect(() => {
    if (!view || view.phase !== "offering" || !view.offerId) return;
    const total = view.countdownMsLeft || COUNTDOWN_MS;
    const left = Math.max(0, total - (Date.now() - shownAt.current.at));
    const id = window.setTimeout(() => {
      setElapsedMs(total);
      if (expired.current === view.offerId) return;
      // Expiry == Not now for THIS call. Never Record.
      expired.current = view.offerId;
      chooseOffer(view, "not_now").catch(() => {});
    }, left);
    return () => window.clearTimeout(id);
  }, [view]);

  if (!view) return null;
  const onRecord = () => {
    acceptOffer(view).catch((e) => setError(String(e)));
  };
  const onChoose = (choice: CallOfferChoice) => {
    chooseOffer(view, choice).catch((e) => setError(String(e)));
  };
  return (
    <div className="call-offer-wrap">
      <CallOfferSurface
        view={view}
        elapsedMs={elapsedMs}
        error={error}
        onRecord={onRecord}
        onChoose={onChoose}
      />
    </div>
  );
}

/** The meeting badge's app name ("Zoom", "FaceTime · your side") while a
 * recording that started from an offer runs. For the pill window. */
export function useCallRecordingView(): CallOfferView | null {
  const [view, setView] = useState<CallOfferView | null>(null);
  useEffect(() => {
    let dead = false;
    let un: (() => void) | null = null;
    listen<CallOfferView>(CALL_OFFER_EVENT, (e) => {
      if (isCallOfferView(e.payload)) setView(e.payload);
    }).then((u) => (dead ? u() : (un = u)));
    invoke<CallOfferView>(CALL_OFFER_STATE)
      .then((v) => { if (!dead && isCallOfferView(v)) setView(v); })
      .catch(() => {});
    return () => { dead = true; un?.(); };
  }, []);
  return view;
}
