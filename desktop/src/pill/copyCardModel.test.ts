// yap-p0-insertion-bridge-fallback — the Rust <-> pill contract for the Copy
// card. No compiler checks a Tauri event name or a serde payload across the
// boundary, so this reads paste.rs itself.
import { describe, expect, it } from "vitest";
import pasteRs from "../../src-tauri/src/paste.rs?raw";
import floatPillRs from "../../src-tauri/src/float_pill.rs?raw";
import {
  COPY_CARD_CLEAR_EVENT,
  COPY_CARD_HINT,
  COPY_CARD_HINT_NO_REPAIR,
  INSERT_COPY_CARD_EVENT,
  canRepair,
  cardHint,
  cardPreview,
  isCopyCard,
} from "./copyCardModel";

describe("copy card contract", () => {
  it("event name and hint line match paste.rs", () => {
    expect(pasteRs).toContain(`pub const INSERT_COPY_CARD_EVENT: &str = "${INSERT_COPY_CARD_EVENT}";`);
    expect(pasteRs).toContain(`pub const COPY_CARD_MESSAGE: &str = "${COPY_CARD_HINT}";`);
    // serde camelCase of `repair_service` is the field the pill reads.
    expect(pasteRs).toMatch(/#\[serde\(rename_all = "camelCase"\)\]\npub struct CopyCard \{/);
    expect(pasteRs).toContain("pub repair_service: Option<&'static str>,");
    expect(pasteRs).toContain(`pub const COPY_CARD_MESSAGE_NO_REPAIR: &str = "${COPY_CARD_HINT_NO_REPAIR}";`);
    expect(floatPillRs).toContain(`pub const COPY_CARD_CLEAR_EVENT: &str = "${COPY_CARD_CLEAR_EVENT}";`);
  });

  it("offers Repair only when Rust named a service (never over a working grant)", () => {
    const base = { text: "ship it", detail: "x", copied: true };
    const noReceipt = { ...base, reason: "no_receipt", repairService: null };
    expect(isCopyCard(noReceipt)).toBe(true);
    expect(canRepair(noReceipt)).toBe(false);
    expect(cardHint(true, false)).toBe("Copied — press ⌘V.");
    expect(cardHint(true, false)).not.toContain("Repair");
    expect(canRepair({ repairService: "accessibility" })).toBe(true);
    expect(cardHint(true, false, "ax_unverified")).not.toContain("Repair");
  });

  it("accepts the Rust payload and renders its line", () => {
    const payload = { text: "ship it", reason: "no_permission", detail: "x", copied: true, repairService: "accessibility" };
    expect(isCopyCard(payload)).toBe(true);
    expect(isCopyCard({ text: "" })).toBe(false);
    expect(isCopyCard(null)).toBe(false);
    expect(cardHint(true)).toBe("Copied — press ⌘V. Repair to auto-insert.");
    expect(cardHint(false)).not.toBe(cardHint(true));
    expect(cardPreview("a  b\n c")).toBe("a b c");
    expect(cardPreview("x".repeat(200), 10)).toHaveLength(10);
  });
});
