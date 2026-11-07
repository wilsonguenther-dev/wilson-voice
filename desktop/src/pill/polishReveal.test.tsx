/**
 * WF-05 — the pill's "Polished — Reveal" toast and its diff card. Renders the
 * SHIPPING view (no DOM: `renderToStaticMarkup`, like backtrackCaught.test.tsx)
 * and presses its buttons by walking the element tree, since the view has no
 * hooks. WHEN the toast is raised (a real rewrite, never punctuation or
 * capitals alone) is Rust's (`dictation::tests::polish_reveal_*`,
 * `tests/polish_reveal.rs`); the Use original replace path is
 * `dictation::tests::use_original_*`.
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactElement, ReactNode } from "react";
import DiffView from "../DiffView";
import { PolishRevealView, pasteOriginal, polishRevealShows } from "./PolishRevealCard";
import {
  LOCAL_ROUTE_LABEL,
  POLISH_REVEAL_EVENT,
  POLISH_REVEAL_TTL_MS,
  SAMPLE_POLISH_REVEAL,
  USE_ORIGINAL_COMMAND,
  USE_ORIGINAL_FAILED,
  forcedPolishReveal,
  isPolishReveal,
  polishRevealLines,
  resultRouteLabel,
  revealRouteLabel,
  useOriginalArgs,
  useOriginalError,
  type PolishReveal,
} from "./polishReveal";
import { matrixPolishRevealScenes } from "./forced";
import { PILL_MESSAGES, pillMessageAllowed } from "./live";

const REVEAL: PolishReveal = {
  id: "take-1",
  raw: "the report is done",
  text: "The quarterly report is finished.",
  stages: "rules,polish",
  changedWords: 2,
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

type Btn = ReactElement<{ className?: string; onClick?: () => void; disabled?: boolean; children?: ReactNode }>;
const buttons = (el: ReactElement): Btn[] => walk(el).filter((e) => e.type === "button") as Btn[];

const noop = () => {};

describe("WF-05 polish reveal — model", () => {
  it("names the event and the command the Rust side uses, and a 4-second toast", () => {
    expect(POLISH_REVEAL_EVENT).toBe("polish_reveal");
    expect(USE_ORIGINAL_COMMAND).toBe("use_original_take");
    expect(POLISH_REVEAL_TTL_MS).toBe(4_000);
  });

  it("accepts only a complete payload that really changed the take", () => {
    expect(isPolishReveal(REVEAL)).toBe(true);
    expect(polishRevealLines({ ...REVEAL, changedWords: 1 }).aria).toBe("Polished your dictation, 1 word changed");
    expect(polishRevealLines(REVEAL).aria).toBe("Polished your dictation, 2 words changed");
    expect(isPolishReveal({ ...REVEAL, stages: null })).toBe(true);
    expect(isPolishReveal({ ...REVEAL, raw: "" })).toBe(false);
    expect(isPolishReveal({ ...REVEAL, text: REVEAL.raw })).toBe(false);
    expect(isPolishReveal({ ...REVEAL, changedWords: 0 })).toBe(false);
    expect(isPolishReveal({ ...REVEAL, changedWords: "2" })).toBe(false);
    expect(isPolishReveal(null)).toBe(false);
  });

  it("Use original sends the take id to the Undo AI Edit paste", async () => {
    expect(useOriginalArgs(REVEAL)).toEqual({ id: "take-1" });
    const calls: Array<[string, Record<string, unknown>]> = [];
    await pasteOriginal(REVEAL, async (cmd, args) => {
      calls.push([cmd, args]);
      return "Pasted";
    });
    expect(calls).toEqual([["use_original_take", { id: "take-1" }]]);
  });

  it("a failed Use original says Rust's sentence, or the generic one", () => {
    expect(useOriginalError("A newer take landed. Use History to paste this one.")).toContain("newer take");
    expect(useOriginalError(new Error("boom"))).toBe(USE_ORIGINAL_FAILED);
    expect(useOriginalError("  ")).toBe(USE_ORIGINAL_FAILED);
  });

  it("names where polish ran in the route chip's words, never a vendor", () => {
    expect(
      revealRouteLabel({
        cloudRoute: { route: "cloud", reason: "online", label: "Cloud", tooltip: "Fast path", job: "polish" },
      }),
    ).toBe("Cloud");
    expect(revealRouteLabel({ cloudRoute: null })).toBe(LOCAL_ROUTE_LABEL);
    expect(
      revealRouteLabel({
        cloudRoute: { route: "local", reason: "user", label: "On this Mac", tooltip: "", job: "polish" },
      }),
    ).toBe("On this Mac");
    // The take's own polish_result route (byom) uses the same two words.
    expect(resultRouteLabel({ route: "cloud" })).toBe("Cloud");
    expect(resultRouteLabel({ route: "local" })).toBe(LOCAL_ROUTE_LABEL);
    expect(POLISH_REVEAL_TTL_MS).toBe(4_000);
    const lines = Object.values(polishRevealLines(REVEAL)).join(" ");
    expect(lines).not.toMatch(/openai|gpt|gemini|claude|anthropic|qwen|llama|groq|mistral/i);
  });
});

describe("WF-05 polish reveal — WF-17 mute", () => {
  it("is a tip, so the Tips switch mutes it", () => {
    expect(PILL_MESSAGES["polish-reveal"]).toBe("tip");
    expect(pillMessageAllowed("polish-reveal", new Set(["tip"]))).toBe(false);
    expect(pillMessageAllowed("polish-reveal", new Set(["news", "milestone"]))).toBe(true);
  });

  it("a muted payload never reaches the pill; an unmuted one does", () => {
    expect(polishRevealShows(REVEAL, new Set())).toBe(true);
    expect(polishRevealShows(REVEAL, new Set(["tip"]))).toBe(false);
    expect(polishRevealShows({ ...REVEAL, changedWords: 0 }, new Set())).toBe(false);
  });
});

describe("WF-05 polish reveal — view", () => {
  it("the toast says Polished and offers Reveal", () => {
    const onReveal = vi.fn();
    const onKeep = vi.fn();
    const el = PolishRevealView({
      reveal: REVEAL,
      view: "toast",
      onReveal,
      onUseOriginal: noop,
      onKeep,
    }) as ReactElement;
    const html = renderToStaticMarkup(el);
    expect(html).toContain("Polished");
    expect(html).toContain('role="status"');
    const [reveal, dismiss] = buttons(el);
    expect(reveal.props.children).toBe("Reveal");
    reveal.props.onClick?.();
    expect(onReveal).toHaveBeenCalledTimes(1);
    dismiss.props.onClick?.();
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("the card shows the shared diff with the route chip and Use original / Keep", () => {
    const onUseOriginal = vi.fn();
    const onKeep = vi.fn();
    const el = PolishRevealView({
      reveal: REVEAL,
      view: "card",
      route: "Cloud",
      onReveal: noop,
      onUseOriginal,
      onKeep,
    }) as ReactElement;
    expect(walk(el).some((e) => e.type === DiffView)).toBe(true);
    const html = renderToStaticMarkup(el);
    expect(html).toContain("diff-view");
    expect(html).toContain("diff-added");
    expect(html).toContain("Cloud");
    expect(html).toContain('role="dialog"');
    const bs = buttons(el);
    expect(bs.map((b) => b.props.children)).toEqual(["Use original", "Keep"]);
    bs[0].props.onClick?.();
    expect(onUseOriginal).toHaveBeenCalledTimes(1);
    bs[1].props.onClick?.();
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("busy disables both answers and an error is announced", () => {
    const el = PolishRevealView({
      reveal: REVEAL,
      view: "card",
      busy: true,
      error: USE_ORIGINAL_FAILED,
      onReveal: noop,
      onUseOriginal: noop,
      onKeep: noop,
    }) as ReactElement;
    expect(buttons(el).every((b) => b.props.disabled)).toBe(true);
    expect(renderToStaticMarkup(el)).toContain('role="alert"');
  });
});

describe("WF-05 DiffView route line", () => {
  it("shows where polish ran only when the polish stage ran", () => {
    const withPolish = renderToStaticMarkup(
      <DiffView raw={REVEAL.raw} formatted={REVEAL.text} stages="rules,polish" route="On this Mac" />,
    );
    expect(withPolish).toContain("diff-route-chip");
    const rulesOnly = renderToStaticMarkup(
      <DiffView raw={REVEAL.raw} formatted={REVEAL.text} stages="rules" route="On this Mac" />,
    );
    expect(rulesOnly).not.toContain("diff-route-chip");
    const history = renderToStaticMarkup(
      <DiffView raw={REVEAL.raw} formatted={REVEAL.text} stages="rules,polish" />,
    );
    expect(history).not.toContain("diff-route-chip");
  });
});

describe("WF-05 polish reveal — matrix scenes", () => {
  it("forces the toast and the card for the headless matrix only", () => {
    expect(forcedPolishReveal("?phase=idle&polish=toast")).toEqual({ reveal: SAMPLE_POLISH_REVEAL, view: "toast" });
    expect(forcedPolishReveal("?polish=card")?.view).toBe("card");
    expect(forcedPolishReveal("?phase=idle")).toBeNull();
    expect(isPolishReveal(SAMPLE_POLISH_REVEAL)).toBe(true);
    expect(matrixPolishRevealScenes().map((s) => s.query)).toEqual(["polish=toast", "polish=card"]);
  });
});
