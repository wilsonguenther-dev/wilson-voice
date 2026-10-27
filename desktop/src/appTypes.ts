// Y5-G — types and pure helpers lifted verbatim out of App.tsx so the shell
// and the seven view modules can share them without importing from App.tsx.
import type { ProbeReport } from "./permissionRepair";
import type {
  FailingCondition,
  GrantStatus,
  PermissionGrantRow,
  StaleGrant,
} from "./permission";
export type Nav =
  | "home"
  | "permissions"
  | "meetings"
  | "insights"
  | "dictionary"
  | "scratchpad"
  | "settings";

// YV27 — Settings is split into labeled sub-panels behind an in-Settings
// segmented sub-nav so the screen is no longer one infinite scroll. Every
// existing control lives under exactly one of these tabs.
export type SettingsTab =
  | "companion"
  | "dictation"
  | "snippets"
  | "audio"
  | "shortcut"
  | "advanced"
  | "meetings"
  | "privacy"
  | "license";

export const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  // yap-uiux-2026-10-16-SETTINGS (walk row T1) — Settings opens on what a
  // first-run user came to set: how speech becomes text, then the key that
  // starts it. Companion cosmetics come after the working parts.
  { id: "dictation", label: "Dictation" },
  { id: "shortcut", label: "Shortcut" },
  { id: "snippets", label: "Snippets" },
  { id: "audio", label: "Audio" },
  { id: "companion", label: "Companion" },
  { id: "advanced", label: "Advanced" },
  // yap-notetaker-detect — "Offer to record detected meetings" + "Use my calendar".
  { id: "meetings", label: "Meetings" },
  { id: "privacy", label: "Privacy" },
  // YP3 — last, because it is the tab you visit twice: once to see what is
  // left of the trial, once to paste the key.
  { id: "license", label: "License" },
];

/**
 * YP3 — the `trial_expires_at_ms` the "your trial is nearly up" toast last
 * fired for. Persisted so the single warning survives a relaunch; keyed on the
 * expiry rather than a boolean so a genuinely new trial is not silenced by an
 * old flag. `localStorage` is the right home: losing it costs at most one extra
 * toast, which is not worth a settings migration.
 */
export const TRIAL_WARN_KEY = "yap.trialWarnedFor";

export function readTrialWarnedFor(): number | null {
  try {
    const raw = window.localStorage.getItem(TRIAL_WARN_KEY);
    const n = raw == null ? NaN : Number(raw);
    return Number.isFinite(n) ? n : null;
  } catch {
    return null; // private mode / storage disabled — warn once per launch
  }
}

export function writeTrialWarnedFor(expiresAtMs: number) {
  try {
    window.localStorage.setItem(TRIAL_WARN_KEY, String(expiresAtMs));
  } catch {
    /* the toast simply gets to fire again next launch */
  }
}

/**
 * YV73 — how many rows the History view holds in memory.
 *
 * The initial load has always asked the backend for this many, but the live
 * `transcript` / `transcript_error` listeners PREPENDED to the array and
 * nothing ever trimmed it, so a long dictating session grew the app's largest
 * retained object (every row carries both the polished text and `rawText`)
 * one take at a time, past the window that is actually rendered. Anything the
 * cap drops is still on disk and comes back with the next `loadHistory`.
 */
export const HISTORY_LIMIT = 200;

