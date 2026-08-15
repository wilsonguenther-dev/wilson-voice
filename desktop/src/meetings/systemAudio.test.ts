/**
 * YV102 — the setup step's copy and state rules.
 *
 * The load-bearing assertion in this file is a NEGATIVE one: no reachable state
 * turns silence into "denied". That is the whole discriminator (finding OS-4) —
 * a denied tap and a healthy tap suffering CoreAudio's all-zero bug produce the
 * identical symptom, so only the Rust-side "did this tap ever deliver a non-zero
 * sample" verdict is allowed to reach the denied branch. If a future edit adds a
 * `if (!heardAnything) return denied` shortcut here, this file goes red.
 */

import { describe, expect, it } from "vitest";
import {
  SYSTEM_AUDIO_PANE,
  SYSTEM_AUDIO_SETUP,
  setupState,
  type SetupVerdict,
  type SystemAudioSetup,
} from "./systemAudio";

const REQUIREMENT = "System audio capture requires macOS 14.4 or later";

function setup(
  verdict: SetupVerdict,
  overrides: Partial<SystemAudioSetup> = {},
): SystemAudioSetup {
  return {
    hasRun: verdict !== "not_run",
    lastRunAt: "2026-08-14T17:04:00Z",
    verdict,
    blocksRecording: false,
    ...overrides,
  };
}

const ALL_VERDICTS: SetupVerdict[] = [
  "not_run",
  "ran",
  "granted",
  "looks_denied",
  "unavailable",
  "failed",
];

describe("the honest denied state", () => {
  it("is reached only by the looks_denied verdict, never by silence", () => {
    const denied = ALL_VERDICTS.filter(
      (v) => setupState(setup(v), true, REQUIREMENT).tone === "bad" &&
        setupState(setup(v), true, REQUIREMENT).label.includes("not received any system audio"),
    );
    expect(denied).toEqual(["looks_denied"]);
  });

  it("says macOS has not granted it, never that the user denied it", () => {
    const state = setupState(setup("looks_denied"), true, REQUIREMENT);
    expect(state.label).toContain("has not granted");
    expect(state.label.toLowerCase()).not.toContain("you denied");
    expect(state.label.toLowerCase()).not.toContain("you refused");
  });

  it("offers the deep link, because after a denial it is the only recovery", () => {
    expect(setupState(setup("looks_denied"), true, REQUIREMENT).showDeepLink).toBe(
      true,
    );
    expect(setupState(setup("looks_denied"), true, REQUIREMENT).label).toContain(
      "System Settings",
    );
  });

  it("never claims to be recording while it has heard nothing", () => {
    for (const verdict of ALL_VERDICTS) {
      if (verdict === "granted") continue;
      const label = setupState(setup(verdict), true, REQUIREMENT).label;
      expect(label).not.toMatch(/is being captured|recording now/i);
    }
  });
});

describe("the granted state", () => {
  it("is the only green one, and it is earned by real audio", () => {
    const green = ALL_VERDICTS.filter(
      (v) => setupState(setup(v), true, REQUIREMENT).tone === "ok",
    );
    expect(green).toEqual(["granted"]);
  });

  it("names the purple indicator — a claim macOS makes, not Yap", () => {
    expect(setupState(setup("granted"), true, REQUIREMENT).label).toContain(
      "purple",
    );
    expect(setupState(setup("granted"), true, REQUIREMENT).showDeepLink).toBe(
      false,
    );
  });
});

describe("the ran state", () => {
  it("admits it does not know, rather than guessing either way", () => {
    const state = setupState(setup("ran"), true, REQUIREMENT);
    expect(state.tone).toBe("idle");
    expect(state.label).toContain("expected");
    expect(state.label).not.toContain("not granted");
    // The link is still offered: it costs nothing and it is the recovery if the
    // answer WAS "don't allow".
    expect(state.showDeepLink).toBe(true);
  });
});

describe("the macOS 14.4 gate (YV101) outranks everything", () => {
  it("refuses to offer a permission that does not exist on this Mac", () => {
    for (const verdict of ALL_VERDICTS) {
      const state = setupState(setup(verdict), false, REQUIREMENT);
      expect(state.canRun).toBe(false);
      expect(state.label).toContain(REQUIREMENT);
      expect(state.showDeepLink).toBe(false);
    }
  });

  it("says mic-only recording still works — 22-A's floor is not regressed", () => {
    expect(setupState(null, false, REQUIREMENT).label).toContain(
      "still record your microphone",
    );
    expect(setupState(setup("unavailable"), true, REQUIREMENT).label).toContain(
      "still record your microphone",
    );
  });
});

describe("the first-run state", () => {
  it("is a call to action, on a null backend answer and on a fresh install", () => {
    for (const state of [
      setupState(null, true, REQUIREMENT),
      setupState(setup("not_run", { hasRun: false }), true, REQUIREMENT),
    ]) {
      expect(state.canRun).toBe(true);
      expect(state.tone).toBe("idle");
      expect(state.actionLabel).toBe(SYSTEM_AUDIO_SETUP.action);
      expect(state.showDeepLink).toBe(false);
      expect(state.label).toContain("mid-call");
    }
  });
});

describe("the copy", () => {
  it("explains the alert BEFORE it can fire — the whole point of the step", () => {
    const prose = SYSTEM_AUDIO_SETUP.paragraphs.join(" ");
    expect(prose).toContain("macOS");
    expect(prose).toContain("purple");
    // The mechanism is stated plainly rather than hidden: a fifth of a second,
    // discarded.
    expect(prose).toMatch(/fifth of a second|200 ms/);
    expect(prose).toContain("throws away");
  });

  it("warns that macOS asks exactly once", () => {
    expect(SYSTEM_AUDIO_SETUP.fine).toContain("not ask again");
  });

  it("does not overclaim: no promise about what the answer will be", () => {
    const all = [
      ...SYSTEM_AUDIO_SETUP.paragraphs,
      SYSTEM_AUDIO_SETUP.fine,
    ].join(" ");
    expect(all).not.toMatch(/guarantee|always works|never fails/i);
  });
});

describe("the deep-link pane", () => {
  it("is the verified anchor's pane name, not the plan's guess", () => {
    // The Rust side maps this to `…?Privacy_AudioCapture`, which was verified by
    // enumerating the Settings extension's anchors and reading the resulting
    // window title back ("Screen & System Audio Recording"). The plan's
    // candidate, `Privacy_SystemAudio`, does not exist in that table at all.
    expect(SYSTEM_AUDIO_PANE).toBe("SystemAudio");
    expect(SYSTEM_AUDIO_PANE).not.toContain("Privacy_");
  });
});

describe("the setup step is never a gate", () => {
  it("carries blocksRecording false in every state the backend can send", () => {
    for (const verdict of ALL_VERDICTS) {
      expect(setup(verdict).blocksRecording).toBe(false);
    }
  });
});
