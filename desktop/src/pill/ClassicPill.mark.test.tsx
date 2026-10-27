/**
 * Y2-D fix round 1 — the upgrade mark must keep working after the pill has been
 * re-docked by a drag.
 *
 * Reviewers' BLOCK on a01f20d: `usePillDrag().dragged()` stays true after a
 * drag until the NEXT pointer-down on the capsule resets it. The mark stops
 * `pointerdown` from propagating (so a press on it never starts a drag), which
 * also means a press on the mark never resets the flag — and the mark's click
 * used to begin with `if (drag.dragged()) return;`. Result: after one re-dock
 * every click on the mark did nothing.
 *
 * This drives ClassicPill's REAL handlers, not a model of them: the component
 * is called inside a server render (so its hooks — including the real
 * `usePillDrag` — run under a live dispatcher), and the element tree it returns
 * is searched for the capsule and the mark. The gesture is replayed exactly as
 * the browser would deliver it: the capsule sees the drag; the mark sees its own
 * pointerdown (propagation stopped, so the capsule does NOT) and its click.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

const { invoke, emit } = vi.hoisted(() => ({
  invoke: vi.fn((..._args: unknown[]) => Promise.resolve()),
  emit: vi.fn((..._args: unknown[]) => Promise.resolve()),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  emit,
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import ClassicPill from "./PillShell";
import { REVEAL_PURCHASE_COMMAND, type PillLicense } from "./license";

type Props = Record<string, unknown> & { children?: unknown };
type El = { type: unknown; props: Props };

function walk(node: unknown, out: El[]): El[] {
  if (Array.isArray(node)) {
    node.forEach((n) => walk(n, out));
  } else if (node && typeof node === "object" && "props" in node) {
    const el = node as El;
    out.push(el);
    walk(el.props.children, out);
  }
  return out;
}

/** Render ClassicPill once and hand back its element tree. */
function tree(props: Parameters<typeof ClassicPill>[0]): El[] {
  let captured: unknown = null;
  function Probe() {
    captured = ClassicPill(props);
    return null;
  }
  renderToStaticMarkup(<Probe />);
  return walk(captured, []);
}

function ev(x: number, y = 0) {
  return {
    button: 0,
    pointerId: 1,
    screenX: x,
    screenY: y,
    stopPropagation: () => {},
    preventDefault: () => {},
    currentTarget: { setPointerCapture: () => {}, releasePointerCapture: () => {} },
  };
}

const TRIAL: PillLicense = {
  show: true,
  tone: "trial",
  glyph: "⧗",
  value: "5d",
  title: "5 days left in your trial",
  action: "purchase",
};

function parts(license: PillLicense) {
  const els = tree({ gate: "idle", license });
  const capsule = els.find((e) => typeof e.props.className === "string" && /^pill\b/.test(e.props.className as string) && typeof e.props.onPointerDown === "function");
  const mark = els.find((e) => typeof e.props.className === "string" && (e.props.className as string).startsWith("pill-upgrade "));
  if (!capsule || !mark) throw new Error("capsule or upgrade mark not rendered");
  const h = capsule.props as Record<string, (e: unknown) => void>;
  const m = mark.props as Record<string, (e: unknown) => void>;
  return { h, m };
}

/** Press the capsule, travel 80pt, release: a re-dock drag. */
function redock(h: Record<string, (e: unknown) => void>) {
  h.onPointerDown(ev(0));
  h.onPointerMove(ev(80));
  h.onPointerUp(ev(80));
}

const calls = (name: string) => invoke.mock.calls.filter((c) => c[0] === name).length;

beforeEach(() => {
  invoke.mockClear();
  emit.mockClear();
});

describe("Y2-D upgrade mark survives a re-dock drag", () => {
  it("a click on the mark AFTER a drag still raises the purchase sheet (every time)", () => {
    const { h, m } = parts(TRIAL);
    redock(h);
    expect(calls("pill_drag_end")).toBe(1);
    for (let i = 0; i < 3; i++) {
      // The mark's own pointerdown stops propagation, so the capsule's
      // onPointerDown is deliberately NOT called here — exactly as in the DOM.
      m.onPointerDown(ev(80));
      m.onClick(ev(80));
    }
    expect(calls(REVEAL_PURCHASE_COMMAND)).toBe(3);
    // …and a press on the mark never starts dictation.
    expect(calls("manual_toggle")).toBe(0);
  });

  it("the license_tab route is equally live after a drag", () => {
    const { h, m } = parts({ ...TRIAL, tone: "problem", value: null, action: "license" });
    redock(h);
    m.onPointerDown(ev(80));
    m.onClick(ev(80));
    expect(calls("show_main")).toBe(1);
    expect(emit.mock.calls).toContainEqual(["settings-tab", "license"]);
    expect(calls(REVEAL_PURCHASE_COMMAND)).toBe(0);
  });

  it("YV65 still holds: the click that CLOSES a capsule drag starts nothing", () => {
    const { h } = parts(TRIAL);
    redock(h);
    h.onClick(ev(80));
    expect(calls("manual_toggle")).toBe(0);
    expect(calls(REVEAL_PURCHASE_COMMAND)).toBe(0);
  });

  it("a plain capsule press (no drag) still toggles dictation", () => {
    const { h } = parts(TRIAL);
    h.onPointerDown(ev(0));
    h.onPointerUp(ev(0));
    h.onClick(ev(0));
    expect(calls("manual_toggle")).toBe(1);
  });
});