export interface AppSettings {
  /**
   * Settings-schema marker (backend `schema_version`, YV41). The UI never sets
   * it — it is read off get_settings and spread straight back on save so the
   * backend's migration state round-trips untouched.
   */
  schemaVersion?: number;
  /** Primary dictation language (ISO code). */
  language: string;
  /** Y10-A — the languages the user speaks; empty = just `language`. */
  languageSet?: string[];
  /** Y10-A — auto-detect among `languageSet` (held to it). */
  languageAuto?: boolean;
  autoPaste: boolean;
  hotkeyLabel: string;
  showFloatingPill: boolean;
  /** fn | fn_control | both */
  pttBinding?: string;
  /**
   * Command mode (YV49): the EXTRA modifier held with `pttBinding` that makes a
   * press edit the current selection instead of typing. command (⌘, default) |
   * option (⌥) | off. Plain dictation is unaffected either way.
   */
  commandBinding?: string;
  keepCmdShiftV?: boolean;
  /**
   * yap-p0-hotkeys-no-permission — the user's rebind of the "Works everywhere"
   * dictate chord (Code + modifiers only). null/absent = ⌃⌥⌘Space.
   */
  dictateToggle?: ChordSetting | null;
  /** focus-2026-10-19 — Record meeting chord rebind; null/absent = ⌃⌘M. */
  meetingChord?: ChordSetting | null;
  /** focus-2026-10-19 — Cancel take key rebind; null/absent = Escape. */
  cancelChord?: ChordSetting | null;
  /** Y8-A — Hands-free (toggle) chord rebind; null/absent = ⌃⌘H. */
  handsFreeChord?: ChordSetting | null;
  /** Y8-A — Copy last transcript chord rebind; null/absent = ⌃⌘C. */
  copyLastChord?: ChordSetting | null;
  /** Y8-A — Open Scratchpad chord rebind; null/absent = ⌃⌘J. */
  scratchpadChord?: ChordSetting | null;
  /** classic (obsidian capsule) | yappy (pixel pet) */
  pillStyle?: string;
  /**
   * Where the pill docks on screen (backend `pill_position`, YV53): "bottom"
   * (centred island, the default) | "left" | "right" — a Wispr-style side dock,
   * vertically centred and flush to that screen edge. The backend moves the
   * NSPanel; the float webview aligns the pill to the same edge live.
   */
  pillPosition?: string;
  /**
   * Companion tone (YV27): friendly | rude | rose (default friendly). Drives
   * Yappy's reactive lines — the pill chatter + the house mood label. Read live
   * by YappyPill (settings event) and YappyHouse (prop). Curse filter stays on.
   */
  companionTone?: string;
  /** auto | plain | list | email | code | notes */
  dictationMode?: string;
  /**
   * Auto-Cleanup level (backend `cleanup_level`): none | light | medium | high.
   * Gates the cleanup pipeline — "none" pastes the raw transcript; higher levels
   * add dictionary → backtrack → formatting → local-LLM polish. Default
   * "medium" since Y4-A — "light" stopped one stage short of the formatting
   * stage, so a fresh install did no spoken punctuation, lists or email shape.
   */
  cleanupLevel?: string;
  /**
   * Y4-A provenance (backend `cleanup_level_set_by_user`): true once the user
   * has actually MOVED the Auto-Cleanup picker. Only `save_settings` writes it,
   * and only when the saved level differs from the live one — so the app can
   * tell a level somebody chose from one it shipped, and a future default
   * change can migrate the second without touching the first.
   */
  cleanupLevelSetByUser?: boolean;
  /**
   * Y4-A (backend `formatting_notice_pending`): set by the v1 → v2 settings
   * migration, which raised a stored "light" to "medium". It is the flag behind
   * the ONE quiet line telling the user formatting is on now and where to turn
   * it off. Cleared here when they dismiss it.
   */
  formattingNoticePending?: boolean;
  /**
   * The local polish model's id (backend `polish_model`), empty when none is
   * installed — which is the shipped state, since no model is bundled. Declared
   * here (Y4-A) because the Auto-Cleanup picker's generated copy depends on it:
   * "High" describes an AI pass when a model is present and says the stage
   * no-ops when it is not, and the effect that re-fetches that copy has to be
   * able to watch this field.
   */
  polishModel?: string;
  /**
   * Where snippet triggers may fire (backend `snippet_scope`, YV48): "inline"
   * (anywhere in the transcript, the default) or "utterance" (only when the
   * trigger phrase is the whole utterance).
   */
  snippetScope?: string;
  /**
   * The tone dial (backend `polish_styles`, YV62 / rule R14), keyed by dictation
   * mode ("email", "chat", …) with "very casual" | "casual" | "default" |
   * "formal" as values. A mode with no entry is "default". It adjusts
   * capitalisation and punctuation density only — never word choice — and it
   * reaches the RULES (the trailing-period rule R3), not just the local model.
   */
  polishStyles?: Record<string, string>;
  /**
   * Y4-H — the local model's hard deadline for one take (backend
   * `polish_deadline_ms`, bounded 100..5000). Never shown as a millisecond
   * field: the screen writes one of `polish::POLISH_SPEEDS`' three named values
   * (Fast / Balanced / Careful) and reads the nearest one back.
   */
  polishDeadlineMs?: number;
  /**
   * The sign-off block appended to a take (backend `signature`, YV62 / R13),
   * e.g. "Wilson — drivia.consulting". Empty by default. Pasted byte for byte
   * after every other stage, so nothing can rewrite it.
   */
  signature?: string;
  /**
   * When that block is appended (backend `signature_mode`): "off" (the default
   * — never) | "cue" (only when the take ends with "sign it") | "auto" (a cue,
   * or any email that closes on a sign-off line).
   */
  signatureMode?: string;
  /**
   * yap-format-structure (backend `structure_enabled`): "Turn spoken lists
   * into bullets". Default on.
   */
  structureEnabled?: boolean;
  /**
   * Per-app list format (backend `structure_app_overrides`), keyed by the
   * app's id (e.g. "com.apple.mail"): "markdown" | "plain" | "off".
   */
  structureAppOverrides?: Record<string, string>;
  /**
   * Denoise the captured clip with RNNoise before transcription (backend
   * `denoise`, YV12). Suppresses steady background noise (fans, hum, keyboard)
   * before the 16 kHz downsample. Default on.
   */
  denoise?: boolean;
  /**
   * Auto-mute the whole Mac's system output while dictating (backend
   * `mute_while_dictating`, YV28). Silences music/video/notifications for the
   * take, then restores the exact prior mute + volume on stop. Default on.
   */
  muteWhileDictating?: boolean;
  /**
   * Y8-C — four short Yappy sounds: start, stop, paste and achievement (a
   * lifetime word milestone). Backend `earcons`; OFF by default. Never plays
   * during a meeting recording. The start sound finishes before the mute.
   */
  earcons?: boolean;
  /** First-run onboarding completed (YV9). Shows the onboarding flow when false. */
  onboarded?: boolean;
  /**
   * Y6-A — at least one dictation has actually pasted (onboarding's try-it
   * step, or any take since). False after a "Skip for now" finish, which keeps
   * the main window's health row raised until a take lands. The backend flips
   * it on the first pasted take and never back.
   */
  dictationProven?: boolean;
  /** Calibration phrase captured during onboarding, kept for later personalization. */
  calibrationSample?: string | null;
  /**
   * Load the speech model at launch instead of on your first dictation
   * (backend `preload_model`, YV80). Default OFF: an idle Yap holds ~930 MB
   * less, and the first take of a session loads the engine while you are
   * already talking. On, Yap goes back to YV38's behaviour — the model is
   * resident from launch, so even the first take starts instantly.
   */
  preloadModel?: boolean;
  /**
   * Launch Yap at login (backend `autostart`, YV42). Installs a macOS
   * LaunchAgent through tauri-plugin-autostart. Default off — the backend
   * applies it the moment this saves, and re-applies it on every launch.
   */
  autostart?: boolean;
  /**
   * Check GitHub Releases for a newer Yap (backend `check_updates`, YV44). The
   * check only ever raises the "Update available" prompt — the download and
   * install run on the user's click. Default on; off means Yap never contacts
   * the release endpoint.
   */
  checkUpdates?: boolean;
  /**
   * A version dismissed with "Skip this version" (backend
   * `skipped_update_version`, YV44). Exactly that version stops being offered;
   * anything newer still is.
   */
  skippedUpdateVersion?: string | null;
}

