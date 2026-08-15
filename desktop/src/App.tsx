import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Onboarding from "./Onboarding";
import { ModelPicker, ModelRibbon, useModelSetup } from "./ModelSetup";
import YappyHouse from "./home/YappyHouse";
import { checkForUpdate, installUpdate, type UpdateInfo } from "./updater";
import { errorText, isLicenseRequired } from "./errors";
// YV95 — the meeting status shape and its label rules are shared with the pill
// (src/pill/meeting.ts) so the two surfaces cannot render the same second
// differently, and so those rules are unit-tested once instead of twice.
import {
  IDLE_MEETING,
  MEETING_EVENT,
  disabledReason,
  elapsedLabel,
  systemAudioBadge,
  recordLabel,
  type MeetingStatus,
} from "./pill/meeting";
import LicensePanel from "./license/LicensePanel";
import PurchasePrompt from "./license/PurchasePrompt";
// YV96 — the one-time meeting-capture notice. Its copy and its two open/close
// rules live in `meetings/consent.ts` so they are unit-tested once, here and in
// the preview page, rather than being inline strings in this file.
import MeetingConsentNotice from "./meetings/MeetingConsentNotice";
import { acknowledgedLabel, type MeetingConsent } from "./meetings/consent";
import TranscriptList from "./meetings/TranscriptList";
// YV102 — the "Set up meeting recording" step. Its copy and the rule that turns
// a verdict into a sentence live in `meetings/systemAudio.ts` so the one thing
// that must never drift — silence is not evidence of a denial — is unit-tested
// rather than spread through this file.
import {
  setupState,
  SYSTEM_AUDIO_PANE,
  SYSTEM_AUDIO_SETUP,
  type SystemAudioSetup,
} from "./meetings/systemAudio";
import SupportBundleSheet from "./support/SupportBundleSheet";
import type {
  SupportBundlePreview,
  SupportSendOutcome,
} from "./support/bundle";
import { watchMeetingConsent } from "./meetings/consentWatch";
import {
  chipFor,
  shouldWarnTrial,
  trialWarningText,
  daysLeft as trialDaysLeft,
  type LicenseStatus,
} from "./license/status";
import "./App.css";

type Nav =
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
type SettingsTab =
  | "companion"
  | "dictation"
  | "snippets"
  | "audio"
  | "shortcut"
  | "advanced"
  | "privacy"
  | "license";

const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  { id: "companion", label: "Companion" },
  { id: "dictation", label: "Dictation" },
  { id: "snippets", label: "Snippets" },
  { id: "audio", label: "Audio" },
  { id: "shortcut", label: "Shortcut" },
  { id: "advanced", label: "Advanced" },
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
const TRIAL_WARN_KEY = "yap.trialWarnedFor";

function readTrialWarnedFor(): number | null {
  try {
    const raw = window.localStorage.getItem(TRIAL_WARN_KEY);
    const n = raw == null ? NaN : Number(raw);
    return Number.isFinite(n) ? n : null;
  } catch {
    return null; // private mode / storage disabled — warn once per launch
  }
}

