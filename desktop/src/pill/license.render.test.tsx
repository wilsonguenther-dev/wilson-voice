/**
 * yap24-PILL1 (carrying Y2-B's render test from 8d85cdf) — the countdown is
 * actually IN THE MARKUP of both pills, for the whole trial.
 *
 * `license.test.ts` proves the POLICY. This proves both capsules DRAW it: for
 * all of Y2-A both pills took the `license` prop and ignored it while every
 * check stayed green. Y2-B's original used jsdom; main runs these through a
 * server render (as ClassicPill.mark.test.tsx does), which runs the real
 * component body and hooks under a live dispatcher without a DOM — enough to
 * read the chip, and no new devDependency.
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import ClassicPill from "./PillShell";
import YappyPill from "./YappyPill";
import { pillLicense } from "./license";
import { sceneLicense } from "./forced";
import { type LicenseStatus } from "../license/status";

function trial(days: number, purchase_open = false): LicenseStatus {
  return {
    state: "trial",
    days_left: days,
    expires_at_ms: 0,
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: days,
    trial_expires_at_ms: 0,
    revocation_checked_at_ms: null,
    revoked_count: 0,
    purchase_open,
    founding_open: false,
  } as LicenseStatus;
}

const classic = (s: LicenseStatus) => renderToStaticMarkup(<ClassicPill license={pillLicense(s)} />);
const yappy = (s: LicenseStatus) => renderToStaticMarkup(<YappyPill license={pillLicense(s)} />);

describe("yap24-PILL1 — both pills draw the countdown for the whole trial", () => {
  for (let days = 14; days >= 0; days--) {
    it(`day ${days}: classic and yappy both carry the numeral and the title`, () => {
      const lic = pillLicense(trial(days));
      const c = classic(trial(days));
      const y = yappy(trial(days));
      expect(c).toContain(`lic-${lic.tone}`);
      expect(c).toContain(`title="${lic.title}"`);
      expect(c).toContain(`>${lic.value}<`);
      expect(y).toContain(`kami-license-${lic.tone}`);
      expect(y).toContain(`title="${lic.title}"`);
      expect(y).toContain(`>${lic.value!.replace(/d$/, "")}<`);
    });
  }

  it("days 14..8 are the quiet tone, with no hourglass on classic", () => {
    const c = classic(trial(10));
    expect(c).toContain("lic-trial-quiet");
    expect(c).not.toContain("pill-upgrade-glyph");
    expect(c).toContain('title="Free trial — 10 days left"');
  });

  it("with checkout closed the chip is an inert note, never a button", () => {
    const c = classic(trial(5));
    expect(c).toMatch(/<span class="pill-upgrade pill-upgrade-info lic-trial"[^>]*role="note"/);
    expect(c).not.toMatch(/<button[^>]*pill-upgrade/);
  });

  it("with checkout open the same chip is the purchase button", () => {
    const c = classic(trial(5, true));
    expect(c).toMatch(/<button[^>]*class="pill-upgrade lic-trial"/);
  });

  it("a licensed install draws nothing on either pill", () => {
    const lic = { ...trial(5), state: "licensed", plan: "lifetime", seats: 3, kid: "k" } as unknown as LicenseStatus;
    expect(classic(lic)).not.toContain("pill-upgrade");
    expect(yappy(lic)).not.toContain("kami-license");
  });

  it("the matrix's trial-quiet scene is the 14-day worst case", () => {
    const s = sceneLicense("trial-quiet")!;
    expect(pillLicense(s).tone).toBe("trial-quiet");
    expect(pillLicense(s).value).toBe("14d");
    expect(sceneLicense("nope")).toBeNull();
  });
});
