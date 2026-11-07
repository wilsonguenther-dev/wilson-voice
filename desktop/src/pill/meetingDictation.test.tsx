// yap-fx-06 — meeting + dictation at the same time: the pill shows both, Esc
// says what it did, Stop is one click, hands-free asks once, and a failed stop
// always has a way out that is not force quit.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import MeetingBadge from "./MeetingBadge";
import { MeetingDictationView } from "./MeetingDictationCard";
import { IDLE_MEETING, type MeetingStatus } from "./meeting";
import { CARD_CLASSES, CARD_SELECTOR, pillLayout } from "./state";
import { matrixMeetingDictationScenes } from "./forced";
import {
  HANDS_FREE_MEETING_ASK_EVENT,
  HANDS_FREE_MEETING_RESPOND,
  MEETING_CANCEL_NOTICE,
  PILL_QUIT_YAP,
  STOP_MEETING_CHORD,
  TAKE_CANCELLED_EVENT,
  forcedMeetingNotice,
  noticeAfterMeetingStatus,
  noticeButtons,
  noticeFromAsk,
  noticeFromCancel,
  noticeFromStopFailure,
  noticeTtlMs,
} from "./meetingDictation";

const RECORDING: MeetingStatus = {
  ...IDLE_MEETING,
  recording: true,
  id: "m1",
  title: "Standup",
  elapsedSeconds: 724,
  elapsedLabel: "00:12:04",
  captureAvailable: true,
};

describe("yap-fx-06 — wire names match the Rust side", () => {
  it("uses the backend's event, command and sentence", () => {
    expect(TAKE_CANCELLED_EVENT).toBe("take_cancelled");
    expect(HANDS_FREE_MEETING_ASK_EVENT).toBe("hands_free_meeting_ask");
    expect(HANDS_FREE_MEETING_RESPOND).toBe("hands_free_meeting_respond");
    expect(PILL_QUIT_YAP).toBe("pill_quit_yap");
    expect(MEETING_CANCEL_NOTICE).toBe("Dictation cancelled — meeting still recording. Stop meeting?");
    expect(STOP_MEETING_CHORD).toBe("⌃⌘M");
  });
});

describe("yap-fx-06 — the pill shows both", () => {
  it("a dictation during a meeting keeps the meeting's clock slot in the layout", () => {
    for (const phase of ["listening", "thinking"] as const) {
      for (const dock of ["bottom", "left", "right"] as const) {
        const l = pillLayout({ phase, meeting: "recording", dock });
        expect(l.clock, `${phase}/${dock}`).not.toBeNull();
        expect(l.bubble).toBeTruthy();
      }
    }
  });

  it("the meeting badge always carries a one-click Stop while the meeting records", () => {
    const html = renderToStaticMarkup(<MeetingBadge status={RECORDING} />);
    expect(html).toContain('class="rec-stop"');
    expect(html).toContain("Stop meeting — recording for 00:12:04");
    expect(renderToStaticMarkup(<MeetingBadge status={IDLE_MEETING} />)).toBe("");
  });
});

describe("yap-fx-06 — Esc cancels only the dictation and says so", () => {
  it("a cancel during a meeting becomes the toast with Stop meeting (⌃⌘M)", () => {
    const n = noticeFromCancel({ message: "Cancelled", failed: null, meetingStillRecording: true, meetingNotice: MEETING_CANCEL_NOTICE });
    expect(n).toEqual({ kind: "cancelled", line: MEETING_CANCEL_NOTICE });
    const buttons = noticeButtons(n!);
    expect(buttons.map((b) => b.label)).toEqual(["Stop meeting", "Keep recording"]);
    expect(buttons[0]).toMatchObject({ action: "stop", primary: true, chord: "⌃⌘M" });
    const html = renderToStaticMarkup(<MeetingDictationView notice={n!} onAction={() => {}} />);
    expect(html).toContain(MEETING_CANCEL_NOTICE);
    expect(html).toContain("⌃⌘M");
    expect(html).toContain('data-meeting-notice="cancelled"');
    expect(html).toContain('aria-keyshortcuts="Control+Meta+M"');
  });

  it("a cancel with no meeting stays quiet (no toast)", () => {
    expect(noticeFromCancel({ message: "Cancelled", failed: null, meetingStillRecording: false, meetingNotice: null })).toBeNull();
    expect(noticeFromCancel({ message: "Cancelled", failed: null })).toBeNull();
    expect(noticeFromCancel(null)).toBeNull();
  });

  it("an older backend payload without the sentence still gets the right line", () => {
    expect(noticeFromCancel({ meetingStillRecording: true })).toEqual({ kind: "cancelled", line: MEETING_CANCEL_NOTICE });
  });
});

describe("yap-fx-06 — hands-free during a meeting asks whether you want both", () => {
  it("offers Both / Just the meeting, Both first", () => {
    const n = noticeFromAsk({ meetingId: "m1", title: "Standup", elapsedLabel: "00:12:04" });
    expect(n.kind).toBe("ask");
    expect(n.line).toContain("A meeting is recording (00:12:04)");
    expect(noticeButtons(n).map((b) => [b.action, b.label])).toEqual([
      ["both", "Both"],
      ["meetingOnly", "Just the meeting"],
    ]);
    expect(noticeTtlMs(n)).toBe(15000);
  });

  it("a malformed ask still asks, without a clock or an id", () => {
    const n = noticeFromAsk(null);
    expect(n).toEqual({ kind: "ask", line: "A meeting is recording. Dictate too, and keep both going?", meetingId: null });
  });
});

describe("yap-fx-06 — a recording you cannot stop is never force-quit-only", () => {
  it("a failed stop offers Try again and Quit Yap and stays until answered", () => {
    const n = noticeFromStopFailure();
    expect(n.line).toMatch(/quitting saves the recording/);
    expect(noticeButtons(n).map((b) => b.action)).toEqual(["retry", "quit"]);
    expect(noticeTtlMs(n)).toBeNull();
    const html = renderToStaticMarkup(<MeetingDictationView notice={n} onAction={() => {}} />);
    expect(html).toContain('role="alert"');
    expect(html).toContain("Quit Yap");
  });

  it("every notice clears once the meeting has stopped, and stays while it records", () => {
    for (const n of [noticeFromStopFailure(), noticeFromAsk({}), noticeFromCancel({ meetingStillRecording: true })]) {
      expect(noticeAfterMeetingStatus(n, true)).toBe(n);
      expect(noticeAfterMeetingStatus(n, false)).toBeNull();
    }
    expect(noticeAfterMeetingStatus(null, false)).toBeNull();
    expect(noticeAfterMeetingStatus(null, true)).toBeNull();
  });
});

describe("yap-fx-06 — the card is laid out and photographed like the others", () => {
  it("is a stacked card", () => {
    expect(CARD_CLASSES).toContain("meeting-dictation-wrap");
    expect(CARD_SELECTOR).toContain(".meeting-dictation-wrap");
  });

  it("has a forced matrix scene per moment", () => {
    const scenes = matrixMeetingDictationScenes();
    expect(scenes.map((s) => s.query)).toEqual([
      "meetingNotice=cancelled",
      "meetingNotice=ask",
      "meetingNotice=stop-failed",
    ]);
    for (const s of scenes) expect(forcedMeetingNotice("?" + s.query)).not.toBeNull();
    expect(forcedMeetingNotice("?phase=idle")).toBeNull();
  });
});
