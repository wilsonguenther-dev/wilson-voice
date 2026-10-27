import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  ModelPicker,
  ModelRibbon,
  fmtSize,
  PolishModelPicker,
  useModelSetup,
  usePolishModel,
} from "./ModelSetup";
import { describePolishOffer } from "./polishOffer";
import { adviceFor, errorText } from "./errors";
import { awaitMicDecision } from "./micStatus";
import { PermissionRepair } from "./PermissionRepairRow";
import type { ProbeReport } from "./permissionRepair";
import {
  deriveMicStatus,
  micIsUsable,
  nameProduct,
  permissionAction,
  permissionCopy,
  type MicPermissionStatus,
  type PermissionGrantRow,
} from "./permission";
import {
  STEP_ORDER,
  classifyTryItFailure,
  completionFor,
  nextStep,
  progressStep,
  tryItSucceeded,
  type Outcome,
  type Step,
} from "./onboarding";

// YV9 — first-run onboarding. Rendered as a full-screen overlay over the main
// app while AppSettings.onboarded is false. Self-contained: it invokes the
// existing permission + recording commands directly and does its own live
// polling / event wiring so App.tsx stays a thin gate.
//
// YV54 — the app still ships with NO model, but the user is never ASKED about
// it. YV33's model-choice step (read two catalog blurbs, press Download, wait
// for it) is gone: `useModelSetup({ autoDownload: true })` starts the catalog's
// recommended download the moment this overlay mounts, in the background, while
// the user grants permissions and calibrates. The only trace of it is the slim
// ribbon in the card footer. The picker moved to Settings → Advanced for the
// power users who do want to choose.
//
// SEC-C — the polish (LLM formatting) model got a real install path, but
// nothing in first run mentioned it, so a new user finished onboarding without
// ever learning the stage exists. The last step now says so — ONCE, at the end,
// after the speech model is settled, behind a button nobody presses for them.
// `describePolishOffer` owns that decision and is tested on its own; the
// `PolishOffer` component below mounts only on the "done" step, so the catalog
// read it performs never touches the welcome/permissions path at all.

// Y6-A — the steps, their order and the routing between them live in
// `onboarding.ts` (`nextStep`), tested on their own. The last real step is
// TRY IT: one dictation pasted into a field inside THIS window — never into
// another app (YV21) — before "done" means anything.

interface PermissionReport {
  /** fix-ax-probe-refresh — "Yap" / "Yap Test". */
  productName?: string;
  accessibility: boolean;
  microphone: boolean;
  /**
   * PERM-A — the real `AVCaptureDevice.authorizationStatus`, see App.tsx.
   * PERM-B renders it: `deriveMicStatus` prefers this field and only falls back
   * to the boolean when it is absent.
   */
  microphoneStatus: string;
  /** PERM-E — the four grants, tri-state. PANEL (4): these ARE this step's
   *  checklist rows, built once in `permissions.rs`, so the onboarding list and
   *  the main window's health row can never disagree. */
  grants: PermissionGrantRow[];
  /** yap-p0-perm-repair — the same live probe the main-window banner reads. */
  probe?: ProbeReport;
  asrOk: boolean;
  asrDetail: string;
  summary: string;
  allCriticalOk: boolean;
}

interface TranscriptEntry {
  id: string;
  text: string;
  wordCount: number;
}

const CALIBRATION_PHRASE =
  "The quick brown fox jumps over the lazy dog, and Yap is listening.";

const NEXT: Outcome = { kind: "next" };
const BACK: Outcome = { kind: "back" };
const SKIP: Outcome = { kind: "skip" };
const RETRY: Outcome = { kind: "retry" };
const SUCCESS: Outcome = { kind: "success" };

/**
 * Y6-A — how long after the `transcript` event the try-it field may take to
 * show the pasted text. The backend posts ⌘V BEFORE it emits the transcript,
 * so the text is normally already there; this covers the webview applying
 * the paste a beat later. Past it, the paste did not land.
 */
const PASTE_LAND_MS = 1500;

/**
 * Last-resort ceiling on one calibration take. YV79 made every terminal take
 * outcome announce itself (`take_done`), so this no longer covers any outcome
 * the backend can name — those now clear `busy` in about a second, with their
 * reason. What is left is a take that never reports at all: a killed worker, a
 * lost event, a backend that died mid-take. 90 s of that was indistinguishable
 * from a hang; 30 s still leaves generous room above a real take (transcribe +
 * paste is ~1 s on Metal) while the user finds out while they still care.
 */
const CALIBRATION_WATCHDOG_MS = 30_000;

/**
 * How often the permissions checklist re-reads the TCC grants while that step
 * is open and the window is visible (YV81). The user is over in System Settings
 * flipping a toggle and glancing back: 2s is under the time that glance takes,
 * and it is the ONLY recurring timer the frontend owns — everything else here
 * is one-shot or event-driven.
 */
const PERMISSION_POLL_MS = 2000;

function StatusDot({ ok }: { ok: boolean }) {
  return <span className={ok ? "dot-ok" : "dot-bad"} aria-hidden />;
}

