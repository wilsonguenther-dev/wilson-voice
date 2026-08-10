import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ModelRibbon, useModelSetup } from "./ModelSetup";

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

type Step = "welcome" | "permissions" | "calibration" | "done";

interface PermissionReport {
  accessibility: boolean;
  microphone: boolean;
  ffmpegOk: boolean;
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

const STEP_ORDER: Step[] = ["welcome", "permissions", "calibration", "done"];

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

function StatusDot({ ok }: { ok: boolean }) {
  return <span className={ok ? "dot-ok" : "dot-bad"} aria-hidden />;
}

export default function Onboarding({
  onFinish,
  onSkip,
}: {
  /** Finish onboarding; sample is the captured calibration phrase (or null). */
  onFinish: (sample: string | null) => void;
  /** Dismiss without recording anything (still marks onboarded). */
  onSkip: () => void;
}) {
  const [step, setStep] = useState<Step>("welcome");
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sample, setSample] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  // Calibration failure (backend `transcript_error`, or the watchdog below).
  // Shown inline with a Retry button so a failed take is a dead end no longer.
  const [calibError, setCalibError] = useState<string | null>(null);
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
  // updates the moment the user flips a toggle in System Settings.
  useEffect(() => {
    if (step !== "permissions") return;
    refreshPerms();
    const t = setInterval(refreshPerms, 1200);
    return () => clearInterval(t);
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
    listen<boolean>("recording", (e) => setRecording(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    listen<TranscriptEntry>("transcript", (e) => {
      if (stepRef.current === "calibration") {
        setSample(e.payload.text);
        setBusy(false);
        setCalibError(null);
        setNote("Voice sample captured — Yap will use it to personalize.");
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV33 — the other half of "busy": the backend emits this whenever a take
    // fails to produce a transcript, so the failure clears the spinner and shows
    // its reason instead of the overlay waiting on a success that never comes.
    listen<{ message: string }>("transcript_error", (e) => {
      setBusy(false);
      setCalibError(e.payload?.message || "Transcription failed.");
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV79 — the terminal marker for a take, whatever its outcome. The three
    // listeners above still miss the SOFT ones, and the first-run user with no
    // mic permission / the wrong input device hits the loudest of them: the
    // no-speech gate, which only ever spoke on the toast channel behind this
    // overlay. Busy now clears for EVERY take, with the backend's own reason.
    listen<{ ok: boolean; message: string | null }>("take_done", (e) => {
      setBusy(false);
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
    try {
      await invoke("request_microphone");
    } catch (e) {
      setNote(String(e));
    }
    setTimeout(refreshPerms, 900);
  }

  async function requestAccessibility() {
    try {
      await invoke("request_accessibility");
    } catch (e) {
      setNote(String(e));
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
      setCalibError(String(e));
    }
  }

  function goNext() {
    const i = STEP_ORDER.indexOf(step);
    if (i < STEP_ORDER.length - 1) setStep(STEP_ORDER[i + 1]);
  }

  function goBack() {
    const i = STEP_ORDER.indexOf(step);
    if (i > 0) setStep(STEP_ORDER[i - 1]);
  }

  const micOk = !!perms?.microphone;
  const axOk = !!perms?.accessibility;
  const modelReady = modelSetup.ready;
  /** Calibration can only run once an engine exists — until then it waits. */
  const engineWaiting = !modelReady && !recording;

  return (
    <div className="onboard-overlay" role="dialog" aria-modal="true">
      <div className="onboard-card">
        <div className="onboard-progress" aria-hidden>
          {STEP_ORDER.map((s) => (
            <span
              key={s}
              className={
                s === step
                  ? "onboard-dot active"
                  : STEP_ORDER.indexOf(s) < STEP_ORDER.indexOf(step)
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
            <p className="muted">
              Two quick steps: grant macOS permissions, then record a short
              calibration phrase so Yap learns your voice. Your speech model is
              already downloading in the background — nothing to pick.
            </p>
            <div className="onboard-actions">
              <button className="primary" onClick={goNext}>
                Get started
              </button>
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
              macOS must allow <strong>Yap</strong> itself to use these. Status
              updates live as you enable each one.
            </p>
            <ul className="onboard-perms">
              <li className={micOk ? "ok" : "bad"}>
                <StatusDot ok={micOk} />
                <div>
                  <strong>Microphone</strong>
                  <p>Needed to hear you. Click Allow when macOS prompts.</p>
                  <button onClick={requestMic}>
                    {micOk ? "Granted ✓" : "Request Microphone"}
                  </button>
                </div>
              </li>
              <li className={axOk ? "ok" : "bad"}>
                <StatusDot ok={axOk} />
                <div>
                  <strong>Accessibility</strong>
                  <p>Lets Yap paste text into any app (simulated ⌘V).</p>
                  <button onClick={requestAccessibility}>
                    {axOk ? "Granted ✓" : "Prompt Accessibility"}
                  </button>
                </div>
              </li>
            </ul>
            {note && <p className="muted tiny">{note}</p>}
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack}>
                Back
              </button>
              <button className="primary" onClick={goNext}>
                {micOk && axOk ? "Continue" : "Continue anyway"}
              </button>
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
              disabled={busy || engineWaiting}
            >
              {recording
                ? "■ Stop & save"
                : busy
                  ? "Transcribing…"
                  : engineWaiting
                    ? (modelSetup.ribbon ?? "Preparing your speech engine…")
                    : sample
                      ? "● Record again"
                      : "● Start recording"}
            </button>
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

        {step === "done" && (
          <div className="onboard-step">
            <h1>You’re all set 🎉</h1>
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
            </ul>
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack}>
                Back
              </button>
              <button className="primary" onClick={() => onFinish(sample)}>
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
