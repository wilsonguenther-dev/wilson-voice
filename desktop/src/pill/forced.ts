/**
 * yap24-UI3 — the pill matrix's handle on the float window. DEV-ONLY.
 *
 * `scripts/pill-matrix.mjs` renders float.html once per phase x dock x
 * character in headless Chrome. A real phase only happens when the backend
 * emits the right events in the right order, which a browser cannot do, so
 * the matrix forces one through the query string instead:
 *
 *     float.html?phase=transcribing&pill=yappy&dock=left
 *
 * `phase` takes the TypeScript variant (`model-loading`) or the product id
 * (`model_loading`, PHASE_ID). `pill` and `dock` are optional and fall back to
 * whatever settings say.
 *
 * WHY THIS NEVER SHIPS: float-main only calls into this module behind
 * `import.meta.env.DEV || __YAP_PILL_MATRIX__`. Both are compile-time
 * constants that are `false` in `npm run build` (vite.config.ts defines
 * `__YAP_PILL_MATRIX__` from `YAP_PILL_MATRIX=1`, which only the matrix sets),
 * so rollup drops the branch and this module with it — a released float
 * window has no query parameter that can move its phase.
 *
 * Pure: no DOM, no React, no timers, so the parse is unit-tested on its own.
 */
import {
  ALL_LIVE_PHASES,
  PHASE_ID,
  SAMPLE_BLIND,
  phaseVisual,
  type BlindState,
  type LivePhase,
  type PhaseVisual,
  type TranscribeProgress,
} from "./live";
import { type LicenseStatus } from "../license/status";

/**
 * yap-1106-retire-classic-pill — Classic is retired, so the matrix no longer
 * forces it: `?pill=classic` is an unknown style (null) and the page falls back
 * to settings, exactly like a stale setting falls back to Yappy.
 */
export const PILL_STYLES = ["yappy"] as const;
export type PillStyle = (typeof PILL_STYLES)[number];
export const PILL_DOCKS = ["bottom", "left", "right"] as const;
export type PillDock = (typeof PILL_DOCKS)[number];

/**
 * `transcribing` is the one phase whose capsule draws data the phase alone
 * does not carry: a determinate fill and its numeral (Y3-C). Without a sample
 * the matrix would photograph a transcribing pill with no progress — a state
 * the app never shows, since that phase is only entered on a progress event.
 */
export const SAMPLE_PROGRESS: TranscribeProgress = { chunk: 3, of: 7, wordsSoFar: 58 };

export interface ForcedPill {
  phase: LivePhase;
  style: PillStyle | null;
  dock: PillDock | null;
  progress: TranscribeProgress | null;
  /**
   * yap24-PILL3 — `blind` is the other phase whose line needs data the phase
   * does not carry (the holder and the chord). Without a sample the matrix
   * would photograph the tone fallback, a line the app only says when the
   * status names neither.
   */
  blind: BlindState | null;
  /** yap24-PILL1 — a forced license payload (`&license=<scene>`), or null. */
  license: LicenseStatus | null;
  /**
   * yap-1106-pill-notes-and-meeting / WF-03 — `&rail=open` draws the resting
   * pill with its hover rail out, so the matrix photographs the rail at every
   * dock without a pointer.
   */
  rail: boolean;
}

/**
 * yap24-PILL1 — license SCENES the matrix photographs on an idle pill, so a
 * PILL item's license treatment is looked at in 3 docks x every character like any
 * phase: `float.html?phase=idle&license=trial-quiet`. Each scene is a payload
 * the backend could really send, with checkout closed (`PURCHASE_OPEN`).
 * `trial-quiet` uses day 14 because "14d" is the widest numeral the policy
 * produces — the geometry worst case on a 30px side dock.
 */
export const LICENSE_SCENES = {
  "trial-quiet": 14,
  trial: 5,
  urgent: 0,
  ended: -1,
} as const;
export type LicenseScene = keyof typeof LICENSE_SCENES;

