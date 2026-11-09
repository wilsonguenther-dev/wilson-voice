/**
 * Float entry — picks the pill style from settings and renders it.
 *   any registered character id (pill/characters/registry.ts); unknown or retired → yappy
 * Live-switches when settings are saved (backend emits "settings").
 *
 * YV53: the same settings carry the dock edge (bottom | left | right). The
 * backend moves the NSPanel; here we only mirror the edge onto <html> so the
 * stage can align the pill flush to it — the pill/world itself is never
 * stretched or letterboxed, it just anchors to that side.
 */
import React, { Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { TEST_CHIP } from "./buildChannel";
import { errorAdvice } from "./errors";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import PillShell from "./pill/PillShell";
import { EngineHealthChip } from "./pill/EngineHealthChip";
import {
  ENGINE_HEALTH_EVENT,
  engineHealthTakesStrip,
  parseEngineHealth,
  type EngineHealth,
} from "./pill/engineHealth";
import { FALLBACK_CHARACTER, resolveCharacter } from "./pill/characters/registry";
import CopyCard from "./pill/CopyCard";
import NudgeRow from "./pill/NudgeRow";
import { DEFAULT_CHATTER_MODE, parseChatterMode, type ChatterMode } from "./pill/chatter";
import { openMeetingNotes, useMeetingPill } from "./pill/MeetingBadge";
import { meetingAfterView } from "./pill/meeting";
import MeetingOfferCard from "./pill/MeetingOfferCard";
import LaunchWalk from "./pill/LaunchWalkScene";
import { forcedLaunchWalk } from "./pill/launchWalk";
import WarmupEngine from "./pill/WarmupEngine";
import { forcedWarmup } from "./pill/warmup";
import LanguageOfferCard from "./pill/LanguageOfferCard";
import MeetingDictationCard from "./pill/MeetingDictationCard";
import WhoTalkingCard from "./pill/WhoTalkingCard";
import { forcedWhoCard } from "./pill/whoTalking";
import { forcedMeetingNotice } from "./pill/meetingDictation";
import BacktrackCaughtCard from "./pill/BacktrackCaughtCard";
import { forcedBacktrackCaught } from "./pill/backtrackCaught";
import PolishRevealCard from "./pill/PolishRevealCard";
import { forcedPolishReveal } from "./pill/polishReveal";
import StyleChipButton from "./pill/StyleChipButton";
import SilentMicRow from "./pill/SilentMicRow";
// yap-1106-partial-transcript-stream — the take's live words in the phase strip.
import { useGovernor } from "./pill/GovernorChip";
import { usePartialTranscript } from "./pill/usePartialTranscript";
import { forcedPartial, partialShows, partialTail } from "./pill/partialTranscript";
import { forcedSilentMic, parseSilentMicWarning, SILENT_MIC_HINT_LINES, silentMicShows } from "./pill/silentMic";
import { startPillControlTargets } from "./pill/hitTargets";
import { forcedStyleChip } from "./pill/styleChip";
import { forcedLanguageOffer } from "./pill/languageOffer";
import { applyDragVisual, currentDock, forcedDragScene } from "./pill/drag";
import { prefersReducedMotion } from "./pill/motion";
import {
  acceptProgress,
  takeErrorLine,
  phaseCopy,
  blindFromStatus,
  blindLine,
  reduceGatePhase,
  reduceTakePhase,
  withBlind,
  TAKE_STAGE_EVENT,
  type TakeStageNotice,
  DEFAULT_STOP_KEY,
  handsFreeLabel,
  isListening,
  stripLine,
  FN_HINT_EVENT,
  FN_HINT_SHOW_MS,
  pasteWaitHint,
  type TakeStageName,
  toChatTone,
  winningPhase,
  advanceLive,
  createLiveState,
  CANCELLED_SETTLE_MS,
  PHASE_HOLD_MS,
  PHASE_ID,
  parseMutedCategories,
  type BlindState,
  type BlindStatusSlice,
  type LivePhase,
  type TakeStatus,
  type TranscribeProgress,
} from "./pill/live";
import {
  GATE_HOLD_IDLE, gateHovered, gateSettleDueIn, onGateHover, reduceGateHold,
  type GateHold,
} from "./pill/gateClick";
import { curseAllowed, curseBlock, voiceFor } from "./pill/voices";
import { useCallRecordingView } from "./pill/CallOffer";
import { isLiveCall } from "./pill/callOfferModel";
import { pillLicense } from "./pill/license";
import { barDock, barRootProps, shellBarPhase, type BarDock } from "./pill/dock";
import {
  CARD_SELECTOR,
  CARD_STACK_VAR,
  stackCards,
  layoutVars,
  meetingMode,
  pillLayout,
  publishPillLayout,
  shrinkDelay,
  layoutCharacterFor,
  type LayoutCharacter,
  type MeetingMode,
  type PillLayout,
} from "./pill/state";
import {
  NO_RAIL_KEYS, railKeysFrom, subscribeRailFailure,
  type ListenEstimate, type RailBindingRow, type RailKeys, type SlotCtx,
} from "./pill/slots";
import {
  forcedPill,
  matrixLanguageOfferScene,
  matrixBacktrackCaughtScene,
  matrixMeetingDictationScenes,
  matrixPolishRevealScenes,
  matrixStyleChipScene,
  matrixSilentMicScene,
  matrixRailScene,
  matrixLicenseScenes,
  matrixPhases,
  matrixDragBlobScene,
  matrixFriendVisitScene,
  matrixLaunchWalkScene,
  matrixFriendVisitWindowScene,
  matrixStuckEngineScenes,
  type ForcedPill,
} from "./pill/forced";
import { FriendVisit } from "./pill/FriendVisit";
import { forcedFriendVisit } from "./pill/friendVisits";
import { type LicenseStatus } from "./license/status";
import { Lock } from "lucide-react";
import "./tokens.css";
import "./float.css";
// Y5-J — the one global focus ring (see main.tsx for why nothing may zero
// `outline`).
import "./a11y/focus.css";

interface Settings {
  pillStyle?: string;
  pillPosition?: string;
  companionTone?: string;
  /** yap-1106-chatter-setting — "Yappy talks": always | less | milestones (backend `yappy_talks`). */
  yappyTalks?: string;
  /** yap-1106-curse-setting — the Badass curse toggle (backend `companion_curse`, off by default). */
  companionCurse?: boolean;
  /**
   * Y8-B — the Scratchpad's "Add to Flow Bar" toggle. Not on the backend's
   * Settings yet: it ships with DB-D (the Scratchpad window), and until then
   * it reads as off and `SCRATCHPAD_WINDOW_LANDED` keeps the slot hidden.
   */
  scratchpadOnBar?: boolean;
  /** WF-17 — Settings › Notifications: the muted tip/news/milestone categories. */
  mutedPillCategories?: string[];
  /** yap-1107 — Settings › Notifications: "Keep the reveal open until I close it". */
  keepPolishRevealOpen?: boolean;
}

/**
 * WF-03 — the rail's keycaps, read from the backend's LIVE hotkey table
 * (`hotkey_bindings`, the same rows Settings › Shortcut draws). Re-read on
 * every `settings` broadcast — a rebind is saved through `save_settings`, which
 * broadcasts — so a tooltip follows a rebind with no restart. A failed read
 * keeps the last good keys and logs once; it never invents a default.
 */
function useRailKeys(): RailKeys {
  const [keys, setKeys] = useState<RailKeys>(NO_RAIL_KEYS);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const read = () => {
      invoke<{ rows?: RailBindingRow[] }>("hotkey_bindings")
        .then((r) => { if (!dead) setKeys(railKeysFrom(r?.rows)); })
        .catch(() => console.warn("pill_rail keys_failed source=hotkey_bindings"));
    };
    read();
    listen("settings", read).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return keys;
}

/** How long a failed rail action's sentence holds the phase strip. */
const RAIL_ERROR_MS = 5000;

/**
 * Y8-B — the WPM slot's numbers while LISTENING: an estimate off voiced
 * seconds (`advanceLive` — the same machine Yappy's commentary runs), plus the
 * wall seconds listened. Driven by the `audio_level` events the backend already
 * emits, re-rendered only when the whole-word estimate or the whole second
 * changes, never on a timer. The seconds are LATCHED when listening ends, so
 * the transcribing rate is real decoded words over the time actually spoken.
 */
function useListenEstimate(listening: boolean): ListenEstimate {
  const [est, setEst] = useState<ListenEstimate>({ estWords: 0, sec: 0 });
  useEffect(() => {
    if (!listening) return;
    const live = createLiveState();
    const t0 = performance.now();
    let last = t0;
    let shownWords = -1;
    let shownSec = -1;
    let dead = false;
    let unsub: (() => void) | undefined;
    setEst({ estWords: 0, sec: 0 });
    listen<number>("audio_level", (e) => {
      const now = performance.now();
      const level = typeof e.payload === "number" ? Math.max(0, Math.min(1, e.payload)) : 0;
      const f = advanceLive(live, (now - last) / 1000, level, "friendly");
      last = now;
      const sec = (now - t0) / 1000;
      if (Math.round(f.estWords) !== shownWords || Math.floor(sec) !== shownSec) {
        shownWords = Math.round(f.estWords);
        shownSec = Math.floor(sec);
        setEst({ estWords: f.estWords, sec });
      }
      // SP-11: the level stream is aimed at the float window; the main window
      // gets its own copy only while it is visible, never a second one here.
    }, { target: "float" }).then((u) => (dead ? u() : (unsub = u)));
    return () => {
      dead = true;
      unsub?.();
      setEst((prev) => ({ ...prev, sec: (performance.now() - t0) / 1000 }));
    };
  }, [listening]);
  return est;
}

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

/**
 * Y5-C — the slice of `build_status` (lib.rs:873) the phase reducer reads. Every
 * field already exists on the event the pill has always listened to; the phase
 * is DERIVED from them, never announced by a new one.
 */
interface BackendStatus extends BlindStatusSlice {
  recording?: boolean;
  busy?: boolean;
  /**
   * Y5-F — `AppStatus` is `#[serde(rename_all = "camelCase")]`, so these three
   * arrive as `engineLoading` / `lastError` / `lastErrorCode`. The snake_case
   * names this interface used to read were never on the wire: the pill could
   * not reach `error` (or `model-loading`) off the status event at all.
   */
  engineLoading?: boolean;
  lastError?: string | null;
  lastErrorCode?: string | null;
  /** yap-hotkey-stop — the live take is in its "Still recording" state. */
  stillRecording?: boolean;
  /**
   * yap24-PILL4 — lib.rs `AppStatus.hands_free`: the take is LATCHED (fn
   * double-tap or the hands-free chord), so the pill shows `listening-locked`.
   */
  handsFree?: boolean;
  /** yap24-PILL4 — lib.rs `AppStatus.chord_label`: the key that ends the latch. */
  chordLabel?: string;
  /** WF-13 — lib.rs `AppStatus.silent_mic_warning`: "I can't hear <mic>". */
  silentMicWarning?: string | null;
}

/** The `transcript` event, for its word count only (lib.rs:2509). */
interface TranscriptNotice { wordCount?: number; word_count?: number }

/**
 * yap24-UI3 — DEV-ONLY: `float.html?phase=…&pill=…&dock=…` pins the pill to one
 * phase so `scripts/pill-matrix.mjs` can photograph every phase x dock x
 * character headlessly. Both operands are compile-time constants that are
 * false in a release build, so rollup removes this branch and `./pill/forced`
 * with it; the released float window ignores its query string entirely.
 */
const FORCED: ForcedPill | null =
  import.meta.env.DEV || __YAP_PILL_MATRIX__ ? forcedPill(window.location.search) : null;
if (import.meta.env.DEV || __YAP_PILL_MATRIX__) {
  // The matrix reads its phase list from HERE — the pill's own phaseVisual
  // table — so a phase a later item adds is photographed automatically.
  (window as unknown as { __YAP_PILL_PHASES__?: unknown }).__YAP_PILL_PHASES__ = [
    // yap-1107-parity-companions — the two narrow parity rows go FIRST (three
    // captures in all): batch 8 stopped the full matrix at its timebox before it
    // reached them, so the sheet's app column had no launch-walk__yappy or
    // friend-visit-window__hoot PNG. First, they are shot even by a cut-short run.
    // The launch walk's first frame (s06), Yappy at the bottom dock.
    matrixLaunchWalkScene(),
    // Yappy visiting Hoot inside the open window (s09).
    matrixFriendVisitWindowScene(),
    ...matrixPhases(),
    ...matrixLicenseScenes(),
    // WF-12 — the language-offer card over an idle pill.
    matrixLanguageOfferScene(),
    matrixBacktrackCaughtScene(),
    // yap-fx-06 — Esc during a meeting, the hands-free question, stop failed.
    ...matrixMeetingDictationScenes(),
    // WF-05 — "Polished — Reveal": the toast, then the opened diff card.
    ...matrixPolishRevealScenes(),
    // WF-20 — the per-app style chip over a listening pill.
    matrixStyleChipScene(),
    // WF-13 — "I can't hear <mic>" + Switch mic over a listening pill.
    matrixSilentMicScene(),
    // yap-1106-pill-notes-and-meeting / WF-03 — the resting rail, forced out.
    matrixRailScene(),
    // WF-02 — the pill mid-drag: the blob plus its three drop zones.
    matrixDragBlobScene(),
    // yap-1106-friend-visits — Tidy visiting a resting pill.
    matrixFriendVisitScene(),
    // yap-1106-stuck-pill-keeps-its-scene — a stuck, then restarting, engine.
    ...matrixStuckEngineScenes(),
  ];
  if (FORCED?.dock) document.documentElement.dataset.dock = FORCED.dock;
  if (forcedDragScene(window.location.search)) {
    const root = document.documentElement;
    applyDragVisual(root, "blob", currentDock(root), prefersReducedMotion());
  }
}

/**
 * WF-04 — right-click (or Ctrl-click, which WebKit reports as the same
 * `contextmenu` event) on the pill pops Yap's native pill menu: Hide for 1
 * hour, the three docks, Microphone, Languages, Open Scratchpad, Open Yap.
 * The rows are decided in Rust from the menu bar's own item table
 * (`tray_menu::pill_context_menu`), so nothing about them lives here. A text
 * field inside a card keeps the system's Cut / Copy / Paste menu.
 */
function wantsPillMenu(target: EventTarget | null): boolean {
  const el = target instanceof Element ? target : null;
  if (!el) return true;
  return !el.closest("input, textarea, [contenteditable=''], [contenteditable='true']");
}

window.addEventListener("contextmenu", (e) => {
  if (!wantsPillMenu(e.target)) return;
  e.preventDefault();
  invoke("pill_context_menu").catch((err: unknown) => {
    // Rust already logged the cause; the pill has no toast surface, and the
    // menu bar icon carries every row of this menu.
    console.warn("pill context_menu failed", err);
  });
});

const dockOf = (s?: Settings): BarDock => barDock(s?.pillPosition);

function Float() {
  // yap-1106-fb-02-pill-first-click — every `[data-pill-control]` publishes
  // its own drawn rect as hit area, holds the panel through a press, and logs
  // `pill.click {control, handled}` (pill/hitTargets.ts).
  useEffect(() => startPillControlTargets(), []);
  const [style, setStyle] = useState<string>(FORCED?.style ?? FALLBACK_CHARACTER);
  // Y5-I — the dock edge is now SHELL state too: it picks the bar's box and
  // its axis (pill/dock.ts). Changing it re-renders the `.bar` root's tokens;
  // the capsule inside keeps its identity, so it never remounts.
  // Y8-B — the dock edge is SHELL state as well as an <html> attribute: the
  // flow-bar slots lay out along its length axis and flip their tooltips off
  // it (pill/slots.tsx).
  const [dock, setDock] = useState<BarDock>(barDock(FORCED?.dock));
  const [scratchpadOnBar, setScratchpadOnBar] = useState(false);
  // PERM-C — the permission phase lives HERE, above both pill styles, because it
  // is a property of the app and not of whichever capsule happens to be drawn.
  // Both pills receive it; neither invents it.
  const [gate, setGate] = useState<LivePhase>("idle");
  // Y3-C — decode progress lives HERE for the same reason `gate` does: it is a
  // property of the take, not of whichever capsule happens to be drawn. Both
  // pills receive it; neither invents it, and neither interpolates it.
  const [progress, setProgress] = useState<TranscribeProgress | null>(FORCED?.progress ?? null);
  const [engineHealth, setEngineHealth] = useState<EngineHealth>(FORCED?.engine ?? "idle");
  useEffect(() => {
    // yap-1106-stuck-pill-keeps-its-scene — the health rides on <html> like the
    // phase (CSS + the matrix's handle); a stuck engine is logged once per change.
    document.documentElement.dataset.engineHealth = engineHealth;
    console.info(`pill engine_scene health=${engineHealth} scene=full`);
  }, [engineHealth]);
  // Y5-C — the TAKE's phase, beside `gate` and for the same reason: which stage
  // of the pipeline is running is a property of the take, not of whichever
  // capsule is drawn. `gate` outranks it (see PHASE_PRECEDENCE); the two are
  // combined ONCE, below, with `winningPhase`.
  const [take, setTake] = useState<LivePhase>("idle");
  // Y5-F — the CODE behind the take's `last_error`. The `error` phase shows the
  // catalogue's line for it (errors.ts), the same line the main window's toast
  // shows — never the raw backend string.
  const [lastErrorCode, setLastErrorCode] = useState<string | null>(null);
  // yap-hotkey-stop — the take the hotkey may have lost track of says so.
  const [stillRecording, setStillRecording] = useState(false);
  // WF-13 — the live take's mic records nothing (record.rs SilentMicCheck).
  const [silentMic, setSilentMic] = useState<string | null>(null);
  // yap24-PILL4 — the key that ends a hands-free take, off the status payload.
  const [chordLabel, setChordLabel] = useState<string>(DEFAULT_STOP_KEY);
  // yap-qa-hotkey-walk-binding — fn held without the binding's ⌃: the backend
  // says so (throttled) and the strip shows it for FN_HINT_SHOW_MS.
  const [fnHint, setFnHint] = useState<string | null>(null);
  // yap-1106-fb-04 — the take's newest backend stage: `paste_waiting` puts
  // "Pastes when you pause" on the strip until the paste goes out.
  const [lastStage, setLastStage] = useState<TakeStageName | null>(null);
  // yap24-PILL3 — the fn key cannot hear the user (Secure Input). Derived from
  // the status payload alone (secure_input.rs's 2 s poll owns the dwell and the
  // loginwindow rule), so it clears the moment the status does; ranked
  // against the take once, below, by `withBlind` (rest only, never a take).
  const [blind, setBlind] = useState<BlindState | null>(null);
  useEffect(() => {
    let dead = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let unsub: (() => void) | undefined;
    listen<string>(FN_HINT_EVENT, (e) => {
      setFnHint(e.payload);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => setFnHint(null), FN_HINT_SHOW_MS);
    }).then((u) => (dead ? u() : (unsub = u)));
    return () => {
      dead = true;
      if (timer) clearTimeout(timer);
      unsub?.();
    };
  }, []);
  const [tone, setTone] = useState<string>("friendly");
  // yap-1106-chatter-setting — "Yappy talks", read live off the settings payload like the tone.
  const [chatter, setChatter] = useState<ChatterMode>(DEFAULT_CHATTER_MODE);
  const [curseSetting, setCurseSetting] = useState(false);
  // WF-17 — quiet mode, read live off the settings payload like the tone.
  const [mutedCats, setMutedCats] = useState<ReadonlySet<string>>(() => new Set());
  const [keepRevealOpen, setKeepRevealOpen] = useState(false);
  // Y2-A — the license lives HERE for the same reason `gate` and `progress` do:
  // it is a property of the app, not of whichever capsule happens to be drawn.
  // `null` until the first payload lands, which is silence and not an
  // assumption (see `pillLicense`).
  //
  // EVENT-DRIVEN, NOT POLLED. One read at mount for the value that already
  // exists, then the backend's own emissions: `license` on every change
  // (activation and removal; no background licensing fetch since yap-lic-00) and
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
    //
    // yap-1106-pill-trial-clickable — the refusal is now the BUY button, so it
    // holds GATE_HOLD_MS (6 s) and for as long as the pointer rests on it, then
    // settles a short grace after the pointer leaves. The policy is pure
    // (pill/gateClick.ts); this effect only owns the one timer.
    let hold: GateHold = reduceGateHold(GATE_HOLD_IDLE, { type: "hover", on: gateHovered(), at: performance.now() });
    let settleTimer: number | null = null;
    const armSettle = () => {
      if (settleTimer !== null) {
        clearTimeout(settleTimer);
        settleTimer = null;
      }
      const due = gateSettleDueIn(hold, performance.now());
      if (due === null || dead) return;
      settleTimer = window.setTimeout(() => {
        settleTimer = null;
        const again = gateSettleDueIn(hold, performance.now());
        if (again === null) return;
        if (again > 0) { armSettle(); return; }
        hold = reduceGateHold(hold, { type: "settled" });
        setGate((p) => reduceGatePhase(p, { type: "gate_settled" }));
      }, due);
    };
    unsubs.push(onGateHover((on) => {
      hold = reduceGateHold(hold, { type: "hover", on, at: performance.now() });
      armSettle();
    }));
    listen<LicenseStatus>("license_required", (e) => {
      setLicense(e.payload);
      setGate((p) => reduceGatePhase(p, { type: "license_required" }));
      hold = reduceGateHold(hold, { type: "shown", at: performance.now() });
      armSettle();
    }).then(push);
    return () => {
      dead = true;
      if (settleTimer !== null) clearTimeout(settleTimer);
      unsubs.forEach((u) => u());
    };
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
  // yap-1106-engine-owner — the speech engine's one health state, pushed by
  // `engine_owner` on every transition (never polled). "Catching up…" while a
  // stuck engine is being waited out, "Restarting…" while it is replaced.
  useEffect(() => {
    let dead = false;
    let unsub: (() => void) | null = null;
    listen<unknown>(ENGINE_HEALTH_EVENT, (e) => setEngineHealth(parseEngineHealth(e.payload))).then(
      (u) => (dead ? u() : (unsub = u)),
    );
    return () => { dead = true; unsub?.(); };
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
  // Y5-C — the take's phase, folded out of the backend's take events.
  // yap24-PILL2 added `take_stage`, the backend's own stage report; the rest: `status` carries `recording`, `busy`,
  // `engine_loading` and `last_error` (build_status, lib.rs:873), `transcript`
  // fires only for a take that produced text (lib.rs:2509), and its ABSENCE is
  // what makes YV16's no-speech exit visible as `empty`.
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    const onStatus = (st?: BackendStatus) => {
      if (!st) return;
      setLastErrorCode(st.lastError ? st.lastErrorCode ?? "take_failed" : null);
      setStillRecording(!!st.recording && !!st.stillRecording);
      setSilentMic(st.recording ? parseSilentMicWarning(st.silentMicWarning) : null);
      const nextBlind = blindFromStatus(st);
      // Keep the same object while nothing changed: every status emit (HUD
      // cadence while recording) would otherwise re-render the shell.
      setBlind((prev) =>
        prev === null || nextBlind === null
          ? nextBlind
          : prev.app === nextBlind.app && prev.chord === nextBlind.chord ? prev : nextBlind);
      setChordLabel(st.chordLabel?.trim() || DEFAULT_STOP_KEY);
      const status: TakeStatus = {
        recording: !!st.recording,
        busy: !!st.busy,
        engineLoading: !!st.engineLoading,
        lastError: st.lastError ?? null,
        handsFree: !!st.handsFree,
      };
      setTake((prev) => reduceTakePhase(prev, { type: "status", status }));
    };
    invoke<BackendStatus>("get_status").then(onStatus).catch(() => {});
    listen<BackendStatus>("status", (e) => onStatus(e.payload)).then(push);
    listen<TranscriptNotice>("transcript", (e) =>
      setTake((prev) => reduceTakePhase(prev, {
        type: "transcript",
        words: e.payload?.wordCount ?? e.payload?.word_count ?? 1,
      }))).then(push);
    listen<unknown>("transcribe_progress", () =>
      setTake((prev) => reduceTakePhase(prev, { type: "progress" }))).then(push);
    // yap24-PILL2 — the backend names the stage (take_stage.rs), which is the
    // only way the pill ever reaches `polishing` and `pasting`.
    listen<TakeStageNotice>(TAKE_STAGE_EVENT, (e) => {
      const st = e.payload;
      if (!st?.stage) return;
      setLastStage(st.stage);
      setTake((prev) => reduceTakePhase(prev, { type: "stage", stage: st.stage, words: st.words }));
    }).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // Y5-C — the DURATION POLICY, armed here because a pure module owns no
  // timers. Every phase but the five that legitimately wait on the user or the
  // OS has a deadline (`PHASE_HOLD_MS`), and the reducer refuses a stale timer
  // that fires after the phase has already moved on.
  useEffect(() => {
    const hold = PHASE_HOLD_MS[take];
    if (hold === null) return;
    const t = window.setTimeout(
      () => setTake((prev) => reduceTakePhase(prev, { type: "hold_elapsed", phase: take })),
      hold,
    );
    return () => clearTimeout(t);
  }, [take]);
  useEffect(() => {
    // A synchronous cleanup can run before the listen() promise resolves
    // (StrictMode double-mount). A `dead` flag unsubscribes a listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    // The dock edge rides on <html> (not React state) — it only drives CSS
    // alignment, so flipping it must not re-render (and remount) the pill.
    const apply = (s?: Settings) => {
      setStyle(FORCED?.style ?? (s?.pillStyle || FALLBACK_CHARACTER));
      const d = FORCED?.dock ? barDock(FORCED.dock) : dockOf(s);
      document.documentElement.dataset.dock = d;
      setDock(d);
      setScratchpadOnBar(!!s?.scratchpadOnBar);
      // Y5-C — the tone decides WHICH line every phase says, so the shell has
      // to know it: the phase strip is drawn once, above both capsules.
      setTone(s?.companionTone || "friendly");
      setChatter(parseChatterMode(s?.yappyTalks));
      setCurseSetting(s?.companionCurse === true);
      setMutedCats(parseMutedCategories(s?.mutedPillCategories));
      setKeepRevealOpen(s?.keepPolishRevealOpen === true);
    };
    invoke<Settings>("get_settings").then(apply).catch(() => {});
    listen<Settings>("settings", (e) => apply(e.payload)).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  // Y2-A — the POLICY is decided once, here, above both capsules. Y2-B/C/D
  // render `lic`; neither pill re-decides whether the trial is worth
  // mentioning.
  // yap24-PILL1 — a forced matrix scene (dev-only, see FORCED) stands in for
  // the backend payload; a release build has FORCED === null.
  const lic = pillLicense(FORCED?.license ?? license);
  // The tone rides on <html> the same way the dock edge does (and for the same
  // reason: it drives CSS and must not remount the pill). It is also what makes
  // this wiring observable in a running app before any capsule draws a chip.
  // Y2-F — the ACTION rides alongside the tone, and for a second reason: it is
  // the one bit that says whether a purchase surface may be reached at all. It
  // is written here, ABOVE the style switch, so `problem` is distinct from
  // `ended` in BOTH capsules without either face deciding anything.
  useEffect(() => {
    document.documentElement.dataset.license = lic.show ? lic.tone : "";
    document.documentElement.dataset.licenseAction = lic.show ? lic.action : "";
  }, [lic.show, lic.tone, lic.action]);
  // Y5-C — ONE phase, decided once. `gate` (permission/license) outranks the
  // take, exactly as PHASE_PRECEDENCE says and exactly as `start_recording`
  // does in the backend: the pill is never allowed to disagree about the reason
  // a press did nothing.
  // yap24-UI3 — a forced phase (dev-only, see FORCED) replaces the derived one
  // wholesale; every consumer below reads `phase`, so nothing else changes.
  const phase = FORCED ? FORCED.phase : withBlind(winningPhase(gate, take), blind);
  const blindNow = FORCED ? FORCED.blind : blind;
  const chatTone = toChatTone(tone);
  useEffect(() => {
    // The phase rides on <html> beside the dock and the license, for the same
    // reason: it drives CSS and must not remount the capsule. It is also the
    // only handle a screenshot or a browser test has on "which state is this".
    document.documentElement.dataset.phase = PHASE_ID[phase];
    document.documentElement.dataset.tone = chatTone;
  }, [phase, chatTone]);
  useEffect(() => {
    // yap-hotkey-stop DO 4 — `.phase-strip` is opacity 0 in `listening`, the
    // only phase a still-recording prompt can occur in; this attribute is the
    // CSS hook that shows it (float.css `html[data-still-recording="true"]`).
    document.documentElement.dataset.stillRecording = stillRecording ? "true" : "false";
  }, [stillRecording]);
  // yap24-NT7 — the meeting lives HERE too, for the reason `gate` does: after
  // stop it is a property of the meeting pipeline, not of whichever capsule is
  // drawn (Y5-K owner decision: the shell owns phases, a character only owns
  // art). Ranked ONCE against the take: a dictation on screen outranks the
  // post-stop badge, so `meetingAfter` is null while one is — except the 2.4 s
  // "Saved" stop beat (fix-b8-1), which keeps its clock box mid-take: it is
  // `meetingAfter` kind "saved", so `meetingMode` is "after" and the layout
  // reserves the slot instead of clipping the beat to 0x0.
  const meeting = useMeetingPill();
  const meetingAfter = meetingAfterView(meeting.phase, phase);
  const dismissMeeting = meeting.dismiss;
  const onMeetingOpen = useCallback(
    (id: string) => {
      openMeetingNotes(id).catch((e) => console.error(e));
      dismissMeeting(id);
    },
    [dismissMeeting],
  );
  useEffect(() => {
    // The same handle `data-phase` gives a screenshot or a browser test.
    document.documentElement.dataset.meetingPhase = meetingAfter?.kind ?? (meeting.status.recording ? "recording" : "");
  }, [meetingAfter?.kind, meeting.status.recording]);
  // yap-1106-launch-walk — a headless capture can pin the walk (`?walk=40`).
  const forcedWalk = FORCED ? forcedLaunchWalk(window.location.search) : null;
  // yap-1106-pill-warmup-engine — the compact warm-up pill paints the same
  // load signal whenever the walk is not on screen (`?warm=` pins it headless).
  const forcedWarm = FORCED ? forcedWarmup(window.location.search) : null;
  const [walkVisible, setWalkVisible] = useState(false);
  const pillProps = { gate: phase, chordLabel, progress, license: lic, meeting: meeting.status, meetingAfter, onMeetingOpen };
  // yap-1106-curse-setting — decided ONCE here, like the voice: the toggle,
  // Badass only, and never in a meeting state (recording, or its after-view on
  // screen). Yap has no screen-share detector (no Screen Recording permission,
  // by rule), so a LIVE CALL the call detector sees (Zoom, Meet, Teams… in any
  // phase but idle, recorded or not) is the stand-in for "the screen may be
  // shared": a call is where screens get shared. `screenShared` is also the
  // hook a real share signal plugs into later.
  const liveCall = isLiveCall(useCallRecordingView());
  const curseGateIn = {
    enabled: curseSetting,
    voice: voiceFor(tone),
    meeting: meeting.status.recording || meetingAfter !== null,
    screenShared: liveCall,
  };
  const curseOn = curseAllowed(curseGateIn);
  const curseWhy = curseBlock(curseGateIn);
  useEffect(() => {
    console.info(`pill voice_curse state=${curseWhy ?? "on"}`);
  }, [curseWhy]);
  useEffect(() => {
    document.documentElement.dataset.fnHint = fnHint ? "true" : "false";
  }, [fnHint]);
  // Y8-B — the slot context is decided ONCE, here, like every other shell
  // input: the phase, the dock, the meeting, Y3-C's real progress and the
  // listening estimate. The capsule lays the slots out; it decides none of it.
  const listenEst = useListenEstimate(isListening(phase));
  const railKeys = useRailKeys();
  // A failed rail action says so in the strip, once, then the strip returns.
  const [railError, setRailError] = useState<string | null>(null);
  useEffect(() => subscribeRailFailure(setRailError), []);
  useEffect(() => {
    if (!railError) return undefined;
    const t = window.setTimeout(() => setRailError(null), RAIL_ERROR_MS);
    return () => clearTimeout(t);
  }, [railError]);
  useEffect(() => {
    document.documentElement.dataset.railError = railError ? "true" : "false";
  }, [railError]);
  // yap-1106-partial-transcript-stream — the governor's view gates the live
  // words (off whenever the engine is not at full).
  const governorView = useGovernor();
  const slots: SlotCtx = {
    phase,
    dock,
    meetingOnScreen: meeting.status.recording || meetingAfter !== null,
    progress,
    listen: listenEst,
    scratchpadOnBar,
    // yap-1106-pill-notes-and-meeting — the rail's inputs (the capsule adds `railOpen`).
    meetingRecording: meeting.status.recording,
    meetingClock: meeting.status.elapsedLabel ?? null,
    meetingTitle: meeting.status.title ?? null,
    keys: railKeys,
    stopKey: chordLabel,
    // A matrix scene forces the rail out; a real pill opens it on hover/focus (PillShell).
    railOpen: FORCED?.rail ? true : undefined,
  };
  // Y5-K — `pill_style` resolves to a registered character ONCE, here, and an
  // unknown or retired id falls back to yappy. The shell never branches on an id: a
  // character that still owns its whole window declares a `stage`.
  const character = resolveCharacter(style);
  const Stage = character.stage;
  // yap-1106-one-pill — ONE layout for the whole panel, decided here from the
  // same three inputs everything else above reads: the phase, the meeting and
  // the dock (pill/state.ts). It places the clock, the hint and the bubble
  // against the character's box and sizes the panel to their union.
  const layoutCharacter: LayoutCharacter = layoutCharacterFor(Boolean(Stage));
  const mMode: MeetingMode = meetingMode(meeting.status, meetingAfter);
  const cardH = useCardOnScreen();
  // WF-13 — while "I can't hear <mic>" is up, the hint slot holds its two-row
  // chip (SILENT_MIC_ROW_H), so the layout reserves SILENT_MIC_HINT_LINES.
  const silentMicWarning = FORCED ? forcedSilentMic(window.location.search) : silentMic;
  const silentMicUp = silentMicShows(silentMicWarning, phase);
  const target = usePanelLayout(phase, mMode, dock, layoutCharacter, cardH, silentMicUp ? SILENT_MIC_HINT_LINES : undefined);
  // yap-1106-partial-transcript-stream — SP-05's committed words plus its live tentative guess, drawn in the
  // phase strip while the take records (pill only, never history; hidden when
  // the governor is not at full). Every line that already owns the strip during
  // a take — a rail error, "Still recording", the fn hint, the engine's health —
  // keeps it; the live words only fill what was a silent strip.
  const partialLive = usePartialTranscript(FORCED ? null : governorView);
  const partialWords = FORCED ? (forcedPartial(window.location.search) ?? "") : partialLive;
  const partialUp = partialShows(partialWords, phase, FORCED ? null : governorView)
    && !railError && !stillRecording && !fnHint
    && !engineHealthTakesStrip(engineHealth, PHASE_ID[phase]);
  useEffect(() => {
    // `.phase-strip` is opacity 0 in `listening`; this is the CSS hook that
    // shows it while live words are drawn (float.css `html[data-partial]`).
    document.documentElement.dataset.partial = partialUp ? "true" : "false";
  }, [partialUp]);
  const pill = Stage ? (
    <Suspense fallback={null}>
      <Stage {...pillProps} tone={chatTone} voice={voiceFor(tone)} chatter={chatter} curse={curseOn} engineHealth={engineHealth} occupant={character.id} />
    </Suspense>
  ) : (
    <PillShell {...pillProps} slots={slots} character={character} tone={chatTone} />
  );
  // Y5-I — the BOX is the shell's, decided ONCE here for every character: the
  // table row for this phase at this dock (pill/dock-geometry.json), carried as
  // two orientation-neutral tokens plus the override attributes float.css reads.
  // A meeting takes the bar only while no take is on screen.
  const bar = barRootProps(
    shellBarPhase(phase, meeting.status.recording || meetingAfter !== null),
    dock,
  );
  // Y5-C — the phase line is placed ONCE, by the SHELL, at all three docks
  // (OWNER DECISION 2026-09-13). It is character-agnostic TEXT; what is
  // per-character is the ART, and that stays inside each capsule. Placed by
  // `.phase-strip` in float.css off `<html data-dock>`, so "bottom, left,
  // right" is three CSS rules and not three components — and not two copies.
  //
  // `error` is the one phase whose line is not a preset: it says WHAT went
  // wrong, in one line — the catalogue's line for the take's error code.
  // yap24-PILL3 — `blind` names the holder and the user's real chord.
  //
  // yap24-PILL4 — `listening-locked` is the other one: its line is the WAY OUT,
  // built from the key the user bound (`chordLabel`), and it wears the shell's
  // lock chip — at all three docks, for both characters, because it is drawn
  // here and not inside either capsule.
  const line = railError
    ?? (phase === "error" ? takeErrorLine(lastErrorCode, errorAdvice(lastErrorCode).line) : null)
    ?? (phase === "blind" && blindNow ? blindLine(blindNow) : null)
    ?? (phase === "listening-locked" ? handsFreeLabel(chordLabel) : null)
    ?? phaseCopy(phase, chatTone);
  return (
    <>
      <PanelFrameSync layout={target} />
      <div className="bar" {...bar}>
        {pill}
        {/* yap-1106-launch-walk — Yappy walks to work while the engine loads
            (drawn from the backend's load percentage). A take during the walk
            still records: the walk only turns into a run. A matrix scene and a
            meeting keep their own world. */}
        <LaunchWalk
          recording={isListening(phase)}
          forced={forcedWalk}
          suppressed={(Boolean(FORCED) && !forcedWalk) || meeting.status.recording || meetingAfter !== null}
          onVisibleChange={setWalkVisible}
        />
        {/* yap-1106-pill-warmup-engine — Yappy starts a little engine in the
            compact pill (72 x 24 window + fill bar): the first-launch download
            and every load after the launch walk. fn still records during it. */}
        <WarmupEngine
          recording={isListening(phase)}
          forced={forcedWarm}
          suppressed={walkVisible || (Boolean(FORCED) && !forcedWarm) || meeting.status.recording || meetingAfter !== null}
        />
      </div>
      {/* yap-1106-friend-visits — a friend visits for its own event only: at
          most 4 s, once a day each, never during a meeting, a screen share or
          Do Not Disturb, and less often after 50 takes (pill/friendVisits.ts). */}
      <FriendVisit
        /* fix-b8-1 — the 2.4 s "Saved" stop beat is still the meeting's
           moment: no visitor walks over Hoot's tape hand-over. */
        meetingRecording={meeting.status.recording || meeting.phase.kind === "saved"}
        meetingNotesReady={meeting.phase.kind === "notes-ready"}
        occupant={character.id}
        forced={FORCED ? forcedFriendVisit(window.location.search) : null}
        chatter={chatter}
      />
      <CopyCard />
      {/* yap-notetaker-detect — "Record this meeting?" (never auto-records). */}
      <MeetingOfferCard />
      {/* WF-12 — "Sounded like Spanish — Add Spanish?" (Add / Not now). */}
      <LanguageOfferCard forced={FORCED ? forcedLanguageOffer(window.location.search) : null} />
      {/* yap-fx-06 — "Dictation cancelled — meeting still recording. Stop meeting?",
          the once-per-meeting hands-free question, and a Stop that failed. */}
      <MeetingDictationCard forced={FORCED ? forcedMeetingNotice(window.location.search) : null} />
      {/* yap-1106-fb-22a — "Who's talking right now?" (off by default; never
          during a screen share; never takes focus). pill/whoTalking.ts. */}
      <WhoTalkingCard
        recording={meeting.status.recording}
        forced={FORCED ? forcedWhoCard(window.location.search) : null}
      />
      {/* WF-06 — "Backtrack caught: Dropped ‘2’, kept ‘3’" (Undo / Dismiss). */}
      <BacktrackCaughtCard
        forced={FORCED ? forcedBacktrackCaught(window.location.search) : null}
        muted={mutedCats}
      />
      {/* WF-05 — "Polished — Reveal" (Reveal → diff with Use original / Keep). Muted with Tips. */}
      <PolishRevealCard
        forced={FORCED ? forcedPolishReveal(window.location.search) : null}
        muted={mutedCats}
        keepOpen={keepRevealOpen}
      />
      {/* Y8-D — the coaching nudge (`growthNudge`). Never during a take: it
          is only evaluated at rest, and a forced matrix phase never shows one. */}
      {!FORCED && <NudgeRow phase={phase} tone={chatTone} muted={mutedCats} chatter={chatter} />}
      <div
        className="phase-strip"
        data-phase={PHASE_ID[phase]}
        data-tone={chatTone}
        // Y5-J — plain visible copy, NOT a live region: the pill root's
        // PhaseAnnouncer is the one place a phase change is announced, and a
        // second region here read every change twice.
      >
        {phase === "listening-locked" && (
          <Lock className="lock-chip" data-lock-chip="true" aria-hidden="true" strokeWidth={2.4} />
        )}
        {/* yap-1106-engine-owner — while the engine is loading, stuck or
            restarting, THAT is the one line worth saying during a decode. */}
        {engineHealthTakesStrip(engineHealth, PHASE_ID[phase]) ? (
          <EngineHealthChip state={engineHealth} />
        ) : partialUp ? (
          // The newest words that fit one line; the hands-free way out stays
          // the strip's tooltip and the rail's Stop. Not a live region: the
          // words change every half second and would flood a screen reader.
          <span
            className="partial-words"
            data-partial-words="true"
            title={phase === "listening-locked" ? handsFreeLabel(chordLabel) : undefined}
          >
            {partialTail(partialWords)}
          </span>
        ) : phase === "listening-locked" && !railError && !stillRecording && !fnHint ? (
          // Item 18 — the hands-free hint, shorter: the key as a keycap and two
          // words. The full sentence stays the accessible name and the
          // announcement (`handsFreeLabel`); the clickable Stop is the rail's.
          <span className="hands-free-hint" title={line ?? undefined}>
            <kbd className="keycap">{chordLabel.trim() || DEFAULT_STOP_KEY}</kbd> to stop
          </span>
        ) : stripLine(line, stillRecording, fnHint ?? pasteWaitHint(lastStage, phase), chordLabel)}
      </div>
      {/* WF-20 — "Slack · casual": the app-aware mode for this app; a click
          changes it for this app for 15 minutes. Listening only, and never
          when app-aware modes are off. A later sibling of .bar, so the side
          dock rules in float.css can place it like the phase line. */}
      <StyleChipButton phase={phase} forced={FORCED ? forcedStyleChip(window.location.search) : null} />
      {/* WF-13 — "I can't hear <mic>" + Switch mic, while the take keeps
          recording. Takes the hint row; the style chip yields it (float.css). */}
      <SilentMicRow
        warning={silentMicWarning}
        phase={phase}
        forced={!!FORCED}
      />
    </>
  );
}

/**
 * yap-1106-one-pill — the layout, memoised on its geometric identity so a
 * phase change inside the same class (listening → transcribing) is not a new
 * layout.
 */
function usePanelLayout(
  phase: LivePhase,
  meeting: MeetingMode,
  dock: BarDock,
  character: LayoutCharacter,
  cardH: number | null,
  hintLines?: number,
): PillLayout {
  const next = pillLayout({ phase, meeting, dock, character, card: cardH ? { h: cardH } : null, hintLines });
  return useMemo(() => next, [next.key]);
}

/**
 * yap-1106-one-pill, fix-b1-2 — the height of the card row: every card on
 * screen that CARD_SELECTOR (pill/state.ts) names — the copy card
 * `.copy-card`, "Record this meeting?" `.meeting-offer-wrap`, WF-12's
 * "Sounded like Spanish — Add Spanish?" `.language-offer-wrap` and the
 * coaching nudge `.nudge-row` — or null when none is up. The cards mount and
 * unmount themselves, so the shell watches the DOM for them rather than
 * re-deciding when they show; the panel then makes room for them instead of
 * clipping them or drawing them over Yappy. Two cards up at once (a nudge and
 * a language offer) are STACKED, not the first match taken: each card gets
 * its place in the row as CARD_STACK_VAR, which float.css adds to --card-y,
 * and the layout reserves the whole stack.
 */
function useCardOnScreen(): number | null {
  const [h, setH] = useState<number | null>(null);
  useEffect(() => {
    const root = document.getElementById("root");
    if (!root) return;
    let els: HTMLElement[] = [];
    let lastCount = -1;
    const measure = () => {
      const live = els.filter((el) => el.isConnected);
      const { offsets, total } = stackCards(live.map((el) => el.getBoundingClientRect().height));
      live.forEach((el, i) => el.style.setProperty(CARD_STACK_VAR, `${offsets[i]}px`));
      if (live.length !== lastCount) {
        // One line per change in the card count, never per resize.
        console.info(`pill card_row cards=${live.length} h=${total}`);
        lastCount = live.length;
      }
      setH(total > 0 ? total : null);
    };
    const ro = new ResizeObserver(() => measure());
    const track = () => {
      const found = Array.from(root.querySelectorAll<HTMLElement>(CARD_SELECTOR));
      if (found.length !== els.length || found.some((el, i) => el !== els[i])) {
        for (const el of els) ro.unobserve(el);
        els = found;
        for (const el of els) ro.observe(el);
      }
      measure();
    };
    track();
    // childList only: the stack offsets this hook writes are style attributes,
    // so writing them never re-triggers the observer.
    const mo = new MutationObserver(track);
    mo.observe(root, { childList: true, subtree: true });
    return () => {
      mo.disconnect();
      ro.disconnect();
    };
  }, []);
  return h;
}

/**
 * yap-1106-one-pill — applies the layout: grows at once, shrinks after
 * SHRINK_HOLD_MS (so Yappy's closing animation is never clipped), writes the
 * slot positions float.css reads, publishes the layout for Yappy's draw loop
 * and the pill matrix, and asks the backend for the matching panel frame.
 * Renders nothing.
 */
function PanelFrameSync({ layout }: { layout: PillLayout }) {
  const [shown, setShown] = useState<PillLayout>(layout);
  const lastSurfaceRef = useRef<string>("");
  useEffect(() => {
    if (shown.key === layout.key) return;
    const delay = shrinkDelay(shown, layout);
    if (delay === 0) {
      setShown(layout);
      return;
    }
    const t = window.setTimeout(() => setShown(layout), delay);
    return () => window.clearTimeout(t);
  }, [layout, shown]);
  useEffect(() => {
    const de = document.documentElement;
    de.dataset.onePill = shown.legacy ? "false" : "true";
    de.dataset.pillMeeting = shown.meeting;
    // yap-1107-one-pill-during-meeting — the ONE meeting shell (float.css draws
    // it behind Yappy and the clock), and whether the scene is the character.
    de.dataset.meetingShell = !shown.legacy && shown.shell ? "true" : "false";
    de.dataset.sceneCharacter = !shown.legacy && shown.sceneCharacter ? "true" : "false";
    const surface = shown.legacy ? "legacy" : shown.sceneCharacter ? "scene" : shown.shell ? "shell" : "none";
    if (surface !== lastSurfaceRef.current) {
      console.info(`pill meeting_surface mode=${surface} meeting=${shown.meeting} dock=${shown.dock}`);
      lastSurfaceRef.current = surface;
    }
    const vars = layoutVars(shown);
    for (const [k, v] of Object.entries(vars)) {
      if (shown.legacy) de.style.removeProperty(k);
      else de.style.setProperty(k, v);
    }
    publishPillLayout(shown.legacy ? null : shown);
    invoke("pill_set_panel_frame", {
      w: shown.panel.w,
      h: shown.panel.h,
      anchorX: shown.anchor.x,
      anchorY: shown.anchor.y,
    }).catch((e) => console.warn("pill panel_frame failed", e));
  }, [shown]);
  return null;
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <Float />
    {TEST_CHIP && (
      <div className="float-test-chip" role="note" title="Local test build — not a release">
        {TEST_CHIP}
      </div>
    )}
  </React.StrictMode>,
);
