/**
 * YV98 — dev tooling: the crash-report sheet, without a crash.
 *
 * Reviewing this surface for real means owning a Mac that has crashed, a log
 * with something worth redacting in it, and — for the interesting half — a Mac
 * whose mail client `canPerformWithItems:` refuses to drive. This mounts the
 * REAL component against both states, so a screenshot of this page is a
 * screenshot of the shipping UI.
 *
 * Same rules as `license/preview.tsx` and `meetings/preview.tsx`: a BUILD ENTRY
 * behind `YAP_DEV_TOOLING=1`, so no shipped build carries it.
 *
 *   YAP_DEV_TOOLING=1 npm run build   # then dist/dev/support-bundle-preview.html
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import SupportBundleSheet from "./SupportBundleSheet";
import { type SupportBundlePreview } from "./bundle";
import "../App.css";

/** The redacted log the Rust test prints — verbatim, so the two agree. */
const REDACTED_LOG = `[2026-08-12T09:00:01Z INFO  wilson_voice_lib::logging] file logging → ‹path› ‹path›
[2026-08-12T09:00:01Z INFO  wilson_voice_lib] installing Yap 0.8.0 (user-approved)
[2026-08-12T09:00:02Z WARN  wilson_voice_lib::models] model download attempt failed (https://huggingface.co/‹redacted›): timed out
[2026-08-12T09:00:03Z INFO  wilson_voice_lib::license] ‹redacted prose›(3 words) ‹email› ‹redacted prose›(1 word) ‹token›
[2026-08-12T09:00:04Z INFO  wilson_voice_lib::db] ‹redacted prose›(4 words) ‹user›
[2026-08-12T09:12:44Z INFO  wilson_voice_lib::dictation] transcript stored: ‹redacted›
[2026-08-12T09:12:44Z INFO  wilson_voice_lib::dictation] polished text = ‹redacted prose›(21 words)
[2026-08-12T09:12:45Z INFO  wilson_voice_lib::dictation] polished text = ‹redacted prose›(7 words) 8 ‹redacted prose›(8 words)
[2026-08-12T09:12:49Z INFO  wilson_voice_lib::paste_tx] reliable-paste: read confirmed — prior clipboard restored
[2026-08-12T09:13:01Z WARN  wilson_voice_lib::db] wal_checkpoint failed: database is locked
[2026-08-12T09:13:02Z INFO  wilson_voice_lib::polish] polish sidecar starting
[2026-08-12T09:14:00Z ERROR wilson_voice_lib::logging] PANIC at src/transcription.rs:212:9 ‹redacted›
[2026-08-12T09:14:00Z DEBUG wilson_voice_lib::hygiene] hygiene: could not remove ‹path› ‹path›: No such file or directory
[2026-08-12T09:15:11Z INFO  wilson_voice_lib] recording cancelled
`;

const ENVIRONMENT = `[Info.plist]
CFBundleDisplayName: Yap
CFBundleIdentifier: com.wilsonguenther.wilson-voice
CFBundleShortVersionString: 0.8.0
LSMinimumSystemVersion: 12.0

[OS]
os: macOS 26.5.2 (25F84)
arch: aarch64
build: release
`;

const CRASH_SUMMARY = `Yap — crash summary (local only, never uploaded)
generated: 2026-08-12T14:15:30+00:00

1 recorded crash(es), newest first:

[2026-08-12T09:14:00+00:00] native — EXC_CRASH (SIGABRT)
process: wilson-voice
version: 0.8.0
exception: EXC_CRASH (SIGABRT)
`;

function entry(name: string, excerpt: string, truncated = false) {
  return {
    name,
    bytes: excerpt.length,
    lines: excerpt.split("\n").length,
    excerpt,
    truncated,
  };
}

const PREVIEW: SupportBundlePreview = {
  fileName: "Yap-Diagnostics-0.8.0-20260812-141530.zip",
  recipient: "wilson@drivia.consulting",
  mailAvailable: true,
  totalBytes: 18_432,
  entries: [
    entry("README.txt", "Yap — diagnostics bundle\ngenerated: 2026-08-12T14:15:30+00:00\n"),
    entry("environment.txt", ENVIRONMENT),
    entry("crash-summary.txt", CRASH_SUMMARY),
    entry("permissions.txt", "accessibility: true\nmicrophone: true\nasr_model_ready: true\n"),
    entry("models.txt", "selected_asr_model: small.en\n\n[asr catalog]\nsmall.en: downloaded=true\n"),
    entry("logs/yap.log", REDACTED_LOG, true),
    entry("logs/yap.log.1", REDACTED_LOG, true),
  ],
};

const SCENES = [
  "mail-available",
  "no-mail-client",
  "redacted-log",
  "building",
  "settings",
] as const;
type Scene = (typeof SCENES)[number];

function sceneFromHash(): Scene {
  const h = window.location.hash.replace("#", "") as Scene;
  return SCENES.includes(h) ? h : "mail-available";
}

function Preview() {
  const [scene, setScene] = useState<Scene>(sceneFromHash());
  const preview =
    scene === "building"
      ? null
      : { ...PREVIEW, mailAvailable: scene !== "no-mail-client" };

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
          {["Home", "Meetings", "Permissions", "Insights", "Settings"].map((label) => (
            <button
              key={label}
              className={label === "Settings" ? "nav-item active" : "nav-item"}
            >
              <span>{label}</span>
            </button>
          ))}
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
            <section className="settings-panel">
              <h2 className="settings-section">
                Stability
                <span className="sub">
                  Crashes Yap noticed, read from this Mac&rsquo;s own crash
                  reports. Stored locally, never uploaded, and they never contain
                  anything you dictated.
                </span>
              </h2>
              <div className="actions wrap">
                <button className="primary" onClick={() => setScene("mail-available")}>
                  Send crash report to Wilson
                </button>
              </div>
              <p className="tiny">
                Packs the logs, the crash summary, your permission states and
                which models are downloaded. Transcripts, recordings and your
                database are never in it, and the logs are redacted before they
                are packed &mdash; you read the whole thing first.
              </p>
            </section>
          </div>
        </div>
      </section>

      {scene !== "settings" && (
        <SupportBundleSheet
          preview={preview}
          busy={false}
          onSend={() => undefined}
          onClose={() => setScene("settings")}
          initialOpen={scene === "redacted-log" ? "logs/yap.log" : null}
        />
      )}

      {/* Harness controls, deliberately unstyled by the app's own classes so
          they can never be mistaken for product UI in a screenshot. */}
      <div
        style={{
          position: "fixed",
          right: 12,
          bottom: 12,
          zIndex: 999,
          display: "flex",
          gap: 6,
          font: "11px ui-monospace, monospace",
          opacity: 0.55,
        }}
      >
        {SCENES.map((k) => (
          <button
            key={k}
            onClick={() => {
              window.location.hash = k;
              setScene(k);
            }}
            style={{ padding: "2px 6px", borderRadius: 4 }}
          >
            {k}
          </button>
        ))}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Preview />
  </React.StrictMode>,
);
