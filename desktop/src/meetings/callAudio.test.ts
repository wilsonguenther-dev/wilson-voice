/**
 * yap-1106-call-audio-truth — the chip over the four tap states, and the
 * negative that matters: no state turns silence into "denied".
 */
import { describe, expect, it } from "vitest";
import {
  CALL_AUDIO_QUIET_AFTER_S,
  CALL_AUDIO_UNHEARD_ACTION_AFTER_S,
  callAudioChip,
  chipFor,
  recordedTracks,
  TRACKS_FALLBACK_WHY,
} from "./callAudio";
import { IDLE_MEETING, type CallAudioLive, type MeetingStatus } from "../pill/meeting";

function live(overrides: Partial<CallAudioLive> = {}): CallAudioLive {
  return {
    planned: true,
    attached: true,
    everNonzero: false,
    silentSeconds: 0,
    lost: false,
    grantUnknown: false,
    ...overrides,
  };
}

function recording(callAudio: CallAudioLive | null): MeetingStatus {
  return { ...IDLE_MEETING, recording: true, elapsedSeconds: 10, callAudio };
}

describe("callAudioChip — the four tap states", () => {
  it("1. no tap → off; an in-person room says so without apology", () => {
    const c = chipFor(live({ attached: false, planned: false }));
    expect(c.state).toBe("off");
    expect(c.label).toBe("Call audio: off");
    expect(c.action).toBeNull();
    expect(c.detail).toMatch(/In-person/);
  });

  it("1b. no tap on a call, grant unknown → off with the one action", () => {
    const c = chipFor(live({ attached: false, planned: true, grantUnknown: true }));
    expect(c.state).toBe("off");
    expect(c.action).toBe("turn-on");
    // With proof of the grant there is nothing to turn on.
    expect(chipFor(live({ attached: false, planned: true, grantUnknown: false })).action).toBeNull();
  });

  it("2. tap open, never delivered → silent, never denied", () => {
    const c = chipFor(live({ everNonzero: false, silentSeconds: 5 }));
    expect(c.state).toBe("silent");
    expect(c.detail).not.toMatch(/deni|permission/i);
    expect(c.action).toBeNull();
  });

  it("2b. never delivered past the grace with no proof of the grant → the action", () => {
    const early = chipFor(live({ grantUnknown: true, silentSeconds: CALL_AUDIO_UNHEARD_ACTION_AFTER_S - 1 }));
    expect(early.action).toBeNull();
    const late = chipFor(live({ grantUnknown: true, silentSeconds: CALL_AUDIO_UNHEARD_ACTION_AFTER_S }));
    expect(late.state).toBe("silent");
    expect(late.action).toBe("turn-on");
    // A granted Mac is never asked to turn it on — silence is the other side.
    expect(chipFor(live({ grantUnknown: false, silentSeconds: 600 })).action).toBeNull();
  });

  it("3. tap delivering → on", () => {
    const c = chipFor(live({ everNonzero: true, silentSeconds: 2 }));
    expect(c.state).toBe("on");
    expect(c.label).toBe("Call audio: on");
    expect(c.action).toBeNull();
  });

  it("4. delivered then quiet → silent; lost → off, mic still recording", () => {
    const quiet = chipFor(live({ everNonzero: true, silentSeconds: CALL_AUDIO_QUIET_AFTER_S }));
    expect(quiet.state).toBe("silent");
    expect(quiet.detail).toMatch(/quiet for/);
    expect(quiet.action).toBeNull();
    const lost = chipFor(live({ everNonzero: true, lost: true }));
    expect(lost.state).toBe("off");
    expect(lost.detail).toMatch(/microphone is still recording/);
  });

  it("shows nothing when idle, and nothing when the backend has no field", () => {
    expect(callAudioChip(null)).toBeNull();
    expect(callAudioChip(IDLE_MEETING)).toBeNull();
    expect(callAudioChip(recording(null))).toBeNull();
    expect(callAudioChip(recording(live({ everNonzero: true })))?.state).toBe("on");
  });
});

describe("recordedTracks — the detail header", () => {
  const base = { state: "complete", kind: "virtual", error: null as string | null };
  it("a second wav is both tracks", () => {
    expect(recordedTracks({ ...base, sysWavPath: "/x/sys.wav" })).toEqual({
      label: "Your mic + call audio",
      detail: null,
      twoTrack: true,
    });
  });
  it("mic only carries the engine's reason, or the fallback", () => {
    const why = "Couldn't confirm system audio: none was captured in this meeting.";
    expect(recordedTracks({ ...base, error: why, sysWavPath: null })).toMatchObject({
      label: "Mic only: call audio was not captured",
      detail: why,
      twoTrack: false,
    });
    expect(recordedTracks({ ...base, sysWavPath: null }).detail).toBe(TRACKS_FALLBACK_WHY);
  });
  it("an in-person meeting is mic only by design", () => {
    expect(recordedTracks({ ...base, kind: "in_person" }).label).toBe("Mic only: in-person meeting");
  });
  it("a meeting still recording does not claim mic only", () => {
    expect(recordedTracks({ ...base, state: "recording" }).label).toMatch(/Recording now/);
  });
});
