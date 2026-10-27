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
    booting, permsError, perms, refreshPerms,
    runSystemAudioSetup, status, sysAudio, sysAudioBusy, sysAudioGate,
    toast, toastError,
  } = useAppCtx();
  // PANEL 2026-09-12 — Permissions has NO empty state. There are always rows
  // to show; a report with nothing in it is a failed read, which is the error
  // state below, not "you have no permissions". So this view enumerates
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
  const openPane = (pane: string) =>
    invoke("open_privacy_settings", { pane }).catch((e) => toastError(e));
  // PERM-A: once macOS has answered "denied" (or a policy restricts it) the
  // request call is a no-op, so the row's one button becomes the deep link.
  const micAsked =
    perms.microphone ||
    perms.microphoneStatus === "denied" ||
    perms.microphoneStatus === "restricted";
  async function requestMicrophone() {
    // PERM-A: the command returns immediately; macOS answers the dialog on
    // its own schedule. Read the authoritative status back instead of
    // guessing with a fixed timeout.
    try {
      const st = await awaitMicDecision({
        request: () => invoke("request_microphone"),
        read: () => invoke("microphone_status"),
        sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      });
      if (st === "denied") {
        toast(
          nameProduct(
            "Microphone denied — macOS will not ask again. Turn Yap on in System Settings → Privacy & Security → Microphone.",
            product,
          ),
        );
      } else if (st === "restricted") {
        toast(
          "Microphone is restricted by a device policy — an administrator has to allow it.",
        );
      }
    } catch (e) {
      toastError(e);
    }
    refreshPerms();
  }
  async function promptAccessibility() {
    try {
      await invoke("request_accessibility");
    } catch (e) {
      toastError(e);
    }
    setTimeout(refreshPerms, 800);
  }
  const step = setupState(
    sysAudio,
    sysAudioGate.available,
    sysAudioGate.message,
    product,
  );
  const sysAudioDenied =
    step.showDeepLink && sysAudio?.hasRun === true && sysAudio.verdict === "looks_denied";
  // yap-uiux-2026-10-16-SETTINGS (walk row T3) — the grants lead: one row per
  // grant, its status and ONE button. The bundle-id prose is a footnote now,
  // and the speech model is not a permission, so it is a status line here and
  // its picker lives only in Settings → Advanced.
  return (
            <div className="perms">
              <div className="perms-head">
                <p className="muted" data-settled-state="permissions">
                  {permsAllClear(perms)
                    ? `Everything ${product} needs is granted. Nothing to fix here.`
                    : perms.summary}
                </p>
                <button className="primary" onClick={refreshPerms}>
                  Re-check permissions
                </button>
              </div>

              <ul className="perm-list">
                <li data-row className={perms.microphone ? "ok" : "bad"}>
                  <StatusDot ok={perms.microphone} />
                  <div>
                    <strong>Microphone</strong>
                    <p>Turn on <strong>{product}</strong> in Privacy → Microphone.</p>
                    {micAsked ? (
                      <button onClick={() => openPane("Microphone")}>
                        Open Microphone settings
                      </button>
                    ) : (
                      <button className="primary" onClick={requestMicrophone}>
                        Request Microphone
                      </button>
                    )}
                  </div>
                </li>
                <li data-row className={perms.accessibility ? "ok" : "bad"}>
                  <StatusDot ok={perms.accessibility} />
                  <div>
                    <strong>Accessibility</strong>
                    <p>Lets {product} paste with ⌘V and hear the fn hold.</p>
                    {perms.accessibility ? (
                      <button onClick={() => openPane("Accessibility")}>
                        Open Accessibility settings
                      </button>
                    ) : (
                      <button className="primary" onClick={promptAccessibility}>
                        Prompt Accessibility
                      </button>
                    )}
                  </div>
                </li>
                {/* YV102 — the system-audio row. It sits in the SAME list as
                    Microphone and Accessibility rather than in a Notetaker-only
                    corner, because from the user's side it is one more macOS
                    permission for one more Yap feature. What it must never do is
                    read like the others: mic permission is knowable, this one is
                    not, so the row shows what Yap has observed rather than a
                    status it cannot query. After a denial macOS never asks
                    again, so the deep link IS the one button, but ONLY for the
                    looks_denied verdict. setupState also offers the deep link
                    for "failed" (a fault, not a refusal) and for "ran" (a quiet
                    first run); there the remedy is running the check again, so
                    those keep the run button (#268 fix round 1). */}
                <li
                  data-row
                  className={step.tone === "ok" ? "ok" : step.tone === "bad" ? "bad" : ""}
                >
                  <StatusDot ok={step.tone === "ok"} />
                  <div>
                    <strong>System audio (meetings)</strong>
                    <p>{step.label}</p>
                    {sysAudioDenied ? (
                      <button onClick={() => openPane(SYSTEM_AUDIO_PANE)}>
                        {SYSTEM_AUDIO_SETUP.openSettings}
                      </button>
                    ) : (
                      <button
                        disabled={!step.canRun || sysAudioBusy}
                        onClick={runSystemAudioSetup}
                      >
                        {sysAudioBusy ? "Asking macOS…" : step.actionLabel}
                      </button>
                    )}
                  </div>
                </li>
                <li data-row className={carbonRegistered(status) ? "ok" : "bad"}>
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
                <li data-row className={fnDelivering(status) ? "ok" : "bad"}>
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
                <li data-row className={perms.asrOk ? "ok" : "bad"}>
                  <StatusDot ok={perms.asrOk} />
                  <div>
                    <strong>Speech model</strong>
                    <p className="muted">{perms.asrDetail}</p>
                  </div>
                </li>
              </ul>

              <p className="muted tiny perms-footnote">
                Bundle id <code>com.wilsonguenther.wilson-voice</code> appears
                as <strong>{product}</strong> in System Settings → Privacy &amp;
                Security; {product} runs no helper process, so that one row is all you
                ever enable. Capture is in-process, so Dictate never asks for
                Desktop access. After a reinstall, if paste stops working, toggle
                Accessibility off and on, then Re-check.
              </p>
            </div>
  );
}
