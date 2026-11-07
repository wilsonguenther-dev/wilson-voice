/**
 * yap-1107-polish-diff-readable — the shared DiffView (polish card + History)
 * renders phrase runs, Before/After blocks when changes are dense, and the
 * "Show as before/after" toggle. No DOM here (`renderToStaticMarkup`, like
 * pill/polishReveal.test.tsx); pressing the toggle in the real window is the
 * integrator's capture step.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactElement, ReactNode } from "react";
import DiffView, { SHOW_BEFORE_AFTER, needsPairGap } from "./DiffView";
import { PolishRevealView } from "./pill/PolishRevealCard";
import { POLISH_REVEAL_DIFF_SAMPLES, SAMPLE_POLISH_REVEAL, forcedPolishReveal } from "./pill/polishReveal";

const FIX = POLISH_REVEAL_DIFF_SAMPLES.fixture;

function walk(node: ReactNode, out: ReactElement[] = []): ReactElement[] {
  if (Array.isArray(node)) node.forEach((n) => walk(n, out));
  else if (node && typeof node === "object" && "props" in node) {
    const el = node as ReactElement<{ children?: ReactNode }>;
    out.push(el);
    walk(el.props.children, out);
  }
  return out;
}

const count = (html: string, needle: string) => html.split(needle).length - 1;

describe("DiffView — phrase runs inline", () => {
  it("renders Wilson's fixture as 3 hunks, inline, with no glued words", () => {
    const html = renderToStaticMarkup(<DiffView raw={FIX.raw} formatted={FIX.text} />);
    expect(html).toContain('data-layout="inline"');
    expect(count(html, 'class="diff-removed"')).toBe(3);
    expect(count(html, "diff-added")).toBe(3);
    // "my" then "your" carry the gap class; "Go ahead and add " already ends in a space.
    expect(count(html, "diff-pair-gap")).toBe(1);
    expect(html).not.toContain("Before</p>");
  });

  it("offers the toggle, unpressed, on a changed take", () => {
    const html = renderToStaticMarkup(<DiffView raw={FIX.raw} formatted={FIX.text} />);
    expect(html).toContain(SHOW_BEFORE_AFTER);
    expect(html).toContain('aria-pressed="false"');
    expect(html).toContain('type="button"');
  });

  it("gaps a removed run from the added run only when neither carries a space", () => {
    expect(needsPairGap({ op: "removed", text: "my" }, { op: "added", text: "your" })).toBe(true);
    expect(needsPairGap({ op: "removed", text: "add " }, { op: "added", text: "Add " })).toBe(false);
    expect(needsPairGap({ op: "equal", text: "to " }, { op: "added", text: "your" })).toBe(false);
    expect(needsPairGap(undefined, { op: "added", text: "now" })).toBe(false);
  });
});

describe("DiffView — Before / After blocks", () => {
  it("shows two labelled paragraphs when the user picks before/after", () => {
    const html = renderToStaticMarkup(<DiffView raw={FIX.raw} formatted={FIX.text} layout="blocks" />);
    expect(html).toContain('data-layout="blocks"');
    expect(html).toContain("Before</p>");
    expect(html).toContain("After</p>");
    expect(html).toContain(FIX.raw);
    expect(html).toContain(FIX.text);
    expect(html).toContain('aria-pressed="true"');
    expect(html).not.toContain("diff-removed");
  });

  it("goes back to inline when the choice is inline", () => {
    const html = renderToStaticMarkup(<DiffView raw={FIX.raw} formatted={FIX.text} layout="inline" />);
    expect(html).toContain('data-layout="inline"');
  });

  it("uses blocks by default when changes are dense", () => {
    const html = renderToStaticMarkup(
      <DiffView raw={SAMPLE_POLISH_REVEAL.raw} formatted={SAMPLE_POLISH_REVEAL.text} />,
    );
    expect(html).toContain('data-layout="blocks"');
    expect(html).toContain('aria-pressed="true"');
  });

  it("shows an unchanged take as text with no toggle", () => {
    const same = POLISH_REVEAL_DIFF_SAMPLES.same;
    const html = renderToStaticMarkup(<DiffView raw={same.raw} formatted={same.text} layout="blocks" />);
    expect(html).toContain("Nothing changed");
    expect(html).not.toContain(SHOW_BEFORE_AFTER);
    expect(html).toContain('data-layout="inline"');
  });
});

describe("DiffView — History does not regress", () => {
  it("keeps the stage line, the silent-skip line and the thumbs", () => {
    const html = renderToStaticMarkup(
      <DiffView
        raw={FIX.raw}
        formatted={FIX.text}
        stages="rules,polish"
        polishSkipReason="deadline"
        feedback={1}
        onFeedback={() => {}}
        onUndo={() => {}}
      />,
    );
    expect(html).toContain("Stages that ran: Formatting rules → AI polish");
    expect(html).toContain("the model ran out of time for a take this long");
    expect(html).toContain("👍");
    expect(html).toContain("👎");
    expect(html).toContain("Undo — paste raw");
    expect(html).toContain("10 removed · 5 added");
  });
});

describe("polish card — layout is held for the card session", () => {
  it("passes the card's layout choice and setter through to DiffView", () => {
    const onLayoutChange = () => {};
    const el = PolishRevealView({
      reveal: FIX,
      view: "card",
      layout: "blocks",
      onLayoutChange,
      onReveal: () => {},
      onUseOriginal: () => {},
      onKeep: () => {},
    });
    const dv = walk(el).find((e) => e.type === DiffView) as ReactElement<{
      layout?: string | null;
      onLayoutChange?: unknown;
    }>;
    expect(dv.props.layout).toBe("blocks");
    expect(dv.props.onLayoutChange).toBe(onLayoutChange);
  });

  it("forces the capture-sheet scenes", () => {
    expect(forcedPolishReveal("?polish=card&diff=fixture")).toEqual({ reveal: FIX, view: "card" });
    expect(forcedPolishReveal("?polish=card&diff=fixture&layout=blocks")?.layout).toBe("blocks");
    expect(forcedPolishReveal("?polish=card&diff=one")?.reveal).toBe(POLISH_REVEAL_DIFF_SAMPLES.one);
    expect(forcedPolishReveal("?polish=card&diff=same")?.reveal).toBe(POLISH_REVEAL_DIFF_SAMPLES.same);
    expect(forcedPolishReveal("?polish=card")?.reveal).toBe(SAMPLE_POLISH_REVEAL);
  });
});
