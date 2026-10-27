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
  COPY_CARD_HINT_SECURE_FIELD,
  COPY_CARD_HINT_TARGET_MOVED,
  blockedLine,
  canInsertHere,
  canRepair,
  cardHint,
  cardPreview,
  isCopyCard,
  isHeldTake,
  settingsPaneFor,
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

describe("yap-insert-bridge-2026-10-26 — the card names the blocked permission", () => {
  it("pins the Rust field and the names it sends", () => {
    expect(pasteRs).toContain("pub blocked_permissions: Vec<&'static str>,");
    expect(pasteRs).toContain('CopyCardReason::NoPermission => vec!["PostEvent", "Accessibility"],');
  });

  it("says which permission is off and offers Open Settings", () => {
    const both = { blockedPermissions: ["PostEvent", "Accessibility"] };
    expect(blockedLine(both)).toBe("Yap can't type here: PostEvent and Accessibility are off.");
    expect(settingsPaneFor(both)).toBe("Accessibility");
    expect(blockedLine({ blockedPermissions: ["PostEvent"] })).toBe("Yap can't type here: PostEvent is off.");
  });

  it("names nothing (and offers no Settings) when a grant works", () => {
    expect(blockedLine({ blockedPermissions: [] })).toBeNull();
    expect(blockedLine({})).toBeNull();
    expect(settingsPaneFor({ blockedPermissions: [] })).toBeNull();
    const noReceipt = { text: "t", reason: "no_receipt", detail: "x", copied: true, repairService: null, blockedPermissions: [] };
    expect(isCopyCard(noReceipt)).toBe(true);
  });
});

describe("yap-test-channel-product-name — the blocked line names the running product", () => {
  it("names Yap Test on the test channel and Yap otherwise", () => {
    const both = { blockedPermissions: ["PostEvent", "Accessibility"] };
    expect(blockedLine({ ...both, product: "Yap Test" })).toBe(
      "Yap Test can't type here: PostEvent and Accessibility are off.",
    );
    expect(blockedLine({ ...both, product: "Yap" })).toBe("Yap can't type here: PostEvent and Accessibility are off.");
    expect(blockedLine({ ...both, product: null })).toBe("Yap can't type here: PostEvent and Accessibility are off.");
  });

  it("pins the Rust field and fills it at every card the pill is shown", () => {
    expect(pasteRs).toContain("pub product: String,");
    const shown = pasteRs.match(/show_copy_card\(CopyCard \{/g) ?? [];
    const named = pasteRs.match(/product: crate::perm_truth::product_name\(\),/g) ?? [];
    expect(shown.length).toBeGreaterThan(0);
    expect(named.length).toBe(shown.length);
  });
});

describe("Y6-C — a take whose target moved is held and insertable", () => {
  it("pins the pill lines to paste.rs verbatim", () => {
    const one = (name: string) => {
      const m = pasteRs.match(new RegExp(`pub const ${name}: &str =\\s*"([^"]*)";`));
      return m?.[1];
    };
    expect(one("TARGET_MOVED_MESSAGE")).toBe(COPY_CARD_HINT_TARGET_MOVED);
    expect(one("SECURE_FIELD_MESSAGE")).toBe(COPY_CARD_HINT_SECURE_FIELD);
    expect(pasteRs).toContain("TargetMoved,");
    expect(pasteRs).toContain("SecureField,");
  });

  it("offers Insert only for a moved target, and never times a held take out", () => {
    expect(canInsertHere({ reason: "target_moved" })).toBe(true);
    expect(canInsertHere({ reason: "secure_field" })).toBe(false);
    expect(canInsertHere({ reason: "no_receipt" })).toBe(false);
    expect(isHeldTake({ reason: "target_moved" })).toBe(true);
    expect(isHeldTake({ reason: "secure_field" })).toBe(true);
    expect(isHeldTake({ reason: "no_receipt" })).toBe(false);
    expect(cardHint(true, false, "target_moved")).toBe(COPY_CARD_HINT_TARGET_MOVED);
    expect(cardHint(false, false, "secure_field")).toBe(COPY_CARD_HINT_SECURE_FIELD);
    expect(COPY_CARD_HINT_SECURE_FIELD).toContain("not on your clipboard");
  });
});
