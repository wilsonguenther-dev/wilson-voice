import { describe, expect, it } from "vitest";
import { errorCode, errorText, isLicenseRequired, LICENSE_REQUIRED } from "./errors";

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
