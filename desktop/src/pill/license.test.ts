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
  PILL_QUIET_FROM_DAYS,
  PILL_SHOW_DAYS,
  PILL_VALUE_MAX_CHARS,
  TRIAL_ENDS_TODAY,
  freeTrialTitle,
  splitValue,
  valueFitsSideDock,
  PILL_URGENT_DAYS,
  REVEAL_PURCHASE_COMMAND,
  offersPurchase,
  pillLicense,
  pillLicenseRoute,
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
    purchase_open: false,
    founding_open: false,
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
    purchase_open: false,
    founding_open: false,
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
    purchase_open: false,
    founding_open: false,
    ...over,
  } as LicenseStatus;
}

/** day → the exact answer the pill must give. 14 down to 0, nothing skipped. */
const FORTNIGHT: Array<[number, Pick<PillLicense, "show" | "tone" | "value">]> = [
  [14, { show: true, tone: "trial-quiet", value: "14d" }],
  [13, { show: true, tone: "trial-quiet", value: "13d" }],
  [12, { show: true, tone: "trial-quiet", value: "12d" }],
  [11, { show: true, tone: "trial-quiet", value: "11d" }],
  [10, { show: true, tone: "trial-quiet", value: "10d" }],
  [9, { show: true, tone: "trial-quiet", value: "9d" }],
  [8, { show: true, tone: "trial-quiet", value: "8d" }],
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

  it("yap24-PILL1: is never silent during a trial — quiet above PILL_SHOW_DAYS, not absent", () => {
    expect(PILL_SHOW_DAYS).toBe(7); // NOT repurposed: the tray's purchase threshold
    expect(PILL_QUIET_FROM_DAYS).toBe(14);
    expect(pillLicense(trial(PILL_SHOW_DAYS + 1)).show).toBe(true);
    expect(pillLicense(trial(PILL_SHOW_DAYS + 1)).tone).toBe("trial-quiet");
    expect(pillLicense(trial(PILL_SHOW_DAYS)).tone).toBe("trial");
    expect(pillLicense(trial(PILL_QUIET_FROM_DAYS)).tone).toBe("trial-quiet");
    expect(pillLicense(trial(PILL_QUIET_FROM_DAYS + 1)).show).toBe(false);
  });

  it("uses the backend's days_left (rollback floor included), never the expiry clock", () => {
    // trial_expires_at_ms is the same far-future instant for every row; only
    // days_left differs, and only days_left moves the numeral.
    const a = pillLicense(trial(10, { trial_expires_at_ms: 1, expires_at_ms: 1 } as Partial<LicenseStatus>));
    expect(a.value).toBe("10d");
  });

  // Y2-E — the urgency threshold is now a named constant shared with the menu
  // bar, so the boundary is asserted against the NAME as well as the day. The
  // cross-language half of this lives in `src-tauri/tests/tray_license.rs`.
  it("turns urgent exactly one threshold-day out, by name", () => {
    expect(PILL_URGENT_DAYS).toBe(1);
    expect(pillLicense(trial(PILL_URGENT_DAYS)).tone).toBe("trial");
    expect(pillLicense(trial(PILL_URGENT_DAYS - 1)).tone).toBe("urgent");
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

describe("Y2-D — where a press on the pill goes", () => {
  it("routes the unpaid tones to the purchase SHEET, never to a URL", () => {
    const open = { purchase_open: true };
    expect(pillLicenseRoute(pillLicense(trial(0, open)))).toBe("reveal_purchase"); // urgent
    expect(pillLicenseRoute(pillLicense(required("trial_expired", open)))).toBe("reveal_purchase"); // ended
    expect(pillLicenseRoute(pillLicense(trial(5, open)))).toBe("reveal_purchase"); // counting down
    expect(pillLicenseRoute(pillLicense(trial(10, open)))).toBe("reveal_purchase"); // quiet
    expect(REVEAL_PURCHASE_COMMAND).toBe("reveal_purchase_prompt");
  });

  it("is inert for a licensed install — no route, so no mark and no dead click region", () => {
    const got = pillLicense(licensed());
    expect(got.show).toBe(false);
    expect(pillLicenseRoute(got)).toBeNull();
    expect(offersPurchase(got)).toBe(false);
  });

  it("is inert before any license payload arrives, and for a silent trial", () => {
    expect(pillLicenseRoute(null)).toBeNull();
    expect(pillLicenseRoute(undefined)).toBeNull();
    expect(pillLicenseRoute(pillLicense(trial(PILL_QUIET_FROM_DAYS + 1)))).toBeNull();
  });

  it("never sends a stored-key holder to a checkout (Y2-F)", () => {
    const got = pillLicense(required("trial_expired", { has_stored_license: true }));
    expect(got.tone).toBe("problem");
    expect(pillLicenseRoute(got)).toBe("license_tab");
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

  it("a lapsed trial with NO stored key is untouched — still `ended`, still sells once checkout is open", () => {
    const p = pillLicense(required("trial_expired", { purchase_open: true }));
    expect(p.tone).toBe("ended");
    expect(p.action).toBe("purchase");
    expect(offersPurchase(p)).toBe(true);
  });
});

/**
 * yap24-PILL1 — the Buy chip is truthful. Checkout is CLOSED until launch day
 * (Rust `PURCHASE_OPEN = false`), so while the backend says so, no tone the pill
 * can produce carries a purchase action, and the copy promises nothing.
 */
describe("yap24-PILL1 — no purchase action while checkout is closed", () => {
  it("every day 14..0 counts down with action none", () => {
    for (let days = 14; days >= 0; days--) {
      const p = pillLicense(trial(days));
      expect(p.show).toBe(true);
      expect(p.action).toBe("none");
      expect(offersPurchase(p)).toBe(false);
      expect(pillLicenseRoute(p)).toBeNull();
    }
  });

  it("an ended trial routes to the License panel, never to a Buy that dead-ends", () => {
    const p = pillLicense(required());
    expect(p.action).toBe("license");
    expect(pillLicenseRoute(p)).toBe("license_tab");
  });

  it("an absent purchase_open (older payload) reads as closed", () => {
    const p = pillLicense(trial(5, { purchase_open: undefined } as unknown as Partial<LicenseStatus>));
    expect(p.action).toBe("none");
  });

  it("problem and licensed are unchanged by the switch", () => {
    for (const purchase_open of [true, false]) {
      const prob = pillLicense(trial(5, { has_stored_license: true, purchase_open }));
      expect(prob.tone).toBe("problem");
      expect(prob.action).toBe("license");
      expect(pillLicense(licensed({ purchase_open })).show).toBe(false);
    }
  });
});

/** yap24-PILL1 — the accessible title, one per tone, written down. */
describe("yap24-PILL1 — accessible title per tone", () => {
  it("matches the snapshot", () => {
    const titles = {
      "trial-quiet": pillLicense(trial(10)).title,
      "trial-quiet@14": pillLicense(trial(14)).title,
      trial: pillLicense(trial(5)).title,
      "trial@1": pillLicense(trial(1)).title,
      urgent: pillLicense(trial(0)).title,
      ended: pillLicense(required()).title,
      problem: pillLicense(trial(5, { has_stored_license: true, license_problem: "revoked", license_problem_message: "This license key was revoked." })).title,
    };
    expect(titles).toMatchInlineSnapshot(`
      {
        "ended": "Dictation is paused",
        "problem": "This license key was revoked.",
        "trial": "Free trial — 5 days left",
        "trial-quiet": "Free trial — 10 days left",
        "trial-quiet@14": "Free trial — 14 days left",
        "trial@1": "Free trial — 1 day left",
        "urgent": "Trial ends today",
      }
    `);
    expect(titles.urgent).toBe(TRIAL_ENDS_TODAY);
    expect(titles["trial-quiet"]).toBe(freeTrialTitle(10));
  });

  it("never carries a price or a purchase verb", () => {
    for (let days = 14; days >= 0; days--) {
      for (const purchase_open of [true, false]) {
        const t = pillLicense(trial(days, { purchase_open })).title;
        expect(t).not.toMatch(/\$|buy|purchase|upgrade/i);
      }
    }
  });
});

/** Y2-B (8d85cdf) — the geometry policy, carried and re-based onto 14 days. */
describe("pillLicense — value_is_never_wider_than_the_side_dock", () => {
  it("value_is_never_wider_than_the_side_dock, for every day 0..14", () => {
    for (let days = 0; days <= 14; days++) {
      const { value } = pillLicense(trial(days));
      expect(valueFitsSideDock(value)).toBe(true);
      expect(value === null || value.length <= PILL_VALUE_MAX_CHARS).toBe(true);
    }
  });

  it("holds past the ends of the fortnight too — a clock rollback is not an excuse", () => {
    for (const days of [-30, -3, -1, 15, 60, 3650]) {
      expect(valueFitsSideDock(pillLicense(trial(days)).value)).toBe(true);
    }
  });

  it("rejects anything that would not fit, so the check is not vacuous", () => {
    expect(valueFitsSideDock("14 days")).toBe(false);
    expect(valueFitsSideDock("14d")).toBe(true); // exactly the budget
    expect(valueFitsSideDock("100d")).toBe(false);
    expect(PILL_VALUE_MAX_CHARS).toBe(3);
  });
});

describe("splitValue — the numeral and its unit are set in different faces", () => {
  it("splits every value this policy can produce", () => {
    for (let days = 0; days <= PILL_QUIET_FROM_DAYS; days++) {
      const { value } = pillLicense(trial(days));
      const split = splitValue(value);
      expect(split).not.toBe(null);
      expect(split!.numeral).toMatch(/^\d+$/);
      expect(split!.unit).toBe("d");
      expect(split!.numeral + split!.unit).toBe(value);
    }
  });

  it("is null when there is no numeral, which is the cue to draw the glyph", () => {
    expect(splitValue(pillLicense(required()).value)).toBe(null);
    expect(splitValue(null)).toBe(null);
    expect(splitValue("")).toBe(null);
  });

  it("does not lose a value it cannot parse", () => {
    expect(splitValue("soon")).toEqual({ numeral: "soon", unit: "" });
  });
});
