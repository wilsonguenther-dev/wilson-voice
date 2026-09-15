// Y5-G — App.tsx is the SHELL ONLY: nav, header, the license chip, the toast
// host and the global modals. Every screen lives in ./views/<Name>.tsx and the
// eight settings panels in ./views/settings/. Shell state lives in ./appShell.
import { AppContext, useAppShell } from "./appShell";
import Onboarding from "./Onboarding";
import { ModelRibbon } from "./ModelSetup";
import { PermissionHealthRow } from "./PermissionHealthRow";
import PurchasePrompt from "./license/PurchasePrompt";
import MeetingConsentNotice from "./meetings/MeetingConsentNotice";
import SupportBundleSheet from "./support/SupportBundleSheet";
import Home from "./views/Home";
import Permissions from "./views/Permissions";
import Meetings from "./views/Meetings";
import Insights from "./views/Insights";
import Dictionary from "./views/Dictionary";
import Scratchpad from "./views/Scratchpad";
import Settings from "./views/Settings";
import "./App.css";

export default function App() {
  const ctx = useAppShell();
  const {
    bootError, buyPrompt, buyYap, closeConsentNotice, consentOpen, copyAgainId,
    copyText, dictionary, finishOnboarding, flash,
    history, insights, installUpdateNow, installedVersion, installing, 
    licenseChip, meetings, modelSetup, nav, needsPerms, openLicenseTab,
    perms, pillClass, refreshAll, retryFailed, retryId, retrying,
    scratch, sendSupportBundle, setBuyPrompt, setCopyAgainId, setNav, setSettingsTab,
    setSupportOpen, setSupportPreview, setUpdate, settings, skipUpdateVersion, status,
    supportBusy, supportOpen, supportPreview, toast, toggleRecord, update,
    userName,
  } = ctx;

  if (bootError && !settings) {
    return (
      <div style={{ padding: 24, fontFamily: "system-ui" }}>
        <h2>Yap</h2>
        <p>UI failed to load backend bridge:</p>
        <pre style={{ whiteSpace: "pre-wrap" }}>{bootError}</pre>
        <button type="button" onClick={() => refreshAll()}>
          Retry
        </button>
      </div>
    );
  }

  // YV9 — first-run onboarding gate: show once settings are loaded and the user
  // has not completed it. Rendered over the app; the main UI stays mounted.
  if (settings && !settings.onboarded) {
    return (
      <Onboarding
        onFinish={(sample) => finishOnboarding(sample)}
        onSkip={() => finishOnboarding(null)}
      />
    );
  }

  return (
    <AppContext.Provider value={ctx}>
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="mark">
            <span className={status.recording ? "dot pulse" : "dot"} />
          </div>
          <div>
            <div className="brand-name">Yap</div>
            <div className="brand-tag">v{__APP_VERSION__} · local · private</div>
          </div>
        </div>

        <nav className="nav">
          {(
            [
              ["home", "Home", history.length],
              ["permissions", "Permissions", needsPerms ? 1 : null],
              // YV94 — Meetings sits next to Home because it is the second
              // thing Yap keeps for you, not a setting.
              ["meetings", "Meetings", meetings.length],
              ["insights", "Insights", null],
              ["dictionary", "Dictionary", dictionary.length],
              ["scratchpad", "Scratchpad", scratch.length],
              ["settings", "Settings", null],
            ] as const
          ).map(([id, label, count]) => (
            <button
              key={id}
              className={nav === id ? "nav-item active" : "nav-item"}
              aria-current={nav === id ? "page" : undefined}
              onClick={() => setNav(id)}
            >
              <span>{label}</span>
              {count != null && count > 0 && (
                <span className={id === "permissions" ? "count warn" : "count"}>
                  {count}
                </span>
              )}
            </button>
          ))}
        </nav>

        <div className="sidebar-foot">
          <button
            className={status.recording ? "dictate-side live" : "dictate-side"}
            onClick={toggleRecord}
            disabled={status.busy}
          >
            {/* YV56 — "Dictate · fn⌃" was one string, so the verb and the key
                carried identical weight. The action stays native SF; the key is
                data and wears the pixel voice in its own chip, the same
                treatment the nav counts and the header status pill take. */}
            {status.recording ? (
              "Stop listening"
            ) : status.busy ? (
              "Transcribing…"
            ) : (
              <>
                Dictate
                <span className="dictate-key">fn⌃</span>
              </>
            )}
          </button>
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          <div>
            <h1>
              {nav === "home" && (userName ? `Welcome back, ${userName}` : "Welcome back")}
              {nav === "permissions" && "Permissions"}
              {nav === "meetings" && "Meetings"}
              {nav === "insights" && "Insights"}
              {nav === "dictionary" && "Dictionary"}
              {nav === "scratchpad" && "Scratchpad"}
              {nav === "settings" && "Settings"}
            </h1>
            <p className="lede">
              {nav === "home" &&
                "Hold fn⌃ and talk — your words land where the cursor is."}
              {nav === "permissions" &&
                "macOS must grant these to Yap itself. Without them, dictation or paste fails."}
              {nav === "meetings" &&
                "Recorded meetings, searchable and exportable. Audio is kept for 7 days; the transcript stays."}
              {nav === "insights" &&
                "Local analytics from your SQLite history — nothing leaves this Mac."}
              {nav === "dictionary" &&
                "Custom spellings applied after each transcription."}
              {nav === "scratchpad" && "Park text and assemble prompts."}
              {nav === "settings" &&
                "Your companion, dictation, shortcut, and privacy — all in plain language."}
            </p>
          </div>
          <div className="head-state">
            {/* YP3 — the always-on entitlement chip. It never interrupts: it is
                a button because the one thing you want after reading it is the
                License tab. The numeral wears the pixel voice, the same
                treatment every other piece of data in the app gets. */}
            {licenseChip && (
              <button
                type="button"
                className={`license-chip ${licenseChip.tone}`}
                title={licenseChip.title}
                onClick={() => {
                  setNav("settings");
                  setSettingsTab("license");
                }}
              >
                <span className="license-chip-label">{licenseChip.label}</span>
                {licenseChip.value && (
                  <span className="license-chip-value">{licenseChip.value}</span>
                )}
              </button>
            )}
            <div className={pillClass}>{status.message}</div>
          </div>
        </header>

        {flash && (
          <div className="toast">
            <span>{flash}</span>
            {/* YV52 — the take that just failed still has its audio: retry runs
                ASR again on that clip (the model may have finished downloading,
                or the engine recovered) instead of making the user re-speak. */}
            {retryId && (
              <button
                className="toast-retry"
                disabled={retrying === retryId}
                onClick={() => retryFailed(retryId)}
              >
                {retrying === retryId ? "Retrying…" : "Retry"}
              </button>
            )}
            {/* YV74 — the paste was not confirmed by a read receipt, so the
                text may not have landed anywhere. Put it back on the clipboard
                on demand rather than making the user hunt through History. */}
            {copyAgainId && (
              <button
                className="toast-copy-again"
                onClick={() => {
                  const row = history.find((h) => h.id === copyAgainId);
                  if (row) copyText(row.text);
                  else toast("That transcript is in History");
                  setCopyAgainId(null);
                }}
              >
                Copy again
              </button>
            )}
          </div>
        )}

        {/* YV43 — another app holds macOS Secure Input, so the CGEvent tap
            behind fn / fn⌃ is blind and push-to-talk is dead RIGHT NOW. It
            outranks every other banner because nothing below it can be reached
            with the hotkey while this is on, and it names the holder so the
            user knows which app to go release. */}
        {status.secureInputBlocked && (
          <div className="banner blocked">
            <strong>
              Dictation paused — another app is blocking keyboard monitoring
              (Secure Input)
            </strong>
            <span className="banner-detail">{status.secureInputDetail}</span>
          </div>
        )}

        {/* YV54 — no usable model is no longer a "Model needed" demand that
            routes the user into a decision screen: the download is already
            running, so this is the same slim ribbon onboarding shows, and it
            retires itself the moment the engine is ready. */}
        <ModelRibbon setup={modelSetup} />

        {/* PERM-E — the single permission surface. Renders NOTHING while all
            four grants are fine, which is why it sits unconditionally here
            rather than behind a nav check: it is its own visibility rule. */}
        <PermissionHealthRow grants={perms?.grants} />

        {status.modelReady && needsPerms && nav === "home" && (
          <div className="banner warn" onClick={() => setNav("permissions")}>
            Setup incomplete — open Permissions to enable Mic / Accessibility
            for Yap
          </div>
        )}

        {/* YV44 — a newer Yap EXISTS. Nothing has been downloaded: this banner
            is the whole notification, it never blocks the app, and the install
            happens only if the user asks for it here. "Later" hides it until
            the next launch; "Skip this version" retires this release for good. */}
        {update && (
          <div className="banner update">
            <strong>
              Update available — Yap {update.version} (you're on{" "}
              {update.currentVersion}). Install now?
            </strong>
            {update.notes && (
              <span className="banner-detail">{update.notes}</span>
            )}
            <div className="banner-actions">
              <button
                className="primary"
                onClick={installUpdateNow}
                disabled={installing}
              >
                {installing ? "Installing…" : "Install now"}
              </button>
              <button onClick={() => setUpdate(null)} disabled={installing}>
                Later
              </button>
              <button onClick={skipUpdateVersion} disabled={installing}>
                Skip this version
              </button>
            </div>
          </div>
        )}

        {installedVersion && (
          <div className="banner update">
            <strong>
              Yap {installedVersion} installed — quit and reopen Yap to finish.
            </strong>
          </div>
        )}

        <div className="content">
          {nav === "permissions" && <Permissions />}

          {nav === "home" && <Home />}

          {/* YV94 — Meetings. Two states in one screen: the list, and one
              meeting's transcript. Nothing here STARTS a meeting — capture is
              YV91 and the entry points (tray, hotkey, pill, the empty state's
              own button) are YV95. This ships the surface that makes a recorded
              meeting findable, readable, exportable and deletable. */}
          {nav === "meetings" && <Meetings />}


          {nav === "insights" && insights && <Insights />}

          {nav === "dictionary" && <Dictionary />}

          {nav === "scratchpad" && <Scratchpad />}

          {nav === "settings" && settings && <Settings />}
        </div>
      </section>

      {/* ── YP3 · the warm purchase sheet ──
          Raised by the gate's `license_required` event (hotkey, pill, tray) and
          by a rejected `manual_toggle`. Dismissible, and everything behind it
          keeps working — that is the promise the copy makes and the code has to
          keep. It sits OUTSIDE `.main` so it covers the sidebar too. */}
      {buyPrompt && (
        <PurchasePrompt
          onBuy={buyYap}
          onEnterKey={openLicenseTab}
          onDismiss={() => setBuyPrompt(false)}
        />
      )}

      {/* ── YV96 · the one-time meeting-capture notice ──
          Raised by the backend's `meeting` event the first time a recording
          actually starts, whichever entry point started it, and re-openable
          from Settings → Privacy. It sits OUTSIDE `.main` for the same reason
          the purchase sheet does, and — the whole point of the closed O1
          decision — the recording behind it is already running. */}
      {consentOpen && (
        <MeetingConsentNotice
          recording={consentOpen === "recording"}
          onClose={closeConsentNotice}
        />
      )}

      {/* ── YV98 · the crash report, shown before it exists ──
          Nothing has been written to disk at this point: the bundle is built in
          memory, and this sheet is the user reading it. The zip only lands on
          the Desktop when they press the action. */}
      {supportOpen && (
        <SupportBundleSheet
          preview={supportPreview}
          busy={supportBusy}
          onSend={sendSupportBundle}
          onClose={() => {
            if (supportBusy) return;
            setSupportOpen(false);
            setSupportPreview(null);
          }}
        />
      )}
    </div>
    </AppContext.Provider>
  );
}
