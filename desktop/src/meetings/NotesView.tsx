/**
 * #250 round 2 — the meeting notes in Open transcript, rendered: the title,
 * then Summary, Key points, Action items, Decisions and Open questions as real
 * sections, instead of the raw Markdown a `pre-wrap` paragraph used to show.
 * Parsing lives in `notes.ts`; this only draws what it returns, as React
 * elements (never `innerHTML`).
 */
import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { notesRouteLine } from "../pill/cloudChip";
import { inlineRuns, parseNotes, type NotesBlock } from "./notes";

function Inline({ text }: { text: string }) {
  return (
    <>
      {inlineRuns(text).map((r, i) => (r.bold ? <strong key={i}>{r.text}</strong> : r.text))}
    </>
  );
}

function Block({ block }: { block: NotesBlock }) {
  switch (block.kind) {
    case "list":
      return (
        <ul>
          {block.items.map((item, i) => (
            <li key={i}>
              <Inline text={item} />
            </li>
          ))}
        </ul>
      );
    case "note":
      return (
        <p className="muted meeting-notes-note">
          <em>{block.text}</em>
        </p>
      );
    default:
      return (
        <p>
          <Inline text={block.text} />
        </p>
      );
  }
}

/**
 * yap-1106-byom-cloud-fast-path — the notes header chip: "Notes written in
 * the cloud" / "on this Mac", from the route the notes job stored. Nothing
 * for notes that predate the route (the line is simply absent).
 */
export function NotesRouteLine({ route }: { route: string | null | undefined }) {
  const line = notesRouteLine(route);
  if (!line) return null;
  return (
    <p className="meeting-notes-route" data-route={route} role="status">
      {line}
    </p>
  );
}

function StoredNotesRoute({ meetingId }: { meetingId: string }) {
  const [route, setRoute] = useState<string | null>(null);
  useEffect(() => {
    let dead = false;
    invoke<string | null>("meeting_notes_route", { id: meetingId })
      .then((r) => { if (!dead) setRoute(r); })
      .catch(() => { /* no line: the backend logged it */ });
    return () => { dead = true; };
  }, [meetingId]);
  return <NotesRouteLine route={route} />;
}

export default function NotesView({ markdown, meetingId, route }: { markdown: string; meetingId?: string; route?: string | null }) {
  const notes = parseNotes(markdown);
  return (
    <article className="meeting-notes" data-testid="meeting-notes">
      {route !== undefined ? <NotesRouteLine route={route} /> : meetingId ? <StoredNotesRoute meetingId={meetingId} /> : null}
      {notes.title && <h3 className="meeting-notes-heading">{notes.title}</h3>}
      {notes.sections.map((section, i) => (
        <section key={i} className="meeting-notes-section">
          {section.heading && <h4>{section.heading}</h4>}
          {section.blocks.map((block, j) => (
            <Block key={j} block={block} />
          ))}
        </section>
      ))}
    </article>
  );
}

/**
 * yap-1106-sp-08 — notes streamed from the hosted route while they are being
 * written. The backend emits `meeting_notes_delta` with `{id, progress}` as
 * each parallel map pass of a long meeting finishes, `{id, delta}` for every
 * piece of the final notes, and `{id, done, route}` at the end. A `done` that
 * is not the cloud route means the bundled model takes over, so anything
 * streamed so far is dropped rather than shown as notes.
 */
export const NOTES_DELTA_EVENT = "meeting_notes_delta";

export type NotesDeltaPayload = {
  id: string;
  delta?: string;
  progress?: { done: number; of: number };
  done?: boolean;
  route?: string;
};

export type StreamedNotes = { text: string; done: number; of: number; finished: boolean };

export const NO_STREAM: StreamedNotes = { text: "", done: 0, of: 0, finished: false };

export function reduceNotesDelta(
  state: StreamedNotes,
  payload: NotesDeltaPayload | null | undefined,
  meetingId: string,
): StreamedNotes {
  if (!payload || payload.id !== meetingId) return state;
  if (payload.done) return payload.route === "cloud" ? { ...state, finished: true } : NO_STREAM;
  if (payload.progress) {
    return { ...state, done: payload.progress.done, of: payload.progress.of };
  }
  if (typeof payload.delta === "string") return { ...state, text: state.text + payload.delta };
  return state;
}

/** The one status sentence above streamed notes; `null` once they are complete. */
export function streamStatusLine(s: StreamedNotes): string | null {
  if (s.finished) return null;
  if (s.text) return "Writing notes…";
  if (s.of > 1) return `Reading the meeting: part ${s.done} of ${s.of} done`;
  return null;
}

/** The streamed view: progress, then the notes as they arrive. */
export function StreamedNotesView({ stream }: { stream: StreamedNotes }) {
  const status = streamStatusLine(stream);
  return (
    <div className="meeting-notes-streaming" aria-busy={!stream.finished}>
      {status && (
        <p className="meeting-notes-route" role="status" aria-live="polite">
          {status}
        </p>
      )}
      {stream.text && <NotesView markdown={stream.text} route={null} />}
    </div>
  );
}

/**
 * Wraps the "No notes yet" state: until a stream for this meeting starts it
 * renders `children` unchanged; once one does, the notes appear as they are
 * written. The stored notes replace this when the job saves them.
 */
export function StreamingNotes({ meetingId, children }: { meetingId: string; children: ReactNode }) {
  const [stream, setStream] = useState<StreamedNotes>(NO_STREAM);
  useEffect(() => {
    setStream(NO_STREAM);
    let unlisten: (() => void) | undefined;
    let gone = false;
    listen<NotesDeltaPayload>(NOTES_DELTA_EVENT, (e) =>
      setStream((s) => reduceNotesDelta(s, e.payload, meetingId)),
    )
      .then((u) => (gone ? u() : (unlisten = u)))
      // No listener: the empty state stays and the stored notes still arrive.
      .catch(() => setStream(NO_STREAM));
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [meetingId]);
  if (!stream.text && !streamStatusLine(stream)) return <>{children}</>;
  return <StreamedNotesView stream={stream} />;
}
