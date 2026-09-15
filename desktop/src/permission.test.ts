import { describe, expect, it } from "vitest";
import {
  MIC_PERMISSION_STATUSES,
  deriveMicStatus,
  micIsUsable,
  parseMicPermissionStatus,
  permissionAction,
  permissionCopy,
  type MicPermissionStatus,
} from "./permission";

describe("permissionCopy", () => {
  it("gives four DISTINCT bodies — one screen per status, no collapse", () => {
    const bodies = MIC_PERMISSION_STATUSES.map((s) => permissionCopy(s).body);
    expect(new Set(bodies).size).toBe(4);
  });

  it("obeys the copy rules: no exclamation marks, and Yap is named", () => {
    for (const s of MIC_PERMISSION_STATUSES) {
      const { title, body } = permissionCopy(s);
      expect(body).not.toMatch(/!/);
      expect(title).not.toMatch(/!/);
      expect(body).not.toMatch(/\bthe app\b/i);
      expect(body).toMatch(/\bYap\b/);
    }
  });

  it("says plainly, on denied, that macOS will not ask again and Settings is the way through", () => {
    const { body, tone } = permissionCopy("denied");
    expect(body).toMatch(/will not ask again/i);
    expect(body).toMatch(/System Settings/);
    expect(tone).toBe("blocked");
  });

  it("tells a restricted user that a profile decides it, without promising a fix", () => {
    const { body } = permissionCopy("restricted");
    expect(body).toMatch(/configuration profile/i);
    expect(permissionCopy("restricted").body).not.toBe(
      permissionCopy("denied").body,
    );
  });

  it("leaves authorized settled", () => {
    expect(permissionCopy("authorized").tone).toBe("settled");
  });
});

describe("permissionAction", () => {
  it("asks macOS only when nobody has been asked yet", () => {
    expect(permissionAction("not_determined")).toEqual({
      kind: "request",
      label: "Allow microphone access",
    });
  });

  it("offers System Settings on denied, with the pane the Rust side matches on", () => {
    const action = permissionAction("denied");
    expect(action.kind).toBe("settings");
    if (action.kind !== "settings") throw new Error("unreachable");
    expect(action.pane).toBe("Microphone");
    expect(action.label).toBe("Open System Settings");
  });

  it("offers NO settings action for restricted — the toggle is greyed out by MDM", () => {
    const action = permissionAction("restricted");
    expect(action.kind).not.toBe("settings");
    expect(action.kind).toBe("none");
  });

  it("shows no button at all once authorized", () => {
    expect(permissionAction("authorized")).toEqual({ kind: "none" });
  });

  it("pairs four statuses with four distinct copy+action pairs", () => {
    const pairs = MIC_PERMISSION_STATUSES.map((s) =>
      JSON.stringify([permissionCopy(s), permissionAction(s)]),
    );
    expect(new Set(pairs).size).toBe(4);
  });

  it("never hands the frontend a deep-link URL — the pane is a name, not a URL", () => {
    for (const s of MIC_PERMISSION_STATUSES) {
      expect(JSON.stringify(permissionAction(s))).not.toMatch(
        /x-apple\.systempreferences/,
      );
    }
  });
});

describe("deriveMicStatus", () => {
  it("does NOT show the denied screen to a first-run user who was never asked", () => {
    expect(deriveMicStatus({ microphone: false, asked: false })).toBe(
      "not_determined",
    );
  });

  it("calls it denied only after Yap actually asked and was refused", () => {
    expect(deriveMicStatus({ microphone: false, asked: true })).toBe("denied");
  });

  it("is authorized when the grant is in hand", () => {
    expect(deriveMicStatus({ microphone: true, asked: false })).toBe(
      "authorized",
    );
    expect(micIsUsable("authorized")).toBe(true);
    expect(micIsUsable("denied")).toBe(false);
    expect(micIsUsable("not_determined")).toBe(false);
    expect(micIsUsable("restricted")).toBe(false);
  });

  it("prefers a real backend status over the boolean derivation when one arrives", () => {
    expect(
      deriveMicStatus({
        microphoneStatus: "restricted",
        microphone: false,
        asked: true,
      }),
    ).toBe("restricted");
    expect(
      deriveMicStatus({
        microphoneStatus: "not_determined",
        microphone: false,
        asked: true,
      }),
    ).toBe("not_determined");
  });

  it("falls back rather than inventing a denial when the backend says something unknown", () => {
    expect(
      deriveMicStatus({
        microphoneStatus: "banana",
        microphone: false,
        asked: false,
      }),
    ).toBe("not_determined");
    expect(
      deriveMicStatus({
        microphoneStatus: "banana",
        microphone: true,
        asked: false,
      }),
    ).toBe("authorized");
  });
});

describe("parseMicPermissionStatus", () => {
  it("round-trips every real status", () => {
    for (const s of MIC_PERMISSION_STATUSES) {
      expect(parseMicPermissionStatus(s)).toBe(s);
    }
  });

  it("rejects junk without throwing", () => {
    const junk: unknown[] = [null, undefined, 0, "", "Denied", {}, []];
    for (const v of junk) {
      const out: MicPermissionStatus = parseMicPermissionStatus(v);
      expect(MIC_PERMISSION_STATUSES).toContain(out);
      expect(out).toBe("not_determined");
    }
  });
});
