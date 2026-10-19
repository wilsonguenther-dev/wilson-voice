/**
 * #250 round 2 — the meeting notes in Open transcript, rendered: the title,
 * then Summary, Key points, Action items, Decisions and Open questions as real
 * sections, instead of the raw Markdown a `pre-wrap` paragraph used to show.
 * Parsing lives in `notes.ts`; this only draws what it returns, as React
 * elements (never `innerHTML`).
 */
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

export default function NotesView({ markdown }: { markdown: string }) {
  const notes = parseNotes(markdown);
  return (
    <article className="meeting-notes" data-testid="meeting-notes">
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
