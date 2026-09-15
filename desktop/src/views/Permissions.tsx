import { invoke } from "@tauri-apps/api/core";
import { awaitMicDecision } from "../micStatus";
import { setupState, SYSTEM_AUDIO_PANE, SYSTEM_AUDIO_SETUP } from "../meetings/systemAudio";
import StatusDot from "../StatusDot";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState } from "../ViewStates";

export default function Permissions() {
  const {
    booting, modelSetup, openModelSettings, permsError, perms, refreshPerms,
    runSystemAudioSetup, status, sysAudio, sysAudioBusy, sysAudioGate,
    toast,
  } = useAppCtx();
  // PANEL 2026-09-12 — Permissions has NO empty state. There are always seven
  // rows to show; a report with nothing in it is a failed read, which is the
  // error state below, not "you have no permissions". So this view enumerates
  // loading + error + a settled state, and the settled state's good news
  // ("nothing to fix here") is carried by the rows themselves.
  const state = viewState(perms, booting, permsError);
  if (state === "error")
    return (
      <ErrorState
        data-error-state="permissions"
        view="permissions"
        error={permsError}
        actionLabel="Re-check permissions"
        onAction={() => void refreshPerms()}
      />
    );
  if (state === "loading" || !perms)
    return <LoadingState data-loading-state="permissions" noun="permissions" rows={4} onRetry={() => void refreshPerms()} />;
  return (
            <div className="perms">
              <div className="panel intro">
                <h3>Enable for this app only</h3>
                <p>
                  Bundle id <code>com.wilsonguenther.wilson-voice</code> must
                  appear as <strong>Yap</strong> in System Settings →
                  Privacy &amp; Security. Yap runs no helper process, so that
                  one row is the only thing you ever enable.
                </p>
                <p className="muted" data-settled-state="permissions">
                  {perms.allCriticalOk
                    ? "Everything Yap needs is granted. Nothing to fix here."
                    : perms.summary}
                </p>
                <div className="actions">
                  <button className="primary" onClick={refreshPerms}>
                    Re-check permissions
                  </button>
                  {/* YV33 — replaces "Install local ASR", the button that
                      bootstrapped the (now deleted, YV34) Python sidecar and
                      froze the app for minutes doing it. YV54 moved the model
                      picker out of onboarding, so this routes to where it now
                      lives instead of re-opening the overlay. */}
                  <button onClick={openModelSettings}>
                    {modelSetup.ready
                      ? "Manage speech model"
                      : "Choose a speech model"}
                  </button>
                  <button
                    onClick={async () => {
                      // PERM-A: the command returns immediately; macOS answers
                      // the dialog on its own schedule. Read the authoritative
                      // status back instead of guessing with a fixed timeout.
                      try {
                        const status = await awaitMicDecision({
                          request: () => invoke("request_microphone"),
                          read: () => invoke("microphone_status"),
                          sleep: (ms) =>
                            new Promise((r) => setTimeout(r, ms)),
                        });
                        if (status === "denied") {
                          toast(
                            "Microphone denied — macOS will not ask again. Turn Yap on in System Settings → Privacy & Security → Microphone.",
                          );
                        } else if (status === "restricted") {
                          toast(
                            "Microphone is restricted by a device policy — an administrator has to allow it.",
                          );
                        }
                      } catch (e) {
                        toast(String(e));
                      }
                      refreshPerms();
                    }}
                  >
                    Request Microphone
                  </button>
                  <button
                    onClick={async () => {
                      try {
                        await invoke("request_accessibility");
                      } catch (e) {
                        toast(String(e));
                      }
                      setTimeout(refreshPerms, 800);
                    }}
                  >
                    Prompt Accessibility
                  </button>
                </div>
                <p className="muted tiny" style={{ marginTop: 10 }}>
                  Click <strong>Allow</strong> once for Microphone (Yap). After
                  that, Dictate must not re-prompt. ASR runs only under Application
                  Support — never Desktop — so stop-recording must not ask for Desktop
                  folder access. After each reinstall, re-toggle Accessibility if paste
                  stops working.
                </p>
              </div>

              <ul className="perm-list">
                <li className={perms?.microphone ? "ok" : "bad"}>
                  <StatusDot ok={!!perms?.microphone} />
                  <div>
                    <strong>Microphone</strong>
                    <p>
                      In-process capture (Apple Silicon / cpal). Click{" "}
                      <strong>Request Microphone</strong> or Dictate so macOS
                      prompts — then enable <strong>Yap</strong> in
                      Privacy → Microphone.
                    </p>
                    <button
                      onClick={() =>
                        invoke("open_privacy_settings", {
                          pane: "Microphone",
                        }).catch((e) => toast(String(e)))
                      }
                    >
                      Open Microphone settings
                    </button>
                  </div>
                </li>
                <li className={perms?.accessibility ? "ok" : "bad"}>
                  <StatusDot ok={!!perms?.accessibility} />
                  <div>
                    <strong>Accessibility</strong>
                    <p>
                      Required to simulate ⌘V paste (Wispr-style). You already
                      enabled Yap.app — if status still says copy-only,
                      toggle it off/on after this install, then click Re-check.
                    </p>
                    <button
                      onClick={() =>
                        invoke("open_privacy_settings", {
                          pane: "Accessibility",
                        }).catch((e) => toast(String(e)))
                      }
                    >
                      Open Accessibility settings
                    </button>
                  </div>
                </li>
                <li className={status.hotkeyRegistered ? "ok" : "bad"}>
                  <StatusDot ok={status.hotkeyRegistered} />
                  <div>
                    <strong>Hold fn⌃</strong>
                    <p>
                      Carbon hotkey registered by Tauri. If this is red, use the
                      Dictate button. Close Wispr Flow if it steals the combo.
                    </p>
                  </div>
                </li>
                <li className="ok">
                  <StatusDot ok={true} />
                  <div>
                    <strong>Mic capture</strong>
                    <p>
                      In-process audio (cpal) so TCC lists <strong>Yap</strong>,
                      not a helper process. Click Dictate once to trigger the
                      system prompt.
                    </p>
                  </div>
                </li>
                {/* YV102 — the system-audio row. It sits in the SAME list as
                    Microphone and Accessibility rather than in a Notetaker-only
                    corner, because from the user's side it is one more macOS
                    permission for one more Yap feature. What it must never do is
                    read like the others: mic permission is knowable, this one is
                    not, so the row shows what Yap has observed rather than a
                    status it cannot query. */}
                {(() => {
                  const step = setupState(
                    sysAudio,
                    sysAudioGate.available,
                    sysAudioGate.message,
                  );
                  return (
                    <li
                      className={
                        step.tone === "ok"
                          ? "ok"
                          : step.tone === "bad"
                            ? "bad"
                            : ""
                      }
                    >
                      <StatusDot ok={step.tone === "ok"} />
                      <div>
                        <strong>System audio (meetings)</strong>
                        <p>{step.label}</p>
                        <div className="actions wrap">
                          <button
                            disabled={!step.canRun || sysAudioBusy}
                            onClick={runSystemAudioSetup}
                          >
                            {sysAudioBusy ? "Asking macOS…" : step.actionLabel}
                          </button>
                          {step.showDeepLink && (
                            <button
                              onClick={() =>
                                invoke("open_privacy_settings", {
                                  pane: SYSTEM_AUDIO_PANE,
                                }).catch((e) => toast(String(e)))
                              }
                            >
                              {SYSTEM_AUDIO_SETUP.openSettings}
                            </button>
                          )}
                        </div>
                      </div>
                    </li>
                  );
                })()}
                <li className={perms?.asrOk ? "ok" : "bad"}>
                  <StatusDot ok={!!perms?.asrOk} />
                  <div>
                    <strong>Speech model</strong>
                    <p className="muted">{perms?.asrDetail}</p>
                    {!modelSetup.ready && (
                      <button onClick={openModelSettings}>
                        Choose a speech model
                      </button>
                    )}
                  </div>
                </li>
              </ul>
            </div>
  );
}
