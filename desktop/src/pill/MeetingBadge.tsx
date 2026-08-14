/**
 * MeetingBadge — the pill's persistent recording state (YV95, finding #6 item 3).
 *
 * A red dot, the elapsed time, and a stop control. Rendered by BOTH pill styles
 * (classic and yappy) from the same component, because "is Yap recording this
 * meeting" must not look like two different features depending on a cosmetic
 * setting.
 *
 * Energy, per OS-12: the dot's pulse is a CSS `@keyframes` on opacity — a
 * compositor animation that never wakes the JS thread — and the clock text is
 * whatever the backend's 1 Hz emit last said. Nothing here schedules a timer.
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { elapsedLabel, IDLE_MEETING, type MeetingStatus } from "./meeting";

/**
 * Subscribe to the backend's meeting status: the current value on mount (so a
 * pill that appears mid-meeting is correct immediately, not a second later) and
 * every `meeting` event after that.
 */
export function useMeetingStatus(): MeetingStatus {
  const [status, setStatus] = useState<MeetingStatus>(IDLE_MEETING);
  useEffect(() => {
    // A synchronous cleanup can run before listen() resolves (StrictMode
    // double-mount); the `dead` flag unsubscribes a late listener so no native
    // listener leaks. Same pattern as every other subscription in this app.
    let dead = false;
    const unsubs: Array<() => void> = [];
    invoke<MeetingStatus>("meeting_status").then((s) => { if (!dead) setStatus(s); }).catch(() => {});
    listen<MeetingStatus>("meeting", (e) => setStatus(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return status;
}

export default function MeetingBadge({ status }: { status: MeetingStatus }) {
  if (!status.recording) return null;
  return (
    <div className="meeting-badge" role="status" aria-live="off">
      <span className="rec-dot" aria-hidden />
      <span className="rec-time">{elapsedLabel(status)}</span>
      <button
        type="button"
        className="rec-stop"
        aria-label={`Stop meeting — recording for ${elapsedLabel(status)}`}
        onClick={(e) => {
          e.stopPropagation();
          invoke("stop_meeting").catch(() => {});
        }}
      >
        <svg viewBox="0 0 24 24" aria-hidden>
          <rect x="7" y="7" width="10" height="10" rx="1.5" />
        </svg>
      </button>
    </div>
  );
}
