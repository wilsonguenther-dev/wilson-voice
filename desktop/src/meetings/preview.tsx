/**
 * YV96 — dev tooling: the one-time capture notice, on demand.
 *
 * The sheet is, by design, unreachable a second time: it shows on the first
 * meeting anyone records and then never again, and the acknowledgement lives in
 * SQLite. So reviewing it — or screenshotting it — would otherwise mean deleting
 * a row out of a shipped database between takes. This mounts the REAL component
 * inside the app's own chrome against the two states it has, which is why a
 * screenshot of this page is a screenshot of the shipping UI.
 *
 * Same rules as `license/preview.tsx`: it is a BUILD ENTRY behind
 * `YAP_DEV_TOOLING=1`, so no shipped build carries it.
 *
 *   YAP_DEV_TOOLING=1 npm run build   # then dist/dev/meeting-consent-preview.html
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import MeetingConsentNotice from "./MeetingConsentNotice";
import { acknowledgedLabel, type MeetingConsent } from "./consent";
import "../App.css";

const PENDING: MeetingConsent = {
  shouldShow: true,
  acknowledgedAt: null,
  blocksRecording: false,
};
const SEEN: MeetingConsent = {
  shouldShow: false,
  acknowledgedAt: "2026-08-11T18:04:00Z",
  blocksRecording: false,
};

/** `first-meeting` = the notice over a running recording; `settings` = the
 *  Privacy line it leaves behind afterwards. */
const SCENES = ["first-meeting", "settings"] as const;
type Scene = (typeof SCENES)[number];

function sceneFromHash(): Scene {
  const h = window.location.hash.replace("#", "") as Scene;
  return SCENES.includes(h) ? h : "first-meeting";
}

function Preview() {
  const [scene, setScene] = useState<Scene>(sceneFromHash());
  const consent = scene === "settings" ? SEEN : PENDING;

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
          {["Home", "Meetings", "Permissions", "Insights", "Settings"].map((label) => {
            const active =
              (scene === "settings" && label === "Settings") ||
              (scene !== "settings" && label === "Meetings");
            return (
              <button key={label} className={active ? "nav-item active" : "nav-item"}>
                <span>{label}</span>
              </button>
            );
          })}
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
            <h1>{scene === "settings" ? "Settings" : "Meetings"}</h1>
            <p className="lede">
              {scene === "settings"
                ? "Your companion, dictation, shortcut, and privacy — all in plain language."
                : "Recorded meetings, searchable and exportable. Audio is kept for 7 days; the transcript stays."}
            </p>
          </div>
          <div className="head-state">
            <div className="status-pill ready">Ready — hold fn⌃</div>
          </div>
        </header>

        <div className="content">
          {scene === "settings" ? (
            <div className="settings">
              <section className="settings-panel">
                <h2 className="settings-section">
                  Recording other people
                  <span className="sub">
                    Yap does not announce itself. Whether you may record someone is
                    your call, and the law differs by state and country.
                  </span>
                </h2>
                <p className="tiny">{acknowledgedLabel(consent)}</p>
                <div className="actions wrap">
                  <button onClick={() => setScene("first-meeting")}>
                    Read the recording notice
                  </button>
                </div>
              </section>
            </div>
          ) : (
            <div className="empty">
              <h3>No meetings yet</h3>
              <p>
                Recorded meetings land here — searchable, exportable, and deletable in
                one click. Audio is kept for 7 days; the transcript is kept for good.
              </p>
            </div>
          )}
        </div>
      </section>

      {scene === "first-meeting" && (
        <MeetingConsentNotice recording onClose={() => setScene("settings")} />
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
