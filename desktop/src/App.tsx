// Y5-G — App.tsx is the SHELL ONLY: nav, header, the license chip, the toast
// host and the global modals. Every screen lives in ./views/<Name>.tsx and the
// eight settings panels in ./views/settings/. Shell state lives in ./appShell.
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ChartColumn, ChevronsLeft, ChevronsRight, History as HistoryIcon, House, LoaderCircle,
  Mic, Settings as SettingsIcon, ShieldCheck, Square, StickyNote, BookA, Users,
  type LucideIcon,
} from "lucide-react";
import type { Nav } from "./appTypes";
import {
  NARROW_QUERY, SIDEBAR_TOGGLE_HINT, loadCollapsed, navShortcutHint, railReason, railShown,
  saveCollapsed, sidebarKeyAction,
} from "./sidebarRail";
import { uiLog } from "./views/insightsModel";
import { AppContext, useAppShell } from "./appShell";
import { TEST_CHIP } from "./buildChannel";
import { invoke } from "@tauri-apps/api/core";
import { repairPane } from "./hotkeyStatus";
import Onboarding from "./OnboardingFlow";
import { ModelRibbon } from "./ModelSetup";
import { PermissionHealthRow } from "./PermissionHealthRow";
import { unprovenLine } from "./onboarding";
import { PermissionRepair } from "./PermissionRepairRow";
import { canRepair, nameProduct, repairFromPill } from "./permissionRepair";
import PurchasePrompt from "./license/PurchasePrompt";
import MeetingConsentNotice from "./meetings/MeetingConsentNotice";
import SupportBundleSheet from "./support/SupportBundleSheet";
import type { Toast } from "./toast";
import HideHint from "./HideHint";
import Banner from "./Banner";
import { useGovernor } from "./pill/GovernorChip";
import { governorMainBanner } from "./pill/governor";
import Home from "./views/Home";
import History from "./views/History";
import Permissions from "./views/Permissions";
import Meetings from "./views/Meetings";
import Insights from "./views/Insights";
import Dictionary from "./views/Dictionary";
import Scratchpad from "./views/Scratchpad";
import Settings from "./views/Settings";
import SecureInputBanner from "./SecureInputBanner";
import "./App.css";

