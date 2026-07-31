import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// YV9 — first-run onboarding. Rendered as a full-screen overlay over the main
// app while AppSettings.onboarded is false. Self-contained: it invokes the
// existing permission + recording commands directly and does its own live
// polling / event wiring so App.tsx stays a thin gate.
//
// YV33 — the app ships with NO model, so a "Get your model" step now sits
// between permissions and calibration: nothing downstream (calibration, the
// hotkey) can work until a catalog model is on disk and selected. The same step
// is re-openable from the main app when status.modelReady goes false.

export type Step = "welcome" | "permissions" | "model" | "calibration" | "done";

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

/** One catalog model as `list_models` reports it (camelCase on the wire). */
interface ModelEntry {
  id: string;
  name: string;
  description: string;
  architecture: string;
  filename: string;
  quant: string;
  sizeBytes: number;
  downloaded: boolean;
  selected: boolean;
  recommended: boolean;
  recommendedRank: number | null;
}

/** `model_download_progress` payload (plain serde — snake_case on the wire). */
interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
}

const CALIBRATION_PHRASE =
  "The quick brown fox jumps over the lazy dog, and Yap is listening.";

const STEP_ORDER: Step[] = [
  "welcome",
  "permissions",
  "model",
  "calibration",
  "done",
];

/** How many recommended models to surface. Choice, not a catalog dump. */
const MODEL_PICKS = 2;

/**
 * Hard ceiling on one calibration take (transcribe + paste is ~1 s on Metal, and
 * the backend's own transcribe timeout is 120 s for a wedged native call). Before
 * this, `busy` was cleared ONLY by the success event: any failure the UI never
 * heard about — a killed worker, a lost event, a backend that died mid-take —
 * left "Transcribing…" spinning forever. Busy must always be self-clearing.
 */
const CALIBRATION_WATCHDOG_MS = 90_000;

function StatusDot({ ok }: { ok: boolean }) {
  return <span className={ok ? "dot-ok" : "dot-bad"} aria-hidden />;
}

function fmtSize(bytes: number) {
  const gb = bytes / 1e9;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

export default function Onboarding({
  onFinish,
  onSkip,
  initialStep = "welcome",
}: {
  /** Finish onboarding; sample is the captured calibration phrase (or null). */
  onFinish: (sample: string | null) => void;
  /** Dismiss without recording anything (still marks onboarded). */
  onSkip: () => void;
  /** Open straight onto a step — the main app jumps here to fix "Model needed". */
  initialStep?: Step;
}) {
  const [step, setStep] = useState<Step>(initialStep);
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sample, setSample] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  // Calibration failure (backend `transcript_error`, or the watchdog below).
  // Shown inline with a Retry button so a failed take is a dead end no longer.
  const [calibError, setCalibError] = useState<string | null>(null);
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [modelError, setModelError] = useState<string | null>(null);
  /** Catalog id currently downloading, plus its live byte progress. */
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);

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
    listen<DownloadProgress>("model_download_progress", (e) =>
      setProgress(e.payload),
    ).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, []);

  // Watchdog: busy is never clearable by success alone. If neither `transcript`
  // nor `transcript_error` lands within the ceiling, clear it into a visible,
  // retryable error rather than leaving the user staring at "Transcribing…".
  useEffect(() => {
    if (!busy) return;
    const t = setTimeout(() => {
      setBusy(false);
      setCalibError(
        "Transcription timed out — the speech model may still be loading. Retry.",
      );
    }, CALIBRATION_WATCHDOG_MS);
    return () => clearTimeout(t);
  }, [busy]);

  const refreshModels = useCallback(async () => {
    try {
      setModels(await invoke<ModelEntry[]>("list_models"));
      setModelError(null);
    } catch (e) {
      setModelError(String(e));
    }
  }, []);

  // Load the catalog for the model step, and again for the recap on `done` so
  // the summary reflects what is actually on disk.
  useEffect(() => {
    if (step !== "model" && step !== "done") return;
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

  // Download a catalog model (resumable + sha256-verified in Rust), then select
  // it so the engine loads it. Progress arrives on `model_download_progress`.
  async function downloadModel(id: string) {
    setDownloading(id);
    setProgress({ model_id: id, downloaded: 0, total: 0 });
    setModelError(null);
    try {
      await invoke("download_model", { id });
      await invoke("select_model", { id });
    } catch (e) {
      setModelError(String(e));
    }
    setDownloading(null);
    setProgress(null);
    await refreshModels();
  }

  async function selectModel(id: string) {
    try {
      await invoke("select_model", { id });
    } catch (e) {
      setModelError(String(e));
    }
    await refreshModels();
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

  // The models we actually offer: recommended first (by catalog rank), padded
  // with the rest so an unranked catalog still shows something to download.
  const picks = [...models]
    .sort((a, b) => {
      if (a.recommended !== b.recommended) return a.recommended ? -1 : 1;
      return (a.recommendedRank ?? 99) - (b.recommendedRank ?? 99);
    })
    .slice(0, MODEL_PICKS);
  const modelReady = models.some((m) => m.downloaded && m.selected);
  const pct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : 0;

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
              Three quick steps: grant macOS permissions, download your speech
              model, then record a short calibration phrase so Yap learns your
              voice.
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

        {step === "model" && (
          <div className="onboard-step">
            <h1>Get your model</h1>
            <p className="onboard-lede">
              Yap transcribes on this Mac, so it needs a speech model on disk.
              Download one once — after that, dictation works offline and
              nothing leaves this machine.
            </p>
            <ul className="onboard-models">
              {picks.map((m) => (
                <li key={m.id} className={m.downloaded ? "ok" : "bad"}>
                  <StatusDot ok={m.downloaded} />
                  <div>
                    <strong>
                      {m.name}{" "}
                      <span className="muted tiny">
                        {fmtSize(m.sizeBytes)} · {m.quant}
                      </span>
                    </strong>
                    <p>{m.description}</p>
                    {downloading === m.id ? (
                      <div
                        className="onboard-bar"
                        role="progressbar"
                        aria-valuenow={pct}
                        aria-valuemin={0}
                        aria-valuemax={100}
                      >
                        <span style={{ width: `${pct}%` }} />
                        <em className="muted tiny">
                          Downloading… {pct}%
                          {progress && progress.total > 0
                            ? ` (${fmtSize(progress.downloaded)} of ${fmtSize(progress.total)})`
                            : ""}
                        </em>
                      </div>
                    ) : m.downloaded && m.selected ? (
                      <button disabled>Downloaded ✓ · in use</button>
                    ) : m.downloaded ? (
                      <button
                        onClick={() => selectModel(m.id)}
                        disabled={!!downloading}
                      >
                        Use this model
                      </button>
                    ) : (
                      <button
                        className="primary"
                        onClick={() => downloadModel(m.id)}
                        disabled={!!downloading}
                      >
                        Download
                      </button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
            {picks.length === 0 && (
              <p className="muted tiny">Loading the model catalog…</p>
            )}
            {modelError && (
              <p className="onboard-error">
                {modelError}{" "}
                <button className="ghost" onClick={refreshModels}>
                  Retry
                </button>
              </p>
            )}
            <div className="onboard-actions">
              <button className="ghost" onClick={goBack} disabled={!!downloading}>
                Back
              </button>
              <button
                className="primary"
                onClick={goNext}
                disabled={!modelReady || !!downloading}
              >
                {modelReady ? "Continue" : "Download a model to continue"}
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
      </div>
    </div>
  );
}
