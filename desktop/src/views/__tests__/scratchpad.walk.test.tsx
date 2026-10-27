/**
 * yap-uiux-2026-10-16-SCRATCH — the two Scratchpad rows of the 2026-10-16 walk
 * (docs/loop/UIUX-WALK-2026-10-16.md), pinned against the shipping view.
 *
 *   S1 (daily)     one note listed, but its list item carried no row marker
 *                  (the detector read rows=0) and the editor opened blank.
 *   S2 (first-run) two "New note" buttons plus the placeholder copy
 *                  "Saved notes list here.".
 *
 * Same harness as viewStates.render.test.tsx: renderToStaticMarkup inside
 * AppContext, no DOM, no new dependency. Effects do not run under static
 * render, so the "open the most recent note" decision is pinned through
 * noteToOpen (the exact function the view's effect calls) and the smoke walk
 * proves the effect in a real browser.
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));

import { AppContext, type AppCtx } from "../../appShell";
import Scratchpad, { deleteScratchNote, noteToOpen } from "../Scratchpad";

type State = Record<string, unknown>;

function makeCtx(state: State): AppCtx {
  return new Proxy(state, {
    get(target, key: string) {
      if (key in target) return target[key];
      if (/^set[A-Z]/.test(key)) {
        const field = key[3].toLowerCase() + key.slice(4);
        return (v: unknown) => {
          target[field] = v;
        };
      }
      return () => undefined;
    },
  }) as unknown as AppCtx;
}

const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);

const count = (html: string, needle: string) => html.split(needle).length - 1;

const note = (id: string, title: string, updatedAt: string) => ({ id, title, body: "b-" + id, updatedAt });

const base = (over: State = {}): State => ({
  scratch: [], activeNoteId: null, noteTitle: "Scratchpad", noteBody: "",
  scratchComposing: false, booting: false, bootError: null, bootErrors: {}, settings: {},
  ...over,
});

describe("S2 — first run: one New note, no placeholder copy", () => {
  it("renders exactly one 'New note' button and no 'Saved notes list here.'", () => {
    const html = render(<Scratchpad />, base());
    expect(html).toContain('data-empty-state="scratchpad"');
    expect(count(html, ">New note</button>")).toBe(1);
    expect(html).not.toContain("Saved notes list here.");
  });
});

describe("S1 — daily: listed notes are rows, and the editor is contained", () => {
  const daily = () =>
    base({
      scratch: [note("a", "Older", "2026-10-15T09:00:00Z"), note("b", "Newest", "2026-10-16T12:00:00Z")],
      activeNoteId: "b", noteTitle: "Newest", noteBody: "b-b", scratchComposing: true,
    });

  it("marks every listed note data-row", () => {
    const html = render(<Scratchpad />, daily());
    expect(count(html, 'data-row="scratch-note"')).toBe(2);
    expect(count(html, ">New note</button>")).toBe(1);
  });

  it("pins Save / Copy / Paste / Delete in a footer under the text", () => {
    const html = render(<Scratchpad />, daily());
    const footer = html.slice(html.indexOf("data-scratch-footer"));
    for (const label of ["Save", "Copy", "Paste", "Delete"]) expect(footer).toContain(">" + label + "</button>");
    expect(html.indexOf("<textarea")).toBeLessThan(html.indexOf("data-scratch-footer"));
  });
});

describe("S1 — noteToOpen: the editor opens the most recent note", () => {
  const notes = [note("a", "A", "2026-10-15T09:00:00Z"), note("b", "B", "2026-10-16T12:00:00Z"), note("c", "C", "2026-10-14T00:00:00Z")];

  it("picks the most recently updated note when nothing is open", () => {
    expect(noteToOpen(notes, null, false)?.id).toBe("b");
  });
  it("never overrides an open note, a New note draft, or an empty list", () => {
    expect(noteToOpen(notes, "a", true)).toBeNull();
    expect(noteToOpen(notes, null, true)).toBeNull();
    expect(noteToOpen([], null, false)).toBeNull();
  });
});

/*
 * Fix round 1 (#267 BLOCKING) — deleting the open note must never re-open it.
 *
 * The view re-renders after every awaited IPC call, and its effect calls
 * noteToOpen on whatever state that render sees. So the probe below snapshots
 * the context at EVERY point React could render (after each invoke resolves,
 * before the next one does) and asks noteToOpen what the effect would open.
 * The deleted id must never come back, and the editor state must never be
 * "nothing open" while the deleted note is still listed.
 */
describe("S1 — deleting the open note never re-opens it", () => {
  type Ctx = State & { scratch: ReturnType<typeof note>[] };
  async function deleteWithProbe(start: Ctx, id: string, after: ReturnType<typeof note>[]) {
    const ctx = makeCtx(start) as unknown as Ctx & Parameters<typeof deleteScratchNote>[1];
    const seen: (string | null)[] = [];
    const renderPoint = () => {
      const st = start as Ctx;
      seen.push(noteToOpen(st.scratch, st.activeNoteId as string | null, st.scratchComposing as boolean)?.id ?? null);
    };
    const call = (async (cmd: string) => {
      renderPoint(); // what a render between the previous await and this one would open
      await Promise.resolve();
      if (cmd === "list_scratch") return after;
      return null;
    }) as unknown as Parameters<typeof deleteScratchNote>[2];
    await deleteScratchNote(id, ctx, call);
    renderPoint();
    return seen;
  }

  it("two notes: the editor moves to the remaining newest note, never back to the deleted one", async () => {
    const older = note("n-old", "Older note", "2026-10-15T09:00:00Z");
    const newest = note("n-new", "Newest note", "2026-10-16T12:00:00Z");
    const st = base({ scratch: [newest, older], activeNoteId: "n-new", noteTitle: "Newest note", noteBody: "b-n-new", scratchComposing: true }) as Ctx;
    const seen = await deleteWithProbe(st, "n-new", [older]);
    expect(seen).not.toContain("n-new");
    expect(seen[seen.length - 1]).toBe("n-old");
    expect(st.scratch.map((n) => n.id)).toEqual(["n-old"]);
  });

  it("last note: the view lands on the empty state, not on the deleted note", async () => {
    const only = note("n-only", "Only note", "2026-10-16T12:00:00Z");
    const st = base({ scratch: [only], activeNoteId: "n-only", noteTitle: "Only note", noteBody: "b-n-only", scratchComposing: true }) as Ctx;
    const seen = await deleteWithProbe(st, "n-only", []);
    expect(seen).not.toContain("n-only");
    expect(st.activeNoteId).toBeNull();
    const html = render(<Scratchpad />, st);
    expect(html).toContain('data-empty-state="scratchpad"');
    expect(html).not.toContain(">Delete</button>");
  });

  it("deleting a note that is not open leaves the open note alone", async () => {
    const a = note("a", "A", "2026-10-15T09:00:00Z");
    const b = note("b", "B", "2026-10-16T12:00:00Z");
    const st = base({ scratch: [b, a], activeNoteId: "b", noteTitle: "B", noteBody: "b-b", scratchComposing: true }) as Ctx;
    const seen = await deleteWithProbe(st, "a", [b]);
    expect(seen.every((v) => v === null)).toBe(true);
    expect(st.activeNoteId).toBe("b");
    expect(st.noteTitle).toBe("B");
  });
});
