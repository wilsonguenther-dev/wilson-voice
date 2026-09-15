/**
 * Float entry — picks the pill style from settings and renders it.
 *   "classic" → the original obsidian waveform capsule (ClassicPill)
 *   "yappy"   → the pixel-art chick companion (YappyPill)
 * Live-switches when settings are saved (backend emits "settings").
 *
 * YV53: the same settings carry the dock edge (bottom | left | right). The
 * backend moves the NSPanel; here we only mirror the edge onto <html> so the
 * stage can align the pill flush to it — the pill/world itself is never
 * stretched or letterboxed, it just anchors to that side.
 */
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ClassicPill from "./pill/ClassicPill";
import YappyPill from "./pill/YappyPill";
import {
  acceptProgress,
  reduceGatePhase,
  CANCELLED_SETTLE_MS,
  GATED_SETTLE_MS,
  type LivePhase,
  type TranscribeProgress,
} from "./pill/live";
import { pillLicense } from "./pill/license";
import { type LicenseStatus } from "./license/status";
import "./float.css";

interface Settings { pillStyle?: string; pillPosition?: string }

/**
 * PERM-C — the backend's refusal payload. `prompting` is true only while the
 * system dialog is on its way (NotDetermined), which is the difference between
 * "waiting" and "blocked" on the pill.
 */
interface MicGateNotice { status?: string; prompting?: boolean }

/**
 * Y3-C — the backend's per-chunk decode progress. Emitted ONCE per COMPLETED
 * chunk and never for a single-window take, so the mere arrival of one of these
 * is what puts the pill into the `transcribing` phase.
 */
interface ProgressNotice { chunk?: number; of?: number; words_so_far?: number }

const DOCKS = ["bottom", "left", "right"];
const dockOf = (s?: Settings) => {
  const p = s?.pillPosition || "bottom";
  return DOCKS.includes(p) ? p : "bottom";
};

