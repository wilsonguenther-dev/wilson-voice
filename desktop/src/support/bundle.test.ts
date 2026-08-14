import { describe, expect, it } from "vitest";
import {
  actionExplainer,
  actionLabel,
  describeEntry,
  formatBytes,
  isRedacted,
  type SupportBundlePreview,
} from "./bundle";

const BASE: SupportBundlePreview = {
  fileName: "Yap-Diagnostics-0.8.0-20260812-141530.zip",
  recipient: "wilson@drivia.consulting",
  mailAvailable: true,
  totalBytes: 18_432,
  entries: [],
};

describe("the button says what it is about to do", () => {
  it("promises a compose window only when AppKit said one is achievable", () => {
    expect(actionLabel({ ...BASE, mailAvailable: true })).toBe(
      "Open the email with this attached",
    );
    expect(actionLabel({ ...BASE, mailAvailable: false })).toBe(
      "Save it and show me the file",
    );
  });

  it("never promises mail on a Mac whose client cannot be driven", () => {
    // The `canPerformWithItems:` guard is load-bearing, not decorative:
    // Outlook/Airmail/Unibox users are the common case it protects.
    const copy = actionExplainer({ ...BASE, mailAvailable: false });
    expect(copy).toContain("could not find a mail app");
    expect(copy).toContain("clipboard");
    expect(copy).not.toContain("open a message");
  });

  it("is explicit that the compose path sends nothing on its own", () => {
    expect(actionExplainer(BASE)).toContain("Nothing is sent until you press send");
  });

  it("names the file that will appear on the Desktop, in both paths", () => {
    expect(actionExplainer(BASE)).toContain(BASE.fileName);
    expect(actionExplainer({ ...BASE, mailAvailable: false })).toContain(
      BASE.fileName,
    );
  });
});

describe("the preview names files the way a person would", () => {
  it("translates every entry the builder produces", () => {
    for (const [name, expected] of [
      ["README.txt", "What this file is, and what is not in it"],
      ["environment.txt", "App version, bundle id and macOS version"],
      ["crash-summary.txt", "The crashes Yap recorded on this Mac"],
      ["permissions.txt", "Which permissions are granted"],
      ["models.txt", "Which speech models are downloaded"],
      ["logs/yap.log", "The current log — redacted"],
      ["logs/yap.log.3", "Rotated log 3 — redacted"],
    ]) {
      expect(describeEntry(name)).toBe(expected);
    }
  });

  it("marks exactly the entries that went through the redactor", () => {
    expect(isRedacted("logs/yap.log")).toBe(true);
    expect(isRedacted("logs/yap.log.4")).toBe(true);
    expect(isRedacted("crash-summary.txt")).toBe(false);
  });
});

describe("sizes read as sizes", () => {
  it("formats bytes, kilobytes and megabytes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(64 * 1024)).toBe("64 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
  });
});
