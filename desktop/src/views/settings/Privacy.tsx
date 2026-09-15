import { invoke } from "@tauri-apps/api/core";
import { acknowledgedLabel } from "../../meetings/consent";
import { setupState, SYSTEM_AUDIO_PANE, SYSTEM_AUDIO_SETUP } from "../../meetings/systemAudio";
import type { SupportBundlePreview } from "../../support/bundle";
import { type CrashEvent, polishSidecarLabel, formatTime } from "../../appTypes";
import { useAppCtx } from "../../appShell";

export default function Privacy() {
  const {
    consent, crashes, engine, replayOnboarding, runSystemAudioSetup,
    saveSettings, setConsentOpen, setCrashes, setNav, setSupportOpen, setSupportPreview,
    settings, sysAudio, sysAudioBusy, sysAudioGate, toast,
  } = useAppCtx();
  if (!settings) return null;
  async function openSupportBundle() {
    setSupportOpen(true);
    setSupportPreview(null);
    try {
      setSupportPreview(
        await invoke<SupportBundlePreview>("preview_support_bundle"),
      );
    } catch (e) {
      setSupportOpen(false);
      toast(String(e));
    }
  }
  return (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Privacy &amp; Diagnostics
                    <span className="sub">
                      Everything stays on your Mac. Export or inspect your data
                      here.
                    </span>
                  </h2>
                  <div className="actions wrap">
                    <button
                      className="primary"
                      onClick={() => saveSettings(settings)}
                    >
                      Save settings
                    </button>
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
                          toast(String(e));
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
                          toast(String(e));
                        }
                      }}
                    >
                      Export diagnostics (logs)
                    </button>
                    <button onClick={() => setNav("permissions")}>
                      Permissions
                    </button>
                    <button onClick={replayOnboarding}>Replay onboarding</button>
                  </div>

                  {/* ── YV96 Recording others — the one-time notice, findable ──
                      The notice itself shows once, on the first meeting anyone
                      records. This line is how it stays reachable afterwards:
                      a one-time notice a user cannot find again is a notice
                      they cannot act on. It is NOT a reminder toggle and not a
                      home-state setting — both were explicitly cut when O1 was
                      closed (finding #13) — it is the same words, on demand. */}
                  <h2 className="settings-section">
                    Recording other people
                    <span className="sub">
                      Yap does not announce itself. Whether you may record
                      someone is your call, and the law differs by state and
                      country.
                    </span>
                  </h2>
                  <p className="tiny">{acknowledgedLabel(consent)}</p>
                  <div className="actions wrap">
                    <button onClick={() => setConsentOpen("review")}>
                      Read the recording notice
                    </button>
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
                    );
                    return (
                      <>
                        <h2 className="settings-section">
                          {SYSTEM_AUDIO_SETUP.title}
                          <span className="sub">{SYSTEM_AUDIO_SETUP.sub}</span>
                        </h2>
                        {SYSTEM_AUDIO_SETUP.paragraphs.map((p) => (
                          <p className="tiny" key={p.slice(0, 24)}>
                            {p}
                          </p>
                        ))}
                        <p className="tiny muted">{SYSTEM_AUDIO_SETUP.fine}</p>
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
                                }).catch((e) => toast(String(e)))
                              }
                            >
                              {SYSTEM_AUDIO_SETUP.openSettings}
                            </button>
                          )}
                        </div>
                      </>
                    );
                  })()}

                  {/* ── YV75 Engine — what is actually running right now ──
                      High cleanup hands the take to a second process
                      (`yap-polish`), and that process needs seconds to load
                      its model. Until it says it is ready Yap keeps the
                      rules-formatted text instead of waiting on it, so without
                      this line a cold or crashed sidecar looks exactly like a
                      polish stage that quietly did nothing. */}
                  <h2 className="settings-section">
                    Engine
                    <span className="sub">
                      The models running on this Mac. Nothing here leaves it.
                    </span>
                  </h2>
                  <p className="tiny">
                    AI polish sidecar: {polishSidecarLabel(engine?.polishSidecar)}
                  </p>

                  {/* ── YV64 Stability — the crashes Yap read back off disk ──
                      macOS writes a .ips report when a process dies, and the
                      panic hook writes a line to yap.log; both are read at
                      startup so the app is no longer the last to know. Nothing
                      here is uploaded and no dictated text is in a row — the
                      details are the crash's structured facts only. */}
                  <h2 className="settings-section">
                    Stability
                    <span className="sub">
                      Crashes Yap noticed, read from this Mac&rsquo;s own crash
                      reports. Stored locally, never uploaded, and they never
                      contain anything you dictated.
                    </span>
                  </h2>
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
                              toast(String(e));
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
                              toast(String(e));
                            }
                          }}
                        >
                          Clear crash history
                        </button>
                      </div>
                    </>
                  )}

                  {/* ── YV98 · one button, and it is never a dead end ──
                      It builds a diagnostics zip, shows exactly what is inside
                      it, and then either opens a pre-filled message with the
                      file attached or drops the file on the Desktop with the
                      address on the clipboard. Which one you get depends on
                      whether macOS can drive your mail app, and the sheet says
                      so before you press anything. Nothing is uploaded on
                      either path — Yap still makes no outbound connection. */}
                  <div className="actions wrap">
                    <button className="primary" onClick={openSupportBundle}>
                      Send crash report to Wilson
                    </button>
                  </div>
                  <p className="tiny">
                    Packs the logs, the crash summary, your permission states and
                    which models are downloaded. Transcripts, recordings and your
                    database are never in it, and the logs are redacted before
                    they are packed — you read the whole thing first.
                  </p>
                </section>
  );
}
