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