function Float() {
  const [style, setStyle] = useState<string>("classic");
  // PERM-C — the permission phase lives HERE, above both pill styles, because it
  // is a property of the app and not of whichever capsule happens to be drawn.
  // Both pills receive it; neither invents it.
  const [gate, setGate] = useState<LivePhase>("idle");
  // Y3-C — decode progress lives HERE for the same reason `gate` does: it is a
  // property of the take, not of whichever capsule happens to be drawn. Both
  // pills receive it; neither invents it, and neither interpolates it.
  const [progress, setProgress] = useState<TranscribeProgress | null>(null);
  // Y2-A — the license lives HERE for the same reason `gate` and `progress` do:
  // it is a property of the app, not of whichever capsule happens to be drawn.
  // `null` until the first payload lands, which is silence and not an
  // assumption (see `pillLicense`).
  //
  // EVENT-DRIVEN, NOT POLLED. One read at mount for the value that already
  // exists, then the backend's own emissions: `license` on every change
  // (activation, removal, a revocation refresh — lib.rs:4464/4511/4524) and
  // `license_required` from the dictation gate itself (lib.rs:1232), which is
  // the only way the hotkey and pill paths can say why a press did nothing.
  // There is no interval here and there is no second clock: `days_left` arrives
  // already computed, rollback floor included.
  const [license, setLicense] = useState<LicenseStatus | null>(null);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    invoke<LicenseStatus>("license_status")
      .then((s) => { if (!dead) setLicense(s); })
      .catch(() => {
        /* a licensing read that fails must never stop the pill from drawing */
      });
    listen<LicenseStatus>("license", (e) => setLicense(e.payload)).then(push);
    // Y2-C — `license_required` is not just a payload, it is a REFUSED PRESS.
    // Before this, the only thing it moved was the license state, so past the
    // trial the user held the hotkey and the pill did not move — indistinguishable
    // from a broken app. It now also drives the pill's phase, and settles itself
    // back out again so the refusal is answered every press without becoming
    // permanent noise. Fire-and-forget like Y3-D's cancel timer: `gate_settled`
    // only ever moves the phase it owns, so a press that has already moved the
    // pill on is left alone.
    listen<LicenseStatus>("license_required", (e) => {
      setLicense(e.payload);
      setGate((p) => reduceGatePhase(p, { type: "license_required" }));
      setTimeout(
        () => setGate((p) => reduceGatePhase(p, { type: "gate_settled" })),
        GATED_SETTLE_MS,
      );
    }).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<ProgressNotice>("transcribe_progress", (e) =>
      setProgress((p) =>
        acceptProgress(p, {
          chunk: Number(e.payload?.chunk),
          of: Number(e.payload?.of),
          wordsSoFar: Number(e.payload?.words_so_far),
        }),
      ),
    ).then(push);
    // A NEW take wipes the last take's progress, and a finished transcript ends
    // the phase. Without both, a stale "9/12" outlives the decode that made it.
    listen<boolean>("recording", (e) => { if (e.payload) setProgress(null); }).then(push);
    listen<unknown>("transcript", () => setProgress(null)).then(push);
    listen<unknown>("transcript_error", () => setProgress(null)).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    // A press was refused for the microphone. The pill must SHOW it — an
    // invisible refusal is the bug PERM-C exists to fix.
    listen<MicGateNotice>("mic_permission_required", (e) =>
      setGate((p) =>
        reduceGatePhase(p, {
          type: "mic_permission_required",
          status: e.payload?.status ?? "denied",
          prompting: e.payload?.prompting,
        }),
      ),
    ).then(push);
    // TCC answered (the non-blocking request, or the Permissions pane polling).
    listen<string>("microphone-status", (e) =>
      setGate((p) => reduceGatePhase(p, { type: "microphone-status", status: e.payload })),
    ).then(push);
    // A take starting or stopping. Never clears a permission phase — see
    // `reduceGatePhase`.
    listen<boolean>("recording", (e) =>
      setGate((p) => reduceGatePhase(p, { type: "recording", recording: e.payload })),
    ).then(push);

    // Y3-D — the user cancelled the take. The pill acknowledges it and SETTLES
    // back to idle on its own (`cancel_settled`); it never paints an error,
    // because nothing went wrong. The timer is fire-and-forget on purpose: if a
    // new take has already started by the time it fires, `cancel_settled` only
    // ever moves the phase it owns and leaves the new take's alone.
    listen("take_cancelled", () => {
      setGate((p) => reduceGatePhase(p, { type: "take_cancelled" }));
      setTimeout(
        () => setGate((p) => reduceGatePhase(p, { type: "cancel_settled" })),
        CANCELLED_SETTLE_MS,
      );
    }).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  useEffect(() => {
    // A synchronous cleanup can run before the listen() promise resolves
    // (StrictMode double-mount). A `dead` flag unsubscribes a listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    // The dock edge rides on <html> (not React state) — it only drives CSS
    // alignment, so flipping it must not re-render (and remount) the pill.
    const apply = (s?: Settings) => {
      setStyle(s?.pillStyle || "classic");
      document.documentElement.dataset.dock = dockOf(s);
    };
    invoke<Settings>("get_settings").then(apply).catch(() => {});
    listen<Settings>("settings", (e) => apply(e.payload)).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // Y2-A — the POLICY is decided once, here, above both capsules. Y2-B/C/D
  // render `lic`; neither pill re-decides whether the trial is worth
  // mentioning.
  const lic = pillLicense(license);
  // The tone rides on <html> the same way the dock edge does (and for the same
  // reason: it drives CSS and must not remount the pill). It is also what makes
  // this wiring observable in a running app before any capsule draws a chip.
  useEffect(() => {
    document.documentElement.dataset.license = lic.show ? lic.tone : "";
  }, [lic.show, lic.tone]);
  return style === "yappy"
    ? <YappyPill gate={gate} progress={progress} license={lic} />
    : <ClassicPill gate={gate} progress={progress} license={lic} />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
  </React.StrictMode>,
);
