import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Onboarding, { type Step as OnboardStep } from "./Onboarding";
import YappyHouse from "./home/YappyHouse";
import "./App.css";

type Nav =
  | "home"
  | "permissions"
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
  | "audio"
  | "shortcut"
  | "advanced"
  | "privacy";

const SETTINGS_TABS: { id: SettingsTab; label: string }[] = [
  { id: "companion", label: "Companion" },
  { id: "dictation", label: "Dictation" },
  { id: "audio", label: "Audio" },
  { id: "shortcut", label: "Shortcut" },
  { id: "advanced", label: "Advanced" },
  { id: "privacy", label: "Privacy" },
];

interface AppSettings {
  language: string;
  autoPaste: boolean;
  hotkeyLabel: string;
  showFloatingPill: boolean;
  /** fn | fn_control | both */
  pttBinding?: string;
  keepCmdShiftV?: boolean;
  /** classic (obsidian capsule) | yappy (pixel pet) */
  pillStyle?: string;
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
   * shows a "Model needed" route into the onboarding model step, never "Ready".
   */
  modelReady: boolean;
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
}

interface DayCount {
  date: string;
  words: number;
  sessions: number;
}

function fmtInt(n: number | undefined | null) {
  return Math.max(0, Math.round(n ?? 0)).toLocaleString();
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

interface DictEntry {
  id: string;
  term: string;
  preferred?: string | null;
  hits: number;
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
  });
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  const [history, setHistory] = useState<TranscriptEntry[]>([]);
  const [insights, setInsights] = useState<Insights | null>(null);
  const [dailySeries, setDailySeries] = useState<DayCount[]>([]);
  const [monthlySeries, setMonthlySeries] = useState<DayCount[]>([]);
  const [dictionary, setDictionary] = useState<DictEntry[]>([]);
  const [scratch, setScratch] = useState<ScratchNote[]>([]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [query, setQuery] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [newTerm, setNewTerm] = useState("");
  const [newPreferred, setNewPreferred] = useState("");
  const [noteTitle, setNoteTitle] = useState("Scratchpad");
  const [noteBody, setNoteBody] = useState("");
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);
  // YV33 — re-open the onboarding overlay on a specific step (the "Model
  // needed" route). null = not showing it.
  const [setupStep, setSetupStep] = useState<OnboardStep | null>(null);
  // YV15 — key-capture for the push-to-talk shortcut. `capturing` arms a global
  // keydown listener; `captureHint` shows the live result / validation message.
  const [capturing, setCapturing] = useState(false);
  const [captureHint, setCaptureHint] = useState<string | null>(null);

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
      limit: 200,
    });
    setHistory(h);
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
      const [s, st, ins, daily, monthly, dict, notes] = await Promise.all([
        invoke<AppSettings>("get_settings"),
        invoke<AppStatus>("get_status"),
        invoke<Insights>("get_insights"),
        invoke<DayCount[]>("daily_series", { days: 365 }),
        invoke<DayCount[]>("monthly_series", { months: 12 }),
        invoke<DictEntry[]>("list_dictionary"),
        invoke<ScratchNote[]>("list_scratch"),
      ]);
      setSettings(s);
      setStatus(st);
      setInsights(ins);
      setDailySeries(daily);
      setMonthlySeries(monthly);
      setDictionary(dict);
      setScratch(notes);
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
      setHistory((h) => [e.payload, ...h.filter((x) => x.id !== e.payload.id)]);
      try {
        setInsights(await invoke("get_insights"));
        setDictionary(await invoke("list_dictionary"));
      } catch {
        /* ignore */
      }
    }).then((u) => (dead ? u() : unsubs.push(u)));
    listen<string>("paste_outcome", (e) => {
      setFlash(e.payload);
      setTimeout(() => setFlash(null), 2800);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // YV33 — a failed take must be visible IN the app, not only as a macOS
    // notification the user may have muted (or never sees while Yap is focused).
    listen<{ message: string }>("transcript_error", (e) => {
      setFlash(e.payload?.message || "Transcription failed");
      setTimeout(() => setFlash(null), 4000);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // Menu-bar "Settings…" jumps the app to the Settings view (YV26).
    listen<string>("navigate", (e) => {
      const dest = e.payload as Nav;
      setNav(dest);
    }).then((u) => (dead ? u() : unsubs.push(u)));
    // Tray "Keyboard Shortcuts…" jumps straight to the Shortcut sub-tab.
    listen<string>("settings-tab", (e) => {
      setSettingsTab(e.payload as SettingsTab);
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

  async function toast(msg: string) {
    setFlash(msg);
    setTimeout(() => setFlash(null), 2000);
  }

  // Every mutating command returns Result<_, String>; on Err the promise rejects.
  // Without a catch the UI silently no-ops (and throws an unhandled rejection),
  // so a failed paste/save/delete looks identical to success. Surface it.
  async function toggleRecord() {
    try {
      await invoke("manual_toggle");
    } catch (e) {
      toast(String(e));
    }
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

  async function clearAll() {
    if (!confirm("Clear all transcript history from SQLite?")) return;
    try {
      await invoke("clear_history");
      setHistory([]);
    } catch (e) {
      toast(String(e));
      return;
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
      return { cols: 0, cells: [] as { d: DayCount; col: number; row: number }[], max: 0 };
    }
    const wd0 = new Date(dailySeries[0].date + "T12:00:00").getDay(); // 0=Sun
    const max = Math.max(1, ...dailySeries.map((d) => d.words));
    const cells = dailySeries.map((d, i) => {
      const g = wd0 + i;
      return { d, col: Math.floor(g / 7), row: g % 7 };
    });
    const cols = Math.ceil((wd0 + dailySeries.length) / 7);
    return { cols, cells, max };
  }, [dailySeries]);
  const hasActivity = useMemo(
    () => dailySeries.some((d) => d.words > 0),
    [dailySeries],
  );

  // Monthly words bar chart — last 12 months.
  const maxMonthly = useMemo(
    () => Math.max(1, ...monthlySeries.map((d) => d.words)),
    [monthlySeries],
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

  // YV33 — the same overlay, re-opened on one step, is how an already-onboarded
  // install fixes "Model needed" (no model on disk = nothing can transcribe).
  if (settings && setupStep) {
    return (
      <Onboarding
        initialStep={setupStep}
        onFinish={(sample) => {
          setSetupStep(null);
          finishOnboarding(sample);
          refreshAll();
        }}
        onSkip={() => {
          setSetupStep(null);
          refreshAll();
        }}
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
            <div className="brand-tag">v0.6.0 · local · private</div>
          </div>
        </div>

        <nav className="nav">
          {(
            [
              ["home", "Home", history.length],
              ["permissions", "Permissions", needsPerms ? 1 : null],
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
            {status.recording
              ? "Stop listening"
              : status.busy
                ? "Transcribing…"
                : "Dictate · fn⌃"}
          </button>
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          <div>
            <h1>
              {nav === "home" && (userName ? `Welcome back, ${userName}` : "Welcome back")}
              {nav === "permissions" && "Permissions"}
              {nav === "insights" && "Insights"}
              {nav === "dictionary" && "Dictionary"}
              {nav === "scratchpad" && "Scratchpad"}
              {nav === "settings" && "Settings"}
            </h1>
            <p className="lede">
              {nav === "home" &&
                "Hold fn⌃ · double-tap hands-free · tap again to stop. Local Whisper; history in SQLite."}
              {nav === "permissions" &&
                "macOS must grant these to Yap itself. Without them, dictation or paste fails."}
              {nav === "insights" &&
                "Local analytics from your SQLite history — nothing leaves this Mac."}
              {nav === "dictionary" &&
                "Custom spellings applied after each transcription."}
              {nav === "scratchpad" && "Park text and assemble prompts."}
              {nav === "settings" &&
                "Your companion, dictation, shortcut, and privacy — all in plain language."}
            </p>
          </div>
          <div className={pillClass}>{status.message}</div>
        </header>

        {flash && <div className="toast">{flash}</div>}

        {/* YV33 — no usable model means dictation cannot run at all, so this
            outranks the permissions banner and routes straight to the download
            step instead of leaving the user to discover it mid-take. */}
        {!status.modelReady && (
          <div className="banner warn" onClick={() => setSetupStep("model")}>
            Model needed — download a speech model to start dictating
          </div>
        )}

        {status.modelReady && needsPerms && nav === "home" && (
          <div className="banner warn" onClick={() => setNav("permissions")}>
            Setup incomplete — open Permissions to enable Mic / Accessibility
            for Yap
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
                      froze the app for minutes doing it. The embedded GGUF
                      engine needs a downloaded model instead, so this routes to
                      the model step. */}
                  <button onClick={() => setSetupStep("model")}>
                    {status.modelReady ? "Manage speech model" : "Get a speech model"}
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
                <li className={perms?.asrOk ? "ok" : "bad"}>
                  <StatusDot ok={!!perms?.asrOk} />
                  <div>
                    <strong>Speech model</strong>
                    <p className="muted">{perms?.asrDetail}</p>
                    {!status.modelReady && (
                      <button onClick={() => setSetupStep("model")}>
                        Get a speech model
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

              <div className="stats-row">
                <div className="stat glass">
                  <div className="stat-n">{fmtInt(insights?.totalWords)}</div>
                  <div className="stat-l">total words</div>
                  <div className="stat-sub">
                    {fmtInt(insights?.totalSessions)} sessions
                  </div>
                </div>
                <div className="stat glass">
                  <div className="stat-n">{fmtInt(insights?.avgWpm)}</div>
                  <div className="stat-l">avg wpm</div>
                  <div className="stat-sub">
                    speech only · n=
                    {fmtInt(insights?.wpmSampleSessions)}
                  </div>
                </div>
                <div className="stat glass">
                  <div className="stat-n">{fmtInt(insights?.streakDays)}</div>
                  <div className="stat-l">day streak</div>
                  <div className="stat-sub">
                    best {fmtInt(insights?.longestStreak)}
                  </div>
                </div>
                <div className="stat glass">
                  <div className="stat-n">{fmtInt(insights?.wordsToday)}</div>
                  <div className="stat-l">words today</div>
                  <div className="stat-sub">
                    {fmtInt(insights?.sessionsToday)} sessions
                  </div>
                </div>
              </div>

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
                <button className="ghost" onClick={clearAll}>
                  Clear
                </button>
              </div>

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
                      <p>{e.text}</p>
                      <div className="actions">
                        <button onClick={() => copyText(e.text)}>Copy</button>
                        <button
                          className="primary"
                          onClick={() => pasteText(e.text)}
                        >
                          Paste
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
                    </li>
                  ))}
                </ul>
              )}
            </>
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
                    <svg
                      className="heatmap"
                      viewBox={`0 0 ${heat.cols * 13} ${7 * 13}`}
                      preserveAspectRatio="xMinYMid meet"
                      role="img"
                      aria-label="Daily dictation activity heatmap for the last year"
                    >
                      {heat.cells.map(({ d, col, row }) => {
                        const lvl = heatLevel(d.words, heat.max);
                        const op = [0, 0.28, 0.5, 0.75, 1][lvl];
                        return (
                          <rect
                            key={d.date}
                            x={col * 13}
                            y={row * 13}
                            width={10}
                            height={10}
                            rx={2}
                            fill={lvl === 0 ? "var(--bg-elev)" : "var(--accent)"}
                            fillOpacity={lvl === 0 ? 1 : op}
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
                          style={{
                            background:
                              lvl === 0 ? "var(--bg-elev)" : "var(--accent)",
                            opacity: lvl === 0 ? 1 : [0, 0.28, 0.5, 0.75, 1][lvl],
                          }}
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
                  <div className="bars">
                    {monthlySeries.map((d) => (
                      <div key={d.date} className="bar-row">
                        <span className="bar-label">{formatMonth(d.date)}</span>
                        <div className="bar-track">
                          <div
                            className="bar-fill"
                            style={{ width: `${(d.words / maxMonthly) * 100}%` }}
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
                  <ul className="kv">
                    {insights.topApps.map((a) => (
                      <li key={a.app}>
                        <span>{a.app}</span>
                        <strong>
                          {a.words.toLocaleString()} w · {a.sessions} sess
                        </strong>
                      </li>
                    ))}
                  </ul>
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
                  form.
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
              <ul className="dict-list">
                {dictionary.map((d) => (
                  <li key={d.id}>
                    <div>
                      <strong>{d.term}</strong>
                      {d.preferred && (
                        <span className="muted"> → {d.preferred}</span>
                      )}
                      <div className="tiny">{d.hits} hits</div>
                    </div>
                    <button
                      className="ghost danger"
                      onClick={() => removeTerm(d.id)}
                    >
                      Remove
                    </button>
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
                      and format the result. Your words are never dropped.
                    </p>
                    <div className="profile-row">
                      {(
                        [
                          ["none", "None", "Exactly as spoken"],
                          ["light", "Light", "Remove filler + fix re-dos"],
                          ["medium", "Medium", "Light + smart formatting"],
                          ["high", "High", "Medium + AI polish"],
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
                </section>
              )}

              {/* ── Advanced — the speech model ──
                  YV34 retired the speed-profile buttons and the "Voice model
                  (advanced override)" list: both picked a repo id for the
                  deleted Python sidecar. Speed vs. accuracy is now a property
                  of which embedded model you download, so this routes to the
                  one place that choice lives. */}
              {settingsTab === "advanced" && (
                <section className="settings-panel">
                  <h2 className="settings-section">
                    Advanced — Speech model
                    <span className="sub">
                      Leave this as-is unless dictation feels slow or misses
                      words.
                    </span>
                  </h2>
                  <div className="panel">
                    <h3>Speed vs. accuracy</h3>
                    <p className="muted">
                      Yap transcribes inside the app itself — no helper process,
                      nothing installed on the side, nothing off this Mac. Your
                      model is kept loaded so dictation starts the moment you
                      press your key. Smaller models are faster and lighter;
                      larger ones are more accurate on long or technical
                      dictation.
                    </p>
                    <div className="actions" style={{ marginTop: 10 }}>
                      <button
                        className="primary"
                        onClick={() => setSetupStep("model")}
                      >
                        {status.modelReady
                          ? "Manage speech model"
                          : "Get a speech model"}
                      </button>
                    </div>
                    <p className="muted tiny" style={{ marginTop: 8 }}>
                      {perms?.asrDetail}
                    </p>
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
                          const p = await invoke<string>("export_history");
                          toast(`Exported → ${p}`);
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
                          toast("Opened logs folder for diagnostics");
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
                </section>
              )}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
