import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { type ActiveLeave, consentNotice, leaveLines } from "./consent";
import { errorText } from "../errors";

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
  // yap-byom-0 — "Nothing is uploaded" only while no model-provider choice
  // leaves this Mac. null until the backend answers (or if it fails): the
  // notice then makes no upload claim at all.
  const [leaves, setLeaves] = useState<string[] | null>(null);
  const [leavesError, setLeavesError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    invoke<{ activeLeaves: ActiveLeave[] }>("list_model_providers")
      .then((v) => {
        if (live) setLeaves(leaveLines(v.activeLeaves));
      })
      .catch((e: unknown) => {
        if (live) setLeavesError(errorText(e));
      });
    return () => {
      live = false;
    };
  }, []);
  const notice = consentNotice(leaves);

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
        <p className="consent-eyebrow">{notice.eyebrow}</p>
        <h1 id="consent-title">{notice.title}</h1>

        {notice.paragraphs.map((p) => (
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
            {notice.action}
          </button>
        </div>

        {leavesError && (
          <p className="consent-fine" role="status">
            Yap could not read your model-provider choices ({leavesError}), so
            this notice makes no claim about uploads.
          </p>
        )}
        <p className="consent-fine">{notice.fine}</p>
      </div>
    </div>
  );
}
