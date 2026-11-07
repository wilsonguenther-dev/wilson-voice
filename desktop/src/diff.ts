/**
 * Y4-G — the word-level diff behind "see what changed".
 *
 * Formatting that silently rewrites what you said is only acceptable if you can
 * SEE it and revert it. Yap already stores `raw_text` for every take (YV10/51);
 * this module is the other half — turning (raw, formatted) into a token list a
 * panel can render.
 *
 * NO DEPENDENCY, deliberately. The CSP forbids CDN loads, the bundle should not
 * grow for a sixty-line algorithm, and a diff library is a supply-chain surface
 * for something this app can compute exactly. The algorithm is Myers' greedy
 * O((N+M)D) edit script — the right shape here because D (the number of edits a
 * cleanup pass makes) is small for a real take, so it finishes in roughly linear
 * time on the cases that actually happen.
 *
 * It is also HARD-BOUNDED. `maxD` caps the search, so a pathological pair (a
 * 2,000-word take rewritten end to end, two unrelated texts) can never turn into
 * the quadratic blow-up a naive LCS table would be. Past the cap the diff
 * degrades to the coarse-but-true answer — "all of this went, all of that
 * arrived" — instead of hanging the History pane.
 */

/** What happened to one token going from `before` to `after`. */
export type DiffOp = "equal" | "added" | "removed";

/** One run of text with a single verdict. Adjacent same-op runs are merged. */
export interface DiffToken {
  op: DiffOp;
  text: string;
}

/**
 * Default edit-distance ceiling. Work is bounded by ~maxD² token comparisons,
 * so 3,000 keeps the absolute worst case in the low tens of milliseconds while
 * being far above the edit distance of any real cleanup pass.
 */
export const DEFAULT_MAX_D = 3000;

/**
 * Split into words and whitespace RUNS as separate tokens.
 *
 * Whitespace is tokenized rather than discarded because half of what the
 * formatting stages do is whitespace: paragraph breaks (R12), the email shape
 * (R13), list reflow. A diff that dropped it would show a paragraph break as
 * "nothing changed", which is exactly the silent rewrite this panel exists to
 * expose.
 */
export function tokenize(text: string): string[] {
  return text.match(/\s+|\S+/g) ?? [];
}

/** Merge adjacent runs that share a verdict, and drop empty ones. */
function coalesce(tokens: DiffToken[]): DiffToken[] {
  const out: DiffToken[] = [];
  for (const t of tokens) {
    if (t.text === "") continue;
    const last = out[out.length - 1];
    if (last && last.op === t.op) last.text += t.text;
    else out.push({ op: t.op, text: t.text });
  }
  return out;
}

/**
 * Myers' greedy edit script over token arrays, or `null` when the edit distance
 * exceeds `maxD` (the caller then falls back to the coarse answer).
 */
function myers(a: string[], b: string[], maxD: number): DiffToken[] | null {
  const n = a.length;
  const m = b.length;
  const max = Math.min(Math.max(maxD, 0), n + m);
  // +3 so `offset + k ± 1` is always in range at the k = ±d extremes.
  const offset = max + 1;
  const size = 2 * max + 3;
  let v = new Int32Array(size);
  const trace: Int32Array[] = [];

  for (let d = 0; d <= max; d++) {
    // Snapshot BEFORE round d: backtracking at step d reads the frontier the
    // round started from.
    trace.push(v.slice());
    for (let k = -d; k <= d; k += 2) {
      let x: number;
      if (k === -d || (k !== d && v[offset + k - 1] < v[offset + k + 1])) {
        x = v[offset + k + 1];
      } else {
        x = v[offset + k - 1] + 1;
      }
      let y = x - k;
      while (x < n && y < m && a[x] === b[y]) {
        x++;
        y++;
      }
      v[offset + k] = x;
      if (x >= n && y >= m) return backtrack(trace, a, b, d, offset);
    }
    v = v.slice();
  }
  return null;
}