/**
 * SEC-C — the optional formatting model, offered at the end of first run.
 *
 * Mounted ONLY from the "done" step. `usePolishModel` has no `autoDownload`
 * analogue by construction (see ModelSetup.tsx), so mounting this reads the
 * catalog and nothing else: not one byte of the 1.1 GB GGUF moves until the
 * user presses the button inside `PolishModelPicker`.
 */
function PolishOffer({ speechModelReady }: { speechModelReady: boolean }) {
  const polish = usePolishModel();
  const offer = describePolishOffer({
    speechModelReady,
    catalogSize: polish.models.length,
    polishActive: polish.active,
    polishDownloading: polish.downloading,
  });
  if (offer.kind === "hidden") return null;
  return (
    <div className="onboard-polish">
      <h2>{offer.title}</h2>
      <p className="muted">{offer.body}</p>
      {offer.showPicker && <PolishModelPicker polish={polish} />}
    </div>
  );
}

export default function Onboarding({
  onFinish,
  onSkip,
  autoPaste,
  onEnableAutoPaste,
}: {
  /**
   * Finish onboarding; sample is the captured calibration phrase (or null).
   * Y6-A — `proven` is true only when a try-it take actually pasted into the
   * field; an explicit skip finishes with `false`, which keeps the main
   * window's health row raised until a real take pastes.
   */
  onFinish: (sample: string | null, proven: boolean) => void;
  /** "Skip for now" on the welcome screen — finishes unproven. */
  onSkip: () => void;
  /**
   * Fix round 1 — the user's "Paste my words automatically" setting. Off, no
   * take can paste (backend `paste::should_paste`), so try-it names the
   * setting instead of blaming the cursor, and never declares success.
   */
  autoPaste: boolean;
  /** Turn auto-paste on from the try-it screen that names it as the cause. */
  onEnableAutoPaste: () => Promise<void> | void;
}) {
  const [step, setStep] = useState<Step>("welcome");
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  /**
   * PERM-B — whether Yap has invoked `request_microphone` in this session. It
   * is the ONLY thing that separates "has not been asked" from "said no" while
   * the backend still answers with a boolean, and getting it wrong shows a
   * first-run user the denial screen for a question nobody put to them.
   */
  const [micAsked, setMicAsked] = useState(false);
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sample, setSample] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  // Calibration failure (backend `transcript_error`, or the watchdog below).
  // Shown inline with a Retry button so a failed take is a dead end no longer.
  const [calibError, setCalibError] = useState<string | null>(null);
  // Y6-A — the try-it field, what Yap heard, and whether a take has landed.
  const [tryText, setTryText] = useState("");
  const [heard, setHeard] = useState<string | null>(null);
  const [tryError, setTryError] = useState<string | null>(null);
  const [proven, setProven] = useState(false);
  const fieldRef = useRef<HTMLTextAreaElement | null>(null);
  /**
   * Fix round 1 — everything known about the CURRENT try-it take. `baseline`
   * is what the field held when the take started (text already there proves
   * nothing); `transcript` is null until this take produced words;
   * `pasteFailed` is the backend's own "no receipt" (`paste_failed`);
   * `pasted` is the backend's own paste receipt for this take, carried on
   * `take_done` (fix round 2) — a hand ⌘V of the clipboard never sets it;
   * `doneOk` is its terminal `take_done{ok:true}`, which arrives AFTER
   * `transcript` and `paste_failed`, so success is decided only once the
   * backend has said everything it will say about the paste; `settled` stops
   * a take being judged twice.
   */
  const takeRef = useRef<{
    baseline: string;
    transcript: string | null;
    pasteFailed: boolean;
    pasted: boolean;
    doneOk: boolean;
    settled: boolean;
  }>({
    baseline: "",
    transcript: null,
    pasteFailed: false,
    pasted: false,
    doneOk: false,
    settled: false,
  });
  const landTimerRef = useRef(0);
  // YV54 — silent setup: this overlay is the surface that starts the
  // recommended download, on mount, without asking. Nothing here blocks on it
  // except the calibration take, which needs an engine to transcribe with.
  const modelSetup = useModelSetup({ autoDownload: true });

  const refreshPerms = useCallback(async () => {
    try {
      setPerms(await invoke<PermissionReport>("get_permissions"));
    } catch (e) {
      console.error(e);
    }
  }, []);

  // Live grant status: poll while on the permissions step so the checklist
  // updates the moment the user flips a toggle in System Settings. macOS
  // publishes no notification for a TCC grant, so a poll is the only reading
  // there is — but it is bounded twice over (YV81): it runs on THIS step only,
  // and it stops while the window is hidden, because the checklist nobody is
  // looking at has nothing to update. Coming back re-reads immediately, so the
  // 2s interval is never what the user waits on after granting.
  useEffect(() => {
    // Y6-A — the no-mic and no-Accessibility screens are the same errand
    // (flip a toggle in System Settings, come back), so they poll the same way.
    if (
      step !== "permissions" &&
      step !== "try-it-no-mic" &&
      step !== "try-it-no-accessibility"
    )
      return;
    let t = 0;
    const stop = () => { if (t) { clearInterval(t); t = 0; } };
    const start = () => { if (!t) t = window.setInterval(refreshPerms, PERMISSION_POLL_MS); };
    const onVisibility = () => {
      stop();
      if (document.hidden) return;
      refreshPerms();               // whatever changed while we were away
      start();
    };
    // PERM-B — returning from System Settings is an EVENT, not a tick. The 2s
    // poll above still runs (it catches a toggle flipped while both windows are
    // visible side by side), but the common path is: press "Open System
    // Settings", flip the toggle, click back onto Yap. Re-reading on focus
    // makes that click the moment the row updates, instead of up to 2s later —
    // which is exactly the interval in which a user concludes it did not work.
    const onFocus = () => { refreshPerms(); };
    onVisibility();
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("focus", onFocus);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", onFocus);
    };
  }, [step, refreshPerms]);

  // Reflect recording state + capture the calibration transcript. Both the main
  // app and this overlay listen; the duplicate listener is harmless.
  const stepRef = useRef(step);
  useEffect(() => {
    stepRef.current = step;
  }, [step]);

  useEffect(() => {
    // A synchronous cleanup can run before these listen() promises resolve
    // (StrictMode double-mount). A `dead` flag unsubscribes any listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    listen<boolean>("recording", (e) => {
      setRecording(e.payload);
      // Fix round 1 — a new try-it take starts: snapshot the field and forget
      // the previous take's words, so nothing stale is shown or judged.
      if (e.payload && stepRef.current === "try-it") tryItRef.current.onStart();
      // Y6-A — a try-it take is driven by the hotkey, not a button here, so
      // the end of the recording is what arms the "waiting for it" state.
      if (!e.payload && stepRef.current === "try-it") setBusy(true);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<TranscriptEntry>("transcript", (e) => {
      if (stepRef.current === "calibration") {
        setSample(e.payload.text);
        setBusy(false);
        setCalibError(null);
        setNote("Voice sample captured — Yap will use it to personalize.");
      } else if (stepRef.current === "try-it") {
        tryItRef.current.onTranscript(e.payload.text);
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // Y6-A — the ⌘V went out and nothing took it: the try-it take failed at
    // the very last stage, and the field will never fill. Name it now rather
    // than waiting out PASTE_LAND_MS.
    listen<string>("paste_failed", () => {
      if (stepRef.current === "try-it") tryItRef.current.onPasteFailed();
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV33 — the other half of "busy": the backend emits this whenever a take
    // fails to produce a transcript, so the failure clears the spinner and shows
    // its reason instead of the overlay waiting on a success that never comes.
    listen<{ message: string }>("transcript_error", (e) => {
      setBusy(false);
      if (stepRef.current === "try-it") {
        tryItRef.current.onFail(e.payload?.message || null);
        return;
      }
      setCalibError(e.payload?.message || "Transcription failed.");
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV79 — the terminal marker for a take, whatever its outcome. The three
    // listeners above still miss the SOFT ones, and the first-run user with no
    // mic permission / the wrong input device hits the loudest of them: the
    // no-speech gate, which only ever spoke on the toast channel behind this
    // overlay. Busy now clears for EVERY take, with the backend's own reason.
    listen<{ ok: boolean; message: string | null; pasted?: boolean }>("take_done", (e) => {
      setBusy(false);
      if (stepRef.current === "try-it") {
        tryItRef.current.onDone(
          !!e.payload?.ok,
          e.payload?.message || null,
          e.payload?.pasted === true,
        );
        return;
      }
      if (!e.payload?.ok) {
        setCalibError(
          e.payload?.message || "That take produced nothing. Retry.",
        );
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, []);

  // Watchdog: busy is never clearable by success alone. Since YV79 every take
  // that ENDS says so on `take_done`, so reaching this means the take never
  // reported — a cause the app cannot name, hence the generic line. Guessing at
  // one (it used to blame the speech model) sent users off fixing the wrong
  // thing on takes that had failed for another reason entirely.
  useEffect(() => {
    if (!busy) return;
    const t = setTimeout(() => {
      setBusy(false);
      if (stepRef.current === "try-it") {
        tryItRef.current.onFail("Yap didn't hear back from the recorder.");
        return;
      }
      setCalibError("Yap didn't hear back from the recorder. Retry.");
    }, CALIBRATION_WATCHDOG_MS);
    return () => clearTimeout(t);
  }, [busy]);

  // Re-read the catalog when the recap opens so the summary reflects what is
  // actually on disk by then (the download may have landed mid-flow).
  const refreshModels = modelSetup.refresh;
  useEffect(() => {
    if (step !== "done") return;
    refreshModels();
  }, [step, refreshModels]);

  async function requestMic() {
    // PERM-A: non-blocking request + authoritative read-back. A denial comes
    // back instantly with no dialog, so waiting a fixed 900 ms was both wrong
    // and slow; a first-run grant can take a human several seconds.
    //
    // PERM-B records the ask BEFORE the await as a FALLBACK only: it is what
    // separates "never asked" from "said no" on any build where
    // `microphoneStatus` is missing. When PERM-A's field is present it wins.
    setMicAsked(true);
    try {
      const status = await awaitMicDecision({
        request: () => invoke("request_microphone"),
        read: () => invoke("microphone_status"),
        sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      });
      // PERM-B: `status` is not toasted here any more. The microphone row
      // below now OWNS the denied and restricted cases — full copy plus the
      // one action that can actually help (System Settings for denied, nothing
      // at all for restricted) — and a toast repeating it in different words
      // is the same news twice. Clear any stale note instead.
      if (status === "denied" || status === "restricted") setNote(null);
    } catch (e) {
      setNote(adviceFor(e).line);
    }
    refreshPerms();
  }

  /**
   * PERM-B — the deep link lives in Rust (`permissions.rs::open_privacy_pane`),
   * verified against a real OS build. The frontend passes a PANE NAME and never
   * a URL, the same way `App.tsx` already does.
   */
  async function openMicSettings(pane: "Microphone") {
    try {
      await invoke("open_privacy_settings", { pane });
    } catch (e) {
      setNote(errorText(e));
    }
  }

  /**
   * Honest exit from a mic Yap cannot use. Y6-A: this is a SKIP — it ends on
   * "done" unproven, and `Finish setup` then saves `dictationProven: false`.
   */
  function continueWithoutDictation() {
    go(SKIP);
  }

  /** Every step change goes through the tested machine in `onboarding.ts`. */
  function go(outcome: Outcome) {
    if (outcome.kind === "success") setProven(true);
    if (outcome.kind === "skip") setProven(false);
    setStep((s) => nextStep(s, outcome));
  }

  async function openPrivacyPane(pane: "Accessibility") {
    try {
      await invoke("open_privacy_settings", { pane });
    } catch (e) {
      setNote(errorText(e));
    }
  }

  // --- Y6-A try-it ----------------------------------------------------------

  function clearLandTimer() {
    if (landTimerRef.current) {
      clearTimeout(landTimerRef.current);
      landTimerRef.current = 0;
    }
  }

  /** Fix round 1 — a fresh take: snapshot the field, forget the last take. */
  function startTake() {
    clearLandTimer();
    takeRef.current = {
      baseline: fieldRef.current?.value ?? "",
      transcript: null,
      pasteFailed: false,
      pasted: false,
      doneOk: false,
      settled: false,
    };
    setHeard(null);
    setTryError(null);
  }

  /**
   * Did THIS take's transcript arrive in the field? Settles success only when
   * the backend has finished the take (`take_done{ok:true}`) WITH its own
   * paste receipt (`pasted`, fix round 2), sent no
   * `paste_failed`, auto-paste is on, and the transcript occurs in the field
   * more often than it did when the take started.
   */
  function checkLanded(value: string): boolean {
    const take = takeRef.current;
    if (take.settled) return false;
    if (!tryItSucceeded(take, autoPaste, value, take.baseline)) return false;
    take.settled = true;
    clearLandTimer();
    setBusy(false);
    setTryError(null);
    go(SUCCESS);
    return true;
  }

  /**
   * Name the stage that broke the take from a FRESH permission read (PERM-E's
   * grant rows can have moved since the checklist), then route to its screen.
   */
  async function failTryIt(message: string | null) {
    const take = takeRef.current;
    if (take.settled) return;
    take.settled = true;
    clearLandTimer();
    setBusy(false);
    let report = perms;
    try {
      report = await invoke<PermissionReport>("get_permissions");
      setPerms(report);
    } catch (e) {
      console.error(e);
    }
    const mic = deriveMicStatus({
      microphoneStatus: report?.microphoneStatus,
      microphone: !!report?.microphone,
      asked: micAsked,
    });
    setTryError(message);
    go({
      kind: "failure",
      failure: classifyTryItFailure({
        micAuthorized: mic === "authorized",
        modelReady: modelSetup.ready,
        heardText: !!take.transcript,
        autoPaste,
        accessibility: !!report?.accessibility,
      }),
    });
  }

  /** The take produced words. Recorded only; `take_done` decides the outcome. */
  function onTryItTranscript(text: string) {
    const take = takeRef.current;
    if (take.settled) return;
    take.transcript = text;
    setHeard(text);
  }

  /** The backend sent ⌘V and got no receipt: the paste stage failed. */
  function onTryItPasteFailed() {
    takeRef.current.pasteFailed = true;
    failTryIt(null);
  }

  /**
   * The backend's terminal marker for the take. Not ok: the take produced no
   * dictation (no speech, cancel, rejected, silent) — fail with its reason.
   * Ok: give the webview PASTE_LAND_MS to apply the ⌘V, then judge.
   */
  function onTryItDone(ok: boolean, message: string | null, pasted: boolean) {
    const take = takeRef.current;
    if (take.settled) return;
    if (!ok) {
      failTryIt(message);
      return;
    }
    take.doneOk = true;
    take.pasted = pasted;
    // Fix round 2 — no paste receipt for this take (the paste was never
    // attempted, or it got no receipt): nothing the field shows from here on
    // can be this take's paste, only the user's own ⌘V. Fail now, closed.
    if (!autoPaste || !take.transcript || !pasted) {
      // Copy-only by the user's own setting (or a take with nothing to
      // paste): no paste can be proven, so name the cause now.
      failTryIt(null);
      return;
    }
    if (checkLanded(fieldRef.current?.value ?? "")) return;
    clearLandTimer();
    landTimerRef.current = window.setTimeout(() => {
      landTimerRef.current = 0;
      if (!checkLanded(fieldRef.current?.value ?? "")) failTryIt(null);
    }, PASTE_LAND_MS);
  }

  // The event listeners are registered once; they reach the CURRENT handlers
  // (fresh perms, fresh model state, fresh autoPaste) through this ref.
  const tryItRef = useRef({
    onStart: startTake,
    onTranscript: onTryItTranscript,
    onPasteFailed: onTryItPasteFailed,
    onDone: onTryItDone,
    onFail: failTryIt,
  });
  tryItRef.current = {
    onStart: startTake,
    onTranscript: onTryItTranscript,
    onPasteFailed: onTryItPasteFailed,
    onDone: onTryItDone,
    onFail: failTryIt,
  };

  useEffect(() => () => clearLandTimer(), []);

  // Land in the field on arrival, so the take has somewhere to paste. Fix
  // round 1: arriving (or retrying) also opens a fresh take, so a take that
  // never announced its start is still judged against the field as it is now.
  useEffect(() => {
    if (step !== "try-it") return;
    fieldRef.current?.focus();
    tryItRef.current.onStart();
  }, [step]);

  /**
   * The on-screen alternative to the hotkey. `onMouseDown` keeps focus IN the
   * field, which is where the backend's ⌘V lands — a click that moved focus
   * to this button would leave nothing focused to paste into.
   */
  async function toggleTryIt() {
    try {
      setTryError(null);
      await invoke("manual_toggle");
    } catch (e) {
      failTryIt(errorText(e));
    }
  }

  async function enableAutoPaste() {
    try {
      await onEnableAutoPaste();
      go(RETRY);
    } catch (e) {
      setNote(errorText(e));
    }
  }

  async function copyHeard() {
    if (!heard) return;
    try {
      await invoke("copy_entry", { text: heard });
      setNote("Copied — press ⌘V wherever you want it.");
    } catch (e) {
      setNote(errorText(e));
    }
  }

  async function requestAccessibility() {
    try {
      await invoke("request_accessibility");
    } catch (e) {
      setNote(adviceFor(e).line);
    }
    setTimeout(refreshPerms, 800);
  }

  // Calibration uses the existing manual_toggle recording command: first tap
  // starts capture, second tap stops + transcribes. The transcript event above
  // carries the captured phrase back.
  async function toggleCalibration() {
    try {
      if (recording) setBusy(true);
      setCalibError(null);
      await invoke("manual_toggle");
    } catch (e) {
      setBusy(false);
      setCalibError(errorText(e));
    }
  }

  function goNext() {
    go(NEXT);
  }

  function goBack() {
    go(BACK);
  }

  const micStatus: MicPermissionStatus = deriveMicStatus({
    microphoneStatus: perms?.microphoneStatus,
    microphone: !!perms?.microphone,
    asked: micAsked,
  });
  const micOk = micIsUsable(micStatus);
  const micCopy = permissionCopy(micStatus, perms?.productName);
  const micAction = permissionAction(micStatus);
  const axOk = !!perms?.accessibility;
  const modelReady = modelSetup.ready;
  /** Calibration can only run once an engine exists — until then it waits. */
  const engineWaiting = !modelReady && !recording;
  /**
   * PERM-D — calibration RECORDS A REAL TAKE, so it cannot start before the
   * microphone grant is real. `micStatus !== "authorized"` is the only reading
   * that means "macOS will hand us audio": `not_determined` is a dialog nobody
   * has answered, `denied` and `restricted` are refusals, and a stream opened
   * on any of them delivers digital silence. The take that follows can only be
   * silent, the app's own no-speech gate then correctly refuses to paste it,
   * and the user's first experience of Yap is a recording that does nothing and
   * says nothing about permissions.
   *
   * It wears the SAME waiting-ribbon pattern the step already uses for a
   * downloading model (YV54): the button waits, wearing its reason, instead of
   * starting a take that could only fail. A recording in progress is never
   * interrupted by this — stopping it is how the take ends.
   */
  const micWaiting = micStatus !== "authorized" && !recording;
  /** The one line the record button wears while it waits, reason named. */
  const calibrationBlockedReason =
    micStatus === "denied"
      ? "Microphone blocked — grant it above to calibrate"
      : micStatus === "restricted"
        ? "Microphone restricted by this Mac's policy"
        : "Grant the microphone above to calibrate";

  return (
    <div className="onboard-overlay" role="dialog" aria-modal="true">
      <div className="onboard-card">
        <div className="onboard-progress" aria-hidden>
          {STEP_ORDER.map((s) => (
            <span
              key={s}
              className={
                s === progressStep(step)
                  ? "onboard-dot active"
                  : STEP_ORDER.indexOf(s) < STEP_ORDER.indexOf(progressStep(step))
                    ? "onboard-dot done"
                    : "onboard-dot"
              }
            />
          ))}
        </div>

        {step === "welcome" && (
          <div className="onboard-step">
            <h1>Welcome to Yap</h1>
            <p className="onboard-lede">
              Local, private voice-to-text for your Mac. Hold your hotkey, talk,
              and Yap types for you — powered by on-device Whisper. Nothing
              leaves this machine.
            </p>
            {/* yap-uiux-2026-10-16-ONBOARD — the plan as a list, one line per
                step, so the user can see what is coming (and how big the
                background download is) before pressing anything. */}
            <ol className="onboard-plan">
              <li>Grant macOS permissions — each one says why Yap needs it.</li>
              <li>Record a short calibration phrase so Yap learns your voice.</li>
              <li>Try it: dictate once into a field here and watch it paste.</li>
              <li className="muted">
                Meanwhile your speech model
                {modelSetup.target && !modelSetup.ready
                  ? ` (${fmtSize(modelSetup.target.sizeBytes)})`
                  : ""}{" "}
                {modelSetup.ready
                  ? "is ready."
                  : "downloads in the background — progress shows at the foot of this card."}
              </li>
            </ol>
            <div className="onboard-actions">
              <button className="primary" onClick={goNext}>
                Get started
              </button>
              {/* Y6-A — `onSkip` finishes UNPROVEN (`completionFor(skip)`). */}
              <button className="ghost" onClick={onSkip}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "permissions" && (
          <div className="onboard-step">
            <h1>Grant permissions</h1>
            <p className="onboard-lede">
              macOS must allow <strong>{nameProduct("Yap", perms?.productName)}</strong> itself to use these. Status
              updates live as you enable each one.
            </p>
            <ul className="onboard-perms">
              {/* PERM-B — four statuses, four screens. The button is whatever
                  `permissionAction` says it is, including nothing at all: an
                  authorized row is settled, and a restricted row gets NO
                  System Settings button because that toggle is greyed out by
                  an MDM profile and sending the user there is an errand that
                  cannot succeed. */}
              <li className={micOk ? "ok" : "bad"} data-mic-status={micStatus}>
                <StatusDot ok={micOk} />
                <div>
                  {/* ONBOARD — the grant button first, beside the name, and
                      the one line of why after it, on every permission row. */}
                  <div className="onboard-perm-head">
                  <strong>{micCopy.title}</strong>
                  {micAction.kind === "request" && (
                    <button onClick={requestMic}>{micAction.label}</button>
                  )}
                  {micAction.kind === "settings" && (
                    <button
                      className="primary"
                      onClick={() => openMicSettings(micAction.pane)}
                    >
                      {micAction.label}
                    </button>
                  )}
                  </div>
                  <p>{micCopy.body}</p>
                </div>
              </li>
              <li className={axOk ? "ok" : "bad"}>
                <StatusDot ok={axOk} />
                <div>
                  <div className="onboard-perm-head">
                    <strong>Accessibility</strong>
                    <button onClick={requestAccessibility}>
                      {axOk ? "Granted ✓" : "Prompt Accessibility"}
                    </button>
                  </div>
                  <p>{nameProduct("Lets Yap paste text into any app (simulated ⌘V).", perms?.productName)}</p>
                  {/* A prompt macOS already answered does not ask twice: say
                      exactly where the switch is. */}
                  {!axOk && (
                    <p className="muted tiny">
                      {nameProduct(
                        "Already asked? System Settings → Privacy & Security → Accessibility → switch on Yap.",
                        perms?.productName,
                      )}
                    </p>
                  )}
                </div>
              </li>
            </ul>
            {/* yap-p0-perm-repair — a denied or stale grant gets Repair here
                too, from the same probe the main-window banner reads. */}
            <PermissionRepair probe={perms?.probe} onChanged={refreshPerms} productName={perms?.productName} />
            {note && <p className="muted tiny">{note}</p>}
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack}>
                Back
              </button>
              {/* PERM-B — the old escape hatch walked a user past a
                  microphone they do not have into an app whose only feature
                  cannot run, and the bug report that follows says dictation is
                  broken.
                  Without a usable mic the honest move is to name what is being
                  given up and skip calibration, which cannot succeed either. */}
              {micOk ? (
                <button className="primary" onClick={goNext}>
                  Continue
                </button>
              ) : (
                <button className="ghost" onClick={continueWithoutDictation}>
                  Continue without dictation
                </button>
              )}
            </div>
          </div>
        )}

        {step === "calibration" && (
          <div className="onboard-step">
            <h1>Calibrate your voice</h1>
            <p className="onboard-lede">
              Tap record and read this phrase out loud, then tap again to stop.
              Yap keeps the sample locally to personalize your dictation.
            </p>
            <blockquote className="onboard-phrase">
              “{CALIBRATION_PHRASE}”
            </blockquote>
            {/* YV54 — the engine may still be downloading when the user gets
                here. The button WAITS (wearing the ribbon's own line) instead
                of starting a take that could only fail: a take with no model
                is the one failure the app can see coming. Once it lands the
                button arms itself; the watchdog + `transcript_error` below stay
                the failure path for everything the app cannot see coming. */}
            <button
              className={recording ? "onboard-record live" : "onboard-record"}
              onClick={toggleCalibration}
              disabled={busy || micWaiting || engineWaiting}
            >
              {recording
                ? "■ Stop & save"
                : busy
                  ? "Transcribing…"
                  : micWaiting
                    ? calibrationBlockedReason
                    : engineWaiting
                      ? (modelSetup.ribbon ?? "Waiting for the speech model…")
                      : sample
                        ? "● Record again"
                        : "● Start recording"}
            </button>
            {/* PERM-D — the button alone is a dead control with a label; the
                way BACK to the grant is the thing the user needs. Same step,
                one click, no hunting. */}
            {micWaiting && (
              <p className="muted tiny">
                Calibration records a real take, so it needs the microphone.{" "}
                <button className="ghost" onClick={() => setStep("permissions")}>
                  Back to permissions
                </button>
              </p>
            )}
            {sample && (
              <div className="onboard-sample">
                <span className="muted tiny">Heard:</span>
                <p>{sample}</p>
              </div>
            )}
            {calibError && (
              <p className="onboard-error">
                {calibError}{" "}
                <button
                  className="ghost"
                  onClick={() => {
                    setCalibError(null);
                    if (!recording) toggleCalibration();
                  }}
                >
                  Retry
                </button>
              </p>
            )}
            {note && <p className="muted tiny">{note}</p>}
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack}>
                Back
              </button>
              <button className="primary" onClick={goNext} disabled={recording}>
                {sample ? "Continue" : "Skip calibration"}
              </button>
            </div>
          </div>
        )}

        {step === "try-it" && (
          <div className="onboard-step">
            <h1>Try it</h1>
            <p className="onboard-lede">
              Click in the box below, hold <kbd>fn</kbd>+<kbd>⌃</kbd>, say
              anything, and let go. Yap pastes what you said right here — the
              same way it will in every other app.
            </p>
            <textarea
              ref={fieldRef}
              className="onboard-try-field"
              aria-label="Try-it text field"
              placeholder="Your words land here…"
              rows={3}
              value={tryText}
              onChange={(e) => {
                setTryText(e.target.value);
                // Only ever settles once the backend has finished this take.
                checkLanded(e.target.value);
              }}
            />
            {/* YV54 — the SAME waiting affordance calibration uses: the button
                wears the ribbon's line while the engine downloads, instead of
                inviting a take that could only fail. */}
            <button
              className={recording ? "onboard-record live" : "onboard-record"}
              onMouseDown={(e) => e.preventDefault()}
              onClick={toggleTryIt}
              disabled={busy || (!recording && (micWaiting || engineWaiting))}
            >
              {recording
                ? "■ Stop"
                : busy
                  ? "Pasting…"
                  : micWaiting
                    ? calibrationBlockedReason
                    : engineWaiting
                      ? (modelSetup.ribbon ?? "Waiting for the speech model…")
                      : "● Or tap here instead of the hotkey"}
            </button>
            {heard && (
              <div className="onboard-sample">
                <span className="muted tiny">Heard:</span>
                <p>{heard}</p>
              </div>
            )}
            {note && <p className="muted tiny">{note}</p>}
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack} disabled={recording}>
                Back
              </button>
              <button className="ghost" onClick={() => go(SKIP)} disabled={recording}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "try-it-no-mic" && (
          <div className="onboard-step" data-try-it-failure="no-mic">
            <h1>Yap couldn’t hear you</h1>
            <p className="onboard-lede">{micCopy.body}</p>
            {tryError && <p className="onboard-error">{tryError}</p>}
            <div className="onboard-actions">
              {micAction.kind === "request" && (
                <button className="primary" onClick={requestMic}>{micAction.label}</button>
              )}
              {micAction.kind === "settings" && (
                <button className="primary" onClick={() => openMicSettings(micAction.pane)}>
                  {micAction.label}
                </button>
              )}
              <button className="ghost" onClick={() => go(RETRY)} disabled={!micOk}>
                Try again
              </button>
              <button className="ghost" onClick={() => go(SKIP)}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "try-it-no-accessibility" && (
          <div className="onboard-step" data-try-it-failure="no-accessibility">
            <h1>Yap heard you but can’t paste</h1>
            <p className="onboard-lede">
              {nameProduct(
                "Pasting needs the Accessibility permission. Open System Settings → Privacy & Security → Accessibility and switch on Yap, then come back.",
                perms?.productName,
              )}
            </p>
            {heard && (
              <div className="onboard-sample">
                <span className="muted tiny">Heard:</span>
                <p>{heard}</p>
              </div>
            )}
            {tryError && <p className="onboard-error">{tryError}</p>}
            <div className="onboard-actions">
              <button className="primary" onClick={() => openPrivacyPane("Accessibility")}>
                Open Accessibility settings
              </button>
              <button className="ghost" onClick={() => go(RETRY)} disabled={!axOk}>
                Try again
              </button>
              <button className="ghost" onClick={() => go(SKIP)}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "try-it-no-model" && (
          <div className="onboard-step" data-try-it-failure="no-model">
            <h1>The speech model isn’t ready</h1>
            <p className="onboard-lede">
              Yap transcribes on this Mac, so it needs its speech model before
              it can turn your voice into text.
            </p>
            {tryError && <p className="onboard-error">{tryError}</p>}
            <ModelPicker setup={modelSetup} />
            <div className="onboard-actions">
              <button className="primary" onClick={() => go(RETRY)} disabled={!modelReady}>
                Try again
              </button>
              <button className="ghost" onClick={() => go(SKIP)}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "try-it-paste-refused" && (
          <div className="onboard-step" data-try-it-failure="paste-refused">
            <h1>The text didn’t land</h1>
            <p className="onboard-lede">
              Yap pastes into whatever text field has the cursor when you let
              go of the hotkey. This time nothing took the paste — usually
              because the cursor wasn’t in the box. Your words are not lost:
              copy them and paste them yourself, or try again with the cursor
              in the box.
            </p>
            {heard && (
              <div className="onboard-sample">
                <span className="muted tiny">Heard:</span>
                <p>{heard}</p>
              </div>
            )}
            {tryError && <p className="onboard-error">{tryError}</p>}
            {note && <p className="muted tiny">{note}</p>}
            <div className="onboard-actions">
              {heard && (
                <button className="primary" onClick={copyHeard}>
                  Copy to clipboard
                </button>
              )}
              <button className="ghost" onClick={() => go(RETRY)}>
                Try again
              </button>
              <button className="ghost" onClick={() => go(SKIP)}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "try-it-no-speech" && (
          <div className="onboard-step" data-try-it-failure="no-speech">
            <h1>Yap didn’t catch any words</h1>
            <p className="onboard-lede">
              That take ended without any text, so there was nothing to paste.
              Hold the hotkey a moment longer and speak clearly, then let go.
            </p>
            {tryError && <p className="onboard-error">{tryError}</p>}
            <div className="onboard-actions">
              <button className="primary" onClick={() => go(RETRY)}>
                Try again
              </button>
              <button className="ghost" onClick={() => go(SKIP)}>
                Skip for now
              </button>
            </div>
          </div>
        )}

        {step === "try-it-auto-paste-off" && (
          <div className="onboard-step" data-try-it-failure="auto-paste-off">
            <h1>Automatic paste is turned off</h1>
            <p className="onboard-lede">
              Yap heard you and copied your words, but “Paste my words
              automatically” is off in Settings, so it did not paste them.
              Turn it on to have Yap paste for you, or keep it off and paste
              with ⌘V yourself.
            </p>
            {heard && (
              <div className="onboard-sample">
                <span className="muted tiny">Heard:</span>
                <p>{heard}</p>
              </div>
            )}
            {note && <p className="muted tiny">{note}</p>}
            <div className="onboard-actions">
              <button className="primary" onClick={enableAutoPaste}>
                Turn on automatic paste
              </button>
              <button className="ghost" onClick={() => go(SKIP)}>
                Keep it off
              </button>
            </div>
          </div>
        )}

        {step === "done" && (
          <div className="onboard-step">
            <h1>{proven ? "It works — you’re all set 🎉" : "Setup skipped for now"}</h1>
            {!proven && (
              <p className="onboard-lede">
                Dictation hasn’t pasted yet, so Yap will keep a reminder in the
                main window until your first take lands.
              </p>
            )}
            <p className="onboard-lede">
              Hold <kbd>fn</kbd>+<kbd>⌃</kbd> anywhere, talk, and Yap types it
              out. Double-tap to go hands-free. You can replay this setup any
              time from Settings.
            </p>
            <ul className="onboard-recap">
              <li className={micOk ? "ok" : "bad"}>
                <StatusDot ok={micOk} /> Microphone
              </li>
              <li className={axOk ? "ok" : "bad"}>
                <StatusDot ok={axOk} /> Accessibility
              </li>
              <li className={modelReady ? "ok" : "bad"}>
                <StatusDot ok={modelReady} /> Speech model
              </li>
              <li className={sample ? "ok" : "bad"}>
                <StatusDot ok={!!sample} /> Voice sample
              </li>
              <li className={proven ? "ok" : "bad"}>
                <StatusDot ok={proven} /> Dictation pasted
              </li>
            </ul>
            <PolishOffer speechModelReady={modelReady} />
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack}>
                Back
              </button>
              <button
                className="primary"
                onClick={() =>
                  // `done` is reachable only by success or skip, so this is
                  // the one place `onboarded` is written from the flow.
                  onFinish(sample, (completionFor(proven ? SUCCESS : SKIP) ?? { dictationProven: false }).dictationProven)
                }
              >
                Finish setup
              </button>
            </div>
          </div>
        )}

        {/* YV54 — the whole of "model setup" as the user experiences it: one
            slim line at the foot of the card, on every step, that goes away by
            itself when the engine is ready. */}
        <ModelRibbon setup={modelSetup} />
      </div>
    </div>
  );
}
