/**
 * YV102 — dev tooling: the "Set up meeting recording" step, in every state it
 * has.
 *
 * Four of these five states are nearly unreachable on demand in a running app.
 * `looks_denied` needs a real TCC refusal plus a tap that ran past the grace
 * window without one non-zero sample; `unavailable` needs a macOS 13 machine;
 * `failed` needs a CoreAudio call to return non-zero. Screenshotting them by
 * hand would mean three Macs and a denied permission you cannot un-deny —
 * macOS asks once.
 *
 * So the states are mounted here against the REAL `setupState` rule and the
 * REAL copy, in the app's own chrome. A screenshot of this page is a screenshot
 * of the shipping UI, because there is no second implementation on this page —
 * only the four literal props `App.tsx` passes.
 *
 * Same rules as `meetings/preview.tsx`: a BUILD ENTRY behind `YAP_DEV_TOOLING=1`,
 * so no shipped build carries it.
 *
 *   YAP_DEV_TOOLING=1 npm run build   # then dist/dev/system-audio-preview.html
 */
import React from "react";
import ReactDOM from "react-dom/client";
import {
  setupState,
  SYSTEM_AUDIO_SETUP,
  type SetupVerdict,
  type SystemAudioSetup,
} from "./systemAudio";
import "../App.css";

const REQUIREMENT =
  "System audio capture requires macOS 14.4 or later, and this Mac is on macOS 13.6";

function state(
  verdict: SetupVerdict | null,
  lastRunAt = "2026-08-14T17:04:00Z",
): SystemAudioSetup | null {
  if (verdict === null) return null;
  return {
    hasRun: verdict !== "not_run",
    lastRunAt,
    verdict,
    blocksRecording: false,
  };
}

/** Every state the step can be in, with the reason it exists. */
const SCENES: {
  caption: string;
  setup: SystemAudioSetup | null;
  available: boolean;
}[] = [
  {
    caption:
      "Fresh install — the call to action. The alert has not fired yet, and this is the screen it will fire from.",
    setup: state(null),
    available: true,
  },
  {
    caption:
      "Ran, heard nothing. 200 ms on a quiet Mac is not evidence of a refusal, so the step says so instead of guessing.",
    setup: state("ran"),
    available: true,
  },
  {
    caption:
      "Granted — a non-zero sample actually arrived. The only positive proof available without a private API.",
    setup: state("granted"),
    available: true,
  },
  {
    caption:
      "Looks denied — a tap ran past the grace window and never delivered one sample. The deep link is the only recovery: macOS does not ask twice.",
    setup: state("looks_denied"),
    available: true,
  },
  {
    caption:
      "macOS below 14.4 (YV101's gate). Visible, disabled, honest — and mic-only meeting recording keeps working.",
    setup: state("not_run"),
    available: false,
  },
  {
    caption:
      "A CoreAudio call failed. A fault, not a refusal, and worded so the user does not go hunting in System Settings for a setting that is already right.",
    setup: state("failed"),
    available: true,
  },
];

function Step({
  setup,
  available,
}: {
  setup: SystemAudioSetup | null;
  available: boolean;
}) {
  const step = setupState(setup, available, REQUIREMENT);
  return (
    <section className="settings-panel">
      <h2 className="settings-section">
        {SYSTEM_AUDIO_SETUP.title}
        <span className="sub">{SYSTEM_AUDIO_SETUP.sub}</span>
      </h2>
      {SYSTEM_AUDIO_SETUP.paragraphs.map((p) => (
        <p className="tiny" key={p.slice(0, 24)}>
          {p}
        </p>
      ))}
      <p className="tiny muted">{SYSTEM_AUDIO_SETUP.fine}</p>
      <p className={step.tone === "bad" ? "tiny warn" : "tiny muted"}>
        {step.label}
      </p>
      <div className="actions wrap">
        <button
          className={step.tone === "bad" ? "" : "primary"}
          disabled={!step.canRun}
        >
          {step.actionLabel}
        </button>
        {step.showDeepLink && (
          <button>{SYSTEM_AUDIO_SETUP.openSettings}</button>
        )}
      </div>
    </section>
  );
}

function Preview() {
  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="mark">
            <span className="dot" />
          </div>
          <div>
            <div className="brand-name">Yap</div>
            <div className="brand-tag">v{__APP_VERSION__} · local · private</div>
          </div>
        </div>
        <nav className="nav">
          {["Home", "Meetings", "Permissions", "Insights", "Settings"].map(
            (label) => (
              <button
                key={label}
                className={
                  label === "Settings" ? "nav-item active" : "nav-item"
                }
              >
                <span>{label}</span>
              </button>
            ),
          )}
        </nav>
        <div className="sidebar-foot">
          <button className="dictate-side">
            Dictate<span className="dictate-key">fn⌃</span>
          </button>
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          <div>
            <h1>Settings</h1>
            <p className="lede">
              Your companion, dictation, shortcut, and privacy — all in plain
              language.
            </p>
          </div>
          <div className="head-state">
            <div className="status-pill ready">Ready — hold fn⌃</div>
          </div>
        </header>

        <div className="content">
          <div className="settings">
            {SCENES.map((scene) => (
              <React.Fragment key={scene.caption}>
                <p className="tiny muted" style={{ padding: "14px 0 0" }}>
                  <strong>{scene.caption}</strong>
                </p>
                <Step setup={scene.setup} available={scene.available} />
              </React.Fragment>
            ))}
          </div>
        </div>
      </section>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);
