// yap-fx-06 — the pill's card for a meeting and a dictation at the same time.
// See meetingDictation.ts for the three moments (cancelled / ask / stop-failed)
// and why each exists. Mounted by float-main.tsx beside the other cards; its
// wrap class is in CARD_CLASSES so the one-pill layout stacks it with them.
//
// Keyboard: the buttons are real buttons (Tab / Space / Return), Esc on the
// card dismisses it (the same as "Keep recording" / "Both" — the safe answer
// that changes nothing), and ⌃⌘M stops the meeting from anywhere.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { holdPillHitbox } from "./drag";
import { PILL_LAYOUT_EVENT } from "./state";
import { MEETING_EVENT, type MeetingStatus } from "./meeting";
import {
  HANDS_FREE_MEETING_ASK_EVENT,
  HANDS_FREE_MEETING_RESPOND,
  MEETING_STOP_FAILED_EVENT,
  PILL_QUIT_YAP,
  TAKE_CANCELLED_EVENT,
  noticeAfterMeetingStatus,
  noticeButtons,
  noticeFromAsk,
  noticeFromCancel,
  noticeFromStopFailure,
  noticeTtlMs,
  type MeetingNotice,
  type MeetingNoticeAction,
} from "./meetingDictation";

/** Stop the meeting; on failure the card turns into the stop-failed notice. */
export function stopMeetingFromPill(source: string): Promise<boolean> {
  return invoke("stop_meeting")
    .then(() => {
      console.info(`pill meeting_stop clicked source=${source} result=stopped`);
      return true;
    })
    .catch(() => {
      // Never the error text in the log line: the fact and the source.
      console.warn(`pill meeting_stop clicked source=${source} result=error`);
      window.dispatchEvent(new CustomEvent(MEETING_STOP_FAILED_EVENT));
      return false;
    });
}

export function MeetingDictationView({
  notice,
  busy = false,
  onAction,
}: {
  notice: MeetingNotice;
  busy?: boolean;
  onAction: (a: MeetingNoticeAction) => void;
}) {
  const urgent = notice.kind === "stop-failed";
  return (
    <div
      className={`meeting-dictation ${notice.kind}`}
      data-meeting-notice={notice.kind}
      role={urgent ? "alert" : "status"}
      aria-live={urgent ? "assertive" : "polite"}
    >
      <p className="meeting-dictation-line">
        <span className="meeting-dictation-dot" aria-hidden />
        {notice.line}
      </p>
      <div className="meeting-dictation-actions">
        {noticeButtons(notice).map((b) => (
          <button
            key={b.action}
            type="button"
            className={`meeting-dictation-${b.action}${b.primary ? " is-primary" : ""}`}
            disabled={busy}
            aria-keyshortcuts={b.chord === "⌃⌘M" ? "Control+Meta+M" : undefined}
            onClick={(e) => {
              e.stopPropagation();
              onAction(b.action);
            }}
          >
            {b.label}
            {b.chord && <kbd className="meeting-dictation-chord">{b.chord}</kbd>}
          </button>
        ))}
      </div>
    </div>
  );
}

export default function MeetingDictationCard({ forced = null }: { forced?: MeetingNotice | null } = {}) {
  const [notice, setNotice] = useState<MeetingNotice | null>(forced);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  const show = useCallback((n: MeetingNotice | null) => {
    if (!n) return;
    console.info(`pill meeting_notice shown kind=${n.kind}`);
    setNotice(n);
    setBusy(false);
  }, []);

  useEffect(() => {
    if (forced) return;
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen(TAKE_CANCELLED_EVENT, (e) => show(noticeFromCancel(e.payload))).then(push);
    listen(HANDS_FREE_MEETING_ASK_EVENT, (e) => show(noticeFromAsk(e.payload))).then(push);
    listen<MeetingStatus>(MEETING_EVENT, (e) =>
      setNotice((n) => noticeAfterMeetingStatus(n, e.payload?.recording === true)),
    ).then(push);
    const onFail = () => show(noticeFromStopFailure());
    window.addEventListener(MEETING_STOP_FAILED_EVENT, onFail);
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
      window.removeEventListener(MEETING_STOP_FAILED_EVENT, onFail);
    };
  }, [forced, show]);

  const act = useCallback(
    (a: MeetingNoticeAction) => {
      if (!notice) return;
      console.info(`pill meeting_notice answered kind=${notice.kind} action=${a}`);
      const done = () => setNotice(null);
      switch (a) {
        case "keep":
        case "dismiss":
          done();
          return;
        case "both":
          setBusy(true);
          invoke(HANDS_FREE_MEETING_RESPOND, { both: true }).catch(() => {}).finally(done);
          return;
        case "meetingOnly":
          setBusy(true);
          invoke(HANDS_FREE_MEETING_RESPOND, { both: false })
            .catch(() => console.warn("pill meeting_notice respond_failed action=meetingOnly"))
            .finally(done);
          return;
        case "stop":
        case "retry":
          setBusy(true);
          // Success clears the card; a failure re-shows it as stop-failed
          // (through MEETING_STOP_FAILED_EVENT), so it never just vanishes.
          stopMeetingFromPill(`notice_${notice.kind}`).then((ok) => {
            if (ok) done();
            else setBusy(false);
          });
          return;
        case "quit":
          setBusy(true);
          invoke(PILL_QUIT_YAP).catch(() => {
            console.warn("pill quit_yap result=error");
            setBusy(false);
          });
          return;
      }
    },
    [notice],
  );

  useEffect(() => {
    if (!notice) return;
    // The float window is click-through outside the reported hitbox; while
    // the card is up it IS the hitbox. Re-held on resize and after every
    // layout (the one-pill layout moves the card into its row).
    const hold = () => {
      const el = ref.current;
      if (!el || !el.isConnected) return;
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "meeting-dictation");
    };
    hold();
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => hold());
    if (ref.current && ro) ro.observe(ref.current);
    window.addEventListener(PILL_LAYOUT_EVENT, hold);
    const ms = forced ? null : noticeTtlMs(notice);
    const ttl = ms === null ? null : window.setTimeout(() => setNotice(null), ms);
    const onKey = (e: KeyboardEvent) => {
      // A failure is not dismissed by Esc: it stays until Try again or Quit.
      if (e.key === "Escape" && notice.kind !== "stop-failed") act("dismiss");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      if (ttl !== null) window.clearTimeout(ttl);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(PILL_LAYOUT_EVENT, hold);
      ro?.disconnect();
      holdPillHitbox(null, "meeting-dictation");
    };
  }, [notice, forced, act]);

  if (!notice) return null;
  return (
    <div ref={ref} className="meeting-dictation-wrap">
      <MeetingDictationView notice={notice} busy={busy} onAction={act} />
    </div>
  );
}
