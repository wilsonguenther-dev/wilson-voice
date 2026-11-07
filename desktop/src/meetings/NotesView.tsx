/**
 * #250 round 2 — the meeting notes in Open transcript, rendered: the title,
 * then Summary, Key points, Action items, Decisions and Open questions as real
 * sections, instead of the raw Markdown a `pre-wrap` paragraph used to show.
 * Parsing lives in `notes.ts`; this only draws what it returns, as React
 * elements (never `innerHTML`).
 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