/** A Carbon chord: a KeyboardEvent.code plus modifiers. */
export interface ChordSetting {
  code: string;
  ctrl: boolean;
  alt: boolean;
  cmd: boolean;
  shift: boolean;
}

/** yap-p0-hotkeys-no-permission — mirrors `hotkeys::HotkeysStatus`. */
export interface HotkeysStatus {
  carbon:
    | { state: "notAttempted" }
    | { state: "registered" }
    | { state: "failed"; reason: string };
  fnTap:
    | { state: "notAttempted" }
    | { state: "delivering" }
    | { state: "armedNoDelivery" }
    | { state: "blocked"; reason: string }
    | { state: "secureInput"; reason: string };
}

export interface AppStatus {
  recording: boolean;
  busy: boolean;
  lastError: string | null;
  /** Y5-F — the catalogue code behind `lastError` (errors.ts). */
  lastErrorCode?: string | null;
  message: string;
  accessibility: boolean;
  /** The Carbon dictate chord's register result and the fn tap's delivery. */
  hotkeys?: HotkeysStatus;
  /** Main window only (header status chip + home banner; the floating pill
   *  does not render it). "fn is off — <reason>. Use ⌃⌥⌘Space (works without
   *  permissions)." only while the chord's register call returned Ok; a failed
   *  or not-yet-attempted chord is named as not working instead. */
  hotkeyDegraded?: string | null;
  /** Label of the registered dictate chord (default ⌃⌥⌘Space). */
  dictateToggleLabel?: string;
  /**
   * YV33 — dictation can actually run: the selected embedded model is
   * downloaded. Since YV34 that is the only ASR path, so false means the app
   * never says "Ready"; since YV54 it also means a download is already under
   * way, reported by the slim `ModelRibbon` rather than a demand to go pick one.
   */
  modelReady: boolean;
  /**
   * YV43 — another app enabled macOS Secure Input, so the CGEvent tap behind
   * fn / fn⌃ receives nothing and push-to-talk is dead until it is released.
   * There is no fallback to wire (Carbon hotkeys cannot express fn), so saying
   * so IS the fix.
   */
  secureInputBlocked: boolean;
  /** Holder + workaround line for the banner; null unless blocked. */
  secureInputDetail: string | null;
  /**
   * YV80 — the speech engine is loading right now. With the lazy default that
   * happens on the first take of a session, so `message` reads "Preparing your
   * speech engine…" rather than claiming a decode that has not started.
   */
  engineLoading?: boolean;
}

