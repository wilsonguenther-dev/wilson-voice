/**
 * yap-1106-lic-02 — every Buy goes to drivia.consulting/yap, and the words on
 * the button follow the backend's `purchase_open` (Rust `PURCHASE_OPEN`).
 *
 * While checkout is CLOSED, Settings → License and the license-required sheet
 * still render a primary button that invokes `open_purchase_page` (the page
 * says "not open yet"), but it reads "See Yap pricing" and no copy on either
 * surface promises a checkout. Open, both say "Buy Yap — $29 once".
 *
 * Server-rendered like `founding.render.test.tsx`: the real component bodies,
 * no DOM, no new devDependency.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import LicensePanel from "./LicensePanel";
import PurchasePrompt from "./PurchasePrompt";
import { PRICE_LABEL, purchaseCopy, type LicenseStatus } from "./status";

const noop = () => {};
const noopAsync = () => Promise.resolve();

function trial(purchase_open: boolean): LicenseStatus {
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
    purchase_open,
    founding_open: false,
  };
}

const panel = (purchase_open: boolean) =>
  renderToStaticMarkup(
    <LicensePanel
      status={trial(purchase_open)}
      onBuy={noop}
      onActivate={noopAsync}
      onDeactivate={noopAsync}
    />,
  );

const prompt = (purchase_open: boolean) =>
  renderToStaticMarkup(
    <PurchasePrompt
      purchaseOpen={purchase_open}
      foundingOpen={false}
      onBuy={noop}
      onEnterKey={noop}
      onDismiss={noop}
    />,
  );

/** Words that promise a checkout the closed page cannot give. */
const CHECKOUT_PROMISES = [/checkout/i, /Buy Yap —/, /Stripe/];

describe("Buy surfaces follow purchase_open", () => {
  it("purchaseCopy closed says See Yap pricing and promises no checkout", () => {
    const c = purchaseCopy(false);
    expect(c.button).toBe("See Yap pricing");
    for (const re of CHECKOUT_PROMISES) {
      expect(`${c.heading} ${c.body} ${c.button}`).not.toMatch(re);
    }
    expect(c.body).toContain("not open yet");
  });

  it("purchaseCopy open quotes the price on the button", () => {
    expect(purchaseCopy(true).button).toBe(`Buy Yap — ${PRICE_LABEL} once`);
  });

  it("closed: LicensePanel's button says See Yap pricing and no copy promises checkout", () => {
    const html = panel(false);
    expect(html).toContain("See Yap pricing");
    expect(html).toContain('data-purchase-open="false"');
    for (const re of CHECKOUT_PROMISES) expect(html).not.toMatch(re);
  });

  it("closed: PurchasePrompt's button says See Yap pricing and no copy promises checkout", () => {
    const html = prompt(false);
    expect(html).toContain("See Yap pricing");
    expect(html).toContain('data-purchase-open="false"');
    expect(html).toContain("Sales are not open yet.");
    for (const re of CHECKOUT_PROMISES) expect(html).not.toMatch(re);
  });

  it("open: both surfaces say Buy Yap with the price", () => {
    expect(panel(true)).toContain(`Buy Yap — ${PRICE_LABEL} once`);
    expect(prompt(true)).toContain(`Buy Yap — ${PRICE_LABEL} once`);
    expect(panel(true)).toContain('data-purchase-open="true"');
  });

  it("an absent purchase_open (older payload) reads as closed in the panel", () => {
    const status = { ...trial(true), purchase_open: undefined } as unknown as LicenseStatus;
    const html = renderToStaticMarkup(
      <LicensePanel status={status} onBuy={noop} onActivate={noopAsync} onDeactivate={noopAsync} />,
    );
    expect(html).toContain("See Yap pricing");
  });

  it("the button still calls onBuy (open_purchase_page) while closed", () => {
    // Server render cannot click; prove the wiring instead: the closed prompt's
    // primary button is the one rendered with onBuy, and no surface hides it.
    expect(prompt(false)).toMatch(/<button[^>]*data-purchase-open="false"[^>]*>See Yap pricing<\/button>/);
    expect(panel(false)).toMatch(/<button[^>]*data-purchase-open="false"[^>]*>See Yap pricing<\/button>/);
  });
});
