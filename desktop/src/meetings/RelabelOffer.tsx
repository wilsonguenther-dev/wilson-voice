import { useEffect, useRef } from "react";
import { relabelOffer, relabelResult } from "./corrections";

/**
 * YV130 — *"this voice appears in N earlier meetings — label them too?"*
 *
 * Finding #31's fix, on screen. The epic plan's version of this feature was
 * "fully automatic, no review, no undo"; the audit's objection is that the one
 * feature whose value is entirely trust is the one feature that must not act on
 * its own. So this is an INLINE strip, not a modal, and it is the question:
 *
 *   * **Apply** is the only thing that writes. Until it is pressed, nothing in
 *     the database has changed.
 *   * **Not now** is the absence of a call. It cannot half-happen, and it costs
 *     nothing to choose — which is what makes "Apply" a real answer rather than
 *     the only way to dismiss a prompt.
 *   * Once applied, the same strip becomes the receipt and holds the **Undo**
 *     for the whole batch, so the escape hatch is where the decision was made
 *     rather than three menus away.
 *
 * Inline rather than a sheet on purpose: this appears immediately after the user
 * names a speaker, and a modal that interrupts a naming flow to ask a second
 * question gets dismissed reflexively — which for this prompt would mean "Not
 * now" every time, and a feature nobody ever sees work.
 */
export default function RelabelOffer({
  displayName,
  meetings,
  segments,
  applied,
  onApply,
  onDismiss,
  onUndo,
}: {
  /** The voice that was just named. */
  displayName: string;
  /** Earlier meetings the candidate segments span. */
  meetings: number;
  /** Candidate segments the batch would write. */
  segments: number;
  /**
   * Set once Apply has been pressed and the batch came back — what it actually
   * touched, and what it declined to. Absent while the offer is still a
   * question.
   */
  applied?: { touched: number; skippedLocked: number } | null;
  onApply: () => void;
  onDismiss: () => void;
  onUndo: () => void;
}) {
  const applyRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    // Focusable, never focus-stealing: the strip appears mid-flow and grabbing
    // the caret out of whatever the user was doing is how an offer becomes an
    // interruption.
    if (applyRef.current?.dataset.autofocus === "1") applyRef.current.focus();
  }, []);

  if (applied) {
    const { message, undo } = relabelResult(
      displayName,
      applied.touched,
      applied.skippedLocked,
    );
    return (
      <div className="relabel-offer applied" role="status">
        <p className="relabel-question">{message}</p>
        <div className="relabel-actions">
          <button type="button" className="relabel-undo" onClick={onUndo}>
            {undo}
          </button>
        </div>
      </div>
    );
  }

  const copy = relabelOffer(displayName, meetings, segments);
  // No prompt at all when there is nothing to offer: an Apply that would change
  // zero rows still teaches the user that Yap acts on its own.
  if (!copy) return null;

  return (
    <div className="relabel-offer" role="group" aria-label="Label earlier meetings">
      <p className="relabel-question">{copy.question}</p>
      <div className="relabel-actions">
        <button
          type="button"
          ref={applyRef}
          className="relabel-apply"
          onClick={onApply}
        >
          {copy.apply}
        </button>
        <button type="button" className="relabel-dismiss" onClick={onDismiss}>
          {copy.dismiss}
        </button>
      </div>
    </div>
  );
}
