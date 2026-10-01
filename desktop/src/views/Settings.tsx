import Companion from "./settings/Companion";
import Dictation from "./settings/Dictation";
import Snippets from "./settings/Snippets";
import Audio from "./settings/Audio";
import Shortcut from "./settings/Shortcut";
import Advanced from "./settings/Advanced";
import Privacy from "./settings/Privacy";
import License from "./settings/License";
import { SETTINGS_TABS } from "../appTypes";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState } from "../ViewStates";

export default function Settings() {
  const { bootError, booting, refreshAll, settings, settingsTab, setSettingsTab } =
    useAppCtx();
  // PANEL 2026-09-12 — Settings has no empty state either: there is nothing it
  // could be empty OF. Loading + error, and its settled state is the panels.
  const state = viewState(settings, booting, bootError);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="settings"
        view="settings"
        error={bootError}
        actionLabel="Read settings again"
        onAction={() => void refreshAll()}
      />
    );
  if (state === "loading")
    return <LoadingState data-loading-state="settings" noun="settings" rows={5} onRetry={() => void refreshAll()} />;
  return (
            <div className="settings">
              {/* YV27 — segmented sub-nav: one panel at a time, no infinite scroll. */}
              <div className="settings-subnav" role="tablist" aria-label="Settings sections">
                {SETTINGS_TABS.map((t) => (
                  <button
                    key={t.id}
                    type="button"
                    role="tab"
                    aria-selected={settingsTab === t.id}
                    className={
                      settingsTab === t.id ? "subnav-item active" : "subnav-item"
                    }
                    onClick={() => setSettingsTab(t.id)}
                  >
                    {t.label}
                  </button>
                ))}
              </div>

              {/* ── Companion — the on-screen pet + HUD ── */}
              {settingsTab === "companion" && <Companion />}

              {/* ── Dictation — how your speech becomes clean text ── */}
              {settingsTab === "dictation" && <Dictation />}

              {/* ── Snippets (YV48) — spoken triggers expand to saved text ── */}
              {settingsTab === "snippets" && <Snippets />}

              {/* ── Audio — capture cleanup + what plays while you talk ── */}
              {settingsTab === "audio" && <Audio />}

              {/* ── Shortcut — the key you hold to talk ── */}
              {settingsTab === "shortcut" && <Shortcut />}

              {/* ── Advanced — the speech model ──
                  YV34 retired the speed-profile buttons and the "Voice model
                  (advanced override)" list: both picked a repo id for the
                  deleted Python sidecar. Speed vs. accuracy is now a property
                  of which embedded model you download — and since YV54 stopped
                  asking first-run users that question, this panel IS the picker
                  for the people who want to answer it. */}
              {settingsTab === "advanced" && <Advanced />}

              {/* ── Privacy & Diagnostics — your data lives on your Mac ── */}
              {settingsTab === "privacy" && <Privacy />}

              {/* ── YP3 · License — trial, purchase, activation ── */}
              {settingsTab === "license" && <License />}
            </div>
  );
}
