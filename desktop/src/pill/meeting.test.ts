/**
 * YV95 — the label rules shared by the pill, the Meetings banner and the empty
 * state. They are unit-tested for the same reason live.ts is: the frontend gate
 * used to be a type check, and a type check has no opinion about a clock that
 * renders "undefined" or a disabled button that never says why.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  disabledReason,
  elapsedLabel,
  formatElapsed,
  IDLE_MEETING,
  MEETING_FAILED_FALLBACK,
  MEETING_WAITING_FALLBACK,
  MEETING_PILL_IDLE,
  MEETING_PROGRESS_EVENT,
  MEETING_RESULT_HOLD_MS,
  MEETING_STOP_FAILED_DOM_EVENT,
  MEETING_STOP_FAILED_FALLBACK,
  meetingAfterView,
  meetingSavedUntil,
  meetingNavPayload,
  meetingPillHoldMs,
  parseNavigate,
  recordLabel,
  reduceMeetingPill,
  stopFailedSentence,
  systemAudioBadge,
  type MeetingPillInput,
  type MeetingPillPhase,
  type MeetingProgressEvent,
  type MeetingStatus,
} from "./meeting";
import {
  MEETING_DELETED_STATE,
  NOTES_RECHECK_MS,
  NOTES_RECHECK_SLOW_MS,
  NOTES_STALL_MS,
  notesWritingId,
  STILL_WRITING_LABEL,
  type NotesJobSnapshot,
} from "./meeting";
import { MEETING_PROGRESS_EVENT as DECLARED_PROGRESS_EVENT } from "../meetings/progress";
import { SAVED_HOLD_MS, SAVED_LABEL, SCENE_W } from "./meetingScene";
import { MEETING_STOP_FAILED_EVENT } from "./meetingDictation";
import { CLOCK_H, CLOCK_W, meetingMode, pillLayout } from "./state";
import MeetingBadge from "./MeetingBadge";

// fix-l3b1-2-stack-stop-uses-stop-failed-path — the stacked Stop's click runs
// against a mocked invoke (the real one needs the Tauri runtime). Partial
// mocks: every other export stays real, and nothing above calls invoke.
const tauriMocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: tauriMocks.invoke,
}));
// The badge's only hook is the call-offer view; calling the component as a
// plain function (node env, no DOM) needs it to be hook-free.
vi.mock("./CallOffer", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./CallOffer")>()),
  useCallRecordingView: () => null,
}));

const recording = (over: Partial<MeetingStatus> = {}): MeetingStatus => ({
  recording: true,
  id: "m1",
  title: "Meeting 3:04 PM",
  elapsedSeconds: 3725,
  elapsedLabel: "01:02:05",
  captureAvailable: true,
  ...over,
});

describe("formatElapsed", () => {
  it("is always hh:mm:ss so a 3-hour and a 3-minute meeting line up", () => {
    expect(formatElapsed(0)).toBe("00:00:00");
    expect(formatElapsed(9)).toBe("00:00:09");
    expect(formatElapsed(750)).toBe("00:12:30");
    expect(formatElapsed(3725)).toBe("01:02:05");
    expect(formatElapsed(3 * 3600)).toBe("03:00:00");
  });

  it("never renders a negative or a NaN clock", () => {
    expect(formatElapsed(-5)).toBe("00:00:00");
    expect(formatElapsed(Number.NaN)).toBe("00:00:00");
    expect(formatElapsed(Number.POSITIVE_INFINITY)).toBe("00:00:00");
  });

  it("matches the Rust renderer's rule of truncating, not rounding", () => {
    // `meetings::format_offset` floors — a meeting is not 13 seconds old until
    // it is, and a clock that rounds up starts at 00:00:01.
    expect(formatElapsed(12.9)).toBe("00:00:12");
  });
});

describe("elapsedLabel", () => {
  it("prefers the label the backend already rendered", () => {
    expect(elapsedLabel(recording())).toBe("01:02:05");
  });

  it("falls back to formatting locally rather than showing undefined", () => {
    expect(elapsedLabel(recording({ elapsedLabel: undefined }))).toBe("01:02:05");
    expect(elapsedLabel(recording({ elapsedLabel: "" }))).toBe("01:02:05");
  });

  it("has an answer for no status at all", () => {
    expect(elapsedLabel(null)).toBe("00:00:00");
    expect(elapsedLabel(IDLE_MEETING)).toBe("00:00:00");
  });
});

describe("the record control's copy", () => {
  it("names the action, in both states", () => {
    expect(recordLabel(IDLE_MEETING)).toBe("Record this meeting");
    expect(recordLabel(recording())).toBe("Stop meeting");
  });

  it("is enabled when a capture engine is installed", () => {
    expect(disabledReason({ ...IDLE_MEETING, captureAvailable: true })).toBeNull();
  });

  it("says WHY it is disabled instead of just being grey", () => {
    const why = disabledReason({
      ...IDLE_MEETING,
      captureAvailable: false,
      unavailableReason: "Meeting recording needs the capture engine — it is not installed in this build.",
    });
    expect(why).toContain("capture engine");
  });

  it("still has a reason when the backend forgot to send one", () => {
    expect(disabledReason({ ...IDLE_MEETING, captureAvailable: false })).toBeTruthy();
  });

  it("never disables the STOP control — a running meeting can always be stopped", () => {
    expect(disabledReason(recording({ captureAvailable: false }))).toBeNull();
  });
});

/**
 * YV110 — the sentence matrix rows 1 and 2 both end in ("badged", "banner in
 * the pill"). The rule under test is not the wording, which Rust owns: it is
 * that a mic-only meeting is never SILENT about it, and that the badge does not
 * outlive the meeting it describes.
 */