export interface PermissionReport {
  accessibility: boolean;
  microphone: boolean;
  /** PERM-A — the AVFoundation status the UI branches on. A bool cannot tell
   *  "never asked" (show the prompt button) from "denied" (show Settings). */
  microphoneStatus: string;
  /** PERM-E — `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`, tri-state. */
  inputMonitoring: GrantStatus;
  /** PERM-E — unknown by construction; there is no API that reads it. */
  audioCapture: GrantStatus;
  /** PERM-E — THE four rows. Everything permission-shaped renders from this. */
  grants: PermissionGrantRow[];
  asrOk: boolean;
  asrDetail: string;
  summary: string;
  allCriticalOk: boolean;
  /** yap-perm-truth — the bundle path of the process that asked macOS. */
  checkedBundlePath?: string;
  /** fix-ax-probe-refresh — "Yap" / "Yap Test": the name permission copy uses. */
  productName?: string;
  /** Leaf certificate summary of the running binary's signature. */
  signingAuthority?: string;
  /** Each OS call that answered no. */
  failingConditions?: FailingCondition[];
  /** Grants last seen working under a different signature. */
  staleGrants?: StaleGrant[];
  /** Other bundles on disk with Yap's bundle id. */
  duplicateBundles?: string[];
  /** yap-p0-perm-repair — the ONE live per-service probe (with StaleIdentity). */
  probe?: ProbeReport;
}

/**
 * yap-uiux-2026-10-16-H2 (fix round 1) — the grants dictation needs that the
 * user has NOT given, by name. Deliberately NOT `!allCriticalOk`: the backend
 * folds the speech model into that flag (permissions.rs `all_critical_ok =
 * mic authorized && microphone && asr_ok`) and leaves Accessibility out, so
 * it fired for a still-downloading model and stayed silent with Accessibility
 * off. The model has its own surface (ModelRibbon); this reads only grants.
 * `null` perms (not loaded yet / bridge error) is "unknown", never "missing".
 */
