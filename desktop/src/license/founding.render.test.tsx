/**
 * yap-founding-coupon-follows-launch — the founding-price copy follows the
 * backend's `founding_open`, never a calendar and never the webview's own
 * arithmetic.
 *
 * `founding_open` is computed in Rust (`license::founding_valid`: checkout open
 * AND inside the 30 days after `FOUNDING_OPENED_AT`). While it is false there is
 * no "FOUNDING19" and no "Founding price" anywhere in Settings → License or in
 * the purchase sheet: a promo nobody can redeem is a promise Yap cannot keep.
 *
 * Rendered through a server render (as `pill/license.render.test.tsx` does):
 * it runs the real component bodies without a DOM or a new devDependency.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import LicensePanel from "./LicensePanel";
import PurchasePrompt from "./PurchasePrompt";
import { FOUNDING_CODE, type LicenseStatus } from "./status";

const noop = () => {};
const noopAsync = () => Promise.resolve();

function trial(founding_open: boolean): LicenseStatus {
  return {
    state: "trial",
    days_left: 5,
    expires_at_ms: 0,
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: 5,
    trial_expires_at_ms: 0,
    revocation_checked_at_ms: null,
    revoked_count: 0,
    purchase_open: founding_open,
    founding_open,
  };
}

const panel = (founding_open: boolean) =>
  renderToStaticMarkup(
    <LicensePanel
      status={trial(founding_open)}
      onBuy={noop}
      onActivate={noopAsync}
      onDeactivate={noopAsync}
    />,
  );

const prompt = (founding_open: boolean) =>
  renderToStaticMarkup(
    <PurchasePrompt
      foundingOpen={founding_open}
      onBuy={noop}
      onEnterKey={noop}
      onDismiss={noop}
    />,
  );

describe("founding copy follows founding_open", () => {
  it("the code is the one Stripe knows", () => {
    expect(FOUNDING_CODE).toBe("FOUNDING19");
  });

  it("founding_open false: LicensePanel shows no FOUNDING19 and no Founding price", () => {
    const html = panel(false);
    expect(html).toContain("Buy Yap");
    expect(html).not.toContain("FOUNDING19");
    expect(html).not.toContain("Founding price");
  });

  it("founding_open false: PurchasePrompt shows no FOUNDING19 and no Founding price", () => {
    const html = prompt(false);
    expect(html).toContain("Buy Yap");
    expect(html).not.toContain("FOUNDING19");
    expect(html).not.toContain("Founding price");
  });

  it("founding_open true: LicensePanel shows both", () => {
    const html = panel(true);
    expect(html).toContain("FOUNDING19");
    expect(html).toContain("Founding price");
  });

  it("founding_open true: PurchasePrompt shows both", () => {
    const html = prompt(true);
    expect(html).toContain("FOUNDING19");
    expect(html).toContain("Founding price");
  });
});
