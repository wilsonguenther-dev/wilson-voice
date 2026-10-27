/**
 * Float entry — picks the pill style from settings and renders it.
 *   "classic" → the original obsidian waveform capsule (ClassicPill)
 *   "yappy"   → the pixel-art chick companion (YappyPill)
 * Live-switches when settings are saved (backend emits "settings").
 *
 * YV53: the same settings carry the dock edge (bottom | left | right). The
 * backend moves the NSPanel; here we only mirror the edge onto <html> so the
 * stage can align the pill flush to it — the pill/world itself is never
 * stretched or letterboxed, it just anchors to that side.
 */
import React, { useCallback, useEffect, useState } from "react";
import { TEST_CHIP } from "./buildChannel";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ClassicPill from "./pill/ClassicPill";
import YappyPill from "./pill/YappyPill";
import CopyCard from "./pill/CopyCard";
import { openMeetingNotes, useMeetingPill } from "./pill/MeetingBadge";
import { meetingAfterView } from "./pill/meeting";
import {
  acceptProgress,
  errorSentence,
  phaseCopy,
  phaseVisual,
  reduceGatePhase,
  reduceTakePhase,
  stripLine,
  toChatTone,
  winningPhase,
  CANCELLED_SETTLE_MS,
  GATED_SETTLE_MS,
  PHASE_HOLD_MS,
  PHASE_ID,
  type LivePhase,
  type TakeStatus,
  type TranscribeProgress,
} from "./pill/live";
import { pillLicense } from "./pill/license";
import { forcedPill, matrixPhases, type ForcedPill } from "./pill/forced";
import { type LicenseStatus } from "./license/status";
import "./tokens.css";
import "./float.css";

interface Settings { pillStyle?: string; pillPosition?: string; companionTone?: string }

/**
 * PERM-C — the backend's refusal payload. `prompting` is true only while the
 * system dialog is on its way (NotDetermined), which is the difference between
 * "waiting" and "blocked" on the pill.
 */
interface MicGateNotice { status?: string; prompting?: boolean }

/**
 * Y3-C — the backend's per-chunk decode progress. Emitted ONCE per COMPLETED
 * chunk and never for a single-window take, so the mere arrival of one of these
 * is what puts the pill into the `transcribing` phase.
 */
interface ProgressNotice { chunk?: number; of?: number; words_so_far?: number }

/**
 * Y5-C — the slice of `build_status` (lib.rs:873) the phase reducer reads. Every
 * field already exists on the event the pill has always listened to; the phase
 * is DERIVED from them, never announced by a new one.
 */
interface BackendStatus {
  recording?: boolean;
  busy?: boolean;
  engine_loading?: boolean;
  last_error?: string | null;
  /** yap-hotkey-stop — the live take is in its "Still recording" state. */
  stillRecording?: boolean;
}

/** The `transcript` event, for its word count only (lib.rs:2509). */
interface TranscriptNotice { wordCount?: number; word_count?: number }

/**
 * yap24-UI3 — DEV-ONLY: `float.html?phase=…&pill=…&dock=…` pins the pill to one
 * phase so `scripts/pill-matrix.mjs` can photograph every phase x dock x
 * character headlessly. Both operands are compile-time constants that are
 * false in a release build, so rollup removes this branch and `./pill/forced`
 * with it; the released float window ignores its query string entirely.
 */
const FORCED: ForcedPill | null =
  import.meta.env.DEV || __YAP_PILL_MATRIX__ ? forcedPill(window.location.search) : null;
if (import.meta.env.DEV || __YAP_PILL_MATRIX__) {
  // The matrix reads its phase list from HERE — the pill's own phaseVisual
  // table — so a phase a later item adds is photographed automatically.
  (window as unknown as { __YAP_PILL_PHASES__?: unknown }).__YAP_PILL_PHASES__ = matrixPhases();
  if (FORCED?.dock) document.documentElement.dataset.dock = FORCED.dock;
}

const DOCKS = ["bottom", "left", "right"];
const dockOf = (s?: Settings) => {
  const p = s?.pillPosition || "bottom";
  return DOCKS.includes(p) ? p : "bottom";
};