/** Walk the saved frontiers backwards into a token list. */
function backtrack(
  trace: Int32Array[],
  a: string[],
  b: string[],
  d: number,
  offset: number,
): DiffToken[] {
  const out: DiffToken[] = [];
  let x = a.length;
  let y = b.length;

  for (let dd = d; dd > 0; dd--) {
    const v = trace[dd];
    const k = x - y;
    const prevK =
      k === -dd || (k !== dd && v[offset + k - 1] < v[offset + k + 1])
        ? k + 1
        : k - 1;
    const prevX = v[offset + prevK];
    const prevY = prevX - prevK;
    while (x > prevX && y > prevY) {
      x--;
      y--;
      out.push({ op: "equal", text: a[x] });
    }
    if (x === prevX) {
      y--;
      out.push({ op: "added", text: b[y] });
    } else {
      x--;
      out.push({ op: "removed", text: a[x] });
    }
  }
  while (x > 0) {
    x--;
    y--;
    out.push({ op: "equal", text: a[x] });
  }
  out.reverse();
  return out;
}

/**
 * Word-level diff of `before` → `after`.
 *
 * Identical inputs return a single `equal` run (or an empty list for two empty
 * strings) — never a spurious add/remove pair.
 */
export function diffWords(
  before: string,
  after: string,
  opts: { maxD?: number } = {},
): DiffToken[] {
  const maxD = opts.maxD ?? DEFAULT_MAX_D;
  if (before === after) {
    return before === "" ? [] : [{ op: "equal", text: before }];
  }
  const a = tokenize(before);
  const b = tokenize(after);

  // Trim the common head and tail first. A cleanup pass usually touches the
  // middle of a take, so this is what keeps D small — and it costs O(n).
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail] === b[b.length - 1 - tail]
  ) {
    tail++;
  }
  const aMid = a.slice(head, a.length - tail);
  const bMid = b.slice(head, b.length - tail);

  const middle =
    myers(aMid, bMid, maxD) ??
    // Past the ceiling: still TRUE, just coarse. Never a hang.
    ([
      { op: "removed", text: aMid.join("") },
      { op: "added", text: bMid.join("") },
    ] as DiffToken[]);

  return coalesce([
    { op: "equal", text: a.slice(0, head).join("") },
    ...middle,
    { op: "equal", text: tail > 0 ? a.slice(a.length - tail).join("") : "" },
  ]);
}

/** Counts for the panel's one-line summary. Words only — whitespace excluded. */
export function diffSummary(tokens: DiffToken[]): {
  added: number;
  removed: number;
  unchanged: number;
} {
  const words = (t: DiffToken) => (t.text.match(/\S+/g) ?? []).length;
  let added = 0;
  let removed = 0;
  let unchanged = 0;
  for (const t of tokens) {
    if (t.op === "added") added += words(t);
    else if (t.op === "removed") removed += words(t);
    else unchanged += words(t);
  }
  return { added, removed, unchanged };
}

/** True when the formatting pipeline left the take byte-identical. */
export function isUnchanged(tokens: DiffToken[]): boolean {
  return tokens.every((t) => t.op === "equal");
}

// ── yap-1107-polish-diff-readable — phrase-level runs ───────────────────────
//
// Wilson, 2026-11-07 17:22Z: the word-level edit script above is TRUE but not
// READABLE. Every one-word substitution became a removed word glued to an added
// word ("myyour", "calendarcalendar."), and a sentence with five small edits
// alternated strike / highlight / strike word by word until neither version
// could be read. Myers stays the engine (the header's reasons stand); this is a
// post-pass over its output that groups the script into HUNKS:
//
//  * two changes separated by fewer than MIN_EQUAL_WORDS unchanged words are one
//    hunk, and the short unchanged words between them become part of BOTH the
//    removed and the added run;
//  * every hunk is emitted as ONE removed run followed by ONE added run, never
//    alternating single words. Tokens keep their whitespace, so rebuilding
//    either side still reproduces the input byte for byte.
//
// When the hunks cover more than BLOCKS_CHANGED_SHARE of the words the inline
// view is mostly strikes, and two plain paragraphs (Before / After) are shorter
// to read than the interleaving. Under that share, each hunk sits between at
// least three unchanged words the reader can anchor on. The approved design
// ("Show opens the before and after", pill.html, Fix-up notices) is the governing
// idea: the blocks are the fallback AND the "Show as before/after" toggle.

/** Unchanged words a reader needs between two edits to see them as separate. */
export const MIN_EQUAL_WORDS = 3;

/**
 * Past this share of a side's words inside hunks, the card shows Before / After
 * as two paragraphs. Half: beyond it the inline view has more struck text than
 * kept text, so the reader is reconstructing both versions in their head.
 */
