import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./App.css";

type Nav =
  | "home"
  | "insights"
  | "dictionary"
  | "scratchpad"
  | "settings";

interface AppSettings {
  model: string;
  language: string;
  autoPaste: boolean;
  hotkeyLabel: string;
  showFloating: boolean;
}

interface AppStatus {
  recording: boolean;
  busy: boolean;
  lastError: string | null;
  message: string;
  pythonOk: boolean;
  workerOk: boolean;
}

interface TranscriptEntry {
  id: string;
  text: string;
  backend: string;
  asrSeconds: number;
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
  "mlx-community/whisper-large-v3-turbo",
  "mlx-community/whisper-large-v3",
  "mlx-community/whisper-medium",
  "mlx-community/whisper-small",
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

function FloatPill() {
  const [status, setStatus] = useState<AppStatus | null>(null);

  useEffect(() => {
    invoke<AppStatus>("get_status").then(setStatus).catch(() => {});
    const unsubs: Array<() => void> = [];
    listen<AppStatus>("status", (e) => setStatus(e.payload)).then((u) =>
      unsubs.push(u),
    );
    listen<boolean>("recording", (e) =>
      setStatus((s) =>
        s
          ? {
              ...s,
              recording: e.payload,
              message: e.payload ? "Listening…" : s.message,
            }
          : s,
      ),
    ).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, []);

  const live = status?.recording;
  const busy = status?.busy;

  return (
    <div
      className={live ? "float-pill live" : busy ? "float-pill busy" : "float-pill"}
      data-tauri-drag-region
    >
      <button
        className="float-dictate"
        onClick={() => invoke("manual_toggle")}
        disabled={!!busy}
      >
        <span className="float-dot" />
        <span>{live ? "Stop" : busy ? "…" : "Dictate"}</span>
        <kbd>⌥Space</kbd>
      </button>
      <button
        className="float-open"
        title="Open Wilson Voice"
        onClick={() => invoke("show_main")}
      >
        ↗
      </button>
    </div>
  );
}

export default function App() {
  const [isFloat, setIsFloat] = useState(false);
  const [nav, setNav] = useState<Nav>("home");
  const [status, setStatus] = useState<AppStatus>({
    recording: false,
    busy: false,
    lastError: null,
    message: "Loading…",
    pythonOk: false,
    workerOk: false,
  });
  const [history, setHistory] = useState<TranscriptEntry[]>([]);
  const [insights, setInsights] = useState<Insights | null>(null);
  const [dictionary, setDictionary] = useState<DictEntry[]>([]);
  const [scratch, setScratch] = useState<ScratchNote[]>([]);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [query, setQuery] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const [newTerm, setNewTerm] = useState("");
  const [newPreferred, setNewPreferred] = useState("");
  const [noteTitle, setNoteTitle] = useState("Scratchpad");
  const [noteBody, setNoteBody] = useState("");
  const [activeNoteId, setActiveNoteId] = useState<string | null>(null);

  useEffect(() => {
    try {
      setIsFloat(getCurrentWindow().label === "float");
    } catch {
      setIsFloat(false);
    }
  }, []);

  const loadHistory = useCallback(async (q?: string) => {
    const h = await invoke<TranscriptEntry[]>("get_history", {
      query: q || null,
      limit: 200,
    });
    setHistory(h);
  }, []);

  const refreshAll = useCallback(async () => {
    try {
      const [s, st, ins, dict, notes, set] = await Promise.all([
        invoke<AppSettings>("get_settings"),
        invoke<AppStatus>("get_status"),
        invoke<Insights>("get_insights"),
        invoke<DictEntry[]>("list_dictionary"),
        invoke<ScratchNote[]>("list_scratch"),
        Promise.resolve(null),
      ]);
      setSettings(s);
      setStatus(st);
      setInsights(ins);
      setDictionary(dict);
      setScratch(notes);
      void set;
      await loadHistory(query);
    } catch (e) {
      setStatus((p) => ({
        ...p,
        message: `Bridge error: ${e}`,
        lastError: String(e),
      }));
    }
  }, [loadHistory, query]);

  useEffect(() => {
    if (isFloat) return;
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
          ? "Recording… release hotkey to transcribe"
          : s.message,
      })),
    ).then((u) => unsubs.push(u));
    listen<TranscriptEntry>("transcript", async (e) => {
      setHistory((h) => [e.payload, ...h.filter((x) => x.id !== e.payload.id)]);
      setFlash("Saved · copied · pasted");
      setTimeout(() => setFlash(null), 2200);
      try {
        setInsights(await invoke("get_insights"));
      } catch {
        /* ignore */
      }
    }).then((u) => unsubs.push(u));
    return () => unsubs.forEach((u) => u());
  }, [isFloat, refreshAll]);

  useEffect(() => {
    if (isFloat) return;
    const t = setTimeout(() => {
      loadHistory(query).catch(() => {});
    }, 200);
    return () => clearTimeout(t);
  }, [query, loadHistory, isFloat]);

  if (isFloat) return <FloatPill />;

  async function toast(msg: string) {
    setFlash(msg);
    setTimeout(() => setFlash(null), 1800);
  }

  async function toggleRecord() {
    await invoke("manual_toggle");
  }

  async function copyText(text: string) {
    await invoke("copy_entry", { text });
    toast("Copied");
  }

  async function pasteText(text: string) {
    await invoke("paste_entry", { text });
    toast("Pasted into frontmost app");
  }

  async function removeEntry(id: string) {
    await invoke("delete_entry", { id });
    setHistory((h) => h.filter((e) => e.id !== id));
    setInsights(await invoke("get_insights"));
  }

  async function clearAll() {
    if (!confirm("Clear all transcript history from SQLite?")) return;
    await invoke("clear_history");
    setHistory([]);
    setInsights(await invoke("get_insights"));
  }

  async function saveSettings(next: AppSettings) {
    await invoke("save_settings", { settings: next });
    setSettings(next);
    toast("Settings saved");
  }

  async function addTerm() {
    if (!newTerm.trim()) return;
    await invoke("add_dictionary_term", {
      term: newTerm.trim(),
      preferred: newPreferred.trim() || null,
    });
    setNewTerm("");
    setNewPreferred("");
    setDictionary(await invoke("list_dictionary"));
    toast("Dictionary term added");
  }

  async function removeTerm(id: string) {
    await invoke("delete_dictionary_term", { id });
    setDictionary((d) => d.filter((x) => x.id !== id));
  }

  async function saveNote() {
    const note = await invoke<ScratchNote>("save_scratch", {
      id: activeNoteId,
      title: noteTitle || "Note",
      body: noteBody,
    });
    setActiveNoteId(note.id);
    setScratch(await invoke("list_scratch"));
    toast("Scratchpad saved");
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
    await invoke("delete_scratch", { id });
    if (activeNoteId === id) newNote();
    setScratch(await invoke("list_scratch"));
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

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand-block">
          <div className="mark">
            <span className={status.recording ? "dot pulse" : "dot"} />
          </div>
          <div>
            <div className="brand-name">Wilson Voice</div>
            <div className="brand-tag">Local · private · fast</div>
          </div>
        </div>

        <nav className="nav">
          {(
            [
              ["home", "Home", history.length],
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
              {count != null && count > 0 && <span className="count">{count}</span>}
            </button>
          ))}
        </nav>

        <div className="sidebar-foot">
          <button
            className={
              status.recording ? "dictate-side live" : "dictate-side"
            }
            onClick={toggleRecord}
            disabled={status.busy}
          >
            {status.recording
              ? "Stop listening"
              : status.busy
                ? "Transcribing…"
                : "Dictate · ⌥Space"}
          </button>
        </div>
      </aside>

      <section className="main">
        <header className="main-head">
          <div>
            <h1>
              {nav === "home" && "Welcome back, Wilson"}
              {nav === "insights" && "Insights"}
              {nav === "dictionary" && "Dictionary"}
              {nav === "scratchpad" && "Scratchpad"}
              {nav === "settings" && "Settings"}
            </h1>
            <p className="lede">
              {nav === "home" &&
                "Every dictation lands here. Search, copy, or paste again into any app."}
              {nav === "insights" &&
                "Usage analytics from your local SQLite history — nothing leaves this Mac."}
              {nav === "dictionary" &&
                "Teach Whisper your names, products, and jargon so it spells them right."}
              {nav === "scratchpad" &&
                "Capture thoughts, assemble prompts, park text for later."}
              {nav === "settings" &&
                "Hotkeys, model, permissions, floating Dictate control."}
            </p>
          </div>
          <div className={pillClass}>{status.message}</div>
        </header>

        {flash && <div className="toast">{flash}</div>}

        <div className="content">
          {nav === "home" && (
            <>
              <div className="stats-row">
                <div className="stat">
                  <div className="stat-n">
                    {(insights?.totalWords ?? 0).toLocaleString()}
                  </div>
                  <div className="stat-l">total words</div>
                </div>
                <div className="stat">
                  <div className="stat-n">
                    {Math.round(insights?.avgWpm ?? 0)}
                  </div>
                  <div className="stat-l">avg wpm</div>
                </div>
                <div className="stat">
                  <div className="stat-n">{insights?.streakDays ?? 0}</div>
                  <div className="stat-l">day streak</div>
                </div>
                <div className="stat">
                  <div className="stat-n">
                    {(insights?.wordsToday ?? 0).toLocaleString()}
                  </div>
                  <div className="stat-l">words today</div>
                </div>
              </div>

              <div className="toolbar">
                <input
                  type="search"
                  placeholder="Search transcripts (SQLite FTS5)…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <button className="ghost" onClick={clearAll}>
                  Clear all
                </button>
              </div>

              {history.length === 0 ? (
                <div className="empty">
                  <h3>No dictations yet</h3>
                  <p>
                    Hold <kbd>⌥Space</kbd> over any text field — Claude, Codex,
                    ChatGPT, email — and Wilson Voice will transcribe locally,
                    paste, and archive here.
                  </p>
                </div>
              ) : (
                <ul className="feed">
                  {history.map((e) => (
                    <li key={e.id} className="card">
                      <div className="card-meta">
                        <span>{formatTime(e.createdAt)}</span>
                        <span>
                          {e.wordCount} words · {e.backend} ·{" "}
                          {e.asrSeconds.toFixed(1)}s
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
                            setNoteTitle(
                              `From ${formatTime(e.createdAt)}`,
                            );
                            setNoteBody(e.text);
                            setActiveNoteId(null);
                          }}
                        >
                          To scratchpad
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
                  <div className="stat-l">total words dictated</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">
                    {insights.totalSessions.toLocaleString()}
                  </div>
                  <div className="stat-l">sessions</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">{Math.round(insights.avgWpm)}</div>
                  <div className="stat-l">words per minute</div>
                </div>
                <div className="stat big">
                  <div className="stat-n">{insights.streakDays}</div>
                  <div className="stat-l">
                    day streak (best {insights.longestStreak})
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
                  <h3>Engine</h3>
                  <ul className="kv">
                    <li>
                      <span>Avg ASR time</span>
                      <strong>{insights.avgAsrSeconds.toFixed(2)}s</strong>
                    </li>
                    <li>
                      <span>Sessions today</span>
                      <strong>{insights.sessionsToday}</strong>
                    </li>
                    <li>
                      <span>Words today</span>
                      <strong>{insights.wordsToday.toLocaleString()}</strong>
                    </li>
                    <li>
                      <span>Python venv</span>
                      <strong className={status.pythonOk ? "ok" : "bad"}>
                        {status.pythonOk ? "ready" : "missing"}
                      </strong>
                    </li>
                    <li>
                      <span>ASR worker</span>
                      <strong className={status.workerOk ? "ok" : "bad"}>
                        {status.workerOk ? "ready" : "missing"}
                      </strong>
                    </li>
                  </ul>
                  {insights.topApps.length > 0 && (
                    <>
                      <h3 style={{ marginTop: 18 }}>Sources</h3>
                      <ul className="kv">
                        {insights.topApps.map((a) => (
                          <li key={a.app}>
                            <span>{a.app}</span>
                            <strong>
                              {a.words.toLocaleString()} w · {a.sessions}
                            </strong>
                          </li>
                        ))}
                      </ul>
                    </>
                  )}
                </div>
              </div>
            </div>
          )}

          {nav === "dictionary" && (
            <div className="dict">
              <div className="panel intro">
                <h3>Spell the way you do</h3>
                <p>
                  Add personal terms, company names, and client jargon. After
                  each transcription, Wilson Voice rewrites matched tokens to
                  your preferred form — same idea as Wispr Flow Dictionary.
                </p>
                <div className="dict-add">
                  <input
                    placeholder="Term (e.g. Drivia)"
                    value={newTerm}
                    onChange={(e) => setNewTerm(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTerm()}
                  />
                  <input
                    placeholder="Preferred spelling (optional)"
                    value={newPreferred}
                    onChange={(e) => setNewPreferred(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTerm()}
                  />
                  <button className="primary" onClick={addTerm}>
                    Add word
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
                  placeholder="Park text, draft prompts, assemble emails…"
                />
                <div className="actions">
                  <button className="primary" onClick={saveNote}>
                    Save
                  </button>
                  <button onClick={() => copyText(noteBody)} disabled={!noteBody}>
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
              <label className="field">
                <span>Whisper model (local MLX)</span>
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
                <span>Auto-paste into frontmost app after transcription</span>
              </label>
              <label className="toggle">
                <input
                  type="checkbox"
                  checked={settings.showFloating}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      showFloating: e.target.checked,
                    })
                  }
                />
                <span>Show floating Dictate control (Wispr-style pill)</span>
              </label>
              <div className="panel">
                <h3>Hotkeys</h3>
                <p className="muted">{settings.hotkeyLabel}</p>
                <p className="muted">
                  Grant Input Monitoring + Accessibility so global hotkeys and
                  Cmd+V paste work system-wide.
                </p>
              </div>
              <div className="actions wrap">
                <button
                  className="primary"
                  onClick={() => saveSettings(settings)}
                >
                  Save settings
                </button>
                <button onClick={() => invoke("open_data_dir")}>
                  Open data folder
                </button>
                <button
                  onClick={() =>
                    invoke("open_privacy_settings", { pane: "Microphone" })
                  }
                >
                  Mic
                </button>
                <button
                  onClick={() =>
                    invoke("open_privacy_settings", { pane: "Accessibility" })
                  }
                >
                  Accessibility
                </button>
                <button
                  onClick={() =>
                    invoke("open_privacy_settings", {
                      pane: "InputMonitoring",
                    })
                  }
                >
                  Input Monitoring
                </button>
              </div>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
