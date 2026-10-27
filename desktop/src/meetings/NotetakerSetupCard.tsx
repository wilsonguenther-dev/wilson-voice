/**
 * yap-notetaker-onboard — the "Set up Notetaker" card.
 *
 * Sits at the top of the Meetings view until every working step is done or the
 * user closes it. Three working rows (tour, calendar, import) and a fourth,
 * Set up MCP, shown as Coming with no action. One progress dot per working row
 * (empty / done), a plain verb per row, a contained panel with a pinned footer.
 * The X dismisses it; Help → "Set up Notetaker" brings it back (Rust clears
 * `dismissed` and emits `notetaker-setup`).
 *
 * State lives in `settings_kv` `notetaker_setup_v1` — see `notetakerSetup.ts`.
 */
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorText } from "../errors";
import NotetakerTour from "./NotetakerTour";
import {
  allDone,
  dismiss,
  importSummary,
  isImportable,
  loadSetup,
  markDone,
  NOTETAKER_SETUP_EVENT,
  saveSetup,
  setupRows,
  shouldShow,
  type CalendarAccess,
  type ImportOutcome,
  type NotetakerSetup,
} from "./notetakerSetup";

export const CARD_TITLE = "Set up Notetaker";

/** Read what the user picked and hand Rust the TEXT. No paths, no network. */
async function readPicked(list: FileList | null) {
  const files = Array.from(list ?? []).filter((f) => isImportable(f.name));
  return Promise.all(
    files.map(async (f) => ({ name: f.name, content: await f.text(), modifiedMs: f.lastModified || null })),
  );
}

export default function NotetakerSetupCard({
  initial,
  onImported,
}: {
  /** Tests (and a caller that already has it) pass the state; otherwise it is loaded. */
  initial?: NotetakerSetup | null;
  /** Refresh the Meetings list after an import landed. */
  onImported?: () => void;
}) {
  const [setup, setSetup] = useState<NotetakerSetup | null>(initial ?? null);
  const [touring, setTouring] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<{ row: string; text: string } | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const folderInput = useRef<HTMLInputElement>(null);

  useEffect(() => {
    let dead = false;
    if (initial === undefined) {
      loadSetup()
        .then((s) => !dead && setSetup(s))
        .catch(() => !dead && setSetup(null));
    }
    const unlisten = listen<NotetakerSetup>(NOTETAKER_SETUP_EVENT, (e) => {
      if (!dead) setSetup(e.payload);
    });
    return () => {
      dead = true;
      void unlisten.then((u) => u());
    };
  }, [initial]);

  async function persist(next: NotetakerSetup) {
    setSetup(next);
    try {
      setSetup(await saveSetup(next));
    } catch (e) {
      setNote({ row: "card", text: errorText(e) });
    }
  }

  async function connectCalendar() {
    if (!setup) return;
    setBusy("calendar");
    setNote(null);
    try {
      const access = await invoke<CalendarAccess>("connect_calendar");
      if (access.connected) {
        setSetup(markDone(setup, "calendar"));
        setNote({ row: "calendar", text: `Connected — ${access.calendars} calendar${access.calendars === 1 ? "" : "s"}.` });
      } else {
        setNote({ row: "calendar", text: access.message ?? "Calendar access was not granted." });
      }
    } catch (e) {
      setNote({ row: "calendar", text: errorText(e) });
    } finally {
      setBusy(null);
    }
  }

  async function importFiles(list: FileList | null) {
    if (!setup) return;
    setBusy("import");
    setNote(null);
    try {
      const files = await readPicked(list);
      const outcomes = files.length
        ? await invoke<ImportOutcome[]>("import_meetings", { files })
        : [];
      setNote({ row: "import", text: importSummary(outcomes) });
      if (outcomes.some((o) => o.meetingId)) {
        setSetup(markDone(setup, "import"));
        onImported?.();
      }
    } catch (e) {
      setNote({ row: "import", text: errorText(e) });
    } finally {
      setBusy(null);
      if (fileInput.current) fileInput.current.value = "";
      if (folderInput.current) folderInput.current.value = "";
    }
  }

  if (touring && setup) {
    return (
      <NotetakerTour
        onClose={() => setTouring(false)}
        onFinish={() => {
          setTouring(false);
          void persist(markDone(setup, "tour"));
        }}
      />
    );
  }
  if (!shouldShow(setup) || !setup) return null;

  const rows = setupRows(setup);
  const doneCount = [setup.tour, setup.calendar, setup.import].filter(Boolean).length;
  return (
    <section className="panel nt-setup" aria-label={CARD_TITLE} data-notetaker-setup>
      <header className="nt-setup-head">
        <strong>{CARD_TITLE}</strong>
        <span className="tiny">{doneCount} of 3 done</span>
        <button
          className="ghost nt-setup-x"
          aria-label="Close. Reopen anytime from Help"
          title="Reopen anytime from Help"
          onClick={() => void persist(dismiss(setup))}
        >
          ×
        </button>
      </header>
      <ul className="nt-setup-rows">
        {rows.map((row) => (
          <li key={row.id} className="nt-setup-row" data-row={row.id}>
            {row.done === null ? (
              <span className="nt-setup-dot" data-dot="none" aria-hidden />
            ) : (
              <span
                className={`nt-setup-dot${row.done ? " done" : ""}`}
                data-dot={row.done ? "done" : "empty"}
                aria-label={row.done ? "Done" : "Not done"}
              />
            )}
            <div className="nt-setup-copy">
              <span className="nt-setup-title">{row.title}</span>
              <span className="tiny">{row.detail}</span>
              {note?.row === row.id && (
                <span className="tiny" role="status">
                  {note.text}
                </span>
              )}
            </div>
            {row.id === "mcp" && <span className="tiny nt-setup-coming">Coming</span>}
            {row.id === "tour" && row.verb && (
              <button className="ghost" onClick={() => setTouring(true)}>
                {row.verb}
              </button>
            )}
            {row.id === "calendar" && row.verb && (
              <button className="ghost" disabled={busy !== null} onClick={() => void connectCalendar()}>
                {busy === "calendar" ? "Waiting…" : row.verb}
              </button>
            )}
            {row.id === "import" && (
              <span className="nt-setup-actions">
                <button className="ghost" disabled={busy !== null} onClick={() => fileInput.current?.click()}>
                  {busy === "import" ? "Importing…" : "Import files"}
                </button>
                <button className="ghost" disabled={busy !== null} onClick={() => folderInput.current?.click()}>
                  Import folder
                </button>
                <input
                  ref={fileInput}
                  type="file"
                  multiple
                  accept=".txt,.srt"
                  hidden
                  onChange={(e) => void importFiles(e.currentTarget.files)}
                />
                <input
                  ref={folderInput}
                  type="file"
                  multiple
                  hidden
                  {...({ webkitdirectory: "" } as Record<string, string>)}
                  onChange={(e) => void importFiles(e.currentTarget.files)}
                />
              </span>
            )}
          </li>
        ))}
      </ul>
      {note?.row === "card" && <p className="tiny bad">{note.text}</p>}
      <footer className="nt-setup-foot">
        <span className="tiny">Reopen anytime from Help → {CARD_TITLE}.</span>
        {allDone(setup) && <span className="tiny">All set.</span>}
      </footer>
    </section>
  );
}
