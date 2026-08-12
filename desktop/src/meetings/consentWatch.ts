/**
 * YV96 — the wiring that raises the one-time capture notice.
 *
 * This lives beside `consent.ts` rather than inside `App.tsx` for one reason:
 * the notice is separated from the thing that triggers it by a process boundary
 * (Rust emits, the webview listens) and by a string that no compiler checks. A
 * subscription written inline in a 4,000-line component is a subscription no
 * test can reach, and the failure mode is silent — the app builds, every unit
 * test of the copy and the open/close rules passes, and the sheet simply never
 * appears. That is the entire deliverable disappearing behind a green build.
 *
 * Pulled out here it is one small function with injected callbacks, so
 * `consentWatch.test.ts` can mock `@tauri-apps/api/event`, fire a real
 * serialized `MeetingStatus` payload, and assert the sheet opens — which pins
 * BOTH halves of the contract: the event name and the payload field.
 *
 * `consent.ts` stays free of Tauri imports on purpose: its copy and its rules
 * are pure, and the preview page renders them outside the app entirely.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  MEETING_EVENT,
  shouldOpenNotice,
  type MeetingConsent,
  type RecordingSignal,
} from "./consent";

/** What the caller has to supply. Deliberately three callbacks, no state. */
export interface ConsentWatch {
  /**
   * The acknowledgement as it stands *right now*. A getter, not a value: the
   * subscription is made ONCE (re-subscribing on every consent change would
   * drop ticks), so it has to read through to current state at fire time.
   */
  currentConsent: () => MeetingConsent | null;
  /** The backend's answer to "has this ever been shown?", on mount. */
  onConsent: (consent: MeetingConsent) => void;
  /** Raise the sheet. Called at most once — `shouldOpenNotice` is idempotent. */
  onOpen: () => void;
}

/**
 * Read the stored acknowledgement, then watch for a meeting actually starting.
 *
 * The trigger is the backend's `meeting` event rather than any particular
 * button because 22-A has FOUR entry points (tray item, ⌃⌘M, the pill, the
 * Meetings CTA) and a notice wired to one of them is a notice three ways of
 * recording never show.
 *
 * Returns its own teardown. A synchronous unmount can beat the `listen()`
 * promise (StrictMode double-mounts), so a listener that lands after teardown
 * unsubscribes itself instead of leaking a native handle.
 */
export function watchMeetingConsent(watch: ConsentWatch): () => void {
  let dead = false;
  const unsubs: Array<() => void> = [];

  invoke<MeetingConsent>("meeting_consent")
    .then((c) => {
      if (!dead) watch.onConsent(c);
    })
    .catch(() => {
      /* the notice stays shut on an unknown answer — see shouldOpenNotice */
    });

  listen<RecordingSignal>(MEETING_EVENT, (e) => {
    if (shouldOpenNotice(watch.currentConsent(), e.payload)) watch.onOpen();
  })
    .then((u) => (dead ? u() : unsubs.push(u)))
    .catch(() => {
      /* nothing to unsubscribe */
    });

  return () => {
    dead = true;
    unsubs.forEach((u) => u());
  };
}
