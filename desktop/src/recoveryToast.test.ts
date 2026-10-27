import { describe, expect, it } from "vitest";
import {
  FINISH_FAILED_NO_WORDS,
  FINISH_FAILED_ROW_GONE,
  FINISH_FAILED_WITH_WORDS,
  finishFailedToast,
} from "./recoveryToast";

// yap-recovery-followups (d): the toast after a failed Finish is plain words
// naming what the user can do — never engine output.
describe("failed Finish toast", () => {
  it("names Retry, Keep what was transcribed and Discard when words are saved (exact copy)", () => {
    const copy = finishFailedToast({ attemptText: "the words so far" });
    expect(copy).toBe(
      "Yap couldn't finish this take. Nothing is lost: you can Retry, Keep what was transcribed, or Discard it.",
    );
    expect(copy).toBe(FINISH_FAILED_WITH_WORDS);
    for (const action of ["Retry", "Keep what was transcribed", "Discard"]) {
      expect(copy).toContain(action);
    }
  });

  it("does not offer Keep when nothing was transcribed (exact copy)", () => {
    const copy = finishFailedToast({ attemptText: null });
    expect(copy).toBe("Yap couldn't finish this take. Nothing is lost: you can Retry, or Discard it.");
    expect(copy).toBe(FINISH_FAILED_NO_WORDS);
    expect(copy).not.toContain("Keep");
    expect(finishFailedToast({ attemptText: "   " })).toBe(FINISH_FAILED_NO_WORDS);
  });

  it("names all three actions when the row could not be re-read", () => {
    expect(finishFailedToast(undefined)).toBe(FINISH_FAILED_WITH_WORDS);
  });

  it("says plainly when the take was removed", () => {
    expect(finishFailedToast(null)).toBe(
      "This take's audio is gone and none of it had been transcribed, so there was nothing left to finish.",
    );
    expect(FINISH_FAILED_ROW_GONE).toBe(finishFailedToast(null));
  });

  it("carries no error codes, ids, numbers or stack text", () => {
    for (const copy of [FINISH_FAILED_WITH_WORDS, FINISH_FAILED_NO_WORDS, FINISH_FAILED_ROW_GONE]) {
      expect(copy).not.toMatch(/\d/);
      expect(copy).not.toMatch(/error|exception|stack|panic|chunk|window|section|[{}_]|::|\bat \w+\(/i);
    }
  });
});