export function missingDictationGrants(perms: PermissionReport | null | undefined): string[] {
  if (!perms) return [];
  const missing: string[] = [];
  if (perms.microphoneStatus !== "authorized" || !perms.microphone) missing.push("Microphone");
  if (!perms.accessibility) missing.push("Accessibility");
  return missing;
}

export interface TranscriptEntry {
  id: string;
  text: string;
  backend: string;
  asrSeconds: number;
  speechSeconds?: number;
  pipelineMs?: number;
  wordCount: number;
  createdAt: string;
  sourceApp?: string | null;
  /**
   * YV10 — the verbatim ASR transcript before the cleanup pipeline. `text` is
   * the polished result that got pasted. Null only for legacy rows written
   * before the column existed. Powers the YV51 "Paste raw" / undo action.
   */
  rawText?: string | null;
  /**
   * Y4-G — the cleanup stages that actually ran, comma separated in pipeline
   * order (`dictionary,backtrack,rules,polish`). Null for legacy rows.
   */
  stagesThatRan?: string | null;
  /**
   * Y4-G silent-skip signal — why the AI polish stage produced nothing when it
   * was enabled. Null when the stage was off or its rewrite was accepted.
   */
  polishSkipReason?: string | null;
  /** Y4-G — local-only thumbs: 1, -1, or null. Never transmitted. */
  feedback?: number | null;
}

/**
 * YV52 — a take whose transcription failed, with its audio still on disk. The
 * clip is kept for `FAILED_TAKE_RETENTION_DAYS` (7) so the user can re-run ASR
 * on it instead of re-speaking; retrying converts it into a TranscriptEntry.
 */
export interface FailedDictation {
  id: string;
  wavPath: string;
  speechSeconds: number;
  error: string;
  sourceApp?: string | null;
  createdAt: string;
  /** DB-B — the id the take's audio carries on disk, for a recovered take. */
  takeId?: string | null;
  /**
   * DB-B — the words this take had ALREADY decoded before the app went away,
   * read back from `take_chunks` at launch. Its presence is what turns "Retry"
   * (decode this clip again) into "Finish transcribing" (resume from here) —
   * the backend decodes only the remainder and keeps these words.
   */
  partialText?: string | null;
  /**
   * PR #216 re-review — what the last failed retry DID transcribe before a
   * section failed (the recovered words plus every section that decoded).
   * Shown on the row; "Keep what was transcribed" saves exactly these words.
   */
  attemptText?: string | null;
}

/**
 * YV64 — one crash Yap has evidence of, read at startup from macOS' own `.ips`
 * reports and from the panic hook's log lines (backend `db::CrashEvent`).
 * Local-only by construction: the row carries structured crash facts, never
 * transcript text, and nothing about it is ever uploaded.
 */
export interface CrashEvent {
  id: string;
  occurredAt: string;
  /** panic | native | watchdog */
  kind: string;
  signature: string;
  sourceFile: string;
  details: string;
  acknowledged: boolean;
}

/**
 * YV75 — the `yap-polish` sidecar's lifecycle, as the backend reports it
 * (`polish::SidecarStatus`). High cleanup runs a SECOND process, and until it
 * has finished loading its model a take is rules-only — this is what lets
 * Diagnostics say so instead of the stage looking like it silently did nothing.
 */
export interface SidecarStatus {
  state: "not-installed" | "starting" | "ready" | "failed";
  /** Short tag on a failure (`spawn_failed`, `ready_timeout`, `died`, …). */
  reason: string | null;
}

/** Warm-engine snapshot (backend `transcription::EngineStatus`). */
export interface EngineStatus {
  loaded: boolean;
  loading: boolean;
  transcribing: boolean;
  modelId: string | null;
  idleSeconds: number;
  idleUnloadSeconds: number;
  polishSidecar: SidecarStatus;
}

/**
 * The sidecar state in the user's words. The reason tag is a short machine
 * string (never anything dictated), shown only on a failure so support can read
 * it back off a screenshot.
 */
