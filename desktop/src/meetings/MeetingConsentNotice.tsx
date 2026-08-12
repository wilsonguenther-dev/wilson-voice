import { useEffect, useRef } from "react";
import { CONSENT_NOTICE } from "./consent";

/**
 * YV96 — the one-time meeting-capture notice.
 *
 * Shape follows `license/PurchasePrompt.tsx` deliberately: same overlay grammar,
 * same Esc-closes/focus-the-action keyboard contract. What is different is the
 * one thing that matters here — **every way out of this sheet is the same way
 * out**. There is no "Decline", because declining would have to stop a recording
 * the user just started, and O1 closed as a nudge: the notice rides alongside
 * capture, it does not stand in front of it.
 *
 * So the recording is already running behind this. That is the honest design for
 * a notice whose whole message is "this is your call": the user asked for a
 * recording, they get one, and they get told plainly what Yap does and does not
 * do about the other people in the room.
 */
export default function MeetingConsentNotice({
  /** True when the sheet opened over a recording that is running right now. */
  recording,
  /** Acknowledge + close. Esc and the backdrop route here too. */
  onClose,
}: {
  recording: boolean;
  onClose: () => void;
}) {
  const actionRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    actionRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="consent-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="consent-title"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="consent-card">
        <p className="consent-eyebrow">{CONSENT_NOTICE.eyebrow}</p>
        <h1 id="consent-title">{CONSENT_NOTICE.title}</h1>

        {CONSENT_NOTICE.paragraphs.map((p) => (
          <p className="consent-body" key={p.slice(0, 24)}>
            {p}
          </p>
        ))}

        {/* The nudge, made visible: the meeting is already running. Saying so
            is what stops this reading as a permission dialog the user has to
            clear before they are allowed to work. */}
        {recording && (
          <p className="consent-live" role="status">
            <span className="consent-dot" aria-hidden /> Recording now — this notice
            is not holding anything up.
          </p>
        )}

        <div className="consent-actions">
          <button
            ref={actionRef}
            type="button"
            className="primary"
            onClick={onClose}
          >
            {CONSENT_NOTICE.action}
          </button>
        </div>

        <p className="consent-fine">{CONSENT_NOTICE.fine}</p>
      </div>
    </div>
  );
}
