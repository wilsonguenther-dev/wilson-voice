import { describe, expect, it } from "vitest";
import {
  adviceFor, ERROR_CATALOGUE, errorAdvice, errorCode, errorText, GENERIC_ADVICE, isLicenseRequired, LICENSE_REQUIRED,
  pasteFailedToast, REFUSED_CODE,
} from "./errors";

describe("command rejections", () => {
  it("reads the sentence out of a structured CommandError", () => {
    const e = { code: "license_required", message: "Your 14-day Yap trial has ended." };
    expect(errorText(e)).toBe("Your 14-day Yap trial has ended.");
    expect(errorCode(e)).toBe(LICENSE_REQUIRED);
    expect(isLicenseRequired(e)).toBe(true);
  });

  it("still renders the plain-string errors every other command returns", () => {
    expect(errorText("Microphone is busy")).toBe("Microphone is busy");
    expect(errorCode("Microphone is busy")).toBeNull();
    expect(isLicenseRequired("Microphone is busy")).toBe(false);
  });

  // The regression this file exists to prevent: `String({code, message})` is
  // "[object Object]", and a toast saying that is worse than no toast at all.
  it("never renders [object Object]", () => {
    const rendered = errorText({ code: "bad_signature", message: "That key did not check out." });
    expect(rendered).not.toContain("object Object");
  });

  it("falls back safely on shapes it does not recognise", () => {
    expect(errorText(null)).toBe("null");
    expect(errorText({ code: 7 })).toContain("object");
    expect(errorCode({ message: "no code here" })).toBeNull();
  });
});

// Y5-F — the catalogue. These names are grepped by the loop's acceptance.
describe("error catalogue", () => {
  const entries = Object.entries(ERROR_CATALOGUE);

  it("every_catalogued_code_has_a_human_line", () => {
    expect(entries.length).toBeGreaterThan(5);
    for (const [code, advice] of entries) {
      expect(advice.line.trim(), code).not.toBe("");
      expect(errorAdvice(code)).toBe(advice);
      if (advice.action) expect(advice.action.label.trim(), code).not.toBe("");
    }
    expect(GENERIC_ADVICE.line.trim()).not.toBe("");
  });

  it("no_line_leaks_a_rust_type_or_path", () => {
    const leaks = [/Error\(/, /unwrap/, /::/, /\bOs \{/, /\b(String|Result|Option|Vec)</, /\/(Users|private|var|tmp)\//, /\.rs\b/, /[A-Za-z]:\\/];
    for (const { line } of [...entries.map(([, a]) => a), GENERIC_ADVICE]) {
      for (const re of leaks) expect(line, `${line} ~ ${re}`).not.toMatch(re);
    }
  });

  // The take path's codes, enumerated explicitly (take_error.rs
  // TAKE_PATH_CODES is the Rust side of this list and reads the catalogue in
  // its own test). A failure is never silent: each one must reach a toast /
  // pill line of its OWN, never the generic fallback.
  it("every take-path failure has its own line, not the generic one", () => {
    const takePath = [
      "mic_unavailable", "mic_disconnected", "model_missing", "capture_failed", "transcription_failed",
      "history_write_failed", "clipboard_failed", "paste_not_confirmed", "paste_not_copied", "take_failed",
      "silent_capture", "mic_permission_required", "license_required", "cancelled",
    ];
    for (const code of takePath) expect(errorAdvice(code), code).not.toBe(GENERIC_ADVICE);
  });

  it("an unknown code or a raw string gets advice that still says what to do", () => {
    expect(errorAdvice("no_such_code")).toBe(GENERIC_ADVICE);
    expect(errorAdvice(null).action?.kind).toBe("support");
    const raw = 'Os { code: 2, kind: NotFound, message: "No such file or directory" }';
    const a = adviceFor(raw);
    expect(a.line).not.toContain("Os {");
    expect(a.action?.kind).toBe("support");
    expect(adviceFor(raw, "Yap could not save that word.").line).toBe("Yap could not save that word.");
    expect(adviceFor({ code: "model_missing", message: "x" })).toBe(ERROR_CATALOGUE.model_missing);
  });
});

// Y5-F fix round 1 — the two reviewer BLOCKINGs this file can see.
describe("fix round 1", () => {
  it("a refusal Yap wrote reaches the toast word for word (reserved chord)", () => {
    const why = "⌘Space is macOS's Spotlight — pick another chord";
    const a = adviceFor({ code: REFUSED_CODE, message: why });
    expect(a.line).toBe(why);
    expect(a.line).not.toBe(GENERIC_ADVICE.line);
    expect(a.action).toBeNull();
    // A bare string is still never shown raw.
    expect(adviceFor("Os { code: 2 }").line).toBe(GENERIC_ADVICE.line);
    // An uncatalogued command failure (lib.rs COMMAND_FAILED_CODE) stays hidden.
    expect(adviceFor({ code: "command_failed", message: "io: denied" }).line).toBe(GENERIC_ADVICE.line);
  });

  it("paste_failed only says the text is on the clipboard when it is", () => {
    const kept = pasteFailedToast({ id: "r1", code: "paste_not_confirmed" });
    expect(kept.line).toBe(ERROR_CATALOGUE.paste_not_confirmed.line);
    expect(kept.action).toEqual({ kind: "copy-again", label: "Copy again", arg: "r1" });
    for (const code of ["paste_not_copied", null, undefined, "something_else"]) {
      const t = pasteFailedToast({ id: "r2", code });
      expect(t.line).not.toMatch(/is on your clipboard/);
      expect(t.line).toBe(ERROR_CATALOGUE.paste_not_copied.line);
      expect(t.key).toBe("paste");
      expect(t.action.kind).toBe("copy-again");
    }
  });
});
