// yap-1107-secure-input-own-field-quiet — the main window's Secure Input
// surface: Yap's own key field gets one quiet line, never the red banner;
// another app's hold keeps the red banner exactly as before.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import SecureInputBanner, { OWN_FIELD_MESSAGE } from "./SecureInputBanner";

const html = (status: Parameters<typeof SecureInputBanner>[0]["status"]) =>
  renderToStaticMarkup(<SecureInputBanner status={status} />);
const hasBlockedBanner = (out: string) => /class="banner blocked"/.test(out);

describe("Secure Input banner", () => {
  it("own key field: the quiet line, no red banner, no detail, no holder name", () => {
    const out = html({ secureInputBlocked: false, secureInputDetail: null, secureInputOwnField: true });
    expect(out).toContain("Shortcut paused while the key field is focused");
    expect(OWN_FIELD_MESSAGE).toBe("Shortcut paused while the key field is focused");
    expect(hasBlockedBanner(out)).toBe(false);
    expect(out).not.toContain("banner-detail");
    expect(out).not.toContain("wilson-voice");
    expect(out).not.toContain("another app");
  });

  it("another app holds it: the red banner as today, naming that app", () => {
    const detail =
      "Terminal (pid 431) has Secure Input on — macOS is hiding key events from Yap, so fn / fn⌃ cannot start a take.";
    const out = html({ secureInputBlocked: true, secureInputDetail: detail, secureInputOwnField: false });
    expect(hasBlockedBanner(out)).toBe(true);
    expect(out).toContain("another app is blocking keyboard monitoring");
    expect(out).toContain("Terminal (pid 431) has Secure Input on");
    expect(out).not.toContain(OWN_FIELD_MESSAGE);
  });

  it("blocked wins if both flags ever arrive together", () => {
    const out = html({ secureInputBlocked: true, secureInputDetail: "x", secureInputOwnField: true });
    expect(hasBlockedBanner(out)).toBe(true);
    expect(out).not.toContain(OWN_FIELD_MESSAGE);
  });

  it("no hold (or an older build without the flag): renders nothing", () => {
    expect(html({ secureInputBlocked: false, secureInputDetail: null, secureInputOwnField: false })).toBe("");
    expect(html({ secureInputBlocked: false, secureInputDetail: null })).toBe("");
  });
});