describe("the system-audio badge", () => {
  it("shows the backend's sentence while a meeting is recording", () => {
    const denied =
      "Yap has not received any system audio. macOS has not granted System Audio Recording to Yap";
    expect(systemAudioBadge(recording({ systemAudio: denied }))).toBe(denied);
  });

  it("says nothing when both tracks are recording", () => {
    expect(systemAudioBadge(recording())).toBeNull();
    expect(systemAudioBadge(recording({ systemAudio: null }))).toBeNull();
    expect(systemAudioBadge(recording({ systemAudio: "   " }))).toBeNull();
  });

  it("never describes a meeting that is over", () => {
    // The pill can hold the last payload it received after a stop; a badge that
    // survived it would be a sentence about a recording that no longer exists.
    expect(
      systemAudioBadge({ ...IDLE_MEETING, systemAudio: "Recording your microphone only." }),
    ).toBeNull();
    expect(systemAudioBadge(null)).toBeNull();
    expect(systemAudioBadge(undefined)).toBeNull();
  });
});

/**
 * yap24-NT7 — the pill follows the meeting after stop. The reducer is driven by
 * the two events the backend already emits (`meeting` status and the NT2
 * pipeline's `meeting:progress`), in the order the backend emits them.
 */
describe("the pill after a meeting stops (yap24-NT7)", () => {
  const progress = (over: Partial<MeetingProgressEvent> = {}): MeetingPillInput => ({
    type: "progress",
    event: {
      id: "m1",
      state: "transcribing",
      percent: 0,
      processedThroughSeconds: 0,
      durationSeconds: 3725,
      error: null,
      ...over,
    },
  });
  const status = (s: MeetingStatus): MeetingPillInput => ({ type: "status", status: s });
  const run = (inputs: MeetingPillInput[], from: MeetingPillPhase = MEETING_PILL_IDLE) => {
    const seen: MeetingPillPhase[] = [];
    let p = from;
    for (const i of inputs) {
      p = reduceMeetingPill(p, i);
      seen.push(p);
    }
    return seen;
  };

  it("listens to the ONE progress event the pipeline emits, never a re-typed copy", () => {
    expect(MEETING_PROGRESS_EVENT).toBe(DECLARED_PROGRESS_EVENT);
    expect(MEETING_PROGRESS_EVENT).toBe("meeting:progress");
  });

  it("follows the #250 notes worker's phases after the transcript lands", () => {
    const seen = run([
      progress({ state: "complete", percent: 100 }),
      progress({ state: "notes_queued", percent: 0 }),
      progress({ state: "preparing_notes", percent: 40 }),
      progress({ state: "summarizing", percent: 100 }),
      progress({ state: "notes_ready", percent: 100 }),
    ]);
    expect(seen.map((p) => p.kind)).toEqual([
      "notes-ready",
      "processing",
      "processing",
      "processing",
      "notes-ready",
    ]);
    // A notes phase is never left hanging on "Writing notes".
    expect(meetingAfterView(seen[4], "idle")?.label).toBe("Notes ready — Open");
    const failed = run([
      progress({ state: "summarizing", percent: 100 }),
      progress({ state: "notes_failed", percent: 0, error: "The notes model could not be downloaded." }),
    ]);
    expect(failed[1]).toEqual({ kind: "failed", id: "m1", sentence: "The notes model could not be downloaded." });
    const cancelled = run([
      progress({ state: "notes_queued", percent: 0 }),
      progress({ state: "notes_cancelled", percent: 0 }),
    ]);
    expect(cancelled[1].kind).toBe("notes-ready");
  });

  it("walks recording -> saved -> processing 0..100 -> notes-ready", () => {
    // fix-b8-1 — the stop is the "Saved" beat for SAVED_HOLD_MS; the road's
    // events here land after it.
    const T0 = 1_000_000;
    const late = T0 + SAVED_HOLD_MS;
    const at = (i: MeetingPillInput, nowMs: number): MeetingPillInput =>
      i.type === "status" || i.type === "progress" ? { ...i, nowMs } : i;
    const seen = run([
      status(recording()),
      at(status({ ...IDLE_MEETING, captureAvailable: true }), T0), // stop
      at(progress({ percent: 0 }), late), // enqueue
      at(progress({ percent: 42 }), late),
      at(progress({ percent: 100 }), late),
      at(progress({ state: "complete", percent: 100 }), late),
    ]);
    expect(seen.map((p) => p.kind)).toEqual([
      "recording",
      "saved",
      "processing",
      "processing",
      "processing",
      "notes-ready",
    ]);
    expect(seen.slice(2, 5).map((p) => (p.kind === "processing" ? p.percent : -1))).toEqual([0, 42, 100]);
    const views = seen.map((p) => meetingAfterView(p, "idle")?.label ?? null);
    expect(views).toEqual([
      null,
      SAVED_LABEL,
      "Transcribing 0%",
      "Transcribing 42%",
      // 100% of the windows is not notes yet: the label holds at 99 until the
      // pipeline says `complete`, exactly like the Meetings row.
      "Transcribing 99%",
      "Notes ready — Open",
    ]);
    const ready = meetingAfterView(seen[5], "idle");
    expect(ready?.opens).toBe(true);
    expect(ready?.id).toBe("m1");
  });

  it("says 'Writing notes' while the summary model runs, then notes-ready again", () => {
    const seen = run([
      progress({ state: "complete", percent: 100 }),
      progress({ state: "summarizing", percent: 100 }),
      progress({ state: "complete", percent: 100 }),
    ]);
    expect(seen.map((p) => meetingAfterView(p, "idle")?.label)).toEqual([
      "Notes ready — Open",
      "Writing notes",
      "Notes ready — Open",
    ]);
  });

  it("treats a partial transcript as notes-ready (the page says which part failed)", () => {
    expect(run([progress({ state: "partial", percent: 100 })])[0].kind).toBe("notes-ready");
  });

  it("failure path: shows the pipeline's own sentence, and opens the meeting to Retry", () => {
    const why = "Yap could not transcribe this meeting — speech recognition failed on all of its audio. Press Retry.";
    const [p1, p2] = run([progress({ percent: 30 }), progress({ state: "failed", percent: 30, error: why })]);
    expect(p1.kind).toBe("processing");
    expect(p2).toEqual({ kind: "failed", id: "m1", sentence: why });
    const view = meetingAfterView(p2, "idle");
    expect(view?.label).toBe(why);
    expect(view?.opens).toBe(true);
    // A failure the backend did not explain is still a sentence, never blank.
    const [blank] = run([progress({ state: "failed", error: null })]);
    expect(blank.kind === "failed" && blank.sentence).toBe(MEETING_FAILED_FALLBACK);
  });

  it("waiting_for_model (yap24-NT2 R3) leaves 'Transcribing 0%' for a held wait sentence that opens the meeting", () => {
    // NT7 announces the enqueue as transcribing 0%; NT2's no-model job then ends
    // with state waiting_for_model. The pill must not stay on 0% forever.
    const why =
      "No speech model is downloaded yet. Download one in Settings and Yap transcribes this meeting automatically. Your audio is kept.";
    const [queued, waiting] = run([
      progress({ percent: 0 }),
      progress({ state: "waiting_for_model", percent: 0, error: why }),
    ]);
    expect(queued.kind).toBe("processing");
    expect(waiting).toEqual({ kind: "waiting", id: "m1", sentence: why });
    expect(meetingPillHoldMs(waiting)).toBe(10_000);
    const view = meetingAfterView(waiting, "idle");
    expect(view?.kind).toBe("waiting");
    expect(view?.label).toBe("Waiting for a speech model");
    expect(view?.title).toBe(why);
    expect(view?.percent).toBeNull();
    expect(view?.opens).toBe(true);
    // The hold's dismiss clears it, scoped to its meeting.
    expect(reduceMeetingPill(waiting, { type: "dismiss", id: "other" })).toBe(waiting);
    expect(reduceMeetingPill(waiting, { type: "dismiss", id: "m1" })).toBe(MEETING_PILL_IDLE);
    // A wait the backend did not explain still says what to do.
    const [blank] = run([progress({ state: "waiting_for_model", error: null })]);
    expect(blank.kind === "waiting" && blank.sentence).toBe(MEETING_WAITING_FALLBACK);
    // A download requeues the row: the next transcribing event resumes processing.
    const [, , resumed] = run([
      progress({ percent: 0 }),
      progress({ state: "waiting_for_model", percent: 0, error: why }),
      progress({ percent: 5 }),
    ]);
    expect(resumed).toEqual({ kind: "processing", id: "m1", percent: 5, step: "transcribing" });
  });

  it("a dictation during processing still shows the dictation phases (dictation outranks the badge)", () => {
    const [processing] = run([progress({ percent: 42 })]);
    for (const take of ["listening", "thinking", "transcribing", "polishing", "pasting", "done", "error", "empty", "cancelled", "model-loading", "blocked", "waiting", "gated"] as const) {
      expect(meetingAfterView(processing, take)).toBeNull();
    }
    // …and the badge comes back the moment the take is over — the phase itself
    // was never touched by the dictation.
    expect(meetingAfterView(processing, "idle")?.label).toBe("Transcribing 42%");
    expect(meetingAfterView(processing, "sleepy")?.label).toBe("Transcribing 42%");
    const [ready] = run([progress({ state: "complete", percent: 100 })]);
    expect(meetingAfterView(ready, "listening")).toBeNull();
  });

  it("the 1 Hz idle status broadcast never wipes processing or notes-ready", () => {
    const idle = status({ ...IDLE_MEETING, captureAvailable: true });
    const [p] = run([progress({ percent: 42 })]);
    expect(reduceMeetingPill(p, idle)).toBe(p);
    const [r] = run([progress({ state: "complete" })]);
    expect(reduceMeetingPill(r, idle)).toBe(r);
  });

  it("percent never runs backwards for the same meeting", () => {
    const seen = run([progress({ percent: 60 }), progress({ percent: 0 }), progress({ percent: 61 })]);
    expect(seen.map((p) => (p.kind === "processing" ? p.percent : -1))).toEqual([60, 60, 61]);
  });

  it("a live recording outranks any background job's progress, and replaces an unclicked result", () => {
    const [rec] = run([status(recording())]);
    expect(reduceMeetingPill(rec, progress({ id: "older", percent: 10 }))).toBe(rec);
    const [ready] = run([progress({ state: "complete" })]);
    expect(reduceMeetingPill(ready, status(recording())).kind).toBe("recording");
  });

  it("notes-ready and failed hold for 10 s; nothing else holds", () => {
    expect(MEETING_RESULT_HOLD_MS).toBe(10_000);
    expect(meetingPillHoldMs({ kind: "notes-ready", id: "m1" })).toBe(10_000);
    expect(meetingPillHoldMs({ kind: "failed", id: "m1", sentence: "x" })).toBe(10_000);
    expect(meetingPillHoldMs({ kind: "processing", id: "m1", percent: 3, step: "transcribing" })).toBeNull();
    expect(meetingPillHoldMs(MEETING_PILL_IDLE)).toBeNull();
  });

  it("dismiss (click or hold) is scoped to its meeting: a stale timer cannot clear a newer result", () => {
    const [ready] = run([progress({ state: "complete" })]);
    expect(reduceMeetingPill(ready, { type: "dismiss", id: "m1" })).toBe(MEETING_PILL_IDLE);
    expect(reduceMeetingPill(ready, { type: "dismiss", id: "other" })).toBe(ready);
    const [proc] = run([progress({ percent: 5 })]);
    expect(reduceMeetingPill(proc, { type: "dismiss", id: "m1" })).toBe(proc);
  });

  it("ignores a malformed event instead of drawing a phase for no meeting", () => {
    expect(reduceMeetingPill(MEETING_PILL_IDLE, progress({ id: "" }))).toBe(MEETING_PILL_IDLE);
    expect(reduceMeetingPill(MEETING_PILL_IDLE, progress({ state: "recording" }))).toBe(MEETING_PILL_IDLE);
  });

  it("clicking notes-ready navigates to THAT meeting in the Meetings view", () => {
    // lib.rs `meeting_notes_nav_payload` builds the same string for the tray.
    expect(meetingNavPayload("abc-123")).toBe("meetings:abc-123");
    expect(parseNavigate("meetings:abc-123")).toEqual({ nav: "meetings", meetingId: "abc-123" });
    expect(parseNavigate("settings")).toEqual({ nav: "settings", meetingId: null });
    expect(parseNavigate("meetings")).toEqual({ nav: "meetings", meetingId: null });
  });
});

