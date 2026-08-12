/**
 * YV96 — the notice's wiring, tested across the boundary it actually depends on.
 *
 * `consent.test.ts` proves the copy and the open/close rules. None of that is
 * worth anything if the sheet is never asked to open: the trigger crosses a
 * process boundary held together by a string (`"meeting"`) and a serialized
 * field name (`recording`), and neither is checked by `tsc` or by `cargo`. Get
 * either wrong and the app builds, every other test passes, and the one-time
 * legal notice — the entire item — simply never renders.
 *
 * So this file asserts the two things that can silently break:
 *
 *   1. the listener subscribes to exactly the name Rust emits, and
 *   2. a REAL serialized `MeetingStatus` payload opens the sheet.
 *
 * `@tauri-apps/api` is mocked rather than injected so what is under test is the
 * production import path, not a seam invented for the test.
 * `src-tauri/tests/meeting_event_contract.rs` holds up the Rust end.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const listen = vi.fn();
const invoke = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { MEETING_EVENT } = await import("./consent");
const { watchMeetingConsent } = await import("./consentWatch");
import type { MeetingConsent } from "./consent";

const PENDING: MeetingConsent = {
  shouldShow: true,
  acknowledgedAt: null,
  blocksRecording: false,
};
const SEEN: MeetingConsent = {
  shouldShow: false,
  acknowledgedAt: "2026-08-11T18:04:00Z",
  blocksRecording: false,
};

/**
 * `MeetingStatus` as it arrives on the wire — the exact shape YV95's
 * `meeting_status_sink` emits once a second, serialized through
 * `#[serde(rename_all = "camelCase")]`.
 *
 * Written out in full rather than trimmed to `{ recording }`, because the point
 * of this fixture is to fail if the field the notice reads is ever renamed on
 * the Rust side: a partial fixture written from the listener's point of view
 * would agree with the listener no matter what the backend sends.
 */
function meetingStatus(recording: boolean) {
  return {
    recording,
    id: recording ? "mtg_01J" : null,
    title: recording ? "Untitled meeting" : null,
    elapsedSeconds: recording ? 3 : 0,
    elapsedLabel: recording ? "00:00:03" : "00:00:00",
    captureAvailable: true,
    unavailableReason: null,
  };
}

/** Drive the watcher and hand back the payload sink the listener registered. */
async function mount(consent: MeetingConsent | null) {
  const onOpen = vi.fn();
  const onConsent = vi.fn();
  const stop = watchMeetingConsent({
    currentConsent: () => consent,
    onConsent,
    onOpen,
  });
  await vi.waitFor(() => expect(listen).toHaveBeenCalled());
  const [name, handler] = listen.mock.calls[0];
  return {
    name: name as string,
    fire: (recording: boolean) =>
      (handler as (e: { payload: unknown }) => void)({
        payload: meetingStatus(recording),
      }),
    onOpen,
    onConsent,
    stop,
  };
}

describe("the notice is wired to something that can actually raise it", () => {
  beforeEach(() => {
    listen.mockReset();
    invoke.mockReset();
    listen.mockResolvedValue(() => {});
    invoke.mockResolvedValue(PENDING);
  });

  it("subscribes to the literal event name the backend emits", async () => {
    // If this ever disagrees with `meetings::MEETING_EVENT`, the sheet is dead.
    // The Rust half of the pair is `meeting_event_contract.rs`, which reads the
    // constant back out of `consent.ts` and asserts the same string.
    expect(MEETING_EVENT).toBe("meeting");
    const { name } = await mount(PENDING);
    expect(name).toBe("meeting");
  });

  it("opens the sheet on a real serialized MeetingStatus", async () => {
    const { fire, onOpen } = await mount(PENDING);
    fire(true);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("reads the recording flag by its serialized camelCase name", async () => {
    // The failure this guards: `recording` gets renamed on the Rust struct, the
    // payload arrives with the field missing, and `undefined` quietly means
    // "not recording" forever.
    const { onOpen } = await mount(PENDING);
    const handler = listen.mock.calls[0][1] as (e: { payload: unknown }) => void;
    handler({ payload: { ...meetingStatus(true), recording: undefined } });
    expect(onOpen).not.toHaveBeenCalled();
    handler({ payload: meetingStatus(true) });
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("stays shut when a meeting stops, and when the ack is already set", async () => {
    const idle = await mount(PENDING);
    idle.fire(false);
    expect(idle.onOpen).not.toHaveBeenCalled();

    listen.mockReset();
    listen.mockResolvedValue(() => {});
    const seen = await mount(SEEN);
    seen.fire(true);
    expect(seen.onOpen).not.toHaveBeenCalled();
  });

  it("asks the backend for the stored acknowledgement on mount", async () => {
    const { onConsent } = await mount(null);
    expect(invoke).toHaveBeenCalledWith("meeting_consent");
    await vi.waitFor(() => expect(onConsent).toHaveBeenCalledWith(PENDING));
  });

  it("survives a backend that cannot answer", async () => {
    invoke.mockRejectedValue("no such command");
    const { fire, onOpen, onConsent } = await mount(null);
    fire(true);
    expect(onConsent).not.toHaveBeenCalled();
    // Unknown state means shut: guessing "show it" is how a one-time notice
    // shows twice.
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("does not leak a listener that lands after unmount", async () => {
    // StrictMode double-mounts: the synchronous cleanup runs before listen()
    // resolves, so the late handle has to unsubscribe itself.
    const unsub = vi.fn();
    let resolve: (u: () => void) => void = () => {};
    listen.mockReturnValue(
      new Promise<() => void>((r) => {
        resolve = r;
      }),
    );
    const stop = watchMeetingConsent({
      currentConsent: () => PENDING,
      onConsent: () => {},
      onOpen: () => {},
    });
    stop();
    resolve(unsub);
    await vi.waitFor(() => expect(unsub).toHaveBeenCalledTimes(1));
  });

  it("unsubscribes on a normal unmount", async () => {
    const unsub = vi.fn();
    listen.mockResolvedValue(unsub);
    const { stop } = await mount(PENDING);
    stop();
    expect(unsub).toHaveBeenCalledTimes(1);
  });
});
