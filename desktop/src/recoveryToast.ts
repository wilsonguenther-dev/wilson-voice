// yap-recovery-followups (d) — the toast after "Finish transcribing" / "Retry"
// fails on a recovered take.
//
// It used to be the backend's error string verbatim ("…(2 of 5 sections
// failed)…", engine text, paths). What the user needs at that moment is what
// they can DO, in plain words: Retry, Keep what was transcribed (only when
// there are words to keep), or Discard. No error codes, no ids, no stack text.
// The row's own line under the take still says why it failed.

/** The part of a failed-dictation row this copy depends on. */
export type FinishFailedRow = { attemptText?: string | null };

export const FINISH_FAILED_WITH_WORDS =
  "Yap couldn't finish this take. Nothing is lost: you can Retry, Keep what was transcribed, or Discard it.";

export const FINISH_FAILED_NO_WORDS =
  "Yap couldn't finish this take. Nothing is lost: you can Retry, or Discard it.";

export const FINISH_FAILED_ROW_GONE =
  "This take's audio is gone and none of it had been transcribed, so there was nothing left to finish.";

/**
 * The toast for a failed Finish. `row` is the take as History holds it AFTER
 * the attempt: `null` when the backend dropped it (no audio and no words),
 * `undefined` when it could not be re-read.
 */
export function finishFailedToast(row: FinishFailedRow | null | undefined): string {
  if (row === null) return FINISH_FAILED_ROW_GONE;
  // Keep is offered exactly when the row has saved words (Home.tsx shows the
  // button on the same condition). Unknown row: name everything the row can
  // offer — the button the user sees is the authority.
  if (row === undefined || (row.attemptText ?? "").trim() !== "") {
    return FINISH_FAILED_WITH_WORDS;
  }
  return FINISH_FAILED_NO_WORDS;
}