describe("the Saved stop beat has a real slot (fix-b8-1-meeting-saved-beat-gets-a-slot)", () => {
  const T0 = 5_000_000;
  const DOCKS = ["bottom", "left", "right"] as const;
  const stopped: MeetingStatus = { ...IDLE_MEETING, captureAvailable: true };
  const progress = (over: Partial<MeetingProgressEvent> = {}, nowMs?: number): MeetingPillInput => ({
    type: "progress",
    event: { id: "m1", state: "transcribing", percent: 0, processedThroughSeconds: 0, durationSeconds: 3725, error: null, ...over },
    ...(nowMs === undefined ? {} : { nowMs }),
  });
  const rec: MeetingPillPhase = reduceMeetingPill(MEETING_PILL_IDLE, { type: "status", status: recording() });
  const stop = (from: MeetingPillPhase = rec, nowMs: number = T0): MeetingPillPhase =>
    reduceMeetingPill(from, { type: "status", status: stopped, nowMs, lastElapsedS: 3725 });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("(a) recording -> stopped with no progress yet: meetingMode 'after' and a clock box >= 112 x CLOCK_H at every dock", () => {
    const saved = stop();
    expect(saved.kind).toBe("saved");
    expect(meetingSavedUntil(saved)).toBe(T0 + SAVED_HOLD_MS);
    const view = meetingAfterView(saved, "idle");
    expect(view?.kind).toBe("saved");
    expect(view?.label).toBe(SAVED_LABEL);
    expect(view?.opens).toBe(false);
    // The scene keeps the meeting's last elapsed time, not the stopped 0.
    expect(view?.elapsedS).toBe(3725);
    const mode = meetingMode(stopped, view);
    expect(mode).toBe("after");
    for (const dock of DOCKS) {
      const l = pillLayout({ phase: "idle", meeting: mode, dock });
      expect(l.clock).not.toBeNull();
      expect(l.clock!.w).toBeGreaterThanOrEqual(SCENE_W);
      expect(l.clock!.h).toBe(CLOCK_H);
    }
    // The 112 pt scene plus the word "Saved" fit the after slot.
    expect(CLOCK_W.after).toBeGreaterThanOrEqual(SCENE_W + 48);
  });

  it("(b) stopping while a dictation is live still reserves the clock box for the beat", () => {
    const saved = stop();
    for (const take of ["listening", "thinking", "done"] as const) {
      const view = meetingAfterView(saved, take);
      expect(view?.kind).toBe("saved");
      const mode = meetingMode(stopped, view);
      expect(mode).toBe("after");
      for (const dock of DOCKS) {
        const l = pillLayout({ phase: take, meeting: mode, dock });
        expect(l.clock).not.toBeNull();
        expect(l.clock!.w).toBeGreaterThanOrEqual(SCENE_W);
        expect(l.clock!.h).toBe(CLOCK_H);
      }
    }
    // Only the beat keeps its box mid-take: the processing badge still yields.
    const proc = reduceMeetingPill(MEETING_PILL_IDLE, progress({ percent: 5 }));
    expect(meetingAfterView(proc, "listening")).toBeNull();
  });

  it("(c) a 'failed' result inside the hold replaces Saved at once, and a failed stop never says Saved", () => {
    const saved = stop();
    const failed = reduceMeetingPill(saved, progress({ state: "failed", error: "The recording could not be read." }, T0 + 100));
    expect(failed).toEqual({ kind: "failed", id: "m1", sentence: "The recording could not be read." });
    expect(meetingAfterView(failed, "idle")?.label).toBe("The recording could not be read.");
    // Folded first, failed second: the failure still wins inside the hold.
    const folded = reduceMeetingPill(saved, progress({ percent: 3 }, T0 + 50));
    expect(folded.kind).toBe("saved");
    expect(reduceMeetingPill(folded, progress({ state: "failed", error: null }, T0 + 60))).toEqual({
      kind: "failed",
      id: "m1",
      sentence: MEETING_FAILED_FALLBACK,
    });
    // stop_meeting rejected, then the status dropped anyway: the error, not Saved.
    const refused = reduceMeetingPill(rec, { type: "stop_failed", sentence: "The recorder did not answer.", nowMs: T0 - 10 });
    expect(refused.kind).toBe("recording");
    expect(stop(refused, T0)).toEqual({ kind: "failed", id: "m1", sentence: "The recorder did not answer." });
    // The status dropped first, then the stop reported an error: Saved goes.
    expect(reduceMeetingPill(saved, { type: "stop_failed", sentence: "", nowMs: T0 + 5 })).toEqual({
      kind: "failed",
      id: "m1",
      sentence: MEETING_STOP_FAILED_FALLBACK,
    });
    // A failure long before a later Stop that worked does not taint it.
    const old = reduceMeetingPill(rec, { type: "stop_failed", sentence: "x", nowMs: T0 - SAVED_HOLD_MS - 1 });
    expect(stop(old, T0).kind).toBe("saved");
    expect(stopFailedSentence(new Error("  boom  "))).toBe("boom");
    expect(stopFailedSentence(undefined)).toBe(MEETING_STOP_FAILED_FALLBACK);
    // The pill's Stop (stopMeetingFromPill, yap-fx-06) raises the stop-failed
    // card's window event with no detail; the shell's phase listens on the
    // SAME name, so a failed badge Stop reaches reduceMeetingPill.
    expect(MEETING_STOP_FAILED_EVENT).toBe(MEETING_STOP_FAILED_DOM_EVENT);
  });

  it("(c2) a stop error with no meeting on screen changes nothing; an id-less Saved yields to its road; stale saved_elapsed is ignored", () => {
    expect(reduceMeetingPill(MEETING_PILL_IDLE, { type: "stop_failed", sentence: "x", nowMs: T0 })).toBe(MEETING_PILL_IDLE);
    const anon = reduceMeetingPill({ kind: "recording", id: null }, { type: "status", status: stopped, nowMs: T0 });
    expect(anon.kind).toBe("saved");
    expect(reduceMeetingPill(anon, { type: "stop_failed", sentence: "  ", nowMs: T0 + 1 })).toEqual(MEETING_PILL_IDLE);
    expect(reduceMeetingPill({ kind: "recording", id: null, stopFailed: { sentence: "no", atMs: T0 } }, { type: "status", status: stopped, nowMs: T0 + 1 })).toEqual(MEETING_PILL_IDLE);
    const saved = stop();
    expect(reduceMeetingPill(saved, { type: "saved_elapsed", until: -1 })).toBe(saved);
    expect(reduceMeetingPill(MEETING_PILL_IDLE, { type: "saved_elapsed", until: -1 })).toBe(MEETING_PILL_IDLE);
  });

  it("(d) after SAVED_HOLD_MS (fake timers) a transcribing event shows the processing badge", () => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
    const saved = reduceMeetingPill(rec, { type: "status", status: stopped, lastElapsedS: 60 });
    expect(saved.kind).toBe("saved");
    const until = meetingSavedUntil(saved);
    expect(until).toBe(T0 + SAVED_HOLD_MS);
    // Inside the hold: the beat stays, the road folds underneath it, and the
    // deadline (the hook's ONE timer key) does not move.
    vi.advanceTimersByTime(1000);
    const held = reduceMeetingPill(saved, progress({ percent: 7 }));
    expect(held.kind).toBe("saved");
    expect(meetingSavedUntil(held)).toBe(until);
    expect(meetingAfterView(held, "idle")?.label).toBe(SAVED_LABEL);
    // The 1 Hz idle broadcast does not end it early.
    expect(reduceMeetingPill(held, { type: "status", status: stopped })).toBe(held);
    vi.advanceTimersByTime(SAVED_HOLD_MS);
    const proc = reduceMeetingPill(held, progress({ percent: 12 }));
    expect(proc.kind).toBe("processing");
    expect(meetingAfterView(proc, "idle")?.label).toBe("Transcribing 12%");
    // The timer path: saved_elapsed hands over the road folded so far…
    const handed = reduceMeetingPill(held, { type: "saved_elapsed", until: until! });
    expect(handed).toEqual({ kind: "processing", id: "m1", percent: 7, step: "transcribing" });
    expect(meetingAfterView(handed, "idle")?.label).toBe("Transcribing 7%");
    // …and a stale timer from an earlier stop is a no-op.
    expect(reduceMeetingPill(held, { type: "saved_elapsed", until: until! - 1 })).toBe(held);
    // With no road yet, the beat lets go to idle.
    expect(reduceMeetingPill(saved, { type: "saved_elapsed", until: until! })).toBe(MEETING_PILL_IDLE);
  });

  it("a new recording replaces the beat, and the beat is not a 10 s result hold", () => {
    const saved = stop();
    expect(reduceMeetingPill(saved, { type: "status", status: recording({ id: "m2" }) })).toEqual({ kind: "recording", id: "m2" });
    expect(meetingPillHoldMs(saved)).toBeNull();
    expect(reduceMeetingPill(saved, { type: "dismiss", id: "m1" })).toBe(saved);
  });
});