export const BLOCKS_CHANGED_SHARE = 0.5;

/** How the card should lay a diff out by default. */
export type DiffLayout = "inline" | "blocks";

/** Everything a diff panel needs, computed once. */
export interface ReadableDiff {
  /** The exact word-level script (`diffWords`): what the summary counts. */
  exact: DiffToken[];
  /** The same edits grouped into hunks: what the inline view renders. */
  runs: DiffToken[];
  /** Number of hunks (one removed run and/or one added run each). */
  hunks: number;
  /** Larger of the two sides' share of words inside hunks, 0..1. */
  changedShare: number;
  /** The default layout: blocks when the hunks are too dense to read inline. */
  layout: DiffLayout;
}

function wordCount(text: string): number {
  return (text.match(/\S+/g) ?? []).length;
}

/**
 * Group an edit script (coalesced or not) into hunks. Equal stretches with fewer
 * than `minEqualWords` words BETWEEN two changes are folded into both sides of
 * the hunk; leading and trailing equal text is never folded.
 */
export function groupHunks(script: DiffToken[], minEqualWords = MIN_EQUAL_WORDS): DiffToken[] {
  const out: DiffToken[] = [];
  const n = script.length;
  let i = 0;
  while (i < n) {
    if (script[i].op === "equal") {
      out.push({ op: "equal", text: script[i].text });
      i++;
      continue;
    }
    let removed = "";
    let added = "";
    let j = i;
    while (j < n) {
      const t = script[j];
      if (t.op === "removed") {
        removed += t.text;
        j++;
        continue;
      }
      if (t.op === "added") {
        added += t.text;
        j++;
        continue;
      }
      // An equal stretch: fold it in only if another change follows it and it
      // is too short to read as context.
      let k = j;
      let text = "";
      while (k < n && script[k].op === "equal") {
        text += script[k].text;
        k++;
      }
      if (k < n && wordCount(text) < minEqualWords) {
        removed += text;
        added += text;
        j = k;
        continue;
      }
      break;
    }
    out.push({ op: "removed", text: removed }, { op: "added", text: added });
    i = j;
  }
  return coalesce(out);
}

/** Phrase-level diff: `diffWords` grouped into hunks. Same inputs, same truth. */
export function diffPhrases(
  before: string,
  after: string,
  opts: { maxD?: number; minEqualWords?: number } = {},
): DiffToken[] {
  return groupHunks(diffWords(before, after, opts), opts.minEqualWords);
}

/** How many separate changes a run list shows (a removed+added pair counts once). */
export function changeRuns(tokens: DiffToken[]): number {
  let runs = 0;
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i];
    if (t.op === "equal") continue;
    // An added run right after a removed run is the same hunk.
    if (t.op === "added" && i > 0 && tokens[i - 1].op === "removed") continue;
    runs++;
  }
  return runs;
}

/** The diff a panel renders: exact counts, hunked runs, and the default layout. */
export function readableDiff(
  before: string,
  after: string,
  opts: { maxD?: number; minEqualWords?: number } = {},
): ReadableDiff {
  const exact = diffWords(before, after, opts);
  const runs = groupHunks(exact, opts.minEqualWords);
  const hunks = changeRuns(runs);
  let inBefore = 0;
  let inAfter = 0;
  for (const t of runs) {
    if (t.op === "removed") inBefore += wordCount(t.text);
    else if (t.op === "added") inAfter += wordCount(t.text);
  }
  // A side with no words has no words in a hunk either (the runs are built
  // from that side's tokens), so an empty side is 0 percent changed.
  const share = (inHunks: number, total: number) => (total > 0 ? inHunks / total : 0);
  const changedShare = Math.max(share(inBefore, wordCount(before)), share(inAfter, wordCount(after)));
  const layout: DiffLayout = hunks > 0 && changedShare > BLOCKS_CHANGED_SHARE ? "blocks" : "inline";
  return { exact, runs, hunks, changedShare, layout };
}

/**
 * One structured telemetry line for the diff panel, `diff_view <event> k=v`.
 * Counts and layout names only — never the take's text.
 */
export function diffLogLine(event: string, fields: Record<string, string | number | boolean> = {}): string {
  const kv = Object.entries(fields).map(([k, v]) => `${k}=${String(v).replace(/\s+/g, "_")}`);
  return ["diff_view", event, ...kv].join(" ");
}
