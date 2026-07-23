import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type Nav =
  | "home"
  | "permissions"
  | "insights"
  | "dictionary"
  | "scratchpad"
  | "settings";

interface AppSettings {
  model: string;
  language: string;
  autoPaste: boolean;
  hotkeyLabel: string;
  showFloatingPill: boolean;
  /** fast | balanced | max */
  speedProfile: string;
  /** fn | fn_control | both */
  pttBinding?: string;
  keepCmdShiftV?: boolean;
  /** classic (obsidian capsule) | yappy (pixel pet) */
  pillStyle?: string;
  /** auto | plain | list | email | code | notes */
  dictationMode?: string;
}

interface AppStatus {
  recording: boolean;
  busy: boolean;
  lastError: string | null;
  message: string;
  pythonOk: boolean;
  workerOk: boolean;
  accessibility: boolean;
  hotkeyRegistered: boolean;
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

function fmtInt(n: number | undefined | null) {
  return Math.max(0, Math.round(n ?? 0)).toLocaleString();
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

const MODELS = [
  "mlx-community/whisper-tiny-mlx",
  "mlx-community/whisper-base-mlx",
  "mlx-community/whisper-small-mlx",
  "mlx-community/whisper-large-v3-turbo",
  "mlx-community/whisper-medium-mlx",
  "mlx-community/whisper-large-v3-mlx",
];

const PROFILES: { id: string; label: string; blurb: string; model: string }[] =
  [
    {
      id: "fast",
      label: "Fast",
      blurb: "whisper-small-mlx — best during heavy coding (lowest local load)",
      model: "mlx-community/whisper-small-mlx",
    },
    {
      id: "balanced",
      label: "Balanced",
      blurb: "large-v3-turbo — default accuracy, warm daemon",
      model: "mlx-community/whisper-large-v3-turbo",
    },
    {
      id: "max",
      label: "Max",
      blurb: "large-v3-mlx — slowest, highest accuracy (long notes)",
      model: "mlx-community/whisper-large-v3-mlx",
    },
  ];

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
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    lastError: null,
    message: "Loading…",
    pythonOk: false,
    workerOk: false,
    accessibility: false,
    hotkeyRegistered: false,
  });
  const [perms, setPerms] = useState<PermissionReport | null>(null);
  const [history, setHistory] = useState<TranscriptEntry[]>([]);
  const [insights, setInsights] = useState<Insights | null>(null);
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

  // Read the live query without making `refreshAll` depend on it — otherwise the
  // mount effect that registers event listeners re-runs on every keystroke,
  // tearing down + re-adding all listeners and dropping events in the gap.
  const queryRef = useRef(query);
  useEffect(() => {
    queryRef.current = query;
  }, [query]);

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
      const [s, st, ins, dict, notes] = await Promise.all([
        invoke<AppSettings>("get_settings"),
        invoke<AppStatus>("get_status"),
        invoke<Insights>("get_insights"),
        invoke<DictEntry[]>("list_dictionary"),
        invoke<ScratchNote[]>("list_scratch"),
      ]);
      setSettings(s);
      setStatus(st);
      setInsights(ins);
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
    const unsubs: Array<() => void> = [];
    listen<AppStatus>("status", (e) => setStatus(e.payload)).then((u) =>
      unsubs.push(u),
    );
    listen<boolean>("recording", (e) =>
      setStatus((s) => ({
        ...s,
        recording: e.payload,
        message: e.payload
          ? "Recording… release fn⌃ or click Stop"
          : s.message,
      })),
    ).then((u) => unsubs.push(u));
    listen<TranscriptEntry>("transcript", async (e) => {
      setHistory((h) => [e.payload, ...h.filter((x) => x.id !== e.payload.id)]);
      try {
        setInsights(await invoke("get_insights"));
        setDictionary(await invoke("list_dictionary"));
      } catch {
        /* ignore */
      }
    }).then((u) => unsubs.push(u));
    listen<string>("paste_outcome", (e) => {
      setFlash(e.payload);
      setTimeout(() => setFlash(null), 2800);
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
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
      setInsights(await invoke("get_insights"));
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

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="mark">
            <span className={status.recording ? "dot pulse" : "dot"} />
          </div>
          <div>
            <div className="brand-name">Yap</div>
            <div className="brand-tag">v0.4 · local · private</div>
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
              {nav === "home" && "Welcome back, Wilson"}
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
                "macOS must grant these to Yap (not Python). Without them, dictation or paste fails."}
              {nav === "insights" &&
                "Local analytics from your SQLite history — nothing leaves this Mac."}
              {nav === "dictionary" &&
                "Custom spellings applied after each transcription."}
              {nav === "scratchpad" && "Park text and assemble prompts."}
              {nav === "settings" && "Model, language, auto-paste."}
            </p>
          </div>
          <div className={pillClass}>{status.message}</div>
        </header>

        {flash && <div className="toast">{flash}</div>}

        {needsPerms && nav === "home" && (
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
                  Privacy &amp; Security. Do not enable “Python” for this
                  product.
                </p>
                <p className="muted">{perms?.summary}</p>
                <div className="actions">
                  <button className="primary" onClick={refreshPerms}>
                    Re-check permissions
                  </button>
                  <button
                    onClick={async () => {
                      toast("Installing local ASR… (one-time network)");
                      try {
                        const msg = await invoke<string>("setup_asr_venv");
                        toast(msg);
                      } catch (e) {
                        toast(String(e));
                      }
                      await refreshPerms();
                    }}
                  >
                    Install local ASR
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
                      In-process audio (cpal) so TCC lists <strong>Yap</strong>, not
                      Python or ffmpeg. Click Dictate once to trigger the system prompt.
                    </p>
                  </div>
                </li>
                <li className={perms?.asrOk ? "ok" : "bad"}>
                  <StatusDot ok={!!perms?.asrOk} />
                  <div>
                    <strong>Local Whisper (MLX)</strong>
                    <p className="muted">{perms?.asrDetail}</p>
                  </div>
                </li>
              </ul>
            </div>
          )}

          {nav === "home" && (
            <>
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
              <div className="panel">
                <h3>Speed profile (local MLX — no AWS required)</h3>
                <p className="muted">
                  Warm daemon keeps the model loaded. Use <strong>Fast</strong>{" "}
                  during heavy coding so dictation does not fight your session
                  for Metal/RAM. AWS GPUs are for offline fine-tune / batch —
                  not every dictation.
                </p>
                <div className="actions wrap" style={{ marginTop: 10 }}>
                  {PROFILES.map((p) => (
                    <button
                      key={p.id}
                      className={
                        (settings.speedProfile || "balanced") === p.id
                          ? "primary"
                          : undefined
                      }
                      onClick={() =>
                        setSettings({
                          ...settings,
                          speedProfile: p.id,
                          model: p.model,
                        })
                      }
                      title={p.blurb}
                    >
                      {p.label}
                    </button>
                  ))}
                </div>
                <p className="muted tiny" style={{ marginTop: 8 }}>
                  {
                    PROFILES.find(
                      (p) => p.id === (settings.speedProfile || "balanced"),
                    )?.blurb
                  }
                </p>
              </div>
              <label className="field">
                <span>Whisper model (override)</span>
                <select
                  value={settings.model}
                  onChange={(e) =>
                    setSettings({ ...settings, model: e.target.value })
                  }
                >
                  {MODELS.map((m) => (
                    <option key={m} value={m}>
                      {m.replace("mlx-community/", "")}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>Language</span>
                <select
                  value={settings.language}
                  onChange={(e) =>
                    setSettings({ ...settings, language: e.target.value })
                  }
                >
                  <option value="en">English</option>
                  <option value="es">Spanish</option>
                  <option value="fr">French</option>
                  <option value="ht">Haitian Creole</option>
                </select>
              </label>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={settings.autoPaste}
                  onChange={(e) =>
                    setSettings({ ...settings, autoPaste: e.target.checked })
                  }
                />
                <span>
                  Auto-paste after transcription (needs Accessibility)
                </span>
              </label>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={settings.showFloatingPill ?? true}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      showFloatingPill: e.target.checked,
                    })
                  }
                />
                <span>
                  Always show Dictate island (glass HUD; also appears while
                  recording)
                </span>
              </label>
              <div className="panel">
                <h3>Pill style</h3>
                <p>Pick your Dictate companion — it switches live.</p>
                <div className="profile-row">
                  {(
                    [
                      ["classic", "Classic", "Obsidian waveform capsule"],
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
                      onClick={() => saveSettings({ ...settings, pillStyle: id })}
                    >
                      <strong>{label}</strong>
                      <span>{blurb}</span>
                    </button>
                  ))}
                </div>
              </div>
              <div className="panel">
                <h3>Dictation mode</h3>
                <p>
                  Auto adapts formatting to the app you’re typing into; pick a
                  fixed mode to force it. Text is never lost.
                </p>
                <div className="profile-row">
                  {(
                    [
                      ["auto", "Auto", "Adapt to the focused app"],
                      ["plain", "Plain", "Verbatim — no reformatting"],
                      ["list", "List", "Turn spoken lists into bullets"],
                      ["email", "Email", "Compose-friendly formatting"],
                      ["code", "Code", "Leave identifiers untouched"],
                      ["notes", "Notes", "Note-taking formatting"],
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
                <h3>Hold-to-talk key</h3>
                <p>
                  Default is <strong>fn + Control</strong>. Hold to talk;
                  double-tap to latch hands-free; tap again to stop. Needs
                  Accessibility. Set Keyboard → “Press 🌐 key to” →{" "}
                  <strong>Do Nothing</strong>.
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
                      onClick={() =>
                        setSettings({
                          ...settings,
                          pttBinding: id,
                          hotkeyLabel:
                            id === "fn"
                              ? "fn"
                              : id === "both"
                                ? "fn / fn⌃"
                                : "fn⌃",
                        })
                      }
                    >
                      <strong>{label}</strong>
                      <span>{blurb}</span>
                    </button>
                  ))}
                </div>
                <label className="toggle" style={{ marginTop: 12 }}>
                  <input
                    type="checkbox"
                    checked={settings.keepCmdShiftV ?? false}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        keepCmdShiftV: e.target.checked,
                      })
                    }
                  />
                  <span>Also keep ⌘⇧V as secondary hold</span>
                </label>
                <p style={{ marginTop: 12 }}>
                  <strong>{settings.hotkeyLabel}</strong> — island stays
                  bottom-center and rides every Space swipe. Dictionary harvest
                  improves jargon over time.
                </p>
              </div>
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
              </div>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