describe("the stacked Stop takes the stop-failed path (fix-l3b1-2-stack-stop-uses-stop-failed-path)", () => {
  const T0 = 7_000_000;
  type TreeNode = { props?: { className?: unknown; children?: unknown; onClick?: unknown } };
  const findByClass = (node: unknown, cls: string): TreeNode | null => {
    if (Array.isArray(node)) {
      for (const n of node) {
        const hit = findByClass(n, cls);
        if (hit) return hit;
      }
      return null;
    }
    if (!node || typeof node !== "object") return null;
    const el = node as TreeNode;
    if (el.props?.className === cls) return el;
    return findByClass(el.props?.children, cls);
  };

  afterEach(() => {
    tauriMocks.invoke.mockReset();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("a rejected stop_meeting from the stack dispatches yap:meeting-stop-failed exactly once", async () => {
    const target = new EventTarget();
    vi.stubGlobal("window", target);
    const heard: Event[] = [];
    target.addEventListener(MEETING_STOP_FAILED_DOM_EVENT, (e) => heard.push(e));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    tauriMocks.invoke.mockRejectedValue(new Error("the recorder did not answer"));

    const tree = MeetingBadge({ status: recording(), stacked: true });
    const stopButton = findByClass(tree, "rec-stop");
    expect(stopButton, "the stacked badge carries the one-click Stop").not.toBeNull();
    let stopped = 0;
    const onClick = stopButton!.props!.onClick as (e: { stopPropagation: () => void }) => void;
    onClick({ stopPropagation: () => { stopped += 1; } });

    await vi.waitFor(() => expect(heard).toHaveLength(1));
    // Let every pending microtask and timer drain: still exactly one event.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(heard).toHaveLength(1);
    expect(stopped).toBe(1);
    expect(tauriMocks.invoke).toHaveBeenCalledTimes(1);
    expect(tauriMocks.invoke).toHaveBeenCalledWith("stop_meeting");
    // The helper's own telemetry line names the stack as the source, never the error text.
    expect(warn).toHaveBeenCalledWith("pill meeting_stop clicked source=pill_stack result=error");
  });

  it("after a failed stack Stop, a status drop within SAVED_HOLD_MS gives the failed phase, never saved", async () => {
    const target = new EventTarget();
    vi.stubGlobal("window", target);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    tauriMocks.invoke.mockRejectedValue(new Error("the recorder did not answer"));
    // The shell's useMeetingPill listener, mirrored: the window event becomes a stop_failed input.
    let phase: MeetingPillPhase = reduceMeetingPill(MEETING_PILL_IDLE, { type: "status", status: recording() });
    expect(phase.kind).toBe("recording");
    target.addEventListener(MEETING_STOP_FAILED_DOM_EVENT, (e) => {
      const detail = (e as CustomEvent).detail;
      phase = reduceMeetingPill(phase, { type: "stop_failed", sentence: typeof detail === "string" ? detail : "", nowMs: T0 });
    });

    const onClick = findByClass(MeetingBadge({ status: recording(), stacked: true }), "rec-stop")!.props!
      .onClick as (e: { stopPropagation: () => void }) => void;
    onClick({ stopPropagation: () => {} });
    await vi.waitFor(() => expect(phase.kind === "recording" && phase.stopFailed !== undefined).toBe(true));

    for (const dropAt of [T0 + 1, T0 + SAVED_HOLD_MS - 1, T0 + SAVED_HOLD_MS]) {
      const after = reduceMeetingPill(phase, {
        type: "status",
        status: { ...IDLE_MEETING, captureAvailable: true },
        nowMs: dropAt,
        lastElapsedS: 3725,
      });
      expect(after.kind, `status drop at +${dropAt - T0} ms`).toBe("failed");
      expect(after.kind).not.toBe("saved");
      expect(after).toEqual({ kind: "failed", id: "m1", sentence: MEETING_STOP_FAILED_FALLBACK });
    }
  });
});

// yap-1107-pill-writing-notes-always-clears — "Writing notes" is driven by the
// notes job's status in the store (list_notes_jobs); events are hints.
describe("the pill's 'Writing notes' always clears (yap-1107-pill-writing-notes-always-clears)", () => {
  const ev = (id: string, state: string, over: Partial<MeetingProgressEvent> = {}): MeetingPillInput => ({
    type: "progress",
    event: { id, state, percent: 0, processedThroughSeconds: 0, durationSeconds: 60, error: null, ...over },
    nowMs: 10_000_000,
  });
  const job = (id: string, phase: string, reason: string | null = null): NotesJobSnapshot => ({ id, phase, percent: 0, reason });
  const reconcile = (jobs: NotesJobSnapshot[]): MeetingPillInput => ({ type: "reconcile", jobs, nowMs: 10_000_000 });
  const fold = (inputs: MeetingPillInput[], from: MeetingPillPhase = MEETING_PILL_IDLE) =>
    inputs.reduce<MeetingPillPhase>((p, i) => reduceMeetingPill(p, i), from);
  /** A result phase lets go via its hold (the dismiss the shell's timer sends). */
  const toRest = (p: MeetingPillPhase): MeetingPillPhase =>
    p.kind === "notes-ready" || p.kind === "failed" || p.kind === "waiting" ? reduceMeetingPill(p, { type: "dismiss", id: p.id }) : p;
  const writing = fold([ev("m1", "complete", { percent: 100 }), ev("m1", "notes_queued"), ev("m1", "summarizing", { percent: 100 })]);

  it("the cadence: one 5 s re-check, 30 s after that (never faster), 90 s to 'Still writing'", () => {
    expect(NOTES_RECHECK_MS).toBe(5_000);
    expect(NOTES_RECHECK_SLOW_MS).toBeGreaterThanOrEqual(30_000);
    expect(NOTES_STALL_MS).toBe(90_000);
  });

  it("(1) the terminal event is dropped entirely: the store showing done or failed returns the pill to rest at once", () => {
    expect(writing).toEqual({ kind: "processing", id: "m1", percent: 100, step: "summarizing" });
    expect(notesWritingId(writing)).toBe("m1");
    // Store still active => the pill keeps writing (and the SAME object, so no timer re-arms).
    expect(reduceMeetingPill(writing, reconcile([job("m1", "summarizing")]))).toBe(writing);
    // Store done, no event ever came: the result (held), then rest.
    const done = reduceMeetingPill(writing, reconcile([job("m1", "notes_ready")]));
    expect(done).toEqual({ kind: "notes-ready", id: "m1" });
    expect(toRest(done)).toEqual(MEETING_PILL_IDLE);
    // Store failed: the worker's own sentence, then rest.
    const failed = reduceMeetingPill(writing, reconcile([job("m1", "notes_failed", "The notes model could not start.")]));
    expect(failed).toEqual({ kind: "failed", id: "m1", sentence: "The notes model could not start." });
    expect(toRest(failed)).toEqual(MEETING_PILL_IDLE);
    // Cancelled in the store, or no job at all (the worker restarted): rest.
    expect(toRest(reduceMeetingPill(writing, reconcile([job("m1", "notes_cancelled")])))).toEqual(MEETING_PILL_IDLE);
    expect(reduceMeetingPill(writing, reconcile([]))).toEqual(MEETING_PILL_IDLE);
    expect(reduceMeetingPill(writing, reconcile([job("other", "summarizing")]))).toEqual(MEETING_PILL_IDLE);
    // A phase this build does not know is still not "Writing notes".
    expect(reduceMeetingPill(writing, reconcile([job("m1", "notes_mystery")]))).toEqual(MEETING_PILL_IDLE);
    // Under the saved beat the road is reconciled too; a failure replaces the beat at once.
    const beat: MeetingPillPhase = { kind: "saved", id: "m2", until: 20_000_000, elapsedS: 5, next: writing };
    expect(reduceMeetingPill(beat, reconcile([job("m1", "summarizing")]))).toBe(beat);
    expect(reduceMeetingPill(beat, reconcile([]))).toEqual({ ...beat, next: MEETING_PILL_IDLE });
    expect(reduceMeetingPill(beat, reconcile([job("m1", "notes_failed", "x")])).kind).toBe("failed");
  });

  it("(1) a reconcile never touches a phase that is not 'Writing notes'", () => {
    const transcribing = fold([ev("m1", "transcribing", { percent: 40 })]);
    expect(reduceMeetingPill(transcribing, reconcile([]))).toBe(transcribing);
    const rec: MeetingPillPhase = { kind: "recording", id: "m1" };
    expect(reduceMeetingPill(rec, reconcile([]))).toBe(rec);
    expect(reduceMeetingPill(MEETING_PILL_IDLE, reconcile([job("m1", "summarizing")]))).toBe(MEETING_PILL_IDLE);
  });

  it("(2) no progress for 90 s => 'Still writing... tap to open notes', and the tap opens that meeting", () => {
    vi.useFakeTimers();
    try {
      let p = writing;
      // The shell's one stall timer (MeetingBadge useMeetingPill) — fake clock.
      const t = setTimeout(() => { p = reduceMeetingPill(p, { type: "stall", id: "m1" }); }, NOTES_STALL_MS);
      vi.advanceTimersByTime(NOTES_STALL_MS - 1);
      expect(meetingAfterView(p, "idle")?.label).toBe("Writing notes");
      vi.advanceTimersByTime(1);
      clearTimeout(t);
      const view = meetingAfterView(p, "idle");
      expect(view?.label).toBe(STILL_WRITING_LABEL);
      expect(STILL_WRITING_LABEL).toBe("Still writing... tap to open notes");
      expect(view?.opens).toBe(true);
      expect(view?.id).toBe("m1");
      // The stall is scoped to its meeting, and progress clears it.
      expect(reduceMeetingPill(writing, { type: "stall", id: "m9" })).toBe(writing);
      expect(meetingAfterView(reduceMeetingPill(p, ev("m1", "summarizing", { percent: 100 })), "idle")?.label).toBe("Writing notes");
      // Store done while stalled => rest.
      expect(toRest(reduceMeetingPill(p, reconcile([job("m1", "notes_ready")])))).toEqual(MEETING_PILL_IDLE);
      // Under a stacked meeting's saved beat the stall reaches the road beneath it,
      // and a stall for another meeting leaves the beat as it is.
      const beat: MeetingPillPhase = { kind: "saved", id: "m2", until: 20_000_000, elapsedS: 5, next: writing };
      const stalledBeat = reduceMeetingPill(beat, { type: "stall", id: "m1" });
      expect(stalledBeat).toEqual({ ...beat, next: { ...writing, stalled: true } });
      expect(reduceMeetingPill(beat, { type: "stall", id: "m9" })).toBe(beat);
      // The re-checks stay armed for the road under the beat.
      expect(notesWritingId(beat)).toBe("m1");
      expect(notesWritingId({ ...beat, next: MEETING_PILL_IDLE })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("(3) a deleted meeting clears any pill phase about it, and a late cancel for it changes nothing", () => {
    const gone = reduceMeetingPill(writing, ev("m1", MEETING_DELETED_STATE));
    expect(gone).toEqual(MEETING_PILL_IDLE);
    expect(reduceMeetingPill(gone, ev("m1", "notes_cancelled"))).toEqual(MEETING_PILL_IDLE);
    // A delete of ANOTHER meeting leaves this one's notes alone.
    expect(reduceMeetingPill(writing, ev("m2", MEETING_DELETED_STATE))).toBe(writing);
  });

  it("(3) Wilson's 22:06Z sequence: notes running, another meeting deleted, 'transcript changed' failure => pill at rest", () => {
    // fbd4eea9 transcribed and its notes deferred, cloud notes ran; at 22:06:52
    // meeting 4b002714 was deleted; at 22:06:53 the store check failed the job.
    const failedRoad = fold([
      ev("fbd4eea9", "transcribing", { percent: 100 }),
      ev("fbd4eea9", "complete", { percent: 100 }),
      ev("fbd4eea9", "notes_queued"),
      ev("fbd4eea9", "summarizing", { percent: 100 }),
      ev("4b002714", MEETING_DELETED_STATE),
      ev("fbd4eea9", "notes_failed", { error: "the transcript changed while the notes were being written. Press Retry notes to write them again." }),
    ]);
    expect(failedRoad.kind).toBe("failed");
    expect(toRest(failedRoad)).toEqual(MEETING_PILL_IDLE);
    // Same sequence with the terminal event LOST: the store still settles it.
    const lost = fold([ev("fbd4eea9", "notes_queued"), ev("fbd4eea9", "summarizing"), ev("4b002714", MEETING_DELETED_STATE)]);
    expect(notesWritingId(lost)).toBe("fbd4eea9");
    expect(toRest(reduceMeetingPill(lost, reconcile([job("fbd4eea9", "notes_failed", "x")])))).toEqual(MEETING_PILL_IDLE);
    // With the worker's one requeue the notes land instead: still at rest after.
    const requeued = fold([ev("fbd4eea9", "summarizing"), ev("fbd4eea9", "notes_queued"), ev("fbd4eea9", "summarizing"), ev("fbd4eea9", "notes_ready", { percent: 100 })]);
    expect(toRest(requeued)).toEqual(MEETING_PILL_IDLE);
  });

  it("(4) boot: the pill starts at rest and a boot-time store read never puts it on 'Writing notes'", () => {
    expect(notesWritingId(MEETING_PILL_IDLE)).toBeNull();
    expect(reduceMeetingPill(MEETING_PILL_IDLE, reconcile([]))).toEqual(MEETING_PILL_IDLE);
    expect(reduceMeetingPill(MEETING_PILL_IDLE, reconcile([job("crashed", "notes_failed", "x"), job("live", "summarizing")]))).toEqual(MEETING_PILL_IDLE);
    expect(meetingAfterView(MEETING_PILL_IDLE, "idle")).toBeNull();
  });

  // (5) one case per route, meeting -> stop -> notes; each returns to rest.
  const stopRoad: MeetingPillInput[] = [
    { type: "status", status: recording({ id: "m1" }), nowMs: 1_000 },
    { type: "status", status: { ...IDLE_MEETING }, nowMs: 2_000 },
    { type: "saved_elapsed", until: 2_000 + SAVED_HOLD_MS },
    ev("m1", "transcribing", { percent: 50 }),
    ev("m1", "complete", { percent: 100 }),
  ];
  const routes: Record<string, MeetingPillInput[]> = {
    local: [ev("m1", "notes_queued"), ev("m1", "preparing_notes", { percent: 40 }), ev("m1", "summarizing", { percent: 100 }), ev("m1", "notes_ready", { percent: 100 })],
    cloud: [ev("m1", "notes_queued"), ev("m1", "summarizing", { percent: 100 }), ev("m1", "notes_ready", { percent: 100 })],
    "cloud-fallback-to-local": [ev("m1", "notes_queued"), ev("m1", "summarizing"), ev("m1", "preparing_notes", { percent: 10 }), ev("m1", "summarizing"), ev("m1", "notes_ready", { percent: 100 })],
    failure: [ev("m1", "notes_queued"), ev("m1", "summarizing"), ev("m1", "notes_failed", { error: "Notes failed." })],
    "meeting-deleted-mid-notes": [ev("m1", "notes_queued"), ev("m1", "summarizing"), ev("m1", MEETING_DELETED_STATE), ev("m1", "notes_cancelled")],
    "terminal-event-lost": [ev("m1", "notes_queued"), ev("m1", "summarizing"), reconcile([job("m1", "notes_ready")])],
  };
  for (const [route, notes] of Object.entries(routes)) {
    it(`(5) route ${route}: meeting -> stop -> notes, and the pill returns to rest`, () => {
      const afterStop = fold(stopRoad);
      expect(afterStop.kind).toBe("notes-ready");
      const sawWriting = notes.some((_, i) => notesWritingId(fold(notes.slice(0, i + 1), afterStop)) === "m1");
      expect(sawWriting).toBe(true);
      const end = toRest(fold(notes, afterStop));
      expect(end).toEqual(MEETING_PILL_IDLE);
      expect(notesWritingId(end)).toBeNull();
    });
  }

  it("(5) stacked meeting: m1's notes, m2 recorded on top, both settle and the pill rests", () => {
    let p = fold([ev("m1", "notes_queued"), ev("m1", "summarizing")]);
    p = reduceMeetingPill(p, { type: "status", status: recording({ id: "m2" }), nowMs: 3_000 });
    expect(p.kind).toBe("recording");
    // m1's terminal lands while m2 records (the recording outranks it).
    p = reduceMeetingPill(p, ev("m1", "notes_ready", { percent: 100 }));
    p = reduceMeetingPill(p, { type: "status", status: { ...IDLE_MEETING }, nowMs: 4_000 });
    expect(p.kind).toBe("saved");
    p = reduceMeetingPill(p, reconcile([job("m1", "notes_ready")]));
    p = reduceMeetingPill(p, { type: "saved_elapsed", until: 4_000 + SAVED_HOLD_MS });
    p = fold([ev("m2", "transcribing", { percent: 30 }), ev("m2", "complete", { percent: 100 }), ev("m2", "notes_queued"), ev("m2", "summarizing")], p);
    expect(notesWritingId(p)).toBe("m2");
    p = reduceMeetingPill(p, reconcile([job("m1", "notes_ready"), job("m2", "notes_ready")]));
    expect(toRest(p)).toEqual(MEETING_PILL_IDLE);
  });
});
