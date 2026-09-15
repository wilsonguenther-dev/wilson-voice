/**
 * Y2-A — the pill's license policy, driven over the WHOLE fortnight.
 *
 * The point of the table is the boundary: day 8 is silent, day 7 speaks, day 0
 * says "1d" and not "0d". Those three rows are the entire design, and none of
 * them is visible in a type check or a green build.
 */
import { describe, expect, it } from "vitest";
import { type LicenseStatus } from "../license/status";
import { PILL_TRIAL_DAYS, pillLicense, type PillLicense } from "./license";

const EXPIRES = 1_800_000_000_000;

function trial(days: number, over: Partial<LicenseStatus> = {}): LicenseStatus {
  return {
    state: "trial",
    days_left: days,
    expires_at_ms: EXPIRES,
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: days,
    trial_expires_at_ms: EXPIRES,
    revocation_checked_at_ms: null,
    revoked_count: 0,
    ...over,
  } as LicenseStatus;
}

function licensed(over: Partial<LicenseStatus> = {}): LicenseStatus {
  return {
    state: "licensed",
    plan: "lifetime",
    seats: 3,
    kid: "yap_live_abc",
    license_problem: null,
    license_problem_message: null,
    has_stored_license: true,
    trial_days_left: 0,
    trial_expires_at_ms: EXPIRES,
    revocation_checked_at_ms: EXPIRES,
    revoked_count: 0,
    ...over,
  } as LicenseStatus;
}

function required(reason = "trial_expired", over: Partial<LicenseStatus> = {}): LicenseStatus {
  return {
    state: "license_required",
    reason,
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: 0,
    trial_expires_at_ms: EXPIRES,
    revocation_checked_at_ms: null,
    revoked_count: 0,
    ...over,
  } as LicenseStatus;
}

/** day → the exact answer the pill must give. 14 down to 0, nothing skipped. */
const FORTNIGHT: Array<[number, Pick<PillLicense, "show" | "tone" | "value">]> = [
  [14, { show: false, tone: "trial", value: null }],
  [13, { show: false, tone: "trial", value: null }],
  [12, { show: false, tone: "trial", value: null }],
  [11, { show: false, tone: "trial", value: null }],
  [10, { show: false, tone: "trial", value: null }],
  [9, { show: false, tone: "trial", value: null }],
  [8, { show: false, tone: "trial", value: null }],
  [7, { show: true, tone: "trial", value: "7d" }],
  [6, { show: true, tone: "trial", value: "6d" }],
  [5, { show: true, tone: "trial", value: "5d" }],
  [4, { show: true, tone: "trial", value: "4d" }],
  [3, { show: true, tone: "trial", value: "3d" }],
  [2, { show: true, tone: "trial", value: "2d" }],
  [1, { show: true, tone: "trial", value: "1d" }],
  [0, { show: true, tone: "urgent", value: "1d" }],
];

describe("pillLicense — the trial fortnight", () => {
  for (const [days, want] of FORTNIGHT) {
    it(`day ${days} → show=${want.show} tone=${want.tone} value=${want.value}`, () => {
      const got = pillLicense(trial(days));
      expect(got.show).toBe(want.show);
      expect(got.tone).toBe(want.tone);
      expect(got.value).toBe(want.value);
    });
  }

  it("goes quiet above the pill's own window, which is wider than the toast's", () => {
    expect(PILL_TRIAL_DAYS).toBe(7);
    expect(pillLicense(trial(PILL_TRIAL_DAYS + 1)).show).toBe(false);
    expect(pillLicense(trial(PILL_TRIAL_DAYS)).show).toBe(true);
  });

  it("never says 0d — the last day still has a day in it", () => {
    expect(pillLicense(trial(0)).value).toBe("1d");
    expect(pillLicense(trial(-3)).value).toBe("1d");
    expect(pillLicense(trial(-3)).tone).toBe("urgent");
  });

  it("carries a glyph and a title whenever it shows", () => {
    for (const days of [7, 4, 1, 0]) {
      const got = pillLicense(trial(days));
      expect(got.glyph).not.toBe("");
      expect(got.title).not.toBe("");
    }
  });
});

describe("pillLicense — licensed is silent, unconditionally", () => {
  it("shows nothing for a plain lifetime license", () => {
    expect(pillLicense(licensed()).show).toBe(false);
  });

  // A licensed Mac still carries the trial fields it had before the purchase.
  // If the pill looked at those instead of `state`, a paying customer would be
  // nagged forever — which is the single worst thing this surface could do.
  for (const days of [14, 7, 3, 1, 0, -1]) {
    it(`shows nothing for a licensed status whose trial fields say ${days}`, () => {
      const got = pillLicense(licensed({ trial_days_left: days, trial_expires_at_ms: EXPIRES }));
      expect(got.show).toBe(false);
      expect(got.value).toBe(null);
    });
  }

  it("shows nothing even when a revocation check has run and counted keys", () => {
    expect(pillLicense(licensed({ revoked_count: 5, trial_days_left: 0 })).show).toBe(false);
  });
});

describe("pillLicense — the trial is over", () => {
  it("speaks with the ended tone and no numeral", () => {
    const got = pillLicense(required());
    expect(got.show).toBe(true);
    expect(got.tone).toBe("ended");
    expect(got.value).toBe(null);
    expect(got.title).not.toBe("");
  });

  it("says the same thing whatever the refusal reason was", () => {
    for (const reason of ["trial_expired", "revoked", "bad_signature", "wrong_plan"]) {
      const got = pillLicense(required(reason));
      expect(got.show).toBe(true);
      expect(got.tone).toBe("ended");
    }
  });
});

describe("pillLicense — nothing has arrived yet", () => {
  it("is silent on null and undefined rather than guessing", () => {
    expect(pillLicense(null).show).toBe(false);
    expect(pillLicense(undefined).show).toBe(false);
  });
});

describe("pillLicense — the pill never quotes a price", () => {
  it("keeps every figure out of every string it hands the pill", () => {
    const strings = [
      ...FORTNIGHT.map(([d]) => pillLicense(trial(d))),
      pillLicense(required()),
      pillLicense(licensed()),
    ].flatMap((r) => [r.title, r.value ?? ""]);
    for (const s of strings) {
      expect(s).not.toMatch(/\$\d/);
    }
  });
});
