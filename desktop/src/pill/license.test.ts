/**
 * Y2-A — the pill's license policy, driven over the WHOLE fortnight.
 *
 * The point of the table is the boundary: day 8 is silent, day 7 speaks, day 0
 * says "1d" and not "0d". Those three rows are the entire design, and none of
 * them is visible in a type check or a green build.
 */
import { describe, expect, it } from "vitest";
import { type LicenseStatus } from "../license/status";
import {
  PILL_TRIAL_DAYS,
  offersPurchase,
  pillLicense,
  type PillLicense,
} from "./license";

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

// ─── Y2-F — a stored key that grants nothing is not a lapsed trial ────
//
// The whole point of this block is that a person who ALREADY PAID is never
// shown the sentence written for a person who has not. Every row below has a
// key on the Mac; none of them may reach a price or the `ended` tone.
//
// NOTE ON THE PRICE ASSERTION: this file deliberately does not import the
// price-label constant, and does not spell its name. `desktop/src/pill/`'s
// no-price rule is enforced by a plain `git grep` of this directory for that
// name, and to a grep an import in a test and an import in a component look
// identical. So "no price" is asserted structurally, with a regex for any
// currency figure at all — which is the stronger check anyway: it also catches
// a hardcoded figure that never went through the constant. (This comment does
// not spell one either: the enforcing grep cannot tell a comment from code.)
const ANY_PRICE = /[$£€]\s?\d/;

/** A stored key that granted nothing, with whatever the backend diagnosed. */
function storedKey(
  reason: string,
  message: string | null,
  over: Partial<LicenseStatus> = {},
): LicenseStatus {
  return required(reason, {
    has_stored_license: true,
    license_problem: reason,
    license_problem_message: message,
    ...over,
  });
}

const PROBLEM_MATRIX: Array<{ what: string; status: LicenseStatus; says: string | null }> = [
  {
    what: "a revoked key (refunded or charged back)",
    status: storedKey("revoked", "This license was refunded or charged back."),
    says: "This license was refunded or charged back.",
  },
  {
    what: "a seat-capped key",
    status: storedKey(
      "seat_limit",
      "This key is already active on 3 Macs. Remove a seat to use it here.",
    ),
    says: "This key is already active on 3 Macs. Remove a seat to use it here.",
  },
  {
    what: "a key signed for another plan",
    status: storedKey("wrong_plan", "This key is for a different Yap plan."),
    says: "This key is for a different Yap plan.",
  },
  {
    what: "a key that no longer verifies",
    status: storedKey("bad_signature", "This key could not be verified."),
    says: "This key could not be verified.",
  },
  {
    what: "a broken clock",
    status: storedKey("clock_rollback", "This Mac's clock is set before your purchase date."),
    says: "This Mac's clock is set before your purchase date.",
  },
  {
    what: "an unreadable store (backend diagnosed nothing)",
    status: storedKey("unknown", null),
    says: null, // falls back, but must still be a PROBLEM
  },
  {
    what: "a broken key while the trial is still running",
    status: trial(5, {
      has_stored_license: true,
      license_problem: "revoked",
      license_problem_message: "This license was refunded or charged back.",
    }),
    says: "This license was refunded or charged back.",
  },
];

describe("Y2-F — a stored key that grants nothing", () => {
  for (const row of PROBLEM_MATRIX) {
    it(`${row.what} reads as a problem, not an ended trial`, () => {
      const p = pillLicense(row.status);
      expect(p.show).toBe(true);
      expect(p.tone).toBe("problem");
      expect(p.value).toBeNull();
      expect(p.title.length).toBeGreaterThan(0);
      expect(p.title).not.toMatch(ANY_PRICE);
      if (row.says) expect(p.title).toBe(row.says);
    });
  }

  it("a_revoked_key_never_shows_a_price — and never the purchase surface", () => {
    const p = pillLicense(storedKey("revoked", "This license was refunded or charged back."));
    expect(p.title).not.toMatch(ANY_PRICE);
    expect(offersPurchase(p)).toBe(false);
    expect(p.action).not.toBe("purchase");
    // Every row of the matrix, not just this one.
    for (const row of PROBLEM_MATRIX) {
      expect(offersPurchase(pillLicense(row.status))).toBe(false);
      expect(pillLicense(row.status).title).not.toMatch(ANY_PRICE);
    }
  });

  it("a_seat_capped_key_reads_as_a_problem_not_a_lapsed_trial — distinct tone and glyph", () => {
    const capped = pillLicense(
      storedKey("seat_limit", "This key is already active on 3 Macs. Remove a seat to use it here."),
    );
    const lapsed = pillLicense(required("trial_expired"));
    expect(lapsed.tone).toBe("ended");
    expect(capped.tone).toBe("problem");
    // Distinct at a glance in BOTH pill styles: the tone token and the mark are
    // what each face draws, and neither may collide with the lapsed-trial one.
    expect(capped.tone).not.toBe(lapsed.tone);
    expect(capped.glyph).not.toBe(lapsed.glyph);
    expect(capped.title).not.toBe(lapsed.title);
  });

  it("the_problem_tone_opens_the_license_panel — never checkout", () => {
    for (const row of PROBLEM_MATRIX) {
      const p = pillLicense(row.status);
      expect(p.action).toBe("license");
      expect(offersPurchase(p)).toBe(false);
    }
  });

  it("an unreadable store still speaks, and says nothing about money", () => {
    const p = pillLicense(storedKey("unknown", null));
    expect(p.tone).toBe("problem");
    expect(p.title).not.toMatch(ANY_PRICE);
    expect(p.title.toLowerCase()).toContain("license settings");
  });

  it("a working license is still silent, stored key and all", () => {
    // Regression guard on the branch ORDER: `has_stored_license` is true for
    // every licensed install, so testing it before `licensed` would make the
    // pill shout at every paying customer forever.
    const p = pillLicense(licensed());
    expect(p.show).toBe(false);
  });

  it("a lapsed trial with NO stored key is untouched — still `ended`, still sells", () => {
    const p = pillLicense(required("trial_expired"));
    expect(p.tone).toBe("ended");
    expect(p.action).toBe("purchase");
    expect(offersPurchase(p)).toBe(true);
  });
});