export function sceneLicense(raw: string | null | undefined): LicenseStatus | null {
  if (!raw || !Object.prototype.hasOwnProperty.call(LICENSE_SCENES, raw)) return null;
  const days: number = LICENSE_SCENES[raw as LicenseScene];
  const shared = {
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: Math.max(0, days),
    trial_expires_at_ms: 0,
    revocation_checked_at_ms: null,
    revoked_count: 0,
    purchase_open: false,
    founding_open: false,
  };
  return days < 0
    ? { state: "license_required", reason: "trial_expired", ...shared }
    : { state: "trial", days_left: days, expires_at_ms: 0, ...shared };
}

/** A phase by its variant name or its product id; null for anything else. */
export function resolvePhase(raw: string | null | undefined): LivePhase | null {
  if (!raw) return null;
  const want = raw.trim();
  return ALL_LIVE_PHASES.find((p) => p === want || PHASE_ID[p] === want) ?? null;
}

/**
 * The forced pill named by a query string, or null when it names none.
 * An UNKNOWN phase is null too, never a guess: the matrix asserts that
 * `<html data-phase>` carries the id it asked for, so a typo surfaces there as
 * "phase not applied" instead of a convincing photo of `idle`.
 */
export function forcedPill(search: string): ForcedPill | null {
  const q = new URLSearchParams(search);
  const phase = resolvePhase(q.get("phase"));
  if (!phase) return null;
  const pill = q.get("pill");
  const dock = q.get("dock");
  return {
    phase,
    style: (PILL_STYLES as readonly string[]).includes(pill ?? "") ? (pill as PillStyle) : null,
    dock: (PILL_DOCKS as readonly string[]).includes(dock ?? "") ? (dock as PillDock) : null,
    progress: phase === "transcribing" ? SAMPLE_PROGRESS : null,
    blind: phase === "blind" ? SAMPLE_BLIND : null,
    license: sceneLicense(q.get("license")),
    rail: q.get("rail") === "open",
  };
}

/** One row of the matrix's phase list. */
export interface MatrixPhase {
  phase: LivePhase;
  id: string;
  tone: PhaseVisual["tone"];
  label: string;
}

/**
 * The phase list the matrix walks, read off the pill's OWN table: every
 * declared phase, through `phaseVisual`. The harness never keeps a copy, so a
 * phase a later item adds to the union (and therefore to `phaseVisual`, which
 * is exhaustive) is photographed without anyone editing the script.
 */
export function matrixPhases(): MatrixPhase[] {
  return ALL_LIVE_PHASES.map((phase) => {
    const v = phaseVisual(phase);
    return { phase, id: PHASE_ID[phase], tone: v.tone, label: v.label };
  });
}

/**
 * yap24-PILL1 — the license rows of the matrix: one per scene, each on the
 * idle phase. `id` names the scene (and the PNG); `phaseId` is the phase the
 * page must actually be in; `license` is the query value and what
 * `<html data-license>` must carry, so a scene that silently failed to apply
 * is a finding, not a convincing photo of a plain idle pill.
 */
export interface MatrixLicenseRow extends MatrixPhase {
  phaseId: string;
  license: LicenseScene;
}

export function matrixLicenseScenes(): MatrixLicenseRow[] {
  const v = phaseVisual("idle");
  return (Object.keys(LICENSE_SCENES) as LicenseScene[]).map((scene) => ({
    phase: "idle",
    id: scene,
    phaseId: PHASE_ID.idle,
    license: scene,
    tone: v.tone,
    label: v.label,
  }));
}

/**
 * WF-12 — the "Sounded like Spanish — Add Spanish?" card's matrix row: an idle
 * pill with the sample offer drawn over it (`float.html?phase=idle&langOffer=1`,
 * see `forcedLanguageOffer`). `query` is appended to the scene's URL and
 * `offer` is the code `[data-language-offer]` must carry, so a card that
 * silently failed to mount is a finding, not a photo of a plain idle pill.
 */
export interface MatrixOfferRow extends MatrixPhase {
  phaseId: string;
  query: string;
  offer: string;
}

/**
 * yap-fx-06 — the meeting + dictation card's three matrix rows: an idle pill
 * with `?meetingNotice=cancelled|ask|stop-failed` drawn over it (see
 * `forcedMeetingNotice` in meetingDictation.ts). `offer` is empty: the
 * language-offer detector does not apply to this card.
 */