export function polishSidecarLabel(s: SidecarStatus | undefined): string {
  switch (s?.state) {
    case "ready":
      return "Ready — High cleanup is rewriting your takes.";
    case "starting":
      return "Starting — loading its model. Takes stay rules-only until it is ready.";
    case "failed":
      return `Stopped (${s.reason ?? "unknown"}) — takes stay rules-only for the rest of this session.`;
    case "not-installed":
      return "Not running — High cleanup is using the rules stage only.";
    default:
      return "Checking…";
  }
}

/**
 * YV51 — the raw take to re-paste for "Undo AI edit", or null when there is no
 * AI edit to undo (no stored raw, a blank raw, or a raw that matches what was
 * pasted). Mirrors `dictation::undo_ai_edit_text` in the Rust pipeline — same
 * trimmed comparison — so the button, the tray item and ⌃⌘Z agree on when the
 * action is live.
 */
export function undoAiEditText(e: TranscriptEntry): string | null {
  const raw = e.rawText;
  if (!raw || !raw.trim() || raw.trim() === e.text.trim()) return null;
  return raw;
}

export interface Insights {
  totalWords: number;
  totalSessions: number;
  wordsToday: number;
  sessionsToday: number;
  avgWpm: number;
  streakDays: number;
  longestStreak: number;
  wordsLast7: { date: string; words: number; sessions: number }[];
  topApps: { app: string; words: number; sessions: number }[];
  avgAsrSeconds: number;
  p50PipelineMs?: number;
  p95PipelineMs?: number;
  speechSecondsTotal?: number;
  wpmSampleSessions?: number;
  /** YV94 — the Meetings strip (finding #29). Absent on an older backend. */
  meetings?: MeetingStats;
}

/**
 * YV94 — the local Notetaker rollup. Yap ships no telemetry, so this is the
 * only signal that meetings are being recorded, kept and trusted. The DER proxy
 * and the "meetings with an action checked off" line from finding #29 need
 * diarization (yap23) and summaries (yap25); they are absent rather than faked.
 */
export interface MeetingStats {
  totalMeetings: number;
  meetingsLast7: number;
  totalSeconds: number;
  completeMeetings: number;
  partialMeetings: number;
  failedMeetings: number;
  segmentsIndexed: number;
  firstMeetingAt?: string | null;
  daysToFirstMeeting?: number | null;
  lastMeetingAt?: string | null;
  meetingsWithAudio: number;
  audioRetentionDays: number;
}

/** YV94 — one recorded meeting. Mirrors the `meetings` row. */
export interface Meeting {
  id: string;
  title: string;
  source: string;
  /**
   * YV125 — `virtual | in_person | unknown`, the answer to the
   * start-of-meeting picker. Optional on the wire so a build of this UI can
   * read a row written before migration 4; a missing kind is `unknown`, which
   * is the branch that does not claim the microphone is you.
   */
  kind?: string | null;
  startedAt: string;
  endedAt?: string | null;
  durationSeconds: number;
  /** recording | transcribing | summarizing | complete | failed | partial */
  state: string;
  error?: string | null;
  processedThroughSeconds?: number;
  audioKept: boolean;
  micWavPath?: string | null;
  summary?: string | null;
  summaryModel?: string | null;
  createdAt: string;
  segmentCount: number;
}

/**
 * One chronological transcript segment. 22-A recorded one track (the mic);
 * YV106 gave the row a `track`, and YV108 renders it.
 */
export interface MeetingSegment {
  id: string;
  meetingId: string;
  startSeconds: number;
  endSeconds: number;
  text: string;
  confidence?: number | null;
  createdAt: string;
  /** 0 = mic ("Me"), 1 = system audio ("Them"). Absent = mic, per the column's
   *  `DEFAULT 0`: every row written before migration 3 really was the mic. */
  track?: number | null;
}

/** YV125 — one choice on the start-of-meeting kind picker. Mirrors
 *  `meeting_control::KindChoice`. */
export interface KindChoice {
  kind: string;
  label: string;
  menuId: string;
}

