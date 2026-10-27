import { invoke } from "@tauri-apps/api/core";
import { acknowledgedLabel } from "../../meetings/consent";
import { setupState, SYSTEM_AUDIO_PANE, SYSTEM_AUDIO_SETUP } from "../../meetings/systemAudio";
import { type CrashEvent, polishSidecarLabel, formatTime } from "../../appTypes";
import { useAppCtx } from "../../appShell";
import { nameProduct } from "../../permission";

export default function Privacy() {
  const {
    consent, crashes, engine, replayOnboarding, runSystemAudioSetup,
    saveSettings, setConsentOpen, setCrashes, setNav, openSupportSheet,
    settings, sysAudio, sysAudioBusy, sysAudioGate, toast, toastError, perms,
  } = useAppCtx();
  if (!settings) return null;
  // Y5-F — the same sheet the generic error advice's "Send report" opens.
  const openSupportBundle = openSupportSheet;
  return (
                <section className="settings-panel" data-settings-pane="privacy">
                  <div className="panel" data-row>
                    <h3>Your data</h3>
                    <div className="actions wrap">
                      <button
                        onClick={async () => {
                          try {
                            const { path, count } = await invoke<{
                              path: string;
                              count: number;
                            }>("export_history");
                            toast(
                              `Exported ${count.toLocaleString()} transcripts → ${path}`,
                            );
                          } catch (e) {
                            toastError(e);
                          }
                        }}
                      >
                        Export transcripts
                      </button>
                      <button onClick={() => invoke("open_data_dir")}>
                        Open data folder
                      </button>
                      <button
                        onClick={async () => {
                          try {
                            await invoke("open_logs_dir");
                            // YV64: the export now writes crash-summary.txt into
                            // that folder alongside yap.log.
                            toast("Opened logs folder — crash summary included");
                          } catch (e) {
                            toastError(e);
                          }
                        }}
                      >
                        Export diagnostics (logs)
                      </button>
                      </div>
                    <p className="settings-hint">
                      Everything stays on your Mac; export or open it from here.
                    </p>
                  </div>

                  {/* ── YV96 Recording others — the one-time notice, findable ──
                      The notice itself shows once, on the first meeting anyone
                      records. This line is how it stays reachable afterwards:
                      a one-time notice a user cannot find again is a notice
                      they cannot act on. It is NOT a reminder toggle and not a
                      home-state setting — both were explicitly cut when O1 was
                      closed (finding #13) — it is the same words, on demand. */}
                  <div className="panel" data-row>
                    <h3>Recording other people</h3>
                    <div className="actions wrap">
                      <button onClick={() => setConsentOpen("review")}>
                        Read the recording notice
                      </button>
                    </div>
                    <p className="settings-hint">
                      Yap does not announce itself; whether you may record
                      someone is your call, and the law differs by place.{" "}
                      {acknowledgedLabel(consent)}
                    </p>
                  </div>

                  {/* ── YV102 Set up meeting recording — the TCC pre-warm ──
                      There is NO permission-request API for system audio: the
                      macOS alert is a side effect of starting a process tap,
                      and if it is dismissed it never appears again (OS-10). So
                      the tap is started here, on purpose, from a quiet screen
                      where the explanation is already visible — instead of at
                      T-0 of the user's first Zoom join, where a reflex
                      dismissal is permanent for the install.

                      Order matters in the markup as much as in the code: every
                      paragraph below is above the button, because the alert
                      steals focus the instant the button is pressed. */}
                  {(() => {
                    const step = setupState(
                      sysAudio,
                      sysAudioGate.available,
                      sysAudioGate.message,
                      perms?.productName,
                    );
                    return (
                      <div className="panel" data-row>
                        <h3>{SYSTEM_AUDIO_SETUP.title}</h3>
                        <p className="tiny muted">{nameProduct(SYSTEM_AUDIO_SETUP.sub, perms?.productName)}</p>
                        {SYSTEM_AUDIO_SETUP.paragraphs.map((p) => (
                          <p className="tiny" key={p.slice(0, 24)}>
                            {nameProduct(p, perms?.productName)}
                          </p>
                        ))}
                        <p className="tiny muted">{nameProduct(SYSTEM_AUDIO_SETUP.fine, perms?.productName)}</p>
                        <p
                          className={
                            step.tone === "bad" ? "tiny warn" : "tiny muted"
                          }
                        >
                          {step.label}
                        </p>
                        <div className="actions wrap">
                          <button
                            className={step.tone === "bad" ? "" : "primary"}
                            disabled={!step.canRun || sysAudioBusy}
                            onClick={runSystemAudioSetup}
                          >
                            {sysAudioBusy ? "Asking macOS…" : step.actionLabel}
                          </button>
                          {/* The ONLY recovery after a denial: TCC will not ask
                              a second time, so a working deep link is the whole
                              path back. The anchor is verified on the target OS
                              (see permissions::SYSTEM_AUDIO_PANE) rather than
                              guessed — a wrong one lands on the top of System
                              Settings, which is a worse dead end than no link. */}
                          {step.showDeepLink && (
                            <button
                              onClick={() =>
                                invoke("open_privacy_settings", {
                                  pane: SYSTEM_AUDIO_PANE,
                                }).catch((e) => toastError(e))
                              }
                            >
                              {SYSTEM_AUDIO_SETUP.openSettings}
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })()}

                  {/* ── YV75 Engine — what is actually running right now ──
                      High cleanup hands the take to a second process
                      (`yap-polish`), and that process needs seconds to load
                      its model. Until it says it is ready Yap keeps the
                      rules-formatted text instead of waiting on it, so without
                      this line a cold or crashed sidecar looks exactly like a
                      polish stage that quietly did nothing. */}
                  <div className="panel" data-row>
                    <h3>Engine</h3>
                    <p className="tiny">
                      AI polish sidecar: {polishSidecarLabel(engine?.polishSidecar)}
                    </p>
                    <p className="settings-hint">
                      The models running on this Mac. Nothing here leaves it.
                    </p>
                  </div>

                  {/* ── YV64 Stability — the crashes Yap read back off disk ──
                      macOS writes a .ips report when a process dies, and the
                      panic hook writes a line to yap.log; both are read at
                      startup so the app is no longer the last to know. Nothing
                      here is uploaded and no dictated text is in a row — the
                      details are the crash's structured facts only. */}
                  <div className="panel" data-row>
                    <h3>Stability</h3>
                    {crashes.length === 0 ? (
                      <p className="tiny">
                        No crashes recorded. Yap has not died on this Mac since it
                        started keeping track.
                      </p>
                    ) : (
                      <>
                        <ul className="crash-list">
                          {crashes.map((c) => (
                            <li
                              key={c.id}
                              className={c.acknowledged ? "crash" : "crash new"}
                            >
                              <div className="crash-meta">
                                <span>{formatTime(c.occurredAt)}</span>
                                <span className="crash-kind">{c.kind}</span>
                              </div>
                              <p className="crash-signature">{c.signature}</p>
                              <p className="tiny">{c.sourceFile}</p>
                            </li>
                          ))}
                        </ul>
                        <div className="actions wrap">
                          <button
                            disabled={crashes.every((c) => c.acknowledged)}
                            onClick={async () => {
                              try {
                                await invoke("acknowledge_crash_events");
                                setCrashes(
                                  await invoke<CrashEvent[]>("list_crash_events"),
                                );
                                toast("Marked as seen");
                              } catch (e) {
                                toastError(e);
                              }
                            }}
                          >
                            Mark as seen
                          </button>
                          <button
                            className="ghost danger"
                            onClick={async () => {
                              try {
                                await invoke("clear_crash_events");
                                setCrashes(
                                  await invoke<CrashEvent[]>("list_crash_events"),
                                );
                                toast("Crash history cleared");
                              } catch (e) {
                                toastError(e);
                              }
                            }}
                          >
                            Clear crash history
                          </button>
                        </div>
                      </>
                    )}

                    <p className="settings-hint">
                      Read from this Mac&rsquo;s own crash reports; stored
                      locally, never uploaded, never anything you dictated.
                    </p>
                  </div>
                  {/* ── YV98 · one button, and it is never a dead end ──
                      It builds a diagnostics zip, shows exactly what is inside
                      it, and then either opens a pre-filled message with the
                      file attached or drops the file on the Desktop with the
                      address on the clipboard. Which one you get depends on
                      whether macOS can drive your mail app, and the sheet says
                      so before you press anything. Nothing is uploaded on
                      either path — Yap still makes no outbound connection. */}
                  <div className="panel" data-row>
                    <h3>Support</h3>
                    <div className="actions wrap">
                      <button className="primary" onClick={openSupportBundle}>
                        Send crash report to Wilson
                      </button>
                    </div>
                    <p className="settings-hint">
                      Redacted logs, crash summary, permission states and model
                      list — never transcripts or recordings; you read it first.
                    </p>
                  </div>
                  {/* yap-uiux-2026-10-16-SETTINGS — save and the two "go
                      elsewhere" actions sit in the pinned footer. */}
                  <div className="settings-footer">
                    <button onClick={() => setNav("permissions")}>
                      Permissions
                    </button>
                    <button onClick={replayOnboarding}>Replay onboarding</button>
                    <button
                      className="primary"
                      onClick={() => saveSettings(settings)}
                    >
                      Save settings
                    </button>
                  </div>
                </section>
  );
}
