import { invoke } from "@tauri-apps/api/core";
import { carbonRegistered, dictateLabel, fnDelivering } from "../hotkeyStatus";
import { awaitMicDecision } from "../micStatus";
import { nameProduct } from "../permission";
import { setupState, SYSTEM_AUDIO_PANE, SYSTEM_AUDIO_SETUP } from "../meetings/systemAudio";
import StatusDot from "../StatusDot";
import { useAppCtx } from "../appShell";
import { viewState } from "../viewState";
import { ErrorState, LoadingState } from "../ViewStates";
import type { PermissionReport } from "../appTypes";
import { DEV_HOST_LINE, isDevHostAttributed } from "../permissionRepair";

/**
 * Y5-B fix round (#192 BLOCKING) - the exact string permissions.rs emits when
 * `all_critical_ok && parts.is_empty()`. `allCriticalOk` alone is mic + mic
 * auth + ASR; it leaves out Accessibility, Input Monitoring and audio capture,
 * so it cannot license "Nothing to fix here" by itself.
 */
export const PERMS_ALL_CLEAR_SUMMARY = "All critical permissions look good for Yap.";

/** The all-clear copy shows only when the Rust report itself says nothing nags. */
export function permsAllClear(
  perms: Pick<PermissionReport, "allCriticalOk" | "accessibility" | "summary"> & { productName?: string },
): boolean {
  return (
    perms.allCriticalOk &&
    perms.accessibility &&
    (perms.summary === PERMS_ALL_CLEAR_SUMMARY ||
      perms.summary === PERMS_ALL_CLEAR_SUMMARY.replace(/Yap\.$/, `${perms.productName}.`))
  );
}

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
  const product = perms?.productName || "Yap";
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
  // yap-dev-perm-probe-attribution — an unbundled, ad hoc signed dev binary is
  // attributed by TCC to the terminal that launched it, so every grant macOS
  // reports is the terminal's. ONE line: no green rows (they would be the
  // terminal's grants dressed as Yap's) and no Repair (it would reset an
  // identity macOS never consulted for this process).
  if (isDevHostAttributed(perms.probe))
    return (
      <div className="perms">
        <div className="panel intro">
          <p className="muted" data-dev-host-attributed="permissions">
            {DEV_HOST_LINE}
          </p>
        </div>
      </div>
    );
  return (
            <div className="perms">
              <div className="panel intro">
                <h3>Enable for this app only</h3>
                <p>
                  Bundle id <code>com.wilsonguenther.wilson-voice</code> must
                  appear as <strong>{product}</strong> in System Settings →
                  Privacy &amp; Security. {product} runs no helper process, so that
                  one row is the only thing you ever enable.
                </p>
                <p className="muted" data-settled-state="permissions">
                  {permsAllClear(perms)
                    ? `Everything ${product} needs is granted. Nothing to fix here.`
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
                            nameProduct(
                              "Microphone denied — macOS will not ask again. Turn Yap on in System Settings → Privacy & Security → Microphone.",
                              product,
                            ),
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
                  Click <strong>Allow</strong> once for Microphone ({product}). After
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
                      prompts — then enable <strong>{product}</strong> in
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
                      enabled {product}.app — if status still says copy-only,
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
                <li className={carbonRegistered(status) ? "ok" : "bad"}>
                  <StatusDot ok={carbonRegistered(status)} />
                  <div>
                    <strong>{dictateLabel(status)} — works everywhere</strong>
                    <p>
                      {carbonRegistered(status)
                        ? "Registered. Starts and stops a take with no permissions at all."
                        : status.hotkeys?.carbon.state === "failed"
                          ? `Not registered: ${status.hotkeys.carbon.reason}. Pick another chord in Settings → Shortcut.`
                          : "Not registered yet."}
                    </p>
                  </div>
                </li>
                <li className={fnDelivering(status) ? "ok" : "bad"}>
                  <StatusDot ok={fnDelivering(status)} />
                  <div>
                    <strong>Hold fn⌃</strong>
                    <p>
                      {fnDelivering(status)
                        ? "Delivering key events."
                        : status.hotkeyDegraded ??
                          `No key event has reached ${product} yet — press any key.`}
                    </p>
                  </div>
                </li>
                <li className="ok">
                  <StatusDot ok={true} />
                  <div>
                    <strong>Mic capture</strong>
                    <p>
                      In-process audio (cpal) so TCC lists <strong>{product}</strong>,
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
                    product,
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