export default function App() {
  const ctx = useAppShell();
  const governorBanner = governorMainBanner(useGovernor());
  const {
    bootError, buyPrompt, buyYap, closeConsentNotice, consentOpen,
    copyText, dictionary, dismissToast, finishOnboarding, openModelSettings, openSupportSheet, toasts, toastError,
    history, installUpdateNow, installedVersion, installing, 
    licenseChip, meetings, modelSetup, nav, needsPerms, openLicenseTab,
    perms, pillClass, refreshAll, refreshPerms, replayOnboarding, retryFailed, retrying,
    saveSettings, scratch, sendSupportBundle, setBuyPrompt, setNav, setSettingsTab,
    setSupportOpen, setSupportPreview, setUpdate, settings, skipUpdateVersion, status,
    supportBusy, supportOpen, supportPreview, toast, toggleRecord, update,
    insights, engine,
  } = ctx;

  // yap-1107-sidebar-collapses-to-icons — the icon rail. Both values are read
  // synchronously in the initializers (localStorage + matchMedia), so the
  // first paint after a relaunch already has the right width: no flash.
  const [railChosen, setRailChosen] = useState(() => loadCollapsed(localStore()));
  const [narrow, setNarrow] = useState(() => narrowNow());
  const rail = railShown(railChosen, narrow);
  const railWhy = railReason(railChosen, narrow);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mq = window.matchMedia(NARROW_QUERY);
    let last = mq.matches;
    const onChange = () => {
      if (mq.matches === last) return;
      last = mq.matches;
      setNarrow(mq.matches);
      uiLog("sidebar auto_rail", { narrow: mq.matches, width: window.innerWidth });
    };
    onChange();
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  function setRail(next: boolean, via: "button" | "shortcut") {
    setRailChosen(next);
    const saved = saveCollapsed(localStore(), next);
    uiLog("sidebar toggle", { collapsed: next, via, saved });
    if (!saved) toast("Yap could not save the sidebar choice, so it resets when Yap restarts.");
  }
  // The window-level keys read the latest state through a ref, so the
  // listener is attached once. Ctrl-Cmd-S toggles; Cmd-1..Cmd-8 pick a view.
  // The shortcut recorders consume their keys first (capture phase,
  // preventDefault), and sidebarKeyAction ignores consumed events.
  const railKeys = useRef({ railChosen, narrow, setRail, setNav, ready: false });
  railKeys.current = { railChosen, narrow, setRail, setNav, ready: !!settings?.onboarded };
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const k = railKeys.current;
      if (!k.ready) return;
      const action = sidebarKeyAction(e);
      if (!action) return;
      e.preventDefault();
      if (action.kind === "nav") {
        k.setNav(action.nav);
        uiLog("sidebar nav_shortcut", { nav: action.nav });
        return;
      }
      if (k.narrow) {
        // Under 900 px the rail is the only layout that fits; the remembered
        // choice applies again once the window is wide.
        uiLog("sidebar toggle_ignored", { reason: "narrow" });
        return;
      }
      k.setRail(!k.railChosen, "shortcut");
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Y5-F — what each catalogue action does in the main window. The pill shows
  // the same catalogue line without the button (float-main.tsx).
  function runToastAction(t: Toast) {
    const a = t.action;
    if (!a) return;
    switch (a.kind) {
      case "retry":
        if (a.arg) void retryFailed(a.arg);
        return; // retryFailed drops this toast itself once the row is recovered
      case "copy-again": {
        const row = history.find((h) => h.id === a.arg);
        if (row) copyText(row.text);
        else toast("That transcript is in History");
        break;
      }
      case "support":
        void openSupportSheet();
        break;
      case "permissions":
        setNav("permissions");
        break;
      case "models":
        openModelSettings();
        break;
      case "license":
        openLicenseTab();
        break;
    }
    dismissToast(t.id);
  }

  // yap-1106-home-full-width-room — the greeting is gone from Home (Wilson's
  // approved round two): the page is titled "Home" like every other view, and
  // its lede is the live state the big Hold-fn card used to carry.
  const homeLede = status.recording
    ? "Listening · release fn⌃, or tap Stop, to finish"
    : status.busy
      ? "Transcribing…"
      : "Ready · hold fn⌃ to talk, double-tap for hands-free";
  // The footer's engine sentence: plain words, never a model or vendor name.
  const engineLine = status.modelReady
    ? engine?.loading
      ? "Engine warming up · on this Mac"
      : "Engine ready · on this Mac"
    : "Engine getting ready · on this Mac";
  // yap-uiux-2026-10-16-H6 — the header status chip rendered status.message
  // verbatim, so an idle bridge that reports "" left an unlabeled dark
  // capsule beside the license chip. It always says something now, and
  // carries the same words for VoiceOver and on hover.
  const statusText =
    (status.message ?? "").trim() ||
    (status.recording
      ? "Listening"
      : status.busy
        ? "Transcribing…"
        : status.lastError
          ? "Something went wrong"
          : needsPerms
            ? "Needs permissions"
            : "Ready — hold fn⌃ to dictate");

  // yap-1107-sidebar-collapses-to-icons — Dictate's name when it is an icon.
  const dictateName = status.recording
    ? "Stop listening"
    : status.busy
      ? "Transcribing…"
      : "Dictate  fn⌃";

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
  // has not completed it. This early return REPLACES the main UI, which is not
  // mounted until onboarding finishes (so HideHint claims its hint on mount).
  if (settings && !settings.onboarded) {
    return (
      <Onboarding
        onFinish={(sample, proven) => finishOnboarding(sample, proven)}
        onSkip={() => finishOnboarding(null, false)}
        autoPaste={settings.autoPaste}
        onEnableAutoPaste={() => saveSettings({ ...settings, autoPaste: true }, { quiet: true })}
      />
    );
  }

  return (
    <AppContext.Provider value={ctx}>
    <div className={rail ? "shell rail" : "shell"} data-rail={railWhy}>
      <aside className="sidebar" aria-label="Yap">
        <div className="brand-block">
          <div className="mark">
            <span className={status.recording ? "dot pulse" : "dot"} />
          </div>
          {/* yap-1107-sidebar-version-line-fits — min-width:0 lets this column
              shrink to the 220 px rail instead of growing to its nowrap text. */}
          <div className="brand-text">
            <div className="brand-name">Yap</div>
            <BrandTag version={__APP_VERSION__} />
          </div>
        </div>

        <nav className="nav" id="yap-nav" aria-label="Sections">
          {(
            [
              ["home", "Home", insights?.totalSessions ?? history.length],
              // yap-1106-home-full-width-room — every take, 50 to a page.
              ["history", "History", null],
              ["permissions", "Permissions", needsPerms ? 1 : null],
              // YV94 — Meetings sits next to Home because it is the second
              // thing Yap keeps for you, not a setting.
              ["meetings", "Meetings", meetings.length],
              ["insights", "Insights", null],
              ["dictionary", "Dictionary", dictionary.length],
              ["scratchpad", "Scratchpad", scratch.length],
              ["settings", "Settings", null],
            ] as const
          ).map(([id, label, count]) => {
            const Icon = NAV_ICONS[id];
            const shown = count != null && count > 0;
            const hint = navShortcutHint(id);
            return (
            <button
              key={id}
              className={nav === id ? "nav-item active" : "nav-item"}
              aria-current={nav === id ? "page" : undefined}
              aria-keyshortcuts={hint ? "Meta+" + hint.slice(1) : undefined}
              data-tip={rail ? `${label}${shown ? ` · ${count}` : ""}  ${hint}` : undefined}
              onClick={() => setNav(id)}
            >
              <Icon className="nav-icon" size={18} strokeWidth={1.75} aria-hidden="true" />
              <span className="nav-label">{label}</span>
              {count != null && count > 0 && (
                <span className={id === "permissions" ? "count warn" : "count"}>
                  {count}
                </span>
              )}
            </button>
            );
          })}
        </nav>

        <div className="sidebar-foot">
          {/* YV54 + yap-uiux-2026-10-16-O2 — the speech-model ribbon lives here,
              ONCE, in the shell's own chrome: it used to sit at the top of the
              content area, so every Nav view carried a copy stacked above its
              own heading. It retires itself the moment the engine is ready. */}
          <ModelRibbon setup={modelSetup} />
          <button
            className={status.recording ? "dictate-side live" : "dictate-side"}
            onClick={toggleRecord}
            disabled={status.busy}
            aria-label={rail ? dictateName : undefined}
            data-tip={rail ? dictateName : undefined}
          >
            {/* yap-1107-sidebar-collapses-to-icons — in the rail Dictate is its
                icon, with the same live state (the .live fill, Stop, busy). */}
            {rail ? (
              status.recording ? (
                <Square size={16} strokeWidth={2} aria-hidden="true" />
              ) : status.busy ? (
                <LoaderCircle size={18} strokeWidth={1.75} aria-hidden="true" />
              ) : (
                <Mic size={18} strokeWidth={1.75} aria-hidden="true" />
              )
            ) : null}
            {/* YV56 — "Dictate · fn⌃" was one string, so the verb and the key
                carried identical weight. The action stays native SF; the key is
                data and wears the pixel voice in its own chip, the same
                treatment the nav counts and the header status pill take. */}
            {rail ? null : status.recording ? (
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
          {/* yap-1107-sidebar-collapses-to-icons — the collapse control. Under
              900 px the rail is automatic (spec 1.1), so the control steps
              aside until the window is wide again. */}
          {!narrow && (
            <button
              type="button"
              className="rail-toggle"
              aria-controls="yap-nav"
              aria-expanded={!rail}
              aria-keyshortcuts="Control+Meta+S"
              aria-label={rail ? "Expand sidebar" : "Collapse sidebar"}
              data-tip={rail ? `Expand sidebar  ${SIDEBAR_TOGGLE_HINT}` : undefined}
              title={rail ? undefined : `Collapse sidebar (${SIDEBAR_TOGGLE_HINT})`}
              onClick={() => setRail(!railChosen, "button")}
            >
              {rail ? (
                <ChevronsRight size={16} strokeWidth={1.75} aria-hidden="true" />
              ) : (
                <>
                  <ChevronsLeft size={16} strokeWidth={1.75} aria-hidden="true" />
                  <span>Collapse</span>
                  <span className="rail-toggle-key" aria-hidden="true">{SIDEBAR_TOGGLE_HINT}</span>
                </>
              )}
            </button>
          )}
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          {TEST_CHIP && (
            <div className="test-build-chip" role="note" title="Local test build — not a release">
              {TEST_CHIP}
            </div>
          )}
          <div>
            <h1>
              {nav === "home" && "Home"}
              {nav === "history" && "History"}
              {nav === "permissions" && "Permissions"}
              {nav === "meetings" && "Meetings"}
              {nav === "insights" && "Insights"}
              {nav === "dictionary" && "Dictionary"}
              {nav === "scratchpad" && "Scratchpad"}
              {nav === "settings" && "Settings"}
            </h1>
            <p className="lede">
              {nav === "home" && homeLede}
              {nav === "history" && "Every take you have dictated, 50 to a page, newest first."}
              {nav === "permissions" &&
                nameProduct(
                  "macOS must grant these to Yap itself. Without them, dictation or paste fails.",
                  perms?.productName,
                )}
              {nav === "meetings" &&
                "Every meeting you record, searchable and exportable."}
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
            <div
              className={pillClass}
              role="status"
              aria-label={`Yap status: ${statusText}`}
              title={statusText}
            >
              {statusText}
            </div>
          </div>
        </header>

        {/* Y5-F — the ONE toast surface (toast.ts): at most TOAST_CAP up, the
            same code refreshes instead of stacking, each line stays up for a
            time proportional to its length, and every one can be dismissed.
            The action is the catalogue's (errors.ts) — Retry on the take that
            just failed (YV52), Copy again on an unconfirmed paste (YV74), the
            support sheet for anything Yap has no better advice for. */}
        {toasts.length > 0 && (
          <div className="toast-stack" role="status" aria-live="polite">
            {toasts.map((t) => {
              const busy = t.action?.kind === "retry" && retrying === t.action.arg;
              return (
                <div key={t.id} className={`toast toast-${t.tone}`}>
                  <span>
                    {t.line}
                    {t.count > 1 && <span className="toast-count"> ×{t.count}</span>}
                  </span>
                  {t.action && (
                    <button className="toast-action" disabled={busy} onClick={() => runToastAction(t)}>
                      {busy ? "Retrying…" : t.action.label}
                    </button>
                  )}
                  <button className="toast-dismiss" aria-label="Dismiss" onClick={() => dismissToast(t.id)}>
                    ×
                  </button>
                </div>
              );
            })}
          </div>
        )}

        {/* YV43 — another app holds macOS Secure Input, so the CGEvent tap
            behind fn / fn⌃ is blind and push-to-talk is dead RIGHT NOW. It
            outranks every other banner because nothing below it can be reached
            with the hotkey while this is on, and it names the holder so the
            user knows which app to go release. */}
        {/* yap-1107-secure-input-own-field-quiet — when the holder is Yap's
            own masked key field, SecureInputBanner renders one quiet neutral
            line instead of the red banner. */}
        <SecureInputBanner status={status} />

        {/* yap-p0-hotkeys-no-permission — degraded mode, said out loud: fn is
            not delivering and/or the dictate chord did not register, so the
            line names why — and offers the chord only when it registered. */}
        {status.hotkeyDegraded && !status.secureInputBlocked && (
          <div className="banner blocked" role="status">
            <strong>{status.hotkeyDegraded}</strong>
            {repairPane(status) && (
              <button
                type="button"
                onClick={() =>
                  // yap-p0-perm-repair: a denied or stale Input Monitoring
                  // grant gets the real Repair (tccutil reset for Yap's own
                  // bundle id + the pane + re-probe). A grant the probe reads
                  // as fine is never reset — that only opens the pane.
                  (perms?.probe && canRepair(perms.probe.listenEvent.state)
                    ? repairFromPill("listen_event", {
                        toast,
                        openPane: () =>
                          invoke("open_privacy_settings", { pane: repairPane(status) }),
                        refresh: refreshPerms,
                      })
                    : invoke("open_privacy_settings", {
                        pane: repairPane(status),
                      })
                  ).catch((e) => toastError(e))
                }
              >
                Repair
              </button>
            )}
          </div>
        )}

        {/* yap-lifecycle — once per install, after the first red-X hide. It
            claims the hint on mount, so a hide during onboarding still shows. */}
        <HideHint />

        {/* PERM-E — the single permission surface. Renders NOTHING while all
            four grants are fine, which is why it sits unconditionally here
            rather than behind a nav check: it is its own visibility rule. */}
        <PermissionHealthRow
          report={perms}
          unproven={unprovenLine(settings)}
          onTryIt={replayOnboarding}
        />
        {/* yap-p0-perm-repair — Repair for each denied or stale grant, from
            the same probe onboarding renders. */}
        <PermissionRepair probe={perms?.probe} onChanged={refreshPerms} productName={perms?.productName} />

        {/* yap-1106-resource-governor (FX-03) — one plain sentence while a
            meeting's transcription waits for power, heat or memory. */}
        {governorBanner && (
          <Banner tone="warn" role="status">
            {governorBanner}
          </Banner>
        )}

        {status.modelReady && needsPerms && nav === "home" && (
          <Banner tone="warn" onActivate={() => setNav("permissions")}>
            Setup incomplete — open Permissions to enable Mic / Accessibility
            for {nameProduct("Yap", perms?.productName)}
          </Banner>
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

          {nav === "history" && <History />}

          {/* YV94 — Meetings. Two states in one screen: the list, and one
              meeting's transcript. Nothing here STARTS a meeting — capture is
              YV91 and the entry points (tray, hotkey, pill, the empty state's
              own button) are YV95. This ships the surface that makes a recorded
              meeting findable, readable, exportable and deletable. */}
          {nav === "meetings" && <Meetings />}


          {/* Y5-B — was `nav === "insights" && insights && <Insights />`. The
              `insights &&` guard is exactly the blank page: on a fresh install
              insights is null, so the shell rendered the Insights HEADING and
              then nothing at all. The view now owns its own loading, empty and
              error states, so the shell must let it render to show them. */}
          {nav === "insights" && <Insights />}

          {nav === "dictionary" && <Dictionary />}

          {nav === "scratchpad" && <Scratchpad />}

          {/* Y5-B fix round (#192): no `settings &&` gate - Settings renders its own LoadingState while settings is null. */}
          {nav === "settings" && <Settings />}
        </div>

        {/* yap-1106-home-contained-scroll-footer — only .content scrolls
            (overscroll contained); this footer, like the sidebar, never moves. */}
        <footer className="app-footer" data-app-footer>
          <span className="app-footer-engine" role="status">
            <span className={status.modelReady ? "dot ok" : "dot"} aria-hidden />
            {engineLine}
          </span>
          <span className="app-footer-version">Yap {__APP_VERSION__}</span>
          <span className="app-footer-links">
            <button
              type="button"
              className="link"
              onClick={() => {
                setSettingsTab("privacy");
                setNav("settings");
              }}
            >
              Privacy
            </button>
            <button
              type="button"
              className="link"
              onClick={() => {
                // Help is the guided tour: the same walk-through first run shows.
                console.info("footer help_opened");
                replayOnboarding();
              }}
            >
              Help
            </button>
            <button
              type="button"
              className="link"
              onClick={() => {
                console.info("footer feedback_opened");
                void openSupportSheet();
              }}
            >
              Send feedback
            </button>
          </span>
        </footer>
      </section>

      {/* ── YP3 · the warm purchase sheet ──
          Raised by the gate's `license_required` event (hotkey, pill, tray) and
          by a rejected `manual_toggle`. Dismissible, and everything behind it
          keeps working — that is the promise the copy makes and the code has to
          keep. It sits OUTSIDE `.main` so it covers the sidebar too. */}
      {buyPrompt && (
        <PurchasePrompt
          purchaseOpen={ctx.license?.purchase_open === true}
          foundingOpen={ctx.license?.founding_open === true}
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

/**
 * yap-1107-sidebar-version-line-fits — a SHIPPED version is bare x.y.z; a test
 * or dev build carries a suffix ("0.9.4-loop3.8", "0.9.4+sha"). Only the
 * suffixed ones can be long, so only they leave the one-line look.
 */
function isReleaseVersion(version: string): boolean {
  return /^\d+\.\d+\.\d+$/.test(version.trim());
}

/**
 * The version line under "Yap" in the sidebar.
 *
 * Shipped ("0.9.4"): the one line it always was, same markup, same pixels.
 * Test builds: TWO lines, "v0.9.4-loop3.8" then "local · private". Two lines
 * rather than one ellipsized line because in a 137 px text column the
 * " · local · private" tail alone is 18 of the ~21 Departure Mono cells, which
 * would leave an ellipsized version of about three characters ("v0…") — the
 * one thing Wilson reads this line for. On its own line the version keeps all
 * 21 cells and only a pathological 40-character version is clipped, with an
 * ellipsis and the full string in a title tooltip (set only when it IS
 * clipped). Text-overflow is visual only, so screen readers always read the
 * whole version.
 */
function BrandTag({ version }: { version: string }) {
  const release = isReleaseVersion(version);
  const verRef = useRef<HTMLSpanElement>(null);
  const [clipped, setClipped] = useState(false);

  useLayoutEffect(() => {
    if (release) return;
    const el = verRef.current;
    if (!el) return;
    let live = true;
    const measure = () => {
      if (live) setClipped(el.scrollWidth > el.clientWidth);
    };
    measure();
    // Departure Mono loads after first layout; re-measure once it has.
    document.fonts?.ready.then(measure).catch(() => {});
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    ro?.observe(el);
    return () => {
      live = false;
      ro?.disconnect();
    };
  }, [release, version]);

  if (release) {
    return (
      <div className="brand-tag" data-testid="brand-tag">
        v{version} · local · private
      </div>
    );
  }
  return (
    <div className="brand-tag brand-tag-build" data-testid="brand-tag">
      <span
        ref={verRef}
        className="brand-ver"
        data-testid="brand-ver"
        title={clipped ? `v${version}` : undefined}
      >
        v{version}
      </span>
      <span className="brand-scope">local · private</span>
    </div>
  );
}

/** yap-1107-sidebar-collapses-to-icons — one line icon per view (lucide, the
 *  set the pill already uses; one stroke weight, never emoji). */
const NAV_ICONS: Record<Nav, LucideIcon> = {
  home: House,
  history: HistoryIcon,
  permissions: ShieldCheck,
  meetings: Users,
  insights: ChartColumn,
  dictionary: BookA,
  scratchpad: StickyNote,
  settings: SettingsIcon,
};

/** localStorage, or null where the webview refuses it (it can throw). */
function localStore(): Storage | null {
  try {
    return typeof window !== "undefined" ? window.localStorage : null;
  } catch {
    return null;
  }
}

/** True when the window is under the spec's 900 px rail line right now. */
function narrowNow(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(NARROW_QUERY).matches
    : false;
}
