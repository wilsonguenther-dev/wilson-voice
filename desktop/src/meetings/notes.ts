/**
 * #250 round 2 — the stored meeting notes (`meetings.summary`), parsed for
 * the Open transcript view instead of being drawn as raw Markdown.
 *
 * The notes are Markdown the app WRITES itself (`summarize::render_notes`), so
 * this reads exactly that subset and nothing more: one `## title`, `### `
 * sections, `- ` list items, paragraphs, `**bold**` and a whole-line
 * `_italic_` note. No HTML is ever produced from the text — the view builds
 * React elements, so a transcript line that happens to contain `<script>` is
 * drawn as the characters it is. An old-layout summary (no headings at all)
 * comes back as one untitled section of paragraphs, which renders the way it
 * always read.
 */

export type NotesBlock =
  | { kind: "paragraph"; text: string }
  | { kind: "list"; items: string[] }
  | { kind: "note"; text: string };

export type NotesSection = { heading: string | null; blocks: NotesBlock[] };

export type ParsedNotes = { title: string | null; sections: NotesSection[] };

/** `**bold**` runs inside one line, split for rendering. */
export type InlineRun = { text: string; bold: boolean };

export function parseNotes(markdown: string): ParsedNotes {
  let title: string | null = null;
  const sections: NotesSection[] = [];
  let current: NotesSection = { heading: null, blocks: [] };
  let para: string[] = [];
  let list: string[] | null = null;

  const flush = () => {
    if (para.length) {
      const text = para.join(" ").trim();
      const note = /^_(.+)_$/.exec(text);
      current.blocks.push(note ? { kind: "note", text: note[1] } : { kind: "paragraph", text });
      para = [];
    }
    if (list) {
      current.blocks.push({ kind: "list", items: list });
      list = null;
    }
  };
  const startSection = (heading: string | null) => {
    flush();
    if (current.heading !== null || current.blocks.length) sections.push(current);
    current = { heading, blocks: [] };
  };

  for (const raw of markdown.replace(/\r\n/g, "\n").split("\n")) {
    const line = raw.trimEnd();
    if (/^#\s/.test(line) || /^##\s/.test(line)) {
      // The notes carry one `##` title; a stray `#` is treated the same way.
      flush();
      const text = line.replace(/^#+\s+/, "").trim();
      if (title === null) title = text;
      else startSection(text);
      continue;
    }
    if (/^#{3,6}\s/.test(line)) {
      startSection(line.replace(/^#+\s+/, "").trim());
      continue;
    }
    const item = /^\s*[-*]\s+(.*)$/.exec(line);
    if (item) {
      if (para.length) flush();
      (list ??= []).push(item[1].trim());
      continue;
    }
    if (line.trim() === "") {
      flush();
      continue;
    }
    if (list) flush();
    para.push(line.trim());
  }
  flush();
  if (current.heading !== null || current.blocks.length) sections.push(current);
  return { title, sections };
}

export function inlineRuns(text: string): InlineRun[] {
  const runs: InlineRun[] = [];
  const re = /\*\*(.+?)\*\*/g;
  let at = 0;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    if (m.index > at) runs.push({ text: text.slice(at, m.index), bold: false });
    runs.push({ text: m[1], bold: true });
    at = m.index + m[0].length;
  }
  if (at < text.length) runs.push({ text: text.slice(at), bold: false });
  return runs;
}
