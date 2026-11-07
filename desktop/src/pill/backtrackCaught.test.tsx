/**
 * WF-06 — the pill's "Backtrack caught" toast. Renders the SHIPPING view (no
 * DOM: `renderToStaticMarkup`, like languageOffer.test.tsx) and presses its
 * buttons by walking the element tree, since the view has no hooks. WHEN the
 * toast is raised (real corrections only, never filler-only cleanup) is Rust's
 * (`dictation::tests::clean_backtrack_reports_what_it_removed`,
 * `tests/backtrack_caught.rs`).
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactElement, ReactNode } from "react";
import { BacktrackCaughtView, undoBacktrack } from "./BacktrackCaughtCard";
import {
  BACKTRACK_CAUGHT_EVENT,
  BACKTRACK_CAUGHT_TTL_MS,
  BACKTRACK_SNIPPET_MAX,
  BACKTRACK_UNDO_COMMAND,
  SAMPLE_BACKTRACK_CAUGHT,
  backtrackCaughtLines,
  backtrackUndoArgs,
  forcedBacktrackCaught,
  isBacktrackCaught,
  snippet,
  type BacktrackCaught,
} from "./backtrackCaught";
import { matrixBacktrackCaughtScene } from "./forced";
import { PILL_MESSAGES, pillMessageAllowed } from "./live";

const CAUGHT: BacktrackCaught = {
  id: "take-1",
  dropped: "2",
  kept: "3",
  raw: "coffee at 2 actually 3",
};

function walk(node: ReactNode, out: ReactElement[] = []): ReactElement[] {
  if (Array.isArray(node)) node.forEach((n) => walk(n, out));
  else if (node && typeof node === "object" && "props" in node) {
    const el = node as ReactElement<{ children?: ReactNode }>;
    out.push(el);
    walk(el.props.children, out);
  }
  return out;
}

function buttons(el: ReactElement): Array<ReactElement<{ className?: string; onClick?: () => void; disabled?: boolean }>> {
  return walk(el).filter((e) => e.type === "button") as Array<
    ReactElement<{ className?: string; onClick?: () => void; disabled?: boolean }>
  >;
}

describe("WF-06 backtrack caught — model", () => {
  it("names the event and the undo command the Rust side uses", () => {
    expect(BACKTRACK_CAUGHT_EVENT).toBe("backtrack_caught");
    expect(BACKTRACK_UNDO_COMMAND).toBe("paste_entry");
    expect(BACKTRACK_CAUGHT_TTL_MS).toBeGreaterThan(0);
  });

  it("accepts only a complete payload", () => {
    expect(isBacktrackCaught(CAUGHT)).toBe(true);
    expect(isBacktrackCaught(null)).toBe(false);
    expect(isBacktrackCaught("backtrack")).toBe(false);
    expect(isBacktrackCaught({ ...CAUGHT, id: 7 })).toBe(false);
    expect(isBacktrackCaught({ ...CAUGHT, dropped: "  " })).toBe(false);
    expect(isBacktrackCaught({ ...CAUGHT, kept: "" })).toBe(false);
    expect(isBacktrackCaught({ ...CAUGHT, raw: undefined })).toBe(false);
  });

  it("says what was dropped and what was kept", () => {
    const lines = backtrackCaughtLines(CAUGHT);
    expect(lines.label).toBe("Backtrack caught");
    expect(lines.headline).toBe("Dropped ‘2’, kept ‘3’");
    expect(lines.undo).toBe("Undo");
    expect(lines.dismiss).toBe("Dismiss");
    expect(lines.undoFailed).toMatch(/Try Undo again/);
  });

  it("shortens a long retracted clause at a word boundary", () => {
    expect(snippet("meet at   noon")).toBe("meet at noon");
    const long = "book the blue one with the extra long description";
    const s = snippet(long);
    expect(s.length).toBeLessThanOrEqual(BACKTRACK_SNIPPET_MAX);
    expect(s.endsWith("…")).toBe(true);
    expect(long.startsWith(s.slice(0, -1))).toBe(true);
    // No space in the back half: a hard cut, still with the ellipsis.
    expect(snippet("abcdefghijklmnop", 8)).toBe("abcdefg…");
  });

  it("Undo pastes the raw take and nothing else", async () => {
    expect(backtrackUndoArgs(CAUGHT)).toEqual({ text: "coffee at 2 actually 3" });
    const calls: Array<[string, Record<string, unknown>]> = [];
    await undoBacktrack(CAUGHT, async (cmd, args) => {
      calls.push([cmd, args]);
    });
    expect(calls).toEqual([["paste_entry", { text: "coffee at 2 actually 3" }]]);
  });

  it("the forced matrix scene is opt-in", () => {
    expect(forcedBacktrackCaught("?phase=idle&backtrack=1")).toEqual(SAMPLE_BACKTRACK_CAUGHT);
    expect(forcedBacktrackCaught("?phase=idle")).toBeNull();
    const row = matrixBacktrackCaughtScene();
    expect(row.query).toBe("backtrack=1");
    expect(row.id).toBe("backtrack-caught");
  });

  it("is a status message the WF-17 quiet switches never mute", () => {
    expect(PILL_MESSAGES["backtrack-caught"]).toBe("status");
    expect(pillMessageAllowed("backtrack-caught", new Set(["tip", "news", "milestone"]))).toBe(true);
  });
});

describe("WF-06 backtrack caught — view", () => {
  it("renders the receipt with Undo and Dismiss", () => {
    const html = renderToStaticMarkup(
      <BacktrackCaughtView caught={CAUGHT} onUndo={() => {}} onDismiss={() => {}} />,
    );
    expect(html).toContain("Backtrack caught");
    expect(html).toContain("Dropped ‘2’, kept ‘3’");
    expect(html).toContain('data-backtrack-caught="take-1"');
    expect(html).toContain('role="status"');
    expect(html).not.toContain('role="alert"');
  });

  it("wires Undo and Dismiss to their handlers", () => {
    const seen: string[] = [];
    const el = BacktrackCaughtView({
      caught: CAUGHT,
      onUndo: () => seen.push("undo"),
      onDismiss: () => seen.push("dismiss"),
    });
    const [undo, dismiss] = buttons(el);
    expect(undo.props.className).toBe("backtrack-caught-undo");
    undo.props.onClick?.();
    dismiss.props.onClick?.();
    expect(seen).toEqual(["undo", "dismiss"]);
  });

  it("shows an Undo failure as one sentence and disables while busy", () => {
    const html = renderToStaticMarkup(
      <BacktrackCaughtView
        caught={CAUGHT}
        error={backtrackCaughtLines(CAUGHT).undoFailed}
        busy
        onUndo={() => {}}
        onDismiss={() => {}}
      />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain("Couldn’t paste the original.");
    const el = BacktrackCaughtView({ caught: CAUGHT, busy: true, onUndo: () => {}, onDismiss: () => {} });
    expect(buttons(el).every((b) => b.props.disabled)).toBe(true);
  });
});
