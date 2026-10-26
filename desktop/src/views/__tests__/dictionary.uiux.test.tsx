/**
 * yap-uiux-2026-10-16-DICT — the two walk rows on Dictionary, rendered.
 *
 * D1: the add card no longer carries engine jargon ("After ASR… fed to the
 *     recognizer before it decodes"); one plain line, the star explains itself
 *     on its tooltip.
 * D2: the empty dictionary shows ONE add action (the form's "Add"), not a
 *     second "Add a word" button in the empty card.
 * Layout: controls (add, search) render before the list, the list lives in its
 *     own scroll region and a footer is pinned under it.
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../../appShell";
import type { DictEntry } from "../../appTypes";
import Dictionary, { DICT_HELP, STAR_OFF_TIP, STAR_ON_TIP, dictionarySummary, filterDictionary } from "../Dictionary";

type State = Record<string, unknown>;
const ctx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={ctx(state)}>{node}</AppContext.Provider>);

const words: DictEntry[] = [
  { id: "d1", term: "Kubernetes", preferred: null, hits: 4, starred: true } as DictEntry,
  { id: "d2", term: "drivia", preferred: "Drivia", hits: 1, starred: false } as DictEntry,
];
const base = (over: State = {}): State => ({
  booting: false, bootError: null, bootErrors: {}, settings: { onboarded: true },
  dictionary: [], candidates: [], newTerm: "", newPreferred: "", editingTermId: null, editTerm: "", editPreferred: "",
  ...over,
});
const buttons = (html: string) => [...html.matchAll(/<button[^>]*>([^<]*)<\/button>/g)].map((m) => m[1].trim());

describe("Dictionary walk row D1: no engine jargon above the form", () => {
  it("first-run and daily renders say one plain line, never ASR / recognizer / decodes", () => {
    for (const html of [render(<Dictionary />, base()), render(<Dictionary />, base({ dictionary: words }))]) {
      expect(html).toContain(DICT_HELP);
      expect(html).not.toMatch(/\bASR\b|recognizer|decodes|tokens/i);
    }
  });
  it("the star explains itself on its tooltip", () => {
    const html = render(<Dictionary />, base({ dictionary: words }));
    expect(html).toContain(`title="${STAR_ON_TIP}"`);
    expect(html).toContain(`title="${STAR_OFF_TIP}"`);
  });
});

describe("Dictionary walk row D2: one add action on an empty dictionary", () => {
  it("renders the empty marker and exactly one button, the form's Add", () => {
    const html = render(<Dictionary />, base());
    expect(html).toContain('data-empty-state="dictionary"');
    expect(buttons(html)).toEqual(["Add"]);
    expect(html).not.toContain("Add a word");
  });
});

describe("Dictionary layout: controls first, contained list, pinned footer", () => {
  it("search renders before the list region, the footer after it", () => {
    const html = render(<Dictionary />, base({ dictionary: words }));
    const add = html.indexOf('aria-label="Word"');
    const search = html.indexOf("data-dict-search");
    const list = html.indexOf("data-dict-list");
    const foot = html.indexOf("data-dict-footer");
    expect(add).toBeGreaterThan(-1);
    expect(search).toBeGreaterThan(add);
    expect(list).toBeGreaterThan(search);
    expect(foot).toBeGreaterThan(list);
    expect(html).toMatch(/data-dict-list="true" style="[^"]*overflow-y:auto/);
    expect(html).toContain("2 words · 1 starred");
  });
  it("prose comes last: the one plain line sits in the pinned footer, after the list, for both personas", () => {
    for (const html of [render(<Dictionary />, base()), render(<Dictionary />, base({ dictionary: words }))]) {
      const list = html.indexOf("data-dict-list");
      const foot = html.indexOf("data-dict-footer");
      expect(list).toBeGreaterThan(-1);
      expect(foot).toBeGreaterThan(list);
      expect(html.indexOf(DICT_HELP)).toBeGreaterThan(foot);
    }
  });
  it("suggestions scroll inside the one list region instead of eating fixed height above it", () => {
    const cand = [{ id: "c1", term: "Yappy", wrong: "happy", useCount: 3 }];
    const html = render(<Dictionary />, base({ dictionary: words, candidates: cand }));
    const list = html.indexOf("data-dict-list");
    const noticed = html.indexOf("Yap noticed");
    expect(noticed).toBeGreaterThan(list);
    expect(html.indexOf("data-dict-footer")).toBeGreaterThan(noticed);
    expect(html).not.toMatch(/class="cand-list" style=/);
  });
  it("the add row stays one line at every width (overrides the narrow-width stack)", () => {
    const html = render(<Dictionary />, base());
    expect(html).toMatch(/class="dict-add" style="grid-template-columns:minmax\(0, 1fr\) minmax\(0, 1fr\) auto"/);
  });
  it("filterDictionary matches the word or its spelling, case-insensitively", () => {
    expect(filterDictionary(words, "").length).toBe(2);
    expect(filterDictionary(words, "KUBER").map((d) => d.id)).toEqual(["d1"]);
    expect(filterDictionary(words, "Drivia").map((d) => d.id)).toEqual(["d2"]);
    expect(filterDictionary(words, "zzz")).toEqual([]);
  });
  it("dictionarySummary counts words, stars and what a search shows", () => {
    expect(dictionarySummary(words, 2, "")).toBe("2 words · 1 starred");
    expect(dictionarySummary(words.slice(1), 1, "")).toBe("1 word");
    expect(dictionarySummary(words, 1, "kub")).toBe("Showing 1 of 2 words · 1 starred");
  });
});
