import { describe, expect, it } from "vitest";
import trialDaysFixture from "../../src-tauri/tests/fixtures/trial_days.json";
import {
  activationError,
  chipFor,
  daysLeft,
  shouldWarnTrial,
  statusCopy,
  storedKeyProblem,
  trialCountdown,
  trialWarningText,
  TRIAL_WARN_DAYS,
  type LicenseStatus,
} from "./status";

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
    revoked_count: 2,
    ...over,
  } as LicenseStatus;
}

function ended(over: Partial<LicenseStatus> = {}): LicenseStatus {
  return {
    state: "license_required",
    reason: "trial_expired",
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

describe("the trial chip", () => {
  it("counts the fortnight down and stays calm until the last three days", () => {
    expect(chipFor(trial(14))).toMatchObject({ tone: "trial", value: "14d" });
    expect(chipFor(trial(TRIAL_WARN_DAYS + 1)).tone).toBe("trial");
  });

  it("turns urgent inside the warning window, and never goes negative", () => {
    expect(chipFor(trial(TRIAL_WARN_DAYS)).tone).toBe("urgent");
    expect(chipFor(trial(0))).toMatchObject({ tone: "urgent", value: "0d" });
    expect(daysLeft(trial(-4))).toBe(0);
  });

  it("says Licensed with no countdown once a key is in", () => {
    expect(chipFor(licensed())).toMatchObject({
      tone: "licensed",
      label: "Licensed",
      value: null,
    });
  });

  it("says the trial ended rather than showing an error", () => {
    const chip = chipFor(ended());
    expect(chip.tone).toBe("ended");
    expect(chip.label).toBe("Trial ended");
    expect(chip.title).toMatch(/already written stays/i);
  });
});

describe("the ONE trial warning", () => {
  it("does not fire while there is more than three days left", () => {
    expect(shouldWarnTrial(trial(14), null)).toBe(false);
    expect(shouldWarnTrial(trial(TRIAL_WARN_DAYS + 1), null)).toBe(false);
  });

  it("fires once at three days and never again for the same trial", () => {
    const s = trial(TRIAL_WARN_DAYS);
    expect(shouldWarnTrial(s, null)).toBe(true);
    // …the app records the expiry it warned for, and every later launch,
    // every later day of the same trial, stays silent.
    expect(shouldWarnTrial(s, EXPIRES)).toBe(false);
    expect(shouldWarnTrial(trial(2), EXPIRES)).toBe(false);
    expect(shouldWarnTrial(trial(0), EXPIRES)).toBe(false);
  });

  it("is allowed again for a genuinely different trial", () => {
    expect(
      shouldWarnTrial(
        trial(1, { expires_at_ms: EXPIRES + 99, trial_expires_at_ms: EXPIRES + 99 }),
        EXPIRES,
      ),
    ).toBe(true);
  });

  it("never fires for a licensed or ended install", () => {
    expect(shouldWarnTrial(licensed(), null)).toBe(false);
    expect(shouldWarnTrial(ended(), null)).toBe(false);
  });

  it("carries a sentence that promises the words are kept", () => {
    expect(trialWarningText(3)).toContain("3 days left");
    expect(trialWarningText(0)).toMatch(/last day/i);
    for (const d of [3, 1, 0]) {
      expect(trialWarningText(d)).toMatch(/stays yours/i);
    }
  });

  it("pluralises honestly", () => {
    expect(trialCountdown(2)).toBe("2 days left");
    expect(trialCountdown(1)).toBe("1 day left");
    expect(trialCountdown(0)).toBe("Last day");
  });
});

describe("the status card copy", () => {
  it("never calls the end of the trial an error", () => {
    const copy = statusCopy(ended());
    expect(copy.headline).toBe("Dictation is paused");
    expect(copy.body).toMatch(/still here and still exportable/i);
    expect(`${copy.headline} ${copy.body}`).not.toMatch(/error|invalid|failed/i);
  });

  it("names the seat count from the license itself", () => {
    expect(statusCopy(licensed({ seats: 5 })).body).toContain("5 Macs");
  });

  it("mentions the price only when the trial is nearly up", () => {
    expect(statusCopy(trial(9)).body).not.toContain("$29");
    expect(statusCopy(trial(2)).body).toContain("$29");
  });
});

describe("a stored key that granted nothing", () => {
  it("surfaces the backend's own sentence", () => {
    const revoked = ended({
      reason: "revoked",
      has_stored_license: true,
      license_problem: "revoked",
      license_problem_message: "This license was refunded or charged back.",
    });
    expect(storedKeyProblem(revoked)).toBe(
      "This license was refunded or charged back.",
    );
  });

  it("says nothing when there is no stored key, or the stored key works", () => {
    expect(storedKeyProblem(trial(9))).toBeNull();
    expect(storedKeyProblem(licensed())).toBeNull();
  });
});

describe("activation errors", () => {
  it("reads the structured rejection rather than [object Object]", () => {
    expect(
      activationError({ code: "bad_signature", message: "That key did not check out." }),
    ).toBe("That key did not check out.");
  });

  it("falls back to something actionable for a shapeless failure", () => {
    expect(activationError(null)).toMatch(/paste the whole line/i);
    expect(activationError("")).toMatch(/paste the whole line/i);
  });
});

// ─── The cross-language pin ──────────────────────────────────────────
//
// SEC-B. The countdown numeral this file formats and the gate that closes in
// Rust are two implementations of one fortnight, and a UI that says "3 days
// left" on a day the backend already refuses a dictation is worse than either
// bug alone. So neither side owns a hand-written ladder: Rust's
// `evaluate_trial()` generates `src-tauri/tests/fixtures/trial_days.json`
// (desktop/src-tauri/tests/trial_state_machine.rs ::
// each_day_reports_one_fewer_and_zero_is_the_last_day) and these assertions
// replay it through the TypeScript the interface actually calls. Change the
// trial arithmetic on either side and one of the two tests goes red.

type TrialDayRow = {
  elapsed_days: number;
  elapsed_ms: number;
  days_left: number;
  expired: boolean;
  label: string;
};

type TrialDaysFixture = {
  trial_days: number;
  day_ms: number;
  started_at_ms: number;
  expires_at_ms: number;
  rows: TrialDayRow[];
};

// The fixture is imported, not re-parsed: one file, generated by the Rust test,
// read by the bundler the app itself is built with.
const trialDays = trialDaysFixture as TrialDaysFixture;

describe("trial_days.json — Rust and TypeScript count the same fortnight", () => {
  it("covers all fifteen days, 14 down to 0", () => {
    expect(trialDays.rows).toHaveLength(trialDays.trial_days + 1);
    expect(trialDays.rows.map((r) => r.days_left)).toEqual([
      14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    ]);
    expect(trialDays.expires_at_ms - trialDays.started_at_ms).toBe(
      trialDays.trial_days * trialDays.day_ms,
    );
  });

  /** The status the backend would serialise on that day of the fixture. */
  function fixtureStatus(row: TrialDayRow): LicenseStatus {
    const shared = {
      license_problem: null,
      license_problem_message: null,
      has_stored_license: false,
      trial_days_left: row.days_left,
      trial_expires_at_ms: trialDays.expires_at_ms,
      revocation_checked_at_ms: null,
      revoked_count: 0,
    };
    return row.expired
      ? { state: "license_required", reason: "trial_expired", ...shared }
      : {
          state: "trial",
          days_left: row.days_left,
          expires_at_ms: trialDays.expires_at_ms,
          ...shared,
        };
  }

  for (const row of trialDays.rows) {
    it(`day ${row.elapsed_days}: daysLeft and trialCountdown match Rust`, () => {
      const status = fixtureStatus(row);
      expect(daysLeft(status)).toBe(row.days_left);
      expect(trialCountdown(daysLeft(status))).toBe(row.label);
      // The chip never prints a stale numeral on the day the gate closes.
      expect(chipFor(status).value).toBe(
        row.expired ? null : `${row.days_left}d`,
      );
    });
  }

  it("the day the gate closes is the day Rust says it closes", () => {
    const running = trialDays.rows.filter((r) => !r.expired);
    const lastRunning = running[running.length - 1];
    const firstExpired = trialDays.rows.find((r) => r.expired);
    expect(lastRunning.days_left).toBe(1);
    expect(firstExpired?.elapsed_days).toBe(trialDays.trial_days);
    expect(firstExpired?.label).toBe("Last day");
  });
});