function writeTrialWarnedFor(expiresAtMs: number) {
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
const HISTORY_LIMIT = 200;

interface AppSettings {
  /**
   * Settings-schema marker (backend `schema_version`, YV41). The UI never sets
   * it — it is read off get_settings and spread straight back on save so the
   * backend's migration state round-trips untouched.
   */
  schemaVersion?: number;
  language: string;
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
   * add dictionary → backtrack → formatting → local-LLM polish. Default "light".
   */
  cleanupLevel?: string;
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
  /** First-run onboarding completed (YV9). Shows the onboarding flow when false. */
  onboarded?: boolean;
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

interface AppStatus {
  recording: boolean;
  busy: boolean;
  lastError: string | null;
  message: string;
  accessibility: boolean;
  hotkeyRegistered: boolean;
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

interface PermissionReport {
  accessibility: boolean;
  microphone: boolean;
  ffmpegOk: boolean;
  asrOk: boolean;
  asrDetail: string;
  summary: string;
  allCriticalOk: boolean;
}

interface TranscriptEntry {
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
}

/**
 * YV52 — a take whose transcription failed, with its audio still on disk. The
 * clip is kept for `FAILED_TAKE_RETENTION_DAYS` (7) so the user can re-run ASR
 * on it instead of re-speaking; retrying converts it into a TranscriptEntry.
 */
interface FailedDictation {
  id: string;
  wavPath: string;
  speechSeconds: number;
  error: string;
  sourceApp?: string | null;
  createdAt: string;
}

/**
 * YV64 — one crash Yap has evidence of, read at startup from macOS' own `.ips`
 * reports and from the panic hook's log lines (backend `db::CrashEvent`).
 * Local-only by construction: the row carries structured crash facts, never
 * transcript text, and nothing about it is ever uploaded.
 */
interface CrashEvent {
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
interface SidecarStatus {
  state: "not-installed" | "starting" | "ready" | "failed";
  /** Short tag on a failure (`spawn_failed`, `ready_timeout`, `died`, …). */
  reason: string | null;
}

/** Warm-engine snapshot (backend `transcription::EngineStatus`). */
interface EngineStatus {
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
function polishSidecarLabel(s: SidecarStatus | undefined): string {
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
function undoAiEditText(e: TranscriptEntry): string | null {
  const raw = e.rawText;
  if (!raw || !raw.trim() || raw.trim() === e.text.trim()) return null;
  return raw;
}

interface Insights {
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
interface MeetingStats {
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
interface Meeting {
  id: string;
  title: string;
  source: string;
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
interface MeetingSegment {
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

interface MeetingDetail {
  meeting: Meeting;
  segments: MeetingSegment[];
  audioOnDisk: boolean;
}

/** `750` → `12m 30s`. Mirrors `meetings::format_duration` in Rust. */
function formatMeetingDuration(seconds: number): string {
  const total = Number.isFinite(seconds) && seconds > 0 ? Math.round(seconds) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${String(m).padStart(2, "0")}m`;
  if (m > 0) return `${m}m ${String(s).padStart(2, "0")}s`;
  return `${s}s`;
}

interface DayCount {
  date: string;
  words: number;
  sessions: number;
}

// GitHub-style intensity bucket (0 = empty, 1..4 = increasing) from a value
// relative to the window's peak. Kept pure so the heatmap render stays cheap.
function heatLevel(words: number, max: number): number {
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
const HEAT_FILL = [
  "var(--heat-0)",
  "var(--heat-1)",
  "var(--heat-2)",
  "var(--heat-3)",
  "var(--heat-4)",
];

// Month label for a "YYYY-MM" key from monthly_series.
function formatMonth(ym: string) {
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
function monthName(ym: string, style: "short" | "long" = "short") {
  try {
    return new Date(ym + "-15T12:00:00").toLocaleDateString(undefined, {
      month: style,
    });
  } catch {
    return ym;
  }
}

interface DictEntry {
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
interface DictCandidate {
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
interface Snippet {
  id: string;
  trigger: string;
  expansion: string;
  enabled: boolean;
  createdAt: string;
}

interface ScratchNote {
  id: string;
  title: string;
  body: string;
  updatedAt: string;
}

function formatTime(iso: string) {
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

function formatDay(d: string) {
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
function formatDayShort(d: string) {
  try {
    return new Date(d + "T12:00:00").toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
    });
  } catch {
    return d;
  }
}

function StatusDot({ ok }: { ok: boolean }) {
  return <span className={ok ? "dot-ok" : "dot-bad"} aria-hidden />;
}

export default function App() {
  const [nav, setNav] = useState<Nav>("home");
  const [userName, setUserName] = useState("");
  useEffect(() => {
    invoke<string>("user_display_name").then(setUserName).catch(() => {});
  }, []);
  // YV27 — which Settings sub-panel is showing (segmented sub-nav).
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("companion");
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    lastError: null,
    message: "Loading…",
    accessibility: false,
    hotkeyRegistered: false,
    modelReady: false,
    secureInputBlocked: false,
    secureInputDetail: null,
  });
  // YP3 — the entitlement this Mac has right now, and whether the warm purchase
  // sheet is up. `null` until the first `license_status` lands: the chip and the
  // License tab render nothing rather than guessing "trial" for a paying user.
  const [license, setLicense] = useState<LicenseStatus | null>(null);
  const [buyPrompt, setBuyPrompt] = useState(false);
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  const [history, setHistory] = useState<TranscriptEntry[]>([]);
  // YV52 — failed takes whose audio is still recoverable, and the one the live
  // error toast is offering "Retry" for (cleared with the toast itself).
  const [failed, setFailed] = useState<FailedDictation[]>([]);
  const [retryId, setRetryId] = useState<string | null>(null);
  const [retrying, setRetrying] = useState<string | null>(null);
  // YV74 — the transcript of a take whose auto-paste produced no read receipt.
  // The backend only claims "Pasted" when the target app demonstrably read the
  // clipboard; when it didn't, the toast carries a "Copy again" action for this
  // row, because the transcript may no longer be on the clipboard (the user can
  // copy something themselves while we wait, and their copy is left alone).
  const [copyAgainId, setCopyAgainId] = useState<string | null>(null);
  // YV64 — crashes Yap read back off disk at startup. Shown in Settings →
  // Privacy & Diagnostics → Stability; an UNacknowledged one raises the single
  // launch toast below (once per launch, never a modal).
  const [crashes, setCrashes] = useState<CrashEvent[]>([]);
  // YV75 — the engine snapshot behind Privacy & Diagnostics, read when that tab
  // is opened (no timer: the answer is only interesting while it is on screen).
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [insights, setInsights] = useState<Insights | null>(null);
  const [dailySeries, setDailySeries] = useState<DayCount[]>([]);
  const [monthlySeries, setMonthlySeries] = useState<DayCount[]>([]);
  const [dictionary, setDictionary] = useState<DictEntry[]>([]);
  // YV47 — pending "Yap noticed: …" suggestions mined from corrections, and the
  // dictionary row currently being edited in place.
  const [candidates, setCandidates] = useState<DictCandidate[]>([]);
  const [editingTermId, setEditingTermId] = useState<string | null>(null);
  const [editTerm, setEditTerm] = useState("");
  const [editPreferred, setEditPreferred] = useState("");
  // YV47 — the history entry open in "Fix transcription", and its draft text.
  const [fixingId, setFixingId] = useState<string | null>(null);
  const [fixDraft, setFixDraft] = useState("");
  // YV48 — saved snippets plus the "add" draft and the row being edited.
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [newTrigger, setNewTrigger] = useState("");
  const [newExpansion, setNewExpansion] = useState("");
  const [editingSnippetId, setEditingSnippetId] = useState<string | null>(null);
  const [editTrigger, setEditTrigger] = useState("");
  const [editExpansion, setEditExpansion] = useState("");
  const [scratch, setScratch] = useState<ScratchNote[]>([]);
  // YV94 — the Meetings tab. Loaded when the tab is opened rather than in
  // `refreshAll`: meetings are rare compared to dictations, each list row costs
  // a segment-count subquery, and nothing on any other screen reads them.
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [meetingQuery, setMeetingQuery] = useState("");
  // The meeting whose transcript is open. `null` = the list.
  const [openMeeting, setOpenMeeting] = useState<MeetingDetail | null>(null);
  const [meetingBusy, setMeetingBusy] = useState(false);
  // YV95 — is a meeting recording right now? Driven by the backend's 1 Hz
  // `meeting` emit (OS-12 fix 1: no timer in the webview), plus one
  // `meeting_status` call on mount so a window opened mid-meeting is correct
  // immediately rather than a second later.
  const [meetingStatus, setMeetingStatus] = useState<MeetingStatus>(IDLE_MEETING);
  // Delete is destructive and irreversible (rows, search index and the audio),
  // so the button arms first and the second click commits.
  const [confirmDeleteMeeting, setConfirmDeleteMeeting] = useState<string | null>(
    null,
  );
  // YV96 — the one-time capture notice. `null` until the backend answers, which
  // is why `shouldOpenNotice` treats `null` as "do not open".
  const [consent, setConsent] = useState<MeetingConsent | null>(null);
  // YV102 — what Yap currently believes about system-audio permission, and
  // whether this Mac can hold that permission at all (YV101's 14.4 gate,
  // answered by `notetaker_status`). Both `null` until the backend replies;
  // `setupState` treats that as "not run", never as "denied".
  const [sysAudio, setSysAudio] = useState<SystemAudioSetup | null>(null);
  const [sysAudioGate, setSysAudioGate] = useState<{
    available: boolean;
    message: string;
  }>({
    available: true,
    message: "System audio capture requires macOS 14.4 or later",
  });
  // The pre-warm is a real 200 ms CoreAudio tap. The button disables while it
  // runs so a double-press cannot open two.
  const [sysAudioBusy, setSysAudioBusy] = useState(false);
  // Why the sheet is open, not just whether: `recording` is the first-capture
  // showing (a meeting is running behind it), `review` is a deliberate re-read
  // from Settings → Privacy. The sheet says the true thing in each case, and
  // this avoids mirroring YV95's meeting status into a second piece of state.
  const [consentOpen, setConsentOpen] = useState<null | "recording" | "review">(
    null,
  );
  // Read by the `meeting` subscription, which is deliberately subscribed ONCE:
  // re-subscribing whenever the consent state changes would drop ticks. Synced
  // in an effect rather than written during render, which React reserves for
  // itself.
  // YV98 — the crash-report sheet. `null` means closed; an open sheet with a
  // `null` preview is the second or two the backend spends building the bundle.
  // The preview is REAL redacted content, so it is never fabricated here.
  const [supportOpen, setSupportOpen] = useState(false);
  const [supportPreview, setSupportPreview] =
    useState<SupportBundlePreview | null>(null);
  const [supportBusy, setSupportBusy] = useState(false);
  const consentRef = useRef<MeetingConsent | null>(null);
  useEffect(() => {
    consentRef.current = consent;
  }, [consent]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  // YV62 — the signature block is edited locally and saved on blur. A save per
  // keystroke would rewrite settings.json on every character of a multi-line
  // block; `null` means "show whatever is stored".
  const [signatureDraft, setSignatureDraft] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [newTerm, setNewTerm] = useState("");
  const [newPreferred, setNewPreferred] = useState("");
  const [noteTitle, setNoteTitle] = useState("Scratchpad");
  const [noteBody, setNoteBody] = useState("");
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  // YV15 — key-capture for the push-to-talk shortcut. `capturing` arms a global
  // keydown listener; `captureHint` shows the live result / validation message.
  const [capturing, setCapturing] = useState(false);
  const [captureHint, setCaptureHint] = useState<string | null>(null);
  // YV44 — the pending release, if the check found one the user hasn't skipped.
  // Non-blocking: it renders as a banner and nothing downloads until they click.
  const [update, setUpdate] = useState<UpdateInfo | null>(null);
  const [installing, setInstalling] = useState(false);
  const [installedVersion, setInstalledVersion] = useState<string | null>(null);
  // YV78 — clearing history now scrubs the file (FTS rebuild + VACUUM), which
  // on a large history is not instant. The button stays disabled until the
  // command resolves so nothing reports success while bytes are still on disk.
  const [clearing, setClearing] = useState(false);
  // YV54 — silent model setup. The onboarding overlay owns the auto-download
  // during first run, so this instance only takes it over once the user IS
  // onboarded: an install that lands here with no model (fresh profile, a
  // deleted file, an update on a machine that skipped setup) starts fetching
  // the catalog's recommendation on its own, and says so with the same slim
  // ribbon. It also backs the Settings → Advanced picker.
  const modelSetup = useModelSetup({
    autoDownload: settings?.onboarded === true,
  });

  // Read the live query without making `refreshAll` depend on it — otherwise the
  // mount effect that registers event listeners re-runs on every keystroke,
  // tearing down + re-adding all listeners and dropping events in the gap.
  const queryRef = useRef(query);
  useEffect(() => {
    queryRef.current = query;
  }, [query]);

  // YV15 — while the "Set shortcut" control is armed, record the next combo the
  // user presses and map it to a supported push-to-talk binding. The dictation
  // engine binds the fn (Globe) key via a CGEvent tap, so only fn / fn⌃ are wired
  // end-to-end — Control is the reliably-detectable half of the fn⌃ gesture, so a
  // Control-inclusive combo maps to fn_control. Anything else (bare letters, or a
  // modifier we can't wire) is reported rather than silently persisted.
  useEffect(() => {
    if (!capturing) return;
    function onKey(e: KeyboardEvent) {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCapturing(false);
        setCaptureHint("Cancelled — shortcut unchanged.");
        return;
      }
      const fn =
        e.key === "Fn" ||
        e.code === "Fn" ||
        (typeof e.getModifierState === "function" && e.getModifierState("Fn"));
      const ctrl = e.ctrlKey;
      const isModifierKey =
        e.key === "Control" ||
        e.key === "Fn" ||
        e.key === "Meta" ||
        e.key === "Alt" ||
        e.key === "Shift";
      if (ctrl && !e.metaKey && !e.altKey) {
        // fn⌃ gesture — Control is the detectable half; fn is required to talk.
        applyBinding("fn_control");
        setCaptureHint("Shortcut set to fn + Control (fn⌃).");
        setCapturing(false);
      } else if (fn) {
        applyBinding("fn");
        setCaptureHint(
          "Shortcut set to fn. Tip: set Keyboard → “Press 🌐 to → Do Nothing” so fn doesn’t open emoji.",
        );
        setCapturing(false);
      } else if (!isModifierKey) {
        setCaptureHint(
          "That’s not a hold key. Hold fn, or fn together with Control.",
        );
      } else {
        setCaptureHint(
          "Only fn-based shortcuts are wired for dictation. Hold fn, or fn + Control.",
        );
      }
    }
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [capturing, settings]);

  const loadHistory = useCallback(async (q?: string) => {
    const h = await invoke<TranscriptEntry[]>("get_history", {
      query: q || null,
      limit: HISTORY_LIMIT,
    });
    setHistory(h);
  }, []);

  // YV52 — the recoverable takes. Kept separate from `loadHistory` because the
  // backend purges expired clips on this call, so it must not ride the
  // search-as-you-type debounce.
  const loadFailed = useCallback(async () => {
    setFailed(await invoke<FailedDictation[]>("list_failed_dictations"));
  }, []);

  // YV94 — the Meetings list. `query` searches segment text through FTS5 and the
  // title through LIKE, so a meeting is findable by what it was called AND by
  // what was said in it.
  const loadMeetings = useCallback(async (q?: string) => {
    try {
      setMeetings(
        await invoke<Meeting[]>("list_meetings", { query: q || null }),
      );
    } catch (e) {
      console.error(e);
    }
  }, []);

  const openMeetingDetail = useCallback(async (id: string) => {
    try {
      setOpenMeeting(await invoke<MeetingDetail>("get_meeting", { id }));
      setConfirmDeleteMeeting(null);
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshPerms = useCallback(async () => {
    try {
      setPerms(await invoke<PermissionReport>("get_permissions"));
    } catch (e) {
      console.error(e);
    }
  }, []);

  const refreshAll = useCallback(async () => {
    try {
      const [
        s,
        st,
        ins,
        daily,
        monthly,
        dict,
        cands,
        snips,
        notes,
        fails,
        crashRows,
      ] = await Promise.all([
          invoke<AppSettings>("get_settings"),
          invoke<AppStatus>("get_status"),
          invoke<Insights>("get_insights"),
          invoke<DayCount[]>("daily_series", { days: 365 }),
          invoke<DayCount[]>("monthly_series", { months: 12 }),
          invoke<DictEntry[]>("list_dictionary"),
          invoke<DictCandidate[]>("list_dict_candidates"),
          invoke<Snippet[]>("list_snippets"),
          invoke<ScratchNote[]>("list_scratch"),
          invoke<FailedDictation[]>("list_failed_dictations"),
          invoke<CrashEvent[]>("list_crash_events"),
        ]);
      setSettings(s);
      setStatus(st);
      setInsights(ins);
      setDailySeries(daily);
      setMonthlySeries(monthly);
      setDictionary(dict);
      setCandidates(cands);
      setSnippets(snips);
      setScratch(notes);
      setFailed(fails);
      setCrashes(crashRows);
      setBootError(null);
      await loadHistory(queryRef.current);
      await refreshPerms();
    } catch (e) {
      setBootError(String(e));
      setStatus((p) => ({
        ...p,
        message: `Bridge error: ${e}`,
        lastError: String(e),
      }));
    }
  }, [loadHistory, refreshPerms]);

  // YV94 — load (and search) the Meetings list only while that tab is open. The
  // 200 ms debounce is the same shape History's search uses: one query per pause
  // in typing, not one per keystroke.
  useEffect(() => {
    if (nav !== "meetings") return;
    let dead = false;
    const t = setTimeout(() => {
      if (!dead) loadMeetings(meetingQuery);
    }, meetingQuery ? 200 : 0);
    return () => {
      dead = true;
      clearTimeout(t);
    };
  }, [nav, meetingQuery, loadMeetings]);

  // YV96 — the one-time capture notice: read the stored acknowledgement once on
  // mount, then watch for a meeting actually starting.
  //
  // The subscription itself lives in `meetings/consentWatch.ts` so it can be
  // tested across the process boundary it depends on: the event name and the
  // payload field are a contract with Rust that no compiler checks, and a rename
  // on either side would leave this sheet unreachable with the build still
  // green. `shouldOpenNotice` inside it is idempotent, so the 1 Hz tick that
  // follows the first one costs a boolean and changes nothing.
  useEffect(
    () =>
      watchMeetingConsent({
        currentConsent: () => consentRef.current,
        onConsent: setConsent,
        onOpen: () => setConsentOpen("recording"),
      }),
    [],
  );

  // YV102 — the system-audio setup state, read once on mount alongside YV101's
  // 14.4 gate.
  //
  // Two reads, not one, because they answer different questions: `notetaker_status`
  // says whether this Mac COULD hold the permission (macOS 14.4+), and
  // `system_audio_setup` says what Yap currently believes about whether it DOES.
  // A macOS 13 Mac is "cannot" forever and must never be offered the step; a
  // macOS 15 Mac that has not run it yet is a call to action.
  //
  // Both failures are swallowed to their safe direction: an unanswered gate
  // stays `available` (the affordance shows and the backend refuses if it must
  // — `run_system_audio_setup` re-checks the gate itself), and an unanswered
  // setup read stays `null`, which `setupState` renders as "not run", never as
  // a denial.
  const refreshSystemAudio = useCallback(async () => {
    try {
      setSysAudio(await invoke<SystemAudioSetup>("system_audio_setup"));
    } catch {
      /* stays null → "not run", the safe direction */
    }
  }, []);

  useEffect(() => {
    let dead = false;
    invoke<{
      systemAudioAvailable: boolean;
      systemAudioMessage: string | null;
    }>("notetaker_status")
      .then((s) => {
        if (dead) return;
        setSysAudioGate((prev) => ({
          available: s.systemAudioAvailable,
          message: s.systemAudioMessage ?? prev.message,
        }));
      })
      .catch(() => {});
    refreshSystemAudio();
    return () => {
      dead = true;
    };
  }, [refreshSystemAudio]);

  /**
   * YV102 — run the pre-warm. **This is the permission request**; there is no
   * other one (finding OS-10).
   *
   * The contextual copy is already on screen when this fires, which is the
   * entire deliverable: macOS's alert steals focus the moment the tap starts,
   * and it only ever appears once per install.
   */
  const runSystemAudioSetup = useCallback(async () => {
    setSysAudioBusy(true);
    try {
      setSysAudio(await invoke<SystemAudioSetup>("run_system_audio_setup"));
    } catch (e) {
      toast(String(e));
    } finally {
      setSysAudioBusy(false);
    }
  }, []);

  // YV95 — the meeting status subscription. One `invoke` on mount plus the
  // backend's 1 Hz `meeting` event; when a meeting ENDS, the Meetings list is
  // reloaded so the row the user just recorded is there without a refresh.
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    invoke<MeetingStatus>("meeting_status")
      .then((s) => { if (!dead) setMeetingStatus(s); })
      .catch(() => {});
    listen<MeetingStatus>(MEETING_EVENT, (e) => {
      setMeetingStatus((prev) => {
        if (prev.recording && !e.payload.recording) {
          // The meeting just closed out — pull the row it wrote.
          loadMeetings(meetingQuery);
          refreshInsights();
        }
        return e.payload;
      });
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => { dead = true; unsubs.forEach((u) => u()); };
    // `loadMeetings`/`meetingQuery` are read inside the updater; re-subscribing
    // on every keystroke in the search box would drop events mid-meeting, so
    // this deliberately subscribes ONCE.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // YV75 — refresh the engine snapshot (ASR model + polish sidecar) whenever
  // Privacy & Diagnostics is opened. A sidecar that was cold a minute ago is
  // usually warm by now, so the answer is read at the moment it is asked for.
  useEffect(() => {
    if (nav !== "settings" || settingsTab !== "privacy") return;
    let dead = false;
    invoke<EngineStatus>("engine_status")
      .then((e) => !dead && setEngine(e))
      .catch(() => !dead && setEngine(null));
    return () => {
      dead = true;
    };
  }, [nav, settingsTab]);

  useEffect(() => {
    refreshAll();
    // A synchronous cleanup can run before these listen() promises resolve
    // (StrictMode double-mount). A `dead` flag unsubscribes any listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    listen<AppStatus>("status", (e) => setStatus(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    listen<boolean>("recording", (e) =>
      setStatus((s) => ({
        ...s,
        recording: e.payload,
        message: e.payload
          ? "Recording… release fn⌃ or click Stop"
          : s.message,
      })),
    ).then((u) => (dead ? u() : unsubs.push(u)));
    listen<TranscriptEntry>("transcript", async (e) => {
      setHistory((h) =>
        [e.payload, ...h.filter((x) => x.id !== e.payload.id)].slice(
          0,
          HISTORY_LIMIT,
        ),
      );
      try {
        setInsights(await invoke("get_insights"));
        setDictionary(await invoke("list_dictionary"));
      } catch {
        /* ignore */
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<string>("paste_outcome", (e) => {
      setFlash(e.payload);
      setCopyAgainId(null);
      setTimeout(() => setFlash(null), 2800);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV74 — the ⌘V went out but nothing read the clipboard, so the transcript
    // never reached the app the user was typing into. It arrives right after
    // the `paste_outcome` line that explains why, and turns that toast into an
    // action: one click puts the text back on the clipboard.
    listen<string>("paste_failed", (e) => {
      const id = e.payload;
      setCopyAgainId(id);
      setTimeout(() => setCopyAgainId((c) => (c === id ? null : c)), 2800);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV33 — a failed take must be visible IN the app, not only as a macOS
    // notification the user may have muted (or never sees while Yap is focused).
    // YV52 — the take's audio is kept when it fails, so the payload carries the
    // recoverable row: the toast turns into "… Retry", and the take shows up in
    // the History recovery section until it is retried or discarded.
    listen<{ message: string; failed?: FailedDictation | null }>(
      "transcript_error",
      (e) => {
        setFlash(e.payload?.message || "Transcription failed");
        setTimeout(() => setFlash(null), 4000);
        const row = e.payload?.failed;
        if (!row) return;
        setFailed((f) => [row, ...f.filter((x) => x.id !== row.id)]);
        setRetryId(row.id);
        setTimeout(() => setRetryId((id) => (id === row.id ? null : id)), 4000);
      },
    ).then((u) => (dead ? u() : unsubs.push(u)));
    // Menu-bar "Settings…" jumps the app to the Settings view (YV26).
    listen<string>("navigate", (e) => {
      const dest = e.payload as Nav;
      setNav(dest);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // Tray "Keyboard Shortcuts…" jumps straight to the Shortcut sub-tab.
    listen<string>("settings-tab", (e) => {
      setSettingsTab(e.payload as SettingsTab);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV65 — dragging the pill to a screen edge persists the new dock itself, so
    // the Screen position picker has to follow the pill rather than the other way
    // round. Mirror ONLY that field: the rest of this screen is edited live and
    // must never be overwritten by a broadcast landing mid-edit.
    listen<AppSettings>("settings", (e) => {
      const dock = e.payload?.pillPosition;
      if (!dock) return;
      setSettings((s) => (s && s.pillPosition !== dock ? { ...s, pillPosition: dock } : s));
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YP3 — licensing. `license` is emitted on activation, removal and every
    // revocation refresh that changed something; `license_required` is emitted
    // by the gate itself, which is the ONLY way the hotkey / pill / tray paths
    // (none of which can surface a returned error) can say why nothing
    // happened. Both are read-only here: the sheet is a prompt, never a lock.
    invoke<LicenseStatus>("license_status")
      .then((s) => setLicense(s))
      .catch(() => {
        /* a licensing read that fails must never block the app from booting */
      });
    listen<LicenseStatus>("license", (e) => setLicense(e.payload)).then((u) =>
      dead ? u() : unsubs.push(u),
    );
    listen<LicenseStatus>("license_required", (e) => {
      setLicense(e.payload);
      setBuyPrompt(true);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [refreshAll]);

  useEffect(() => {
    const t = setTimeout(() => {
      loadHistory(query).catch(() => {});
    }, 200);
    return () => clearTimeout(t);
  }, [query, loadHistory]);

  // YV54 — a silently-downloaded model landing changes what the BACKEND reports
  // (status `modelReady`, the Permissions ASR row), and selecting one emits
  // `settings`, not `status`. Re-read once on the false→true edge only, so a
  // launch that already had a model does not pay for a second boot refresh.
  const modelIsReady = modelSetup.ready;
  const wasModelReady = useRef<boolean | null>(null);
  useEffect(() => {
    const prev = wasModelReady.current;
    wasModelReady.current = modelIsReady;
    if (prev === false && modelIsReady) refreshAll();
  }, [modelIsReady, refreshAll]);

  // YV64 — if the backend read a crash off disk that the user has not seen yet,
  // say so ONCE, quietly. A crash that already happened is not worth a modal or
  // a second interruption, so this is the same flash strip every other
  // background message uses, guarded by a ref so a later refresh (a model
  // landing, a reconnect) cannot repeat it within the same launch.
  const crashToastShown = useRef(false);
  useEffect(() => {
    if (crashToastShown.current) return;
    if (!crashes.some((c) => !c.acknowledged)) return;
    crashToastShown.current = true;
    setFlash("Yap had a problem last session — see Diagnostics");
    // Same fire-and-forget shape as the other background flashes: no cleanup,
    // so a later `crashes` update cannot cancel the clear and strand the strip.
    setTimeout(() => setFlash(null), 6000);
  }, [crashes]);

  // YP3 — the trial's ONE warning.
  //
  // A countdown that reappears at every launch, or every day of the last week,
  // is nagware, and the person it annoys most is the one who already decided to
  // buy. So: a single flash, the first time the trial is inside three days,
  // recorded against that trial's expiry so it can never fire twice — not on
  // the next launch, not on the last day. Everything else about the trial lives
  // in the always-on chip, which says nothing until you look at it.
  const trialWarnShown = useRef(false);
  useEffect(() => {
    if (trialWarnShown.current || !license) return;
    if (!shouldWarnTrial(license, readTrialWarnedFor())) return;
    trialWarnShown.current = true;
    writeTrialWarnedFor(license.trial_expires_at_ms);
    setFlash(trialWarningText(trialDaysLeft(license)));
    setTimeout(() => setFlash(null), 6000);
  }, [license]);

  // YV44 — one launch-time check that ONLY looks. The backend gates it on the
  // `checkUpdates` setting and on "skip this version", returns the release
  // without touching a byte of it, and answers null when there is nothing to
  // offer. A failed check (offline, endpoint down) is swallowed: an update is a
  // convenience, never something to interrupt the user with at startup.
  useEffect(() => {
    let dead = false;
    checkForUpdate()
      .then((found) => {
        if (!dead) setUpdate(found);
      })
      .catch(() => {
        /* silent by design — the manual check in Settings reports failures */
      });
    return () => {
      dead = true;
    };
  }, []);

  async function toast(msg: string) {
    setFlash(msg);
    setTimeout(() => setFlash(null), 2000);
  }

  // Every mutating command returns Result<_, String>; on Err the promise rejects.
  // Without a catch the UI silently no-ops (and throws an unhandled rejection),
  // so a failed paste/save/delete looks identical to success. Surface it.
  // YP2: `manual_toggle` can now reject with a structured `{code, message}` —
  // the license gate needs a sentence, not `[object Object]`. Only STARTING a
  // dictation can be refused; a take already running always gets to finish.
  // YP3: and when the reason IS the license, a 2-second toast is the wrong
  // surface — that person needs a way to buy, not a sentence that vanishes.
  // The gate also emits `license_required`, so the sheet is usually already up
  // by the time this lands; setting a boolean twice is a no-op.
  async function toggleRecord() {
    try {
      await invoke("manual_toggle");
    } catch (e) {
      if (isLicenseRequired(e)) setBuyPrompt(true);
      else toast(errorText(e));
    }
  }

  // ── YV94 · meeting actions ──

  /**
   * YV96 — close the one-time notice, and record that it was shown.
   *
   * Every exit routes here — the button, Esc, a click on the backdrop — because
   * a one-time notice is about display, not assent: there is no version of
   * "closed it" that means the user did not see it. Nothing on the capture path
   * waits on this call, so a failed write costs a second showing and never a
   * blocked recording.
   */
  async function closeConsentNotice() {
    setConsentOpen(null);
    if (consent && !consent.shouldShow) return;
    // Close the door before the round-trip, not after: the `meeting` tick
    // arrives every second, and a write that took longer than that would
    // re-open the sheet the user just closed. If the write then fails, this
    // session stays quiet (they have seen it) and the next launch shows it
    // again — the safe direction to be wrong in.
    consentRef.current = {
      shouldShow: false,
      acknowledgedAt: null,
      blocksRecording: false,
    };
    try {
      setConsent(await invoke<MeetingConsent>("acknowledge_meeting_consent"));
    } catch {
      /* shown again next time, which is the safe direction to fail in */
    }
  }

  /**
   * YV98 — open the crash report, which means BUILD it first.
   *
   * The sheet opens immediately with a null preview rather than after the
   * build, so a slow log read reads as "building" instead of a dead button.
   * Nothing is on disk yet either way: `preview_support_bundle` works in
   * memory, and the bytes it returns are the bytes `send_support_bundle` will
   * write — that is the only reason showing them means anything.
   */
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

  /**
   * Write it, then hand it to the user's mail client — or, when AppKit says it
   * cannot drive one, to Finder and the clipboard. Both outcomes report what
   * actually happened, because "sent" would be a lie in both: the compose path
   * opens a window the user still has to send, and the reveal path never opened
   * a message at all.
   */
  async function sendSupportBundle() {
    setSupportBusy(true);
    try {
      const outcome = await invoke<SupportSendOutcome>("send_support_bundle");
      setSupportOpen(false);
      setSupportPreview(null);
      toast(outcome.message);
    } catch (e) {
      toast(String(e));
    } finally {
      setSupportBusy(false);
    }
  }

  /**
   * Export one meeting as Markdown. PDF was cut for v1 (finding #33): webview
   * print pagination over a 3-hour transcript is a real time sink for a feature
   * nobody has asked for, and a .md opens in every editor and note app.
   */
  async function exportMeeting(id: string) {
    setMeetingBusy(true);
    try {
      const { path } = await invoke<{ path: string; count: number }>(
        "export_meeting_markdown",
        { id },
      );
      toast(`Exported → ${path}`);
    } catch (e) {
      toast(errorText(e));
    } finally {
      setMeetingBusy(false);
    }
  }

  /**
   * YV95 — start or stop a meeting. The Meetings tab's button, the tray item,
   * ⌃⌘M and the pill's stop control all reach the SAME backend toggle, so the
   * four can never disagree about what pressing them does.
   */
  async function toggleMeetingRecording() {
    setMeetingBusy(true);
    try {
      const next = await invoke<MeetingStatus>("toggle_meeting_recording");
      setMeetingStatus(next);
      if (!next.recording) await loadMeetings(meetingQuery);
    } catch (e) {
      toast(errorText(e));
    } finally {
      setMeetingBusy(false);
    }
  }

  /**
   * Delete a meeting: rows, search index and the audio, in one go. `secure_delete`
   * means the pages are physically overwritten, so on a long meeting this is real
   * work — the button stays disabled until the backend answers rather than
   * reporting success while bytes are still on disk (the YV78 lesson).
   */
  async function removeMeeting(id: string) {
    setMeetingBusy(true);
    try {
      await invoke("delete_meeting", { id });
      setConfirmDeleteMeeting(null);
      setOpenMeeting((m) => (m?.meeting.id === id ? null : m));
      await loadMeetings(meetingQuery);
      toast("Meeting deleted");
    } catch (e) {
      toast(errorText(e));
    } finally {
      setMeetingBusy(false);
    }
  }

  async function renameMeeting(id: string, title: string) {
    const next = title.trim();
    if (!next) return;
    try {
      await invoke("rename_meeting", { id, title: next });
      setOpenMeeting((m) =>
        m?.meeting.id === id
          ? { ...m, meeting: { ...m.meeting, title: next } }
          : m,
      );
      await loadMeetings(meetingQuery);
    } catch (e) {
      toast(errorText(e));
    }
  }

  // ── YP3 · licensing actions ──
  // The URL is NOT here: `open_purchase_page` takes no argument and opens the
  // compile-time `license::PAYMENT_LINK_URL`, so no string the webview can
  // influence ever reaches a process launch.
  async function buyYap() {
    try {
      await invoke("open_purchase_page");
    } catch (e) {
      toast(errorText(e));
    }
  }

  /** Rejects with the backend's typed `{code, message}` so the panel can show it. */
  async function activateLicense(key: string) {
    const status = await invoke<LicenseStatus>("activate_license", { key });
    setLicense(status);
    setBuyPrompt(false);
    toast("Yap is licensed on this Mac");
  }

  async function deactivateLicense() {
    try {
      setLicense(await invoke<LicenseStatus>("deactivate_license"));
      toast("License removed from this Mac");
    } catch (e) {
      toast(errorText(e));
    }
  }

  /** From the purchase sheet: land on the key box, not just the tab. */
  function openLicenseTab() {
    setBuyPrompt(false);
    setNav("settings");
    setSettingsTab("license");
  }

  async function copyText(text: string) {
    try {
      await invoke("copy_entry", { text });
      toast("Copied");
    } catch (e) {
      toast(String(e));
    }
  }

  async function pasteText(text: string) {
    try {
      const msg = await invoke<string>("paste_entry", { text });
      toast(msg);
    } catch (e) {
      toast(String(e));
    }
    await refreshPerms();
  }

  // Best-effort insights refresh — its failure must not toast an error for a
  // mutation that already succeeded.
  async function refreshInsights() {
    try {
      const [ins, daily, monthly] = await Promise.all([
        invoke<Insights>("get_insights"),
        invoke<DayCount[]>("daily_series", { days: 365 }),
        invoke<DayCount[]>("monthly_series", { months: 12 }),
      ]);
      setInsights(ins);
      setDailySeries(daily);
      setMonthlySeries(monthly);
    } catch {
      /* leave stale insights; the mutation itself succeeded */
    }
  }

  async function removeEntry(id: string) {
    try {
      await invoke("delete_entry", { id });
      setHistory((h) => h.filter((e) => e.id !== id));
    } catch (e) {
      toast(String(e));
      return;
    }
    await refreshInsights();
  }

  // YV52 — re-run ASR on a failed take's saved clip. On success the row leaves
  // the recovery list and becomes a normal history entry (the backend does the
  // conversion), with the recovered text on the clipboard.
  async function retryFailed(id: string) {
    if (retrying) return;
    setRetrying(id);
    try {
      const entry = await invoke<TranscriptEntry>("retry_failed_dictation", {
        id,
      });
      setFailed((f) => f.filter((x) => x.id !== id));
      setRetryId((cur) => (cur === id ? null : cur));
      setHistory((h) =>
        [entry, ...h.filter((x) => x.id !== entry.id)].slice(0, HISTORY_LIMIT),
      );
      toast(`Recovered ${entry.wordCount} words — copied to clipboard`);
      await refreshInsights();
    } catch (e) {
      toast(String(e));
      // The clip may have been dropped (audio gone) — resync either way.
      await loadFailed().catch(() => {});
    } finally {
      setRetrying(null);
    }
  }

  // YV52 — throw a failed take away: the row and its audio both go.
  async function discardFailed(id: string) {
    try {
      await invoke("discard_failed_dictation", { id });
      setFailed((f) => f.filter((x) => x.id !== id));
      setRetryId((cur) => (cur === id ? null : cur));
    } catch (e) {
      toast(String(e));
    }
  }

  // YV78 — this really does destroy the words (secure_delete + FTS rebuild +
  // VACUUM), so it can take a moment on a big history. Hold the button until
  // the command resolves rather than clearing the list optimistically.
  async function clearAll() {
    if (clearing) return;
    if (!confirm("Clear all transcript history from SQLite?")) return;
    setClearing(true);
    try {
      await invoke("clear_history");
      setHistory([]);
    } catch (e) {
      toast(String(e));
      return;
    } finally {
      setClearing(false);
    }
    await refreshInsights();
  }

  async function saveSettings(next: AppSettings) {
    try {
      await invoke("save_settings", { settings: next });
      setSettings(next);
      toast("Settings saved");
    } catch (e) {
      toast(String(e));
    }
  }

  // YV44 — the ONLY path that installs an update: the user clicked "Install
  // now" on the prompt. The new bundle applies on the next launch, so we say so
  // instead of restarting Yap out from under a dictation.
  async function installUpdateNow() {
    setInstalling(true);
    try {
      const version = await installUpdate();
      setUpdate(null);
      setInstalledVersion(version);
    } catch (e) {
      toast(String(e));
    } finally {
      setInstalling(false);
    }
  }

  // "Skip this version" — remembered in settings, so this exact release never
  // prompts again while a newer one still will.
  async function skipUpdateVersion() {
    if (!update || !settings) return;
    const version = update.version;
    const next: AppSettings = { ...settings, skippedUpdateVersion: version };
    setUpdate(null);
    try {
      await invoke("save_settings", { settings: next });
      setSettings(next);
      toast(`Skipping Yap ${version} — you'll hear about the next one`);
    } catch (e) {
      toast(String(e));
    }
  }

  // Settings → Advanced "Check for updates now". Unlike the launch check this
  // one always answers the user, including when the check itself failed.
  async function checkForUpdateNow() {
    try {
      // Asking explicitly means "show me anything" — retire an earlier skip so
      // the version they dismissed can be offered again on request.
      if (settings?.skippedUpdateVersion) {
        const next: AppSettings = { ...settings, skippedUpdateVersion: null };
        await invoke("save_settings", { settings: next });
        setSettings(next);
      }
      const found = await checkForUpdate();
      setUpdate(found);
      toast(
        found
          ? `Yap ${found.version} is available`
          : "You're on the latest Yap",
      );
    } catch (e) {
      toast(`Couldn't check for updates: ${e}`);
    }
  }

  // YV15/YV22 — apply a push-to-talk binding + keep the human label in sync.
  // Shared by the preset chips and the key-capture control so both stay
  // consistent. The dictation engine (ptt_macos CGEvent tap) binds the fn (Globe)
  // key, so the persisted value is always one of the supported ids:
  // fn | fn_control | both. Persist via saveSettings so the change survives quit
  // AND re-binds the live PTT engine in-session — the backend save_settings
  // command calls ptt_macos::set_binding, so a remapped shortcut works
  // immediately instead of only after a relaunch.
  async function applyBinding(id: string) {
    if (!settings) return;
    await saveSettings({
      ...settings,
      pttBinding: id,
      hotkeyLabel: id === "fn" ? "fn" : id === "both" ? "fn / fn⌃" : "fn⌃",
    });
  }

  // YV9 — persist onboarding completion (+ optional calibration sample) so the
  // first-run flow does not re-appear on next launch. Uses saveSettings per spec.
  async function finishOnboarding(sample: string | null) {
    if (!settings) return;
    await saveSettings({
      ...settings,
      onboarded: true,
      calibrationSample: sample ?? settings.calibrationSample ?? null,
    });
  }

  // YV54 — the model picker's one home: Settings → Advanced. Every "get / manage
  // a speech model" affordance in the app lands here now that onboarding no
  // longer asks the question.
  function openModelSettings() {
    setNav("settings");
    setSettingsTab("advanced");
  }

  // "Replay onboarding" — clear the gate so the flow shows again.
  async function replayOnboarding() {
    if (!settings) return;
    await saveSettings({ ...settings, onboarded: false });
  }

  async function addTerm() {
    if (!newTerm.trim()) return;
    try {
      await invoke("add_dictionary_term", {
        term: newTerm.trim(),
        preferred: newPreferred.trim() || null,
      });
      setNewTerm("");
      setNewPreferred("");
      setDictionary(await invoke("list_dictionary"));
      toast("Dictionary term added");
    } catch (e) {
      toast(String(e));
    }
  }

  async function removeTerm(id: string) {
    try {
      await invoke("delete_dictionary_term", { id });
      setDictionary((d) => d.filter((x) => x.id !== id));
    } catch (e) {
      toast(String(e));
    }
  }

  // YV47 — star / unstar "always bias". Re-lists because starring changes the
  // ranking the whole screen (and the decoder prompt) is ordered by.
  async function toggleStar(d: DictEntry) {
    try {
      await invoke("set_dictionary_starred", {
        id: d.id,
        starred: !d.starred,
      });
      setDictionary(await invoke("list_dictionary"));
    } catch (e) {
      toast(String(e));
    }
  }

  function startEditTerm(d: DictEntry) {
    setEditingTermId(d.id);
    setEditTerm(d.term);
    setEditPreferred(d.preferred ?? "");
  }

  async function saveEditTerm() {
    if (!editingTermId || !editTerm.trim()) return;
    try {
      await invoke("update_dictionary_term", {
        id: editingTermId,
        term: editTerm.trim(),
        preferred: editPreferred.trim() || null,
      });
      setEditingTermId(null);
      setDictionary(await invoke("list_dictionary"));
      toast("Dictionary term updated");
    } catch (e) {
      toast(String(e));
    }
  }

  // YV48 — snippets CRUD. Every mutation re-lists so the order (enabled first,
  // then A→Z) matches what the matcher will actually run with.
  async function addSnippet() {
    if (!newTrigger.trim() || !newExpansion.trim()) return;
    try {
      await invoke("add_snippet", {
        trigger: newTrigger.trim(),
        expansion: newExpansion,
      });
      setNewTrigger("");
      setNewExpansion("");
      setSnippets(await invoke("list_snippets"));
      toast("Snippet saved");
    } catch (e) {
      toast(String(e));
    }
  }

  function startEditSnippet(s: Snippet) {
    setEditingSnippetId(s.id);
    setEditTrigger(s.trigger);
    setEditExpansion(s.expansion);
  }

  async function saveEditSnippet() {
    if (!editingSnippetId || !editTrigger.trim() || !editExpansion.trim()) return;
    try {
      await invoke("update_snippet", {
        id: editingSnippetId,
        trigger: editTrigger.trim(),
        expansion: editExpansion,
      });
      setEditingSnippetId(null);
      setSnippets(await invoke("list_snippets"));
      toast("Snippet updated");
    } catch (e) {
      toast(String(e));
    }
  }

  async function toggleSnippet(s: Snippet) {
    try {
      await invoke("set_snippet_enabled", { id: s.id, enabled: !s.enabled });
      setSnippets(await invoke("list_snippets"));
    } catch (e) {
      toast(String(e));
    }
  }

  async function removeSnippet(id: string) {
    try {
      await invoke("delete_snippet", { id });
      setSnippets((all) => all.filter((s) => s.id !== id));
    } catch (e) {
      toast(String(e));
    }
  }

  // YV47 — accept a mined suggestion into the dictionary, or hide it for good.
  async function acceptCandidate(c: DictCandidate) {
    try {
      await invoke("promote_dict_candidate", { id: c.id });
      const [dict, cands] = await Promise.all([
        invoke<DictEntry[]>("list_dictionary"),
        invoke<DictCandidate[]>("list_dict_candidates"),
      ]);
      setDictionary(dict);
      setCandidates(cands);
      toast(`Added ${c.term} to your dictionary`);
    } catch (e) {
      toast(String(e));
    }
  }

  async function dismissCandidate(c: DictCandidate) {
    try {
      await invoke("dismiss_dict_candidate", { id: c.id });
      setCandidates((cs) => cs.filter((x) => x.id !== c.id));
    } catch (e) {
      toast(String(e));
    }
  }

  // YV47 — "Fix transcription": the honest correction path. The user edits a
  // known transcript and Yap diffs the result, so what it learns is exactly
  // what they changed — no clipboard sniffing, no accessibility guesswork.
  function startFix(e: TranscriptEntry) {
    setFixingId(e.id);
    setFixDraft(e.text);
  }

  async function saveFix() {
    if (!fixingId || !fixDraft.trim()) return;
    try {
      const learned = await invoke<number>("correct_transcript", {
        id: fixingId,
        text: fixDraft.trim(),
      });
      setFixingId(null);
      const [cands] = await Promise.all([
        invoke<DictCandidate[]>("list_dict_candidates"),
        loadHistory(queryRef.current),
      ]);
      setCandidates(cands);
      toast(
        learned > 0
          ? `Fixed — ${learned} term${learned === 1 ? "" : "s"} to review in Dictionary`
          : "Transcript fixed",
      );
    } catch (e) {
      toast(String(e));
    }
  }

  async function saveNote() {
    try {
      const note = await invoke<ScratchNote>("save_scratch", {
        id: activeNoteId,
        title: noteTitle || "Note",
        body: noteBody,
      });
      setActiveNoteId(note.id);
      setScratch(await invoke("list_scratch"));
      toast("Scratchpad saved");
    } catch (e) {
      toast(String(e));
    }
  }

  function openNote(n: ScratchNote) {
    setActiveNoteId(n.id);
    setNoteTitle(n.title);
    setNoteBody(n.body);
  }

  async function newNote() {
    setActiveNoteId(null);
    setNoteTitle("New note");
    setNoteBody("");
  }

  async function deleteNote(id: string) {
    try {
      await invoke("delete_scratch", { id });
      if (activeNoteId === id) newNote();
      setScratch(await invoke("list_scratch"));
    } catch (e) {
      toast(String(e));
    }
  }

  // YP3 — `null` until the first status lands, so the header never flashes a
  // guessed state at a paying customer.
  const licenseChip = license ? chipFor(license) : null;

  const pillClass = status.recording
    ? "status-pill recording"
    : status.busy
      ? "status-pill busy"
      : status.lastError
        ? "status-pill error"
        : "status-pill ready";

  const maxWeek = useMemo(
    () => Math.max(1, ...(insights?.wordsLast7.map((d) => d.words) ?? [1])),
    [insights],
  );

  // Daily words bar chart — last ~30 days (dailySeries is oldest-first, len 365).
  const daily30 = useMemo(() => dailySeries.slice(-30), [dailySeries]);
  const maxDaily30 = useMemo(
    () => Math.max(1, ...daily30.map((d) => d.words)),
    [daily30],
  );

  // GitHub-style activity heatmap — lay the contiguous 365-day series into
  // week-columns × weekday-rows, aligning the first cell to its real weekday.
  const heat = useMemo(() => {
    if (dailySeries.length === 0) {
      return {
        cols: 0,
        cells: [] as { d: DayCount; col: number; row: number }[],
        months: [] as { key: string; label: string; col: number }[],
        max: 0,
      };
    }
    const wd0 = new Date(dailySeries[0].date + "T12:00:00").getDay(); // 0=Sun
    const max = Math.max(1, ...dailySeries.map((d) => d.words));
    const cells = dailySeries.map((d, i) => {
      const g = wd0 + i;
      return { d, col: Math.floor(g / 7), row: g % 7 };
    });
    const cols = Math.ceil((wd0 + dailySeries.length) / 7);
    // Month ticks (YV55): label the column each month starts in, dropping any
    // label that would sit on top of the previous one or run off the last
    // column (a clipped half-word is worse than no tick).
    const months: { key: string; label: string; col: number }[] = [];
    let lastYm = "";
    for (const { d, col } of cells) {
      const ym = d.date.slice(0, 7);
      if (ym === lastYm) continue;
      lastYm = ym;
      const prev = months[months.length - 1];
      if (prev && col - prev.col < 3) continue;
      if (cols - col < 3) continue; // no room left to draw it in full
      months.push({ key: ym, label: monthName(ym), col });
    }
    return { cols, cells, months, max };
  }, [dailySeries]);
  const hasActivity = useMemo(
    () => dailySeries.some((d) => d.words > 0),
    [dailySeries],
  );

  // YV55 — one honest line for the exact window the heatmap draws, so the year
  // grid is read rather than counted.
  const heatSummary = useMemo(() => {
    let words = 0;
    let sessions = 0;
    let best: DayCount | null = null;
    for (const d of dailySeries) {
      words += d.words;
      sessions += d.sessions;
      if (!best || d.words > best.words) best = d;
    }
    return { words, sessions, best: best && best.words > 0 ? best : null };
  }, [dailySeries]);

  // Monthly words bar chart — YV55: an empty month is not information, so the
  // leading run of silent months collapses into one quiet line and only months
  // that carry words are charted. The current month is always a row (today's
  // ramp has to be visible even at zero) and the chart never shrinks below 3
  // rows, so a young install still reads as a trend rather than a single bar.
  const monthlyView = useMemo(() => {
    if (monthlySeries.length === 0) {
      return { rows: [] as DayCount[], quiet: 0, max: 1 };
    }
    const last = monthlySeries.length - 1;
    const shown = new Set<number>();
    monthlySeries.forEach((d, i) => {
      if (d.words > 0 || i === last) shown.add(i);
    });
    for (let i = last; i >= 0 && shown.size < 3; i--) shown.add(i);
    const rows = monthlySeries.filter((_, i) => shown.has(i));
    const first = Math.min(...shown);
    return {
      rows,
      quiet: first, // every month before the first shown one is silent
      max: Math.max(1, ...rows.map((d) => d.words)),
    };
  }, [monthlySeries]);

  // Top apps (YV55) — each row carries its own bar, scaled to the busiest app.
  const maxApp = useMemo(
    () => Math.max(1, ...(insights?.topApps.map((a) => a.words) ?? [1])),
    [insights],
  );

  const needsPerms = perms && !perms.allCriticalOk;

  if (bootError && !settings) {
    return (
      <div style={{ padding: 24, fontFamily: "system-ui" }}>
        <h2>Yap</h2>
        <p>UI failed to load backend bridge:</p>
        <pre style={{ whiteSpace: "pre-wrap" }}>{bootError}</pre>
        <button type="button" onClick={() => refreshAll()}>
          Retry
        </button>
      </div>
    );
  }

  // YV9 — first-run onboarding gate: show once settings are loaded and the user
  // has not completed it. Rendered over the app; the main UI stays mounted.
  if (settings && !settings.onboarded) {
    return (
      <Onboarding
        onFinish={(sample) => finishOnboarding(sample)}
        onSkip={() => finishOnboarding(null)}
      />
    );
  }

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="mark">
            <span className={status.recording ? "dot pulse" : "dot"} />
          </div>
          <div>
            <div className="brand-name">Yap</div>
            <div className="brand-tag">v{__APP_VERSION__} · local · private</div>
          </div>
        </div>

        <nav className="nav">
          {(
            [
              ["home", "Home", history.length],
              ["permissions", "Permissions", needsPerms ? 1 : null],
              // YV94 — Meetings sits next to Home because it is the second
              // thing Yap keeps for you, not a setting.
              ["meetings", "Meetings", meetings.length],
              ["insights", "Insights", null],
              ["dictionary", "Dictionary", dictionary.length],
              ["scratchpad", "Scratchpad", scratch.length],
              ["settings", "Settings", null],
            ] as const
          ).map(([id, label, count]) => (
            <button
              key={id}
              className={nav === id ? "nav-item active" : "nav-item"}
              aria-current={nav === id ? "page" : undefined}
              onClick={() => setNav(id)}
            >
              <span>{label}</span>
              {count != null && count > 0 && (
                <span className={id === "permissions" ? "count warn" : "count"}>
                  {count}
                </span>
              )}
            </button>
          ))}
        </nav>

        <div className="sidebar-foot">
          <button
            className={status.recording ? "dictate-side live" : "dictate-side"}
            onClick={toggleRecord}
            disabled={status.busy}
          >
            {/* YV56 — "Dictate · fn⌃" was one string, so the verb and the key
                carried identical weight. The action stays native SF; the key is
                data and wears the pixel voice in its own chip, the same
                treatment the nav counts and the header status pill take. */}
            {status.recording ? (
              "Stop listening"
            ) : status.busy ? (
              "Transcribing…"
            ) : (
              <>
                Dictate
                <span className="dictate-key">fn⌃</span>
              </>
            )}
          </button>
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          <div>
            <h1>
              {nav === "home" && (userName ? `Welcome back, ${userName}` : "Welcome back")}
              {nav === "permissions" && "Permissions"}
              {nav === "meetings" && "Meetings"}
              {nav === "insights" && "Insights"}
              {nav === "dictionary" && "Dictionary"}
              {nav === "scratchpad" && "Scratchpad"}
              {nav === "settings" && "Settings"}
            </h1>
            <p className="lede">
              {nav === "home" &&
                "Hold fn⌃ and talk — your words land where the cursor is."}
              {nav === "permissions" &&
                "macOS must grant these to Yap itself. Without them, dictation or paste fails."}
              {nav === "meetings" &&
                "Recorded meetings, searchable and exportable. Audio is kept for 7 days; the transcript stays."}
              {nav === "insights" &&
                "Local analytics from your SQLite history — nothing leaves this Mac."}
              {nav === "dictionary" &&
                "Custom spellings applied after each transcription."}
              {nav === "scratchpad" && "Park text and assemble prompts."}
              {nav === "settings" &&
                "Your companion, dictation, shortcut, and privacy — all in plain language."}
            </p>
          </div>
          <div className="head-state">
            {/* YP3 — the always-on entitlement chip. It never interrupts: it is
                a button because the one thing you want after reading it is the
                License tab. The numeral wears the pixel voice, the same
                treatment every other piece of data in the app gets. */}
            {licenseChip && (
              <button
                type="button"
                className={`license-chip ${licenseChip.tone}`}
                title={licenseChip.title}
                onClick={() => {
                  setNav("settings");
                  setSettingsTab("license");
                }}
              >
                <span className="license-chip-label">{licenseChip.label}</span>
                {licenseChip.value && (
                  <span className="license-chip-value">{licenseChip.value}</span>
                )}
              </button>
            )}
            <div className={pillClass}>{status.message}</div>
          </div>
        </header>

        {flash && (
          <div className="toast">
            <span>{flash}</span>
            {/* YV52 — the take that just failed still has its audio: retry runs
                ASR again on that clip (the model may have finished downloading,
                or the engine recovered) instead of making the user re-speak. */}
            {retryId && (
              <button
                className="toast-retry"
                disabled={retrying === retryId}
                onClick={() => retryFailed(retryId)}
              >
                {retrying === retryId ? "Retrying…" : "Retry"}
              </button>
            )}
            {/* YV74 — the paste was not confirmed by a read receipt, so the
                text may not have landed anywhere. Put it back on the clipboard
                on demand rather than making the user hunt through History. */}
            {copyAgainId && (
              <button
                className="toast-copy-again"
                onClick={() => {
                  const row = history.find((h) => h.id === copyAgainId);
                  if (row) copyText(row.text);
                  else toast("That transcript is in History");
                  setCopyAgainId(null);
                }}
              >
                Copy again
              </button>
            )}
          </div>
        )}

        {/* YV43 — another app holds macOS Secure Input, so the CGEvent tap
            behind fn / fn⌃ is blind and push-to-talk is dead RIGHT NOW. It
            outranks every other banner because nothing below it can be reached
            with the hotkey while this is on, and it names the holder so the
            user knows which app to go release. */}
        {status.secureInputBlocked && (
          <div className="banner blocked">
            <strong>
              Dictation paused — another app is blocking keyboard monitoring
              (Secure Input)
            </strong>
            <span className="banner-detail">{status.secureInputDetail}</span>
          </div>
        )}

        {/* YV54 — no usable model is no longer a "Model needed" demand that
            routes the user into a decision screen: the download is already
            running, so this is the same slim ribbon onboarding shows, and it
            retires itself the moment the engine is ready. */}
        <ModelRibbon setup={modelSetup} />

        {status.modelReady && needsPerms && nav === "home" && (
          <div className="banner warn" onClick={() => setNav("permissions")}>
            Setup incomplete — open Permissions to enable Mic / Accessibility
            for Yap
          </div>
        )}

        {/* YV44 — a newer Yap EXISTS. Nothing has been downloaded: this banner
            is the whole notification, it never blocks the app, and the install
            happens only if the user asks for it here. "Later" hides it until
            the next launch; "Skip this version" retires this release for good. */}
        {update && (
          <div className="banner update">
            <strong>
              Update available — Yap {update.version} (you're on{" "}
              {update.currentVersion}). Install now?
            </strong>
            {update.notes && (
              <span className="banner-detail">{update.notes}</span>
            )}
            <div className="banner-actions">
              <button
                className="primary"
                onClick={installUpdateNow}
                disabled={installing}
              >
                {installing ? "Installing…" : "Install now"}
              </button>
              <button onClick={() => setUpdate(null)} disabled={installing}>
                Later
              </button>
              <button onClick={skipUpdateVersion} disabled={installing}>
                Skip this version
              </button>
            </div>
          </div>
        )}

        {installedVersion && (
          <div className="banner update">
            <strong>
              Yap {installedVersion} installed — quit and reopen Yap to finish.
            </strong>
          </div>
        )}

        <div className="content">
          {nav === "permissions" && (
            <div className="perms">
              <div className="panel intro">
                <h3>Enable for this app only</h3>
                <p>
                  Bundle id <code>com.wilsonguenther.wilson-voice</code> must
                  appear as <strong>Yap</strong> in System Settings →
                  Privacy &amp; Security. Yap runs no helper process, so that
                  one row is the only thing you ever enable.
                </p>
                <p className="muted">{perms?.summary}</p>
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
                      try {
                        await invoke("request_microphone");
                      } catch (e) {
                        toast(String(e));
                      }
                      setTimeout(refreshPerms, 1000);
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
          )}

          {nav === "home" && (
            <>
              <YappyHouse
                wordsToday={insights?.wordsToday}
                streakDays={insights?.streakDays}
                companionTone={settings?.companionTone}
              />

              <button
                className={
                  status.recording
                    ? "record-btn live"
                    : status.busy
                      ? "record-btn busy"
                      : "record-btn"
                }
                onClick={toggleRecord}
                disabled={status.busy}
              >
                <span className="mic" aria-hidden>
                  {status.recording ? "■" : status.busy ? "…" : "●"}
                </span>
                <span>
                  {status.recording
                    ? "Listening — release fn⌃ or tap to stop hands-free"
                    : status.busy
                      ? "Transcribing…"
                      : "Hold fn⌃ · double-tap hands-free"}
                </span>
              </button>

              <div className="toolbar">
                <input
                  type="search"
                  placeholder="Search transcripts (SQLite FTS5)…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <button className="ghost" onClick={clearAll} disabled={clearing}>
                  {clearing ? "Clearing…" : "Clear"}
                </button>
              </div>

              {/* YV52 — takes whose transcription failed. The audio is still on
                  disk, so nothing was lost: retry re-runs ASR on that same clip,
                  discard throws the row and its audio away. Clips are purged
                  automatically after 7 days. */}
              {failed.length > 0 && (
                <section className="failed-takes">
                  <div className="failed-head">
                    <h3>Failed dictations</h3>
                    <span className="tiny">
                      Audio kept for 7 days — retry when the engine is ready,
                      nothing was lost.
                    </span>
                  </div>
                  <ul className="feed">
                    {failed.map((f) => (
                      <li key={f.id} className="card failed">
                        <div className="card-meta">
                          <span>
                            {formatTime(f.createdAt)}
                            {f.sourceApp ? ` · ${f.sourceApp}` : ""}
                          </span>
                          <span>
                            {f.speechSeconds > 0
                              ? `${f.speechSeconds.toFixed(1)}s of audio`
                              : "audio saved"}
                          </span>
                        </div>
                        <p className="failed-why">{f.error}</p>
                        <div className="actions">
                          <button
                            className="primary"
                            disabled={retrying === f.id}
                            onClick={() => retryFailed(f.id)}
                          >
                            {retrying === f.id ? "Retrying…" : "Retry"}
                          </button>
                          <button
                            className="ghost danger"
                            onClick={() => discardFailed(f.id)}
                          >
                            Discard
                          </button>
                        </div>
                      </li>
                    ))}
                  </ul>
                </section>
              )}

              {history.length === 0 ? (
                <div className="empty">
                  <h3>No dictations yet</h3>
                  <p>
                    Click Dictate or hold <kbd>fn</kbd>. Text is stored locally
                    and searchable forever.
                  </p>
                </div>
              ) : (
                <ul className="feed">
                  {history.map((e) => (
                    <li key={e.id} className="card">
                      <div className="card-meta">
                        <span>
                          {formatTime(e.createdAt)}
                          {e.sourceApp ? ` · ${e.sourceApp}` : ""}
                        </span>
                        <span>
                          {e.wordCount} words · {e.backend}
                          {(e.speechSeconds ?? 0) > 0
                            ? ` · ${e.speechSeconds!.toFixed(1)}s speech`
                            : ""}
                          {` · ${e.asrSeconds.toFixed(1)}s asr`}
                          {(e.pipelineMs ?? 0) > 0
                            ? ` · ${e.pipelineMs}ms hold→clip`
                            : ""}
                        </span>
                      </div>
                      {fixingId === e.id ? (
                        <div className="fix-edit">
                          <textarea
                            value={fixDraft}
                            onChange={(ev) => setFixDraft(ev.target.value)}
                            aria-label="Corrected transcript"
                            autoFocus
                          />
                          <p className="tiny">
                            Correct the words Yap got wrong — it learns them as
                            dictionary suggestions.
                          </p>
                          <div className="actions">
                            <button className="primary" onClick={saveFix}>
                              Save fix
                            </button>
                            <button
                              className="ghost"
                              onClick={() => setFixingId(null)}
                            >
                              Cancel
                            </button>
                          </div>
                        </div>
                      ) : (
                        <>
                          <p>{e.text}</p>
                          <div className="actions">
                            <button onClick={() => copyText(e.text)}>
                              Copy
                            </button>
                            <button
                              className="primary"
                              onClick={() => pasteText(e.text)}
                            >
                              Paste
                            </button>
                            {/* YV51: only offered when the cleanup pipeline
                                actually changed this take — otherwise "Paste
                                raw" would be a duplicate of Paste. */}
                            {undoAiEditText(e) && (
                              <button
                                className="ghost"
                                title="Paste exactly what you said, before auto-cleanup"
                                onClick={() =>
                                  pasteText(undoAiEditText(e) as string)
                                }
                              >
                                Paste raw
                              </button>
                            )}
                            <button
                              className="ghost"
                              onClick={() => startFix(e)}
                            >
                              Fix transcription
                            </button>
                            <button
                              className="ghost"
                              onClick={() => {
                                setNav("scratchpad");
                                setNoteTitle(`From ${formatTime(e.createdAt)}`);
                                setNoteBody(e.text);
                                setActiveNoteId(null);
                              }}
                            >
                              Scratchpad
                            </button>
                            <button
                              className="ghost danger"
                              onClick={() => removeEntry(e.id)}
                            >
                              Delete
                            </button>
                          </div>
                        </>
                      )}
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}

          {/* YV94 — Meetings. Two states in one screen: the list, and one
              meeting's transcript. Nothing here STARTS a meeting — capture is
              YV91 and the entry points (tray, hotkey, pill, the empty state's
              own button) are YV95. This ships the surface that makes a recorded
              meeting findable, readable, exportable and deletable. */}
          {nav === "meetings" && !openMeeting && (
            <>
              {/* YV95 — the live recording banner. Present ONLY while a meeting
                  is running, and it carries the same clock the pill shows,
                  rendered once in Rust and emitted at 1 Hz. */}
              {meetingStatus.recording && (
                <div className="recording-bar" role="status">
                  <span className="rec-dot" aria-hidden />
                  <div className="recording-copy">
                    <strong>{meetingStatus.title || "Recording a meeting"}</strong>
                    <span className="tiny">
                      Recording for {elapsedLabel(meetingStatus)} · stop from here, the
                      menu bar, ⌃⌘M, or the pill.
                    </span>
                    {/* YV110 — matrix rows 1 and 2. A meeting that is recording
                        the microphone only, or that lost the call's audio
                        mid-way, says so here in full while it is still
                        recording — never afterwards, and never as a dialog. */}
                    {systemAudioBadge(meetingStatus) && (
                      <span className="tiny rec-sysaudio-note">
                        {systemAudioBadge(meetingStatus)}
                      </span>
                    )}
                  </div>
                  <span className="rec-clock">{elapsedLabel(meetingStatus)}</span>
                  <button
                    className="primary danger"
                    disabled={meetingBusy}
                    onClick={toggleMeetingRecording}
                  >
                    Stop meeting
                  </button>
                </div>
              )}

              <div className="toolbar">
                <input
                  type="search"
                  value={meetingQuery}
                  onChange={(e) => setMeetingQuery(e.target.value)}
                  placeholder="Search meetings — titles and every word said in them…"
                  aria-label="Search meetings"
                />
                {meetingQuery && (
                  <button className="ghost" onClick={() => setMeetingQuery("")}>
                    Clear
                  </button>
                )}
                {/* Once there ARE meetings the CTA is a toolbar button; the
                    big empty-state version below is for the first one. */}
                {meetings.length > 0 && !meetingStatus.recording && (
                  <button
                    className="primary"
                    disabled={meetingBusy || !!disabledReason(meetingStatus)}
                    title={disabledReason(meetingStatus) || undefined}
                    onClick={toggleMeetingRecording}
                  >
                    Record a meeting
                  </button>
                )}
              </div>

              {meetings.length === 0 ? (
                <div className="empty">
                  <h3>
                    {meetingQuery ? "No meetings match that" : "No meetings yet"}
                  </h3>
                  <p>
                    {meetingQuery
                      ? "Search covers meeting titles and every word transcribed inside them."
                      : "Recorded meetings land here — searchable, exportable, and deletable in one click. Audio is kept for 7 days; the transcript is kept for good."}
                  </p>
                  {/* YV95 / finding #6 — the empty state IS the entry point.
                      Everything above this item shipped a feature with no way
                      to reach it; this is the one big button that fixes that. */}
                  {!meetingQuery && (
                    <>
                      <button
                        className="primary big"
                        disabled={meetingBusy || !!disabledReason(meetingStatus)}
                        title={disabledReason(meetingStatus) || undefined}
                        onClick={toggleMeetingRecording}
                      >
                        {recordLabel(meetingStatus)}
                      </button>
                      <p className="tiny">
                        {disabledReason(meetingStatus) ??
                          `Or press ⌃⌘M from anywhere, or pick “Record a meeting” in the menu bar. Everything stays on this Mac.`}
                      </p>
                    </>
                  )}
                </div>
              ) : (
                <ul className="feed">
                  {meetings.map((m) => (
                    <li key={m.id} className="card">
                      <div className="card-meta">
                        <span>
                          {formatTime(m.startedAt)} ·{" "}
                          {formatMeetingDuration(m.durationSeconds)}
                        </span>
                        <span>
                          {m.segmentCount}{" "}
                          {m.segmentCount === 1 ? "segment" : "segments"}
                          {m.audioKept ? "" : " · audio expired"}
                        </span>
                      </div>
                      <p className="meeting-title">
                        {m.title}
                        <span className={`meeting-state ${m.state}`}>
                          {m.state}
                        </span>
                      </p>
                      {m.error && <p className="tiny bad">{m.error}</p>}
                      <div className="actions">
                        <button
                          className="primary"
                          onClick={() => openMeetingDetail(m.id)}
                        >
                          Open transcript
                        </button>
                        <button
                          className="ghost"
                          disabled={meetingBusy}
                          onClick={() => exportMeeting(m.id)}
                        >
                          Export Markdown
                        </button>
                        <button
                          className="ghost danger"
                          disabled={meetingBusy}
                          onClick={() =>
                            confirmDeleteMeeting === m.id
                              ? removeMeeting(m.id)
                              : setConfirmDeleteMeeting(m.id)
                          }
                        >
                          {confirmDeleteMeeting === m.id
                            ? "Delete for good?"
                            : "Delete"}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}

          {nav === "meetings" && openMeeting && (
            <div className="meeting-detail">
              <div className="actions">
                <button className="ghost" onClick={() => setOpenMeeting(null)}>
                  ← All meetings
                </button>
                <button
                  className="primary"
                  disabled={meetingBusy}
                  onClick={() => exportMeeting(openMeeting.meeting.id)}
                >
                  Export Markdown
                </button>
                <button
                  className="ghost danger"
                  disabled={meetingBusy}
                  onClick={() =>
                    confirmDeleteMeeting === openMeeting.meeting.id
                      ? removeMeeting(openMeeting.meeting.id)
                      : setConfirmDeleteMeeting(openMeeting.meeting.id)
                  }
                >
                  {confirmDeleteMeeting === openMeeting.meeting.id
                    ? "Delete for good?"
                    : "Delete meeting"}
                </button>
              </div>

              {/* The title is editable in place — ASR names a meeting
                  "Meeting", a person names it after what it was. */}
              <input
                className="meeting-title-edit"
                defaultValue={openMeeting.meeting.title}
                aria-label="Meeting title"
                key={openMeeting.meeting.id}
                onBlur={(e) =>
                  renameMeeting(openMeeting.meeting.id, e.target.value)
                }
              />
              <p className="card-meta">
                <span>
                  {formatTime(openMeeting.meeting.startedAt)} ·{" "}
                  {formatMeetingDuration(openMeeting.meeting.durationSeconds)} ·{" "}
                  {openMeeting.meeting.state}
                </span>
                <span>
                  {openMeeting.audioOnDisk
                    ? "audio kept"
                    : `audio deleted after ${
                        insights?.meetings?.audioRetentionDays ?? 7
                      } days`}
                </span>
              </p>

              {openMeeting.meeting.summary && (
                <div className="panel">
                  <h3>Summary</h3>
                  <p>{openMeeting.meeting.summary}</p>
                </div>
              )}

              {openMeeting.segments.length === 0 ? (
                <div className="empty">
                  <h3>No transcript yet</h3>
                  <p>
                    {openMeeting.meeting.state === "recording" ||
                    openMeeting.meeting.state === "transcribing"
                      ? "Yap is still working through the audio."
                      : "This meeting has no transcribed segments."}
                  </p>
                </div>
              ) : (
                <TranscriptList segments={openMeeting.segments} />
              )}
            </div>
          )}

          {nav === "insights" && insights && (
            <div className="insights">
              <div className="stats-row">
                <div className="stat big">
                  <div className="stat-n">
                    {insights.totalWords.toLocaleString()}
                  </div>
                  <div className="stat-l">total words</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">
                    {insights.totalSessions.toLocaleString()}
                  </div>
                  <div className="stat-l">sessions</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">{Math.round(insights.avgWpm)}</div>
                  <div className="stat-l">wpm</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">{insights.streakDays}</div>
                  <div className="stat-l">
                    streak (best {insights.longestStreak})
                  </div>
                </div>
              </div>
              {/* YV94 / finding #29 — the Meetings strip. Yap ships no
                  telemetry, so without a local rollup there is no way to tell
                  whether the Notetaker is used, retained or trusted. It renders
                  only once a meeting exists: an all-zero row on a screen about
                  dictation is noise. */}
              {insights.meetings && insights.meetings.totalMeetings > 0 && (
                <div className="panel meeting-strip">
                  <h3>Meetings</h3>
                  <div className="stats-row">
                    <div className="stat">
                      <div className="stat-n">
                        {insights.meetings.totalMeetings.toLocaleString()}
                      </div>
                      <div className="stat-l">recorded</div>
                    </div>
                    <div className="stat">
                      <div className="stat-n">
                        {insights.meetings.meetingsLast7}
                      </div>
                      <div className="stat-l">last 7 days</div>
                    </div>
                    <div className="stat">
                      <div className="stat-n">
                        {Math.round(insights.meetings.totalSeconds / 60)}
                      </div>
                      <div className="stat-l">minutes captured</div>
                    </div>
                    <div className="stat">
                      <div className="stat-n">
                        {insights.meetings.segmentsIndexed.toLocaleString()}
                      </div>
                      <div className="stat-l">segments searchable</div>
                    </div>
                  </div>
                  <ul className="kv">
                    <li>
                      <span>Finished cleanly</span>
                      <strong>
                        {insights.meetings.completeMeetings} of{" "}
                        {insights.meetings.totalMeetings}
                        {insights.meetings.partialMeetings > 0 &&
                          ` · ${insights.meetings.partialMeetings} partial`}
                        {insights.meetings.failedMeetings > 0 &&
                          ` · ${insights.meetings.failedMeetings} failed`}
                      </strong>
                    </li>
                    <li>
                      <span>Audio still on disk</span>
                      <strong>
                        {insights.meetings.meetingsWithAudio} ·{" "}
                        {insights.meetings.audioRetentionDays}-day retention
                      </strong>
                    </li>
                    {insights.meetings.daysToFirstMeeting != null && (
                      <li>
                        <span>Dictation → first meeting</span>
                        <strong>
                          {insights.meetings.daysToFirstMeeting} days
                        </strong>
                      </li>
                    )}
                  </ul>
                </div>
              )}

              <div className="panel-grid">
                <div className="panel">
                  <h3>Last 7 days</h3>
                  <div className="bars">
                    {insights.wordsLast7.map((d) => (
                      <div key={d.date} className="bar-row">
                        <span className="bar-label">{formatDay(d.date)}</span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{
                              width: `${(d.words / maxWeek) * 100}%`,
                            }}
                          />
                        </div>
                        <span className="bar-n">
                          {d.words.toLocaleString()}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
                <div className="panel">
                  <h3>Engine & latency</h3>
                  <ul className="kv">
                    <li>
                      <span>Avg ASR (model)</span>
                      <strong>{insights.avgAsrSeconds.toFixed(2)}s</strong>
                    </li>
                    <li>
                      <span>p50 hold→clipboard</span>
                      <strong>
                        {insights.p50PipelineMs
                          ? `${insights.p50PipelineMs}ms`
                          : "—"}
                      </strong>
                    </li>
                    <li>
                      <span>p95 hold→clipboard</span>
                      <strong>
                        {insights.p95PipelineMs
                          ? `${insights.p95PipelineMs}ms`
                          : "—"}
                      </strong>
                    </li>
                    <li>
                      <span>Speech sample</span>
                      <strong>
                        {(insights.speechSecondsTotal ?? 0).toFixed(0)}s ·{" "}
                        {insights.wpmSampleSessions ?? 0} utt
                      </strong>
                    </li>
                    <li>
                      <span>Hotkey</span>
                      <strong
                        className={status.hotkeyRegistered ? "ok" : "bad"}
                      >
                        {status.hotkeyRegistered
                          ? settings?.hotkeyLabel || "fn ok"
                          : "not registered"}
                      </strong>
                    </li>
                    <li>
                      <span>Accessibility</span>
                      <strong className={status.accessibility ? "ok" : "bad"}>
                        {status.accessibility ? "trusted" : "denied"}
                      </strong>
                    </li>
                  </ul>
                  <p className="muted" style={{ marginTop: 12, fontSize: 13 }}>
                    WPM uses audio duration only — never model latency. Base ASR
                    is OpenAI Whisper weights via MLX (not three proprietary
                    models). Target: p50 hold→clipboard &lt; 800ms on Fast.
                  </p>
                </div>
              </div>

              <div className="panel" style={{ marginTop: 16 }}>
                <h3>Daily words · last 30 days</h3>
                {daily30.some((d) => d.words > 0) ? (
                  <svg
                    className="chart-svg"
                    viewBox="0 0 640 160"
                    preserveAspectRatio="none"
                    role="img"
                    aria-label="Words dictated per day over the last 30 days"
                  >
                    {daily30.map((d, i) => {
                      const step = 640 / Math.max(1, daily30.length);
                      const bw = step * 0.66;
                      const h = (d.words / maxDaily30) * 148;
                      return (
                        <rect
                          key={d.date}
                          x={i * step + (step - bw) / 2}
                          y={156 - h}
                          width={bw}
                          height={Math.max(d.words > 0 ? 2 : 0, h)}
                          fill="var(--accent)"
                        >
                          <title>
                            {formatDay(d.date)} — {d.words.toLocaleString()} words
                          </title>
                        </rect>
                      );
                    })}
                  </svg>
                ) : (
                  <p className="muted chart-empty">
                    No dictation yet. Hold your hotkey and start talking — your
                    daily words will chart here.
                  </p>
                )}
              </div>

              <div className="panel" style={{ marginTop: 16 }}>
                <h3>Activity · last 365 days</h3>
                {hasActivity ? (
                  <div className="heatmap-wrap">
                    <p className="heat-summary">
                      <strong>{heatSummary.words.toLocaleString()}</strong> words
                      · <strong>{heatSummary.sessions.toLocaleString()}</strong>{" "}
                      sessions
                      {heatSummary.best && (
                        <>
                          {" "}
                          · best day{" "}
                          <strong>{formatDayShort(heatSummary.best.date)}</strong>
                        </>
                      )}
                    </p>
                    <svg
                      className="heatmap"
                      viewBox={`0 0 ${heat.cols * 13} ${7 * 13 + 14}`}
                      preserveAspectRatio="xMinYMid meet"
                      role="img"
                      aria-label="Daily dictation activity heatmap for the last year"
                    >
                      {heat.months.map((m) => (
                        <text
                          key={m.key}
                          className="heat-month"
                          x={m.col * 13}
                          y={10}
                        >
                          {m.label}
                        </text>
                      ))}
                      {heat.cells.map(({ d, col, row }) => {
                        const lvl = heatLevel(d.words, heat.max);
                        return (
                          <rect
                            key={d.date}
                            x={col * 13}
                            y={row * 13 + 14}
                            width={10}
                            height={10}
                            rx={2}
                            fill={HEAT_FILL[lvl]}
                          >
                            <title>
                              {formatDay(d.date)} — {d.words.toLocaleString()} words
                            </title>
                          </rect>
                        );
                      })}
                    </svg>
                    <div className="heat-legend">
                      <span>Less</span>
                      {[0, 1, 2, 3, 4].map((lvl) => (
                        <span
                          key={lvl}
                          className="heat-swatch"
                          style={{ background: HEAT_FILL[lvl] }}
                        />
                      ))}
                      <span>More</span>
                    </div>
                  </div>
                ) : (
                  <p className="muted chart-empty">
                    A year of dictation lights up here — one square per day, brighter
                    the more you say.
                  </p>
                )}
              </div>

              {monthlySeries.some((d) => d.words > 0) && (
                <div className="panel" style={{ marginTop: 16 }}>
                  <h3>Words by month · last 12 months</h3>
                  {monthlyView.quiet > 0 && (
                    <p className="quiet-months">
                      {monthlyView.quiet} quiet{" "}
                      {monthlyView.quiet === 1 ? "month" : "months"} before{" "}
                      {monthName(monthlyView.rows[0].date, "long")}
                    </p>
                  )}
                  <div className="bars">
                    {monthlyView.rows.map((d) => (
                      <div key={d.date} className="bar-row">
                        <span className="bar-label">{formatMonth(d.date)}</span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{
                              width: `${(d.words / monthlyView.max) * 100}%`,
                            }}
                          />
                        </div>
                        <span className="bar-n">{d.words.toLocaleString()}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {insights.topApps.length > 0 && (
                <div className="panel" style={{ marginTop: 16 }}>
                  <h3>Top apps</h3>
                  <div className="bars apps">
                    {insights.topApps.map((a) => (
                      <div key={a.app} className="bar-row">
                        <span className="bar-label" title={a.app}>
                          {a.app}
                        </span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{ width: `${(a.words / maxApp) * 100}%` }}
                          />
                        </div>
                        <span className="bar-n">
                          {a.words.toLocaleString()} w · {a.sessions}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}

          {nav === "dictionary" && (
            <div className="dict">
              <div className="panel intro">
                <h3>Spell the way you do</h3>
                <p>
                  After ASR, matching tokens are rewritten to your preferred
                  form. Starred terms are always fed to the recognizer before it
                  decodes, so it expects them.
                </p>
                <div className="dict-add">
                  <input
                    placeholder="Term (e.g. Drivia)"
                    value={newTerm}
                    onChange={(e) => setNewTerm(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTerm()}
                  />
                  <input
                    placeholder="Preferred (optional)"
                    value={newPreferred}
                    onChange={(e) => setNewPreferred(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTerm()}
                  />
                  <button className="primary" onClick={addTerm}>
                    Add
                  </button>
                </div>
              </div>

              {candidates.length > 0 && (
                <div className="panel dict-suggest">
                  <h3>
                    Yap noticed:{" "}
                    {candidates
                      .slice(0, 3)
                      .map((c) => c.term)
                      .join(", ")}
                  </h3>
                  <p className="tiny">
                    Words you fixed by hand. Add them and Yap stops getting them
                    wrong.
                  </p>
                  <ul className="cand-list">
                    {candidates.map((c) => (
                      <li key={c.id}>
                        <div>
                          <strong>{c.term}</strong>
                          {c.wrong && (
                            <span className="muted"> ← heard “{c.wrong}”</span>
                          )}
                          <div className="tiny">
                            fixed {c.useCount}×
                          </div>
                        </div>
                        <div className="cand-actions">
                          <button
                            className="primary"
                            onClick={() => acceptCandidate(c)}
                          >
                            Add
                          </button>
                          <button
                            className="ghost"
                            onClick={() => dismissCandidate(c)}
                          >
                            Dismiss
                          </button>
                        </div>
                      </li>
                    ))}
                  </ul>
                </div>
              )}

              <ul className="dict-list">
                {dictionary.map((d) => (
                  <li key={d.id}>
                    {editingTermId === d.id ? (
                      <div className="dict-edit">
                        <input
                          value={editTerm}
                          onChange={(e) => setEditTerm(e.target.value)}
                          onKeyDown={(e) =>
                            e.key === "Enter" && saveEditTerm()
                          }
                          aria-label="Term"
                          autoFocus
                        />
                        <input
                          value={editPreferred}
                          placeholder="Preferred (optional)"
                          onChange={(e) => setEditPreferred(e.target.value)}
                          onKeyDown={(e) =>
                            e.key === "Enter" && saveEditTerm()
                          }
                          aria-label="Preferred form"
                        />
                        <button className="primary" onClick={saveEditTerm}>
                          Save
                        </button>
                        <button
                          className="ghost"
                          onClick={() => setEditingTermId(null)}
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <>
                        <div>
                          <strong>{d.term}</strong>
                          {d.preferred && (
                            <span className="muted"> → {d.preferred}</span>
                          )}
                          <div className="tiny">
                            {d.hits} hits
                            {d.starred ? " · always biased" : ""}
                          </div>
                        </div>
                        <div className="dict-actions">
                          <button
                            className={d.starred ? "star on" : "star"}
                            onClick={() => toggleStar(d)}
                            aria-pressed={!!d.starred}
                            title={
                              d.starred
                                ? "Starred — always biases the recognizer"
                                : "Star to always bias the recognizer"
                            }
                          >
                            {d.starred ? "★" : "☆"}
                          </button>
                          <button
                            className="ghost"
                            onClick={() => startEditTerm(d)}
                          >
                            Edit
                          </button>
                          <button
                            className="ghost danger"
                            onClick={() => removeTerm(d.id)}
                          >
                            Remove
                          </button>
                        </div>
                      </>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {nav === "scratchpad" && (
            <div className="scratch">
              <div className="scratch-list">
                <button className="primary" onClick={newNote}>
                  New note
                </button>
                {scratch.map((n) => (
                  <button
                    key={n.id}
                    className={
                      activeNoteId === n.id ? "note-item active" : "note-item"
                    }
                    onClick={() => openNote(n)}
                  >
                    <strong>{n.title}</strong>
                    <span className="tiny">{formatTime(n.updatedAt)}</span>
                  </button>
                ))}
              </div>
              <div className="scratch-editor">
                <input
                  className="note-title"
                  value={noteTitle}
                  onChange={(e) => setNoteTitle(e.target.value)}
                />
                <textarea
                  value={noteBody}
                  onChange={(e) => setNoteBody(e.target.value)}
                  placeholder="Park text, draft prompts…"
                />
                <div className="actions">
                  <button className="primary" onClick={saveNote}>
                    Save
                  </button>
                  <button
                    onClick={() => copyText(noteBody)}
                    disabled={!noteBody}
                  >
                    Copy
                  </button>
                  <button
                    onClick={() => pasteText(noteBody)}
                    disabled={!noteBody}
                  >
                    Paste
                  </button>
                  {activeNoteId && (
                    <button
                      className="ghost danger"
                      onClick={() => deleteNote(activeNoteId)}
                    >
                      Delete
                    </button>
                  )}
                </div>
              </div>
            </div>
          )}

          {nav === "settings" && settings && (
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
              {settingsTab === "companion" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Companion
                    <span className="sub">
                      How Yap looks and feels while you talk.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Pill style</h3>
                    <p>
                      Pick your companion — the little helper that appears when
                      you dictate. It switches live, so try both.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["classic", "Classic", "A sleek waveform capsule"],
                          ["yappy", "Yappy 🐥", "A pixel pet in a little world"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pillStyle ?? "classic") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, pillStyle: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="panel">
                    <h3>Screen position</h3>
                    <p>
                      Where your companion sits. Bottom is the classic island;
                      the side docks pin it to that edge, halfway down the
                      screen, out of the way of what you’re typing into.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["bottom", "Bottom", "Centred along the screen bottom"],
                          ["left", "Left edge", "Docked left, halfway down"],
                          ["right", "Right edge", "Docked right, halfway down"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pillPosition ?? "bottom") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, pillPosition: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="panel">
                    <h3>Companion tone</h3>
                    <p>
                      How Yappy talks back when you finish — the reactive line
                      keyed to how much you said. Warm, a little rude, or sweet.
                      Independent of which companion you picked.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["friendly", "Friendly", "Warm and encouraging"],
                          ["rude", "Rude 😏", "Sassy — teases when you ramble"],
                          ["rose", "Rose 🌹", "Sweet and adoring"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.companionTone ?? "friendly") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, companionTone: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.showFloatingPill ?? true}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          showFloatingPill: e.target.checked,
                        })
                      }
                    />
                    <span>
                      Keep the companion on screen at all times (otherwise it
                      only appears while you talk)
                    </span>
                  </label>
                </section>
              )}

              {/* ── Dictation — how your speech becomes clean text ── */}
              {settingsTab === "dictation" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Dictation
                    <span className="sub">
                      How your speech is cleaned up and formatted.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Dictation mode</h3>
                    <p>
                      Auto shapes your text to fit whatever app you’re typing
                      into. Pick a fixed mode to always format the same way. Your
                      words are never dropped.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["auto", "Auto", "Match the app I’m typing into"],
                          ["plain", "Plain", "Exactly what I said, no changes"],
                          ["list", "List", "Turn spoken lists into bullets"],
                          ["email", "Email", "Tidy formatting for messages"],
                          ["code", "Code", "Leave code and names untouched"],
                          ["notes", "Notes", "Formatting for quick notes"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.dictationMode ?? "auto") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, dictationMode: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                  <div className="panel">
                    <h3>Auto-cleanup</h3>
                    <p>
                      How much Yap tidies each transcript. None pastes your exact
                      words; higher levels drop “um”s, fix things you re-said,
                      and format the result. Your words are never dropped — Yap
                      keeps the raw take too, so you can undo the edit with
                      ⌃⌘Z, the menu-bar “Undo AI Edit” item, or “Paste raw” in
                      History.
                    </p>
                    <div className="profile-row">
                      {/* YV51 — blurbs describe what each level ACTUALLY runs
                          today. The local-LLM polish stage is still a no-op
                          stub (dictation::polish_llm), so High currently
                          behaves exactly like Medium and must say so rather
                          than advertise an "AI polish" that never runs. */}
                      {(
                        [
                          ["none", "None", "Exactly as spoken, word for word"],
                          [
                            "light",
                            "Light",
                            "Your dictionary words, minus “um”s and things you re-said",
                          ],
                          [
                            "medium",
                            "Medium",
                            "Light, plus spoken lists become real lists",
                          ],
                          [
                            "high",
                            "High",
                            "Same as Medium for now — the AI polish pass isn’t wired up yet",
                          ],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.cleanupLevel ?? "light") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, cleanupLevel: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                  {/* YV62 (R14) — the tone dial, per surface. It reaches the
                      rules, not just the local model: Formal keeps the full
                      stop, Very Casual drops it everywhere. */}
                  <div className="panel">
                    <h3>Tone</h3>
                    <p>
                      How formal each surface sounds. The dial only changes
                      capitalisation and punctuation — never your words. Formal
                      always keeps the full stop; Very casual drops it
                      everywhere.
                    </p>
                    <div className="tone-dial">
                      {(
                        [
                          ["email", "Email"],
                          ["chat", "Chat and messaging"],
                          ["notes", "Notes"],
                          ["document", "Documents"],
                          ["list", "Lists"],
                          ["plain", "Plain"],
                        ] as const
                      ).map(([mode, label]) => (
                        <label className="field" key={mode}>
                          <span>{label}</span>
                          <select
                            value={settings.polishStyles?.[mode] ?? "default"}
                            onChange={(e) =>
                              saveSettings({
                                ...settings,
                                polishStyles: {
                                  ...(settings.polishStyles ?? {}),
                                  [mode]: e.target.value,
                                },
                              })
                            }
                          >
                            <option value="very casual">Very casual</option>
                            <option value="casual">Casual</option>
                            <option value="default">Default</option>
                            <option value="formal">Formal</option>
                          </select>
                        </label>
                      ))}
                    </div>
                  </div>
                  {/* YV62 (R13) — the sign-off block. Opt-in, and pasted byte
                      for byte after every other stage so nothing rewrites it. */}
                  <div className="panel">
                    <h3>Signature</h3>
                    <p>
                      Your sign-off block, pasted exactly as you type it here.
                      It is added last, after everything else, so nothing
                      rewrites it — and Yap never writes one for you.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["off", "Off", "Never add a signature"],
                          ["cue", "On cue", "Only when I say “sign it”"],
                          [
                            "auto",
                            "Automatic",
                            "On “sign it”, and on emails I end with “thanks”",
                          ],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.signatureMode ?? "off") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, signatureMode: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    <label className="field">
                      <span>Signature</span>
                      <textarea
                        rows={2}
                        placeholder="Wilson — drivia.consulting"
                        aria-label="Signature block"
                        value={signatureDraft ?? settings.signature ?? ""}
                        onChange={(e) => setSignatureDraft(e.target.value)}
                        onBlur={() => {
                          if (
                            signatureDraft !== null &&
                            signatureDraft !== (settings.signature ?? "")
                          ) {
                            saveSettings({
                              ...settings,
                              signature: signatureDraft,
                            });
                          }
                          setSignatureDraft(null);
                        }}
                      />
                    </label>
                  </div>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.autoPaste}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          autoPaste: e.target.checked,
                        })
                      }
                    />
                    <span>
                      Paste my words automatically when I finish talking (needs
                      Accessibility permission)
                    </span>
                  </label>
                  <label className="field">
                    <span>Language I speak</span>
                    <select
                      value={settings.language}
                      onChange={(e) =>
                        saveSettings({ ...settings, language: e.target.value })
                      }
                    >
                      <option value="en">English</option>
                      <option value="es">Spanish</option>
                      <option value="fr">French</option>
                      <option value="ht">Haitian Creole</option>
                    </select>
                  </label>
                </section>
              )}

              {/* ── Snippets (YV48) — spoken triggers expand to saved text ── */}
              {settingsTab === "snippets" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Snippets
                    <span className="sub">
                      Say a short phrase, paste the whole thing.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>New snippet</h3>
                    <p>
                      Say the trigger while dictating and Yap pastes the
                      expansion instead — your email, your address, a sign-off.
                      Matching ignores capitalisation and only fires on whole
                      words.
                    </p>
                    <div className="snip-add">
                      <input
                        placeholder="Trigger phrase (e.g. my email)"
                        value={newTrigger}
                        onChange={(e) => setNewTrigger(e.target.value)}
                        aria-label="Trigger phrase"
                      />
                      <textarea
                        placeholder="Expands to… (multiple lines are fine)"
                        value={newExpansion}
                        onChange={(e) => setNewExpansion(e.target.value)}
                        rows={3}
                        aria-label="Expansion text"
                      />
                      <button
                        className="primary"
                        onClick={addSnippet}
                        disabled={!newTrigger.trim() || !newExpansion.trim()}
                      >
                        Add snippet
                      </button>
                    </div>
                  </div>
                  <div className="panel">
                    <h3>Where triggers fire</h3>
                    <p>
                      Inline replaces the phrase wherever you say it. Whole
                      utterance is stricter — the trigger only expands when it is
                      the entire thing you said.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          [
                            "inline",
                            "Inline",
                            "Anywhere in what I said",
                          ],
                          [
                            "utterance",
                            "Whole utterance",
                            "Only when I say just the trigger",
                          ],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.snippetScope ?? "inline") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, snippetScope: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                  {snippets.length === 0 ? (
                    <div className="panel">
                      <p className="tiny">
                        No snippets yet. Add one above and it takes effect on
                        your next dictation.
                      </p>
                    </div>
                  ) : (
                    <ul className="snip-list">
                      {snippets.map((s) => (
                        <li key={s.id}>
                          {editingSnippetId === s.id ? (
                            <div className="snip-edit">
                              <input
                                value={editTrigger}
                                onChange={(e) => setEditTrigger(e.target.value)}
                                aria-label="Trigger phrase"
                                autoFocus
                              />
                              <textarea
                                value={editExpansion}
                                onChange={(e) =>
                                  setEditExpansion(e.target.value)
                                }
                                rows={3}
                                aria-label="Expansion text"
                              />
                              <div className="snip-actions">
                                <button
                                  className="primary"
                                  onClick={saveEditSnippet}
                                >
                                  Save
                                </button>
                                <button
                                  className="ghost"
                                  onClick={() => setEditingSnippetId(null)}
                                >
                                  Cancel
                                </button>
                              </div>
                            </div>
                          ) : (
                            <>
                              <div className="snip-body">
                                <strong>{s.trigger}</strong>
                                <pre className="snip-expansion">
                                  {s.expansion}
                                </pre>
                                {!s.enabled && (
                                  <div className="tiny">
                                    Off — this one never expands
                                  </div>
                                )}
                              </div>
                              <div className="snip-actions">
                                <label className="toggle inline">
                                  <input
                                    type="checkbox"
                                    checked={s.enabled}
                                    onChange={() => toggleSnippet(s)}
                                    aria-label={`Enable ${s.trigger}`}
                                  />
                                  <span>On</span>
                                </label>
                                <button
                                  className="ghost"
                                  onClick={() => startEditSnippet(s)}
                                >
                                  Edit
                                </button>
                                <button
                                  className="ghost danger"
                                  onClick={() => removeSnippet(s.id)}
                                >
                                  Remove
                                </button>
                              </div>
                            </>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                </section>
              )}

              {/* ── Audio — capture cleanup + what plays while you talk ── */}
              {settingsTab === "audio" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Audio
                    <span className="sub">
                      Cleaning up your mic and quieting the room while you talk.
                    </span>
                  </h2>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.denoise ?? true}
                      onChange={(e) =>
                        saveSettings({ ...settings, denoise: e.target.checked })
                      }
                    />
                    <span>
                      <strong>Denoise</strong> — remove steady background noise
                      like fans, hum, and keyboard clatter before transcribing
                    </span>
                  </label>
                  <label className="toggle">
                    <input
                      type="checkbox"
                      checked={settings.muteWhileDictating ?? true}
                      onChange={(e) =>
                        saveSettings({
                          ...settings,
                          muteWhileDictating: e.target.checked,
                        })
                      }
                    />
                    <span>
                      <strong>Mute the Mac while dictating</strong> — silence
                      system audio so nothing plays over you, and restore your
                      exact volume when you stop
                    </span>
                  </label>
                </section>
              )}

              {/* ── Shortcut — the key you hold to talk ── */}
              {settingsTab === "shortcut" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Shortcut
                    <span className="sub">
                      The key you hold to start talking.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Hold-to-talk key</h3>
                    <p>
                      Default is <strong>fn + Control</strong>. Hold it to talk,
                      double-tap to keep it running hands-free, then tap again to
                      stop. Needs Accessibility permission. Tip: set Keyboard →
                      “Press 🌐 key to” → <strong>Do Nothing</strong> so the
                      Globe key doesn’t open emoji.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["fn_control", "fn⌃", "Hold + double-tap hands-free"],
                          ["fn", "fn", "Bare Globe only"],
                          ["both", "fn / fn⌃", "Either combo"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.pttBinding ?? "fn_control") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() => applyBinding(id)}
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                    {/* YV15 — record the next combo instead of picking a preset. */}
                    <div
                      style={{
                        display: "flex",
                        gap: 8,
                        alignItems: "center",
                        flexWrap: "wrap",
                        marginTop: 12,
                      }}
                    >
                      <button
                        type="button"
                        className={capturing ? "primary" : "ghost"}
                        aria-pressed={capturing}
                        onClick={() => {
                          setCaptureHint(
                            capturing
                              ? null
                              : "Listening… hold your shortcut now (Esc to cancel).",
                          );
                          setCapturing((c) => !c);
                        }}
                      >
                        {capturing
                          ? "Listening… press your keys"
                          : "Set shortcut"}
                      </button>
                      <span className="muted">
                        Currently <strong>{settings.hotkeyLabel}</strong>
                      </span>
                    </div>
                    {captureHint && (
                      <p className="muted" style={{ marginTop: 8 }}>
                        {captureHint}
                      </p>
                    )}
                    <label className="toggle" style={{ marginTop: 12 }}>
                      <input
                        type="checkbox"
                        checked={settings.keepCmdShiftV ?? false}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            keepCmdShiftV: e.target.checked,
                          })
                        }
                      />
                      <span>Also let me hold ⌘⇧V as a backup</span>
                    </label>
                    <p style={{ marginTop: 12 }}>
                      Currently set to <strong>{settings.hotkeyLabel}</strong>.
                      The companion stays bottom-center and follows you across
                      every desktop. Yap also learns the words you use most over
                      time.
                    </p>
                  </div>
                  {/* YV49 — command mode: the same hold, plus one modifier,
                      edits the text you already selected instead of typing. */}
                  <div className="panel">
                    <h3>Command mode</h3>
                    <p>
                      Select some text, then hold{" "}
                      <strong>
                        {settings.hotkeyLabel}
                        {(settings.commandBinding ?? "command") === "option"
                          ? "⌥"
                          : "⌘"}
                      </strong>{" "}
                      and say what to do with it — “make it a list”, “make it
                      uppercase”, “wrap in quotes”, “replace Monday with
                      Tuesday”, “delete that”. Yap only runs commands it knows
                      exactly; anything else is ignored so your text is never
                      guessed at.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["command", "⌘", "Hold your key plus Command"],
                          ["option", "⌥", "Hold your key plus Option"],
                          ["off", "Off", "Every hold just dictates"],
                        ] as const
                      ).map(([id, label, blurb]) => (
                        <button
                          key={id}
                          type="button"
                          className={
                            (settings.commandBinding ?? "command") === id
                              ? "profile active"
                              : "profile"
                          }
                          onClick={() =>
                            saveSettings({ ...settings, commandBinding: id })
                          }
                        >
                          <strong>{label}</strong>
                          <span>{blurb}</span>
                        </button>
                      ))}
                    </div>
                  </div>
                </section>
              )}

              {/* ── Advanced — the speech model ──
                  YV34 retired the speed-profile buttons and the "Voice model
                  (advanced override)" list: both picked a repo id for the
                  deleted Python sidecar. Speed vs. accuracy is now a property
                  of which embedded model you download — and since YV54 stopped
                  asking first-run users that question, this panel IS the picker
                  for the people who want to answer it. */}
              {settingsTab === "advanced" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Advanced
                    <span className="sub">
                      Your speech model and how Yap starts up. Leave this as-is
                      unless dictation feels slow or misses words.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Speech model</h3>
                    <p className="muted">
                      Yap transcribes inside the app itself — no helper process,
                      nothing installed on the side, nothing off this Mac. Your
                      model loads on your first dictation and stays warm after
                      that, so an idle Yap costs almost nothing. Smaller models
                      are faster and lighter; larger ones are more accurate on
                      long or technical dictation. Yap picks the recommended one
                      for you — swap it here if you'd rather choose.
                    </p>
                    <ModelPicker setup={modelSetup} />
                    <p className="muted tiny" style={{ marginTop: 8 }}>
                      {perms?.asrDetail}
                    </p>
                    {/* YV80 — Model & Speed: the memory-vs-first-take trade,
                        made explicit. Off by default; an idle Yap holds ~930 MB
                        less and the first take loads the engine while you talk. */}
                    <label className="toggle" style={{ marginTop: 12 }}>
                      <input
                        type="checkbox"
                        checked={settings.preloadModel ?? false}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            preloadModel: e.target.checked,
                          })
                        }
                      />
                      <span>
                        <strong>Keep the model loaded from launch</strong> —
                        Yap normally loads your speech model on your first
                        dictation, which keeps about 900 MB free while you are
                        not dictating and costs a few seconds on that first
                        take. Turn this on to load it at launch instead: every
                        take starts instantly, and Yap holds the memory the
                        whole time it is running
                      </span>
                    </label>
                  </div>
                  {/* YV42 — launch at login. Off by default; nothing installs a
                      login item behind your back. */}
                  <div className="panel">
                    <h3>Startup</h3>
                    <label className="toggle">
                      <input
                        type="checkbox"
                        checked={settings.autostart ?? false}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            autostart: e.target.checked,
                          })
                        }
                      />
                      <span>
                        <strong>Launch Yap at login</strong> — Yap has to be
                        running to catch your hold-to-talk key, so start it
                        automatically when you sign in
                      </span>
                    </label>
                  </div>
                  {/* YV44 — updates are opt-out and consent-based: Yap asks
                      before it ever downloads or installs anything. */}
                  <div className="panel">
                    <h3>Updates</h3>
                    <label className="toggle">
                      <input
                        type="checkbox"
                        checked={settings.checkUpdates ?? true}
                        onChange={(e) =>
                          saveSettings({
                            ...settings,
                            checkUpdates: e.target.checked,
                          })
                        }
                      />
                      <span>
                        <strong>Check for updates</strong> — look for a newer
                        Yap at launch and tell you about it. Nothing downloads
                        or installs until you say so; turn this off and Yap
                        never contacts the release page at all
                      </span>
                    </label>
                    {(settings.checkUpdates ?? true) && (
                      <div className="actions" style={{ marginTop: 10 }}>
                        <button onClick={checkForUpdateNow}>
                          Check for updates now
                        </button>
                      </div>
                    )}
                  </div>
                </section>
              )}

              {/* ── Privacy & Diagnostics — your data lives on your Mac ── */}
              {settingsTab === "privacy" && (
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
              )}

              {/* ── YP3 · License — trial, purchase, activation ── */}
              {settingsTab === "license" && (
                <LicensePanel
                  status={license}
                  onBuy={buyYap}
                  onActivate={activateLicense}
                  onDeactivate={deactivateLicense}
                />
              )}
            </div>
          )}
        </div>
      </section>

      {/* ── YP3 · the warm purchase sheet ──
          Raised by the gate's `license_required` event (hotkey, pill, tray) and
          by a rejected `manual_toggle`. Dismissible, and everything behind it
          keeps working — that is the promise the copy makes and the code has to
          keep. It sits OUTSIDE `.main` so it covers the sidebar too. */}
      {buyPrompt && (
        <PurchasePrompt
          onBuy={buyYap}
          onEnterKey={openLicenseTab}
          onDismiss={() => setBuyPrompt(false)}
        />
      )}

      {/* ── YV96 · the one-time meeting-capture notice ──
          Raised by the backend's `meeting` event the first time a recording
          actually starts, whichever entry point started it, and re-openable
          from Settings → Privacy. It sits OUTSIDE `.main` for the same reason
          the purchase sheet does, and — the whole point of the closed O1
          decision — the recording behind it is already running. */}
      {consentOpen && (
        <MeetingConsentNotice
          recording={consentOpen === "recording"}
          onClose={closeConsentNotice}
        />
      )}

      {/* ── YV98 · the crash report, shown before it exists ──
          Nothing has been written to disk at this point: the bundle is built in
          memory, and this sheet is the user reading it. The zip only lands on
          the Desktop when they press the action. */}
      {supportOpen && (
        <SupportBundleSheet
          preview={supportPreview}
          busy={supportBusy}
          onSend={sendSupportBundle}
          onClose={() => {
            if (supportBusy) return;
            setSupportOpen(false);
            setSupportPreview(null);
          }}
        />
      )}
    </div>
  );
}
