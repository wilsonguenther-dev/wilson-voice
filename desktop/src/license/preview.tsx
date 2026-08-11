/**
 * YP3 — dev tooling: the licensing surfaces, on demand, without a purchase.
 *
 * Three of the four states this feature has are unreachable by hand: a trial
 * with exactly three days left needs eleven days of waiting, a lifetime license
 * needs $29 and a live Stripe link (which is deliberately `active: false` until
 * launch), and the post-trial prompt needs both. So the REAL components are
 * mounted here against mocked `LicenseStatus` payloads — the same objects the
 * backend serialises — inside the app's own header/sub-nav chrome, so a
 * screenshot of this page is a screenshot of the shipping UI.
 *
 * It is not part of the app. `vite.config.ts` only adds this entry when
 * `YAP_DEV_TOOLING=1`, so no shipped build contains a page that can draw an
 * entitlement out of thin air.
 *
 *   YAP_DEV_TOOLING=1 npm run build
 *   open dist/dev/license-preview.html#licensed
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import LicensePanel from "./LicensePanel";
import PurchasePrompt from "./PurchasePrompt";
import {
  chipFor,
  daysLeft,
  trialWarningText,
  type LicenseStatus,
} from "./status";
import "../App.css";

const EXPIRES = Date.now() + 3 * 24 * 60 * 60 * 1000;

const base = {
  license_problem: null,
  license_problem_message: null,
  has_stored_license: false,
  revocation_checked_at_ms: EXPIRES,
  revoked_count: 0,
} as const;

const MOCKS: Record<string, LicenseStatus> = {
  trial: {
    state: "trial",
    days_left: 11,
    expires_at_ms: EXPIRES,
    trial_days_left: 11,
    trial_expires_at_ms: EXPIRES,
    ...base,
  },
  warning: {
    state: "trial",
    days_left: 3,
    expires_at_ms: EXPIRES,
    trial_days_left: 3,
    trial_expires_at_ms: EXPIRES,
    ...base,
  },
  licensed: {
    state: "licensed",
    plan: "lifetime",
    seats: 3,
    kid: "yap_9f2c41ab7d",
    trial_days_left: 0,
    trial_expires_at_ms: EXPIRES,
    ...base,
    has_stored_license: true,
  },
  prompt: {
    state: "license_required",
    reason: "trial_expired",
    trial_days_left: 0,
    trial_expires_at_ms: EXPIRES,
    ...base,
  },
};

/** Mirrors `SETTINGS_TABS` in App.tsx — labels only, so the chrome reads true. */
const TABS = [
  "Companion",
  "Dictation",
  "Snippets",
  "Audio",
  "Shortcut",
  "Advanced",
  "Privacy",
  "License",
];

/** The chip has four looks; this scene puts them side by side for review. */
const CHIP_SCENES: [string, LicenseStatus][] = [
  ["Day 11 of the trial", MOCKS.trial],
  ["Inside the last three days", MOCKS.warning],
  ["Lifetime license", MOCKS.licensed],
  ["Trial ended", MOCKS.prompt],
];

const SCENES = [...Object.keys(MOCKS), "chips"];

function sceneFromHash(): string {
  const h = window.location.hash.replace("#", "");
  return SCENES.includes(h) ? h : "trial";
}

function Preview() {
  const [scene, setScene] = useState(sceneFromHash());
  const status = MOCKS[scene] ?? MOCKS.trial;
  const chip = chipFor(status);
  const noop = async () => {};

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
          {["Home", "Permissions", "Insights", "Dictionary", "Scratchpad", "Settings"].map(
            (label) => (
              <button
                key={label}
                className={label === "Settings" ? "nav-item active" : "nav-item"}
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
              Your companion, dictation, shortcut, and privacy — all in plain language.
            </p>
          </div>
          <div className="head-state">
            <button type="button" className={`license-chip ${chip.tone}`} title={chip.title}>
              <span className="license-chip-label">{chip.label}</span>
              {chip.value && <span className="license-chip-value">{chip.value}</span>}
            </button>
            <div className="status-pill ready">Ready — hold fn⌃</div>
          </div>
        </header>

        {/* The single trial warning, exactly as `App.tsx` flashes it. */}
        {scene === "warning" && (
          <div className="toast">
            <span>{trialWarningText(daysLeft(status))}</span>
          </div>
        )}

        <div className="content">
          {scene === "chips" ? (
            <div className="settings">
              <h2 className="settings-section">
                The status chip
                <span className="sub">
                  Always on, never interrupting. Its numeral is Departure Mono, tabular,
                  so 14 → 9 does not shuffle the header under it.
                </span>
              </h2>
              {CHIP_SCENES.map(([label, s]) => {
                const c = chipFor(s);
                return (
                  <div className="panel" key={label} style={{ display: "flex", gap: 14, alignItems: "center" }}>
                    <button type="button" className={`license-chip ${c.tone}`} title={c.title}>
                      <span className="license-chip-label">{c.label}</span>
                      {c.value && <span className="license-chip-value">{c.value}</span>}
                    </button>
                    <div>
                      <strong>{label}</strong>
                      <p style={{ margin: "2px 0 0", color: "var(--muted)", fontSize: "0.84rem" }}>
                        {c.title}
                      </p>
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
          <div className="settings">
            <div className="settings-subnav" role="tablist" aria-label="Settings sections">
              {TABS.map((t) => (
                <button
                  key={t}
                  type="button"
                  role="tab"
                  aria-selected={t === "License"}
                  className={t === "License" ? "subnav-item active" : "subnav-item"}
                >
                  {t}
                </button>
              ))}
            </div>
            <LicensePanel
              status={status}
              onBuy={noop}
              onActivate={async () => {
                throw {
                  code: "bad_signature",
                  message:
                    "That license key did not check out. Copy it again straight from your email; if it still fails, reply to that email and we will re-send it.",
                };
              }}
              onDeactivate={noop}
            />
          </div>
          )}
        </div>
      </section>

      {scene === "prompt" && (
        <PurchasePrompt onBuy={noop} onEnterKey={() => setScene("trial")} onDismiss={noop} />
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