export interface MeetingDetail {
  meeting: Meeting;
  segments: MeetingSegment[];
  audioOnDisk: boolean;
}

/** `750` → `12m 30s`. Mirrors `meetings::format_duration` in Rust. */
export function formatMeetingDuration(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.round(seconds) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  if (m > 0) return `${m}m ${String(s).padStart(2, "0")}s`;
  return `${s}s`;
}

export interface DayCount {
  date: string;
  words: number;
  sessions: number;
}

// GitHub-style intensity bucket (0 = empty, 1..4 = increasing) from a value
// relative to the window's peak. Kept pure so the heatmap render stays cheap.
export function heatLevel(words: number, max: number): number {
  if (words <= 0 || max <= 0) return 0;
  const r = words / max;
  if (r > 0.66) return 4;
  if (r > 0.33) return 3;
  if (r > 0.1) return 2;
  return 1;
}

// YV55 — warm amber ramp for the activity heatmap, indexed by heatLevel bucket.
// Empty is the soil the page already sits on; full is the accent ember, so the
// year reads as one material instead of a blue grid pasted onto it.
export const HEAT_FILL = [
  "var(--heat-0)",
  "var(--heat-1)",
  "var(--heat-2)",
  "var(--heat-3)",
  "var(--heat-4)",
];

// Month label for a "YYYY-MM" key from monthly_series.
export function formatMonth(ym: string) {
  try {
    return new Date(ym + "-15T12:00:00").toLocaleDateString(undefined, {
      month: "short",
      year: "2-digit",
    });
  } catch {
    return ym;
  }
}

// Bare month name for a "YYYY-MM" key — "short" for heatmap ticks, "long" for
// the collapsed-quiet-months line.
export function monthName(ym: string, style: "short" | "long" = "short") {
  try {
    return new Date(ym + "-15T12:00:00").toLocaleDateString(undefined, {
      month: style,
    });
  } catch {
    return ym;
  }
}

export interface DictEntry {
  id: string;
  term: string;
  preferred?: string | null;
  hits: number;
  createdAt: string;
  /**
   * YV47 "always bias" — a starred term heads the ranking, survives the harvest
   * purge, and is the last thing dropped from the decoder prompt.
   */
  starred?: boolean;
}

/**
 * YV47 — a word Yap noticed you fixing, waiting to be accepted. Learned only
 * from an explicit "Fix transcription" edit, never from guessing at the
 * clipboard.
 */
export interface DictCandidate {
  id: string;
  term: string;
  /** What ASR produced, or null when the word was missing entirely. */
  wrong?: string | null;
  useCount: number;
  createdAt: string;
}

/**
 * YV48 — a saved trigger phrase and the text it expands to. Expansion runs on
 * the dictation path after cleanup; disabled snippets stay listed but never
 * match.
 */
export interface Snippet {
  id: string;
  trigger: string;
  expansion: string;
  enabled: boolean;
  createdAt: string;
}

export interface ScratchNote {
  id: string;
  title: string;
  body: string;
  updatedAt: string;
}

export function formatTime(iso: string) {
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch {
    return iso;
  }
}

export function formatDay(d: string) {
  try {
    return new Date(d + "T12:00:00").toLocaleDateString(undefined, {
      weekday: "short",
      month: "short",
      day: "numeric",
    });
  } catch {
    return d;
  }
}

// Weekday-less day label ("Jul 29") for dense one-line summaries.
export function formatDayShort(d: string) {
  try {
    return new Date(d + "T12:00:00").toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
    });
  } catch {
    return d;
  }
}

/**
 * PRIV-B — the one sentence that makes retained audio visible in History:
 * "1 clip kept for 7 days" / "3 clips kept for 7 days". The window is
 * `FAILED_TAKE_RETENTION_DAYS` in src-tauri/src/db.rs; keep the two in step.
 */
export const RETAINED_CLIP_DAYS = 7;
export function retainedClipsLabel(count: number): string {
  return `${count} ${count === 1 ? "clip" : "clips"} kept for ${RETAINED_CLIP_DAYS} days`;
}