function Float() {
  const [style, setStyle] = useState<string>(FORCED?.style ?? "classic");
  // PERM-C — the permission phase lives HERE, above both pill styles, because it
  // is a property of the app and not of whichever capsule happens to be drawn.
  // Both pills receive it; neither invents it.
  const [gate, setGate] = useState<LivePhase>("idle");
  // Y3-C — decode progress lives HERE for the same reason `gate` does: it is a
  // property of the take, not of whichever capsule happens to be drawn. Both
  // pills receive it; neither invents it, and neither interpolates it.
  const [progress, setProgress] = useState<TranscribeProgress | null>(FORCED?.progress ?? null);
  // Y5-C — the TAKE's phase, beside `gate` and for the same reason: which stage
  // of the pipeline is running is a property of the take, not of whichever
  // capsule is drawn. `gate` outranks it (see PHASE_PRECEDENCE); the two are
  // combined ONCE, below, with `winningPhase`.
  const [take, setTake] = useState<LivePhase>("idle");
  // The sentence `error` shows. Straight off the backend's own `last_error`
  // (lib.rs:1460 / 2497) — never a code, and never invented here.
  const [lastError, setLastError] = useState<string | null>(null);
  // yap-hotkey-stop — the take the hotkey may have lost track of says so.
  const [stillRecording, setStillRecording] = useState(false);
  const [tone, setTone] = useState<string>("friendly");
  // Y2-A — the license lives HERE for the same reason `gate` and `progress` do:
  // it is a property of the app, not of whichever capsule happens to be drawn.
  // `null` until the first payload lands, which is silence and not an
  // assumption (see `pillLicense`).
  //
  // EVENT-DRIVEN, NOT POLLED. One read at mount for the value that already
  // exists, then the backend's own emissions: `license` on every change
  // (activation, removal, a revocation refresh — lib.rs:4464/4511/4524) and
  // `license_required` from the dictation gate itself (lib.rs:1232), which is
  // the only way the hotkey and pill paths can say why a press did nothing.
  // There is no interval here and there is no second clock: `days_left` arrives
  // already computed, rollback floor included.
  const [license, setLicense] = useState<LicenseStatus | null>(null);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    invoke<LicenseStatus>("license_status")
      .then((s) => { if (!dead) setLicense(s); })
      .catch(() => {
        /* a licensing read that fails must never stop the pill from drawing */
      });
    listen<LicenseStatus>("license", (e) => setLicense(e.payload)).then(push);
    // Y2-C — `license_required` is not just a payload, it is a REFUSED PRESS.
    // Before this, the only thing it moved was the license state, so past the
    // trial the user held the hotkey and the pill did not move — indistinguishable
    // from a broken app. It now also drives the pill's phase, and settles itself
    // back out again so the refusal is answered every press without becoming
    // permanent noise. Fire-and-forget like Y3-D's cancel timer: `gate_settled`
    // only ever moves the phase it owns, so a press that has already moved the
    // pill on is left alone.
    listen<LicenseStatus>("license_required", (e) => {
      setLicense(e.payload);
      setGate((p) => reduceGatePhase(p, { type: "license_required" }));
      setTimeout(
        () => setGate((p) => reduceGatePhase(p, { type: "gate_settled" })),
        GATED_SETTLE_MS,
      );
    }).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<ProgressNotice>("transcribe_progress", (e) =>
      setProgress((p) =>
        acceptProgress(p, {
          chunk: Number(e.payload?.chunk),
          of: Number(e.payload?.of),
          wordsSoFar: Number(e.payload?.words_so_far),
        }),
      ),
    ).then(push);
    // A NEW take wipes the last take's progress, and a finished transcript ends
    // the phase. Without both, a stale "9/12" outlives the decode that made it.
    listen<boolean>("recording", (e) => { if (e.payload) setProgress(null); }).then(push);
    listen<unknown>("transcript", () => setProgress(null)).then(push);
    listen<unknown>("transcript_error", () => setProgress(null)).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    // A press was refused for the microphone. The pill must SHOW it — an
    // invisible refusal is the bug PERM-C exists to fix.
    listen<MicGateNotice>("mic_permission_required", (e) =>
      setGate((p) =>
        reduceGatePhase(p, {
          type: "mic_permission_required",
          status: e.payload?.status ?? "denied",
          prompting: e.payload?.prompting,
        }),
      ),
    ).then(push);
    // TCC answered (the non-blocking request, or the Permissions pane polling).
    listen<string>("microphone-status", (e) =>
      setGate((p) => reduceGatePhase(p, { type: "microphone-status", status: e.payload })),
    ).then(push);
    // A take starting or stopping. Never clears a permission phase — see
    // `reduceGatePhase`.
    listen<boolean>("recording", (e) =>
      setGate((p) => reduceGatePhase(p, { type: "recording", recording: e.payload })),
    ).then(push);

    // Y3-D — the user cancelled the take. The pill acknowledges it and SETTLES
    // back to idle on its own (`cancel_settled`); it never paints an error,
    // because nothing went wrong. The timer is fire-and-forget on purpose: if a
    // new take has already started by the time it fires, `cancel_settled` only
    // ever moves the phase it owns and leaves the new take's alone.
    listen("take_cancelled", () => {
      setGate((p) => reduceGatePhase(p, { type: "take_cancelled" }));
      setTimeout(
        () => setGate((p) => reduceGatePhase(p, { type: "cancel_settled" })),
        CANCELLED_SETTLE_MS,
      );
    }).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // Y5-C — the take's phase, folded out of events the backend ALREADY emits.
  // No new Rust and no new channel: `status` carries `recording`, `busy`,
  // `engine_loading` and `last_error` (build_status, lib.rs:873), `transcript`
  // fires only for a take that produced text (lib.rs:2509), and its ABSENCE is
  // what makes YV16's no-speech exit visible as `empty`.
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    const onStatus = (st?: BackendStatus) => {
      if (!st) return;
      setLastError(st.last_error ?? null);
      setStillRecording(!!st.recording && !!st.stillRecording);
      const status: TakeStatus = {
        recording: !!st.recording,
        busy: !!st.busy,
        engineLoading: !!st.engine_loading,
        lastError: st.last_error ?? null,
      };
      setTake((prev) => reduceTakePhase(prev, { type: "status", status }));
    };
    invoke<BackendStatus>("get_status").then(onStatus).catch(() => {});
    listen<BackendStatus>("status", (e) => onStatus(e.payload)).then(push);
    listen<TranscriptNotice>("transcript", (e) =>
      setTake((prev) => reduceTakePhase(prev, {
        type: "transcript",
        words: e.payload?.wordCount ?? e.payload?.word_count ?? 1,
      }))).then(push);
    listen<unknown>("transcribe_progress", () =>
      setTake((prev) => reduceTakePhase(prev, { type: "progress" }))).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // Y5-C — the DURATION POLICY, armed here because a pure module owns no
  // timers. Every phase but the five that legitimately wait on the user or the
  // OS has a deadline (`PHASE_HOLD_MS`), and the reducer refuses a stale timer
  // that fires after the phase has already moved on.
  useEffect(() => {
    const hold = PHASE_HOLD_MS[take];
    if (hold === null) return;
    const t = window.setTimeout(
      () => setTake((prev) => reduceTakePhase(prev, { type: "hold_elapsed", phase: take })),
      hold,
    );
    return () => clearTimeout(t);
  }, [take]);
  useEffect(() => {
    // A synchronous cleanup can run before the listen() promise resolves
    // (StrictMode double-mount). A `dead` flag unsubscribes a listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    // The dock edge rides on <html> (not React state) — it only drives CSS
    // alignment, so flipping it must not re-render (and remount) the pill.
    const apply = (s?: Settings) => {
      setStyle(FORCED?.style ?? (s?.pillStyle || "classic"));
      document.documentElement.dataset.dock = FORCED?.dock ?? dockOf(s);
      // Y5-C — the tone decides WHICH line every phase says, so the shell has
      // to know it: the phase strip is drawn once, above both capsules.
      setTone(s?.companionTone || "friendly");
    };
    invoke<Settings>("get_settings").then(apply).catch(() => {});
    listen<Settings>("settings", (e) => apply(e.payload)).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // Y2-A — the POLICY is decided once, here, above both capsules. Y2-B/C/D
  // render `lic`; neither pill re-decides whether the trial is worth
  // mentioning.
  const lic = pillLicense(license);
  // The tone rides on <html> the same way the dock edge does (and for the same
  // reason: it drives CSS and must not remount the pill). It is also what makes
  // this wiring observable in a running app before any capsule draws a chip.
  // Y2-F — the ACTION rides alongside the tone, and for a second reason: it is
  // the one bit that says whether a purchase surface may be reached at all. It
  // is written here, ABOVE the style switch, so `problem` is distinct from
  // `ended` in BOTH capsules without either face deciding anything.
  useEffect(() => {
    document.documentElement.dataset.license = lic.show ? lic.tone : "";
    document.documentElement.dataset.licenseAction = lic.show ? lic.action : "";
  }, [lic.show, lic.tone, lic.action]);
  // Y5-C — ONE phase, decided once. `gate` (permission/license) outranks the
  // take, exactly as PHASE_PRECEDENCE says and exactly as `start_recording`
  // does in the backend: the pill is never allowed to disagree about the reason
  // a press did nothing.
  // yap24-UI3 — a forced phase (dev-only, see FORCED) replaces the derived one
  // wholesale; every consumer below reads `phase`, so nothing else changes.
  const phase = FORCED ? FORCED.phase : winningPhase(gate, take);
  const chatTone = toChatTone(tone);
  useEffect(() => {
    // The phase rides on <html> beside the dock and the license, for the same
    // reason: it drives CSS and must not remount the capsule. It is also the
    // only handle a screenshot or a browser test has on "which state is this".
    document.documentElement.dataset.phase = PHASE_ID[phase];
    document.documentElement.dataset.tone = chatTone;
  }, [phase, chatTone]);
  useEffect(() => {
    // yap-hotkey-stop DO 4 — `.phase-strip` is opacity 0 in `listening`, the
    // only phase a still-recording prompt can occur in; this attribute is the
    // CSS hook that shows it (float.css `html[data-still-recording="true"]`).
    document.documentElement.dataset.stillRecording = stillRecording ? "true" : "false";
  }, [stillRecording]);
  // yap24-NT7 — the meeting lives HERE too, for the reason `gate` does: after
  // stop it is a property of the meeting pipeline, not of whichever capsule is
  // drawn (Y5-K owner decision: the shell owns phases, a character only owns
  // art). Ranked ONCE against the take: a dictation on screen outranks the
  // post-stop badge, so `meetingAfter` is null while one is.
  const meeting = useMeetingPill();
  const meetingAfter = meetingAfterView(meeting.phase, phase);
  const dismissMeeting = meeting.dismiss;
  const onMeetingOpen = useCallback(
    (id: string) => {
      openMeetingNotes(id).catch((e) => console.error(e));
      dismissMeeting(id);
    },
    [dismissMeeting],
  );
  useEffect(() => {
    // The same handle `data-phase` gives a screenshot or a browser test.
    document.documentElement.dataset.meetingPhase = meetingAfter?.kind ?? (meeting.status.recording ? "recording" : "");
  }, [meetingAfter?.kind, meeting.status.recording]);
  const pillProps = { gate: phase, progress, license: lic, meeting: meeting.status, meetingAfter, onMeetingOpen };
  const pill = style === "yappy" ? <YappyPill {...pillProps} /> : <ClassicPill {...pillProps} />;
  // Y5-C — the phase line is placed ONCE, by the SHELL, at all three docks
  // (OWNER DECISION 2026-09-13). It is character-agnostic TEXT; what is
  // per-character is the ART, and that stays inside each capsule. Placed by
  // `.phase-strip` in float.css off `<html data-dock>`, so "bottom, left,
  // right" is three CSS rules and not three components — and not two copies.
  //
  // `error` is the one phase whose line is not a preset: it says WHAT went
  // wrong, in one line, off the take's own `last_error`.
  const line = (phase === "error" ? errorSentence(lastError) : null)
    ?? phaseCopy(phase, chatTone);
  return (
    <>
      {pill}
      <CopyCard />
      <div
        className="phase-strip"
        data-phase={PHASE_ID[phase]}
        data-tone={chatTone}
        role="status"
        aria-live="polite"
        aria-label={phaseVisual(phase).label}
      >
        {stripLine(line, stillRecording)}
      </div>
    </>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
    {TEST_CHIP && (
      <div className="float-test-chip" role="note" title="Local test build — not a release">
        {TEST_CHIP}
      </div>
    )}
  </React.StrictMode>,
);