export function matrixMeetingDictationScenes(): MatrixOfferRow[] {
  const v = phaseVisual("idle");
  return (["cancelled", "ask", "stop-failed"] as const).map((kind) => ({
    phase: "idle" as const,
    id: `meeting-dictation-${kind}`,
    phaseId: PHASE_ID.idle,
    query: `meetingNotice=${kind}`,
    offer: "",
    tone: v.tone,
    label: v.label,
  }));
}

/**
 * WF-06 — the "Backtrack caught" toast scene (`float.html?phase=idle&backtrack=1`,
 * the sample in `backtrackCaught.ts`). `offer` is empty: the matrix detector's
 * language-offer check does not apply to this card.
 */
export function matrixBacktrackCaughtScene(): MatrixOfferRow {
  const v = phaseVisual("idle");
  return {
    phase: "idle",
    id: "backtrack-caught",
    phaseId: PHASE_ID.idle,
    query: "backtrack=1",
    offer: "",
    tone: v.tone,
    label: v.label,
  };
}

/**
 * yap-wf-02-the-pill-shrinks-to — WF-02's drag blob (`float.html?phase=idle&drag=blob`,
 * see `forcedDragScene` in drag.ts): the pill mid-drag, collapsed to Yappy's
 * face with the three drop zones up. `drag` is what `<html data-pill-drag>`
 * must carry, so a blob that silently failed to apply is a finding, not a
 * photo of a resting pill. Photographed at every dock like any other row.
 */
export interface MatrixDragRow extends MatrixOfferRow {
  drag: "blob";
}

export function matrixDragBlobScene(): MatrixDragRow {
  const v = phaseVisual("idle");
  return {
    phase: "idle",
    id: "drag-blob",
    phaseId: PHASE_ID.idle,
    query: "drag=blob",
    offer: "",
    drag: "blob",
    tone: v.tone,
    label: v.label,
  };
}

export function matrixLanguageOfferScene(): MatrixOfferRow {
  const v = phaseVisual("idle");
  return {
    phase: "idle",
    id: "lang-offer",
    phaseId: PHASE_ID.idle,
    query: "langOffer=1",
    offer: "es",
    tone: v.tone,
    label: v.label,
  };
}

/**
 * WF-20 — the per-app style chip's matrix row: a listening pill with the sample
 * chip in the hint row (`float.html?phase=listening&styleChip=1`, see
 * `forcedStyleChip`). `chip` is the mode `[data-style-chip]` must carry, so a
 * chip that failed to mount is a finding, not a photo of a plain listening pill.
 */
export interface MatrixChipRow extends MatrixPhase {
  phaseId: string;
  query: string;
  chip: string;
}

export function matrixStyleChipScene(): MatrixChipRow {
  const v = phaseVisual("listening");
  return {
    phase: "listening",
    id: "style-chip",
    phaseId: PHASE_ID.listening,
    query: "styleChip=1",
    chip: "casual",
    tone: v.tone,
    label: v.label,
  };
}

/**
 * yap-1106-pill-notes-and-meeting / WF-03 — the hover rail's matrix row: an
 * idle pill with the rail forced out (`float.html?phase=idle&rail=open`). The
 * matrix crosses it with {no meeting, meeting recording} x 3 docks, which is
 * exactly the rail's two sets (Scratchpad · Dictate · Meeting, and
 * Scratchpad · Dictate · Note · Stop meeting) rotated at every dock.
 */
export function matrixRailScene(): MatrixOfferRow {
  const v = phaseVisual("idle");
  return {
    phase: "idle",
    id: "rail",
    phaseId: PHASE_ID.idle,
    query: "rail=open",
    offer: "",
    tone: v.tone,
    label: v.label,
  };
}

/**
 * yap-1106-friend-visits — a friend visiting a resting pill
 * (`float.html?phase=idle&friend=tidy`, see `forcedFriendVisit` in
 * friendVisits.ts). Tidy, because her event (Backtrack caught) is the one a
 * new user meets first. The matrix photographs it at every dock.
 */
export function matrixFriendVisitScene(): MatrixOfferRow {
  const v = phaseVisual("idle");
  return {
    phase: "idle",
    id: "friend-visit",
    phaseId: PHASE_ID.idle,
    query: "friend=tidy",
    offer: "",
    tone: v.tone,
    label: v.label,
  };
}
