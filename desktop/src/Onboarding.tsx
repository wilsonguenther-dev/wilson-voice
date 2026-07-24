import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// YV9 — first-run onboarding. Rendered as a full-screen overlay over the main
// app while AppSettings.onboarded is false. Self-contained: it invokes the
// existing permission + recording commands directly and does its own live
// polling / event wiring so App.tsx stays a thin gate.

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
        setNote("Voice sample captured — Yap will use it to personalize.");
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, []);

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
      await invoke("manual_toggle");
    } catch (e) {
      setBusy(false);
      setNote(String(e));
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
              calibration phrase so Yap learns your voice.
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
              macOS must allow <strong>Yap</strong> (not Python) to use these.
              Status updates live as you enable each one.
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
            <button
              className={recording ? "onboard-record live" : "onboard-record"}
              onClick={toggleCalibration}
              disabled={busy}
            >
              {recording
                ? "■ Stop & save"
                : busy
                  ? "Transcribing…"
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
      </div>
    </div>
  );
}
