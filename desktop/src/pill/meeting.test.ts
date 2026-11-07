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
import { MEETING_PROGRESS_EVENT as DECLARED_PROGRESS_EVENT } from "../meetings/progress";
import { SAVED_HOLD_MS, SAVED_LABEL, SCENE_W } from "./meetingScene";
import { MEETING_STOP_FAILED_EVENT } from "./meetingDictation";
import { CLOCK_H, CLOCK_W, meetingMode, pillLayout } from "./state";

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
