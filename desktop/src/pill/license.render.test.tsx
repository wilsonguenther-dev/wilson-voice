/**
 * @vitest-environment jsdom
 *
 * Y2-B — the chip is actually IN THE DOM of each pill.
 *
 * WHY THIS FILE EXISTS AT ALL. `license.test.ts` proves the POLICY (`pillLicense`
 * says "5d" on day 5). It proves nothing about whether either pill draws it —
 * and for the whole of Y2-A both pills took the `license` prop and deliberately
 * ignored it while every type check, every unit test and every build stayed
 * green. A pure-function test of a value nothing renders is exactly the
 * "verification that verifies nothing" this repo has been bitten by. So these
 * assertions mount the real components and read the real DOM.
 *
 * The third case is the one that motivated the item: ClassicPill's effect takes
 * an early return under `prefers-reduced-motion: reduce` (ClassicPill.tsx) and
 * paints one calm static frame. The countdown is INFORMATION, not decoration,
 * so it has to be in that frame. Mocking matchMedia to `matches: true` is what
 * makes that branch the one under test.
 */
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import ClassicPill from "./ClassicPill";
import YappyPill from "./YappyPill";
import { pillLicense } from "./license";
import { type LicenseStatus } from "../license/status";

// The pills talk to the backend the moment they mount. In a test there is no
// backend, and an un-mocked `listen()` rejects into an unhandled promise — so
// both modules are stubbed rather than left to fail quietly.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) =>
    cmd === "get_status" ? { recording: false, busy: false, message: "Ready" } : null,
  ),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}));

declare global {
  // eslint-disable-next-line no-var
  var IS_REACT_ACT_ENVIRONMENT: boolean;
}

/**
 * jsdom ships no 2D context, and YappyPill's draw loop would throw on
 * `getContext("2d")!` before it ever rendered a node. This proxy answers every
 * canvas call with itself, which is enough for a loop whose output we are not
 * asserting on — the POSE is canvas pixels and is out of scope here; what is in
 * scope is the numeral, which is DOM.
 */
function fakeCtx(): CanvasRenderingContext2D {
  const store: Record<string | symbol, unknown> = {};
  const p: unknown = new Proxy(store, {
    get(t, k) {
      if (k in t) return t[k];
      return () => p;
    },
    set(t, k, v) {
      t[k] = v;
      return true;
    },
  });
  return p as CanvasRenderingContext2D;
}

let container: HTMLDivElement;
let root: Root;

function setReduceMotion(reduce: boolean) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: reduce && query.includes("prefers-reduced-motion"),
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })) as unknown as typeof window.matchMedia;
}

beforeEach(() => {
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  setReduceMotion(false);
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  HTMLCanvasElement.prototype.getContext = vi.fn(() => fakeCtx()) as unknown as
    typeof HTMLCanvasElement.prototype.getContext;
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
});

function render(el: ReactElement) {
  act(() => {
    root.render(el);
  });
}

const EXPIRES = 1_800_000_000_000;
const trial = (days: number) =>
  ({
    state: "trial",
    days_left: days,
    expires_at_ms: EXPIRES,
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: days,
    trial_expires_at_ms: EXPIRES,
    revocation_checked_at_ms: null,
    revoked_count: 0,
  }) as unknown as LicenseStatus;
const licensed = () =>
  ({
    state: "licensed",
    plan: "lifetime",
    seats: 3,
    kid: "yap_live_abc",
    license_problem: null,
    license_problem_message: null,
    has_stored_license: true,
    trial_days_left: 0,
    trial_expires_at_ms: EXPIRES,
    revocation_checked_at_ms: EXPIRES,
    revoked_count: 0,
  }) as unknown as LicenseStatus;
const required = () =>
  ({
    state: "license_required",
    reason: "trial_expired",
    license_problem: null,
    license_problem_message: null,
    has_stored_license: false,
    trial_days_left: 0,
    trial_expires_at_ms: EXPIRES,
    revocation_checked_at_ms: null,
    revoked_count: 0,
  }) as unknown as LicenseStatus;

describe("ClassicPill — the trial chip is on the capsule", () => {
  it("is present for days_left: 5, and states the numeral", () => {
    render(<ClassicPill license={pillLicense(trial(5))} />);
    const chip = container.querySelector(".lic");
    expect(chip).not.toBe(null);
    expect(chip!.querySelector(".lic-num")!.textContent).toBe("5");
    expect(chip!.querySelector(".lic-unit")!.textContent).toBe("d");
    expect(chip!.className).toContain("lic-trial");
  });

  it("is absent for a licensed Mac — a paying customer is never counted at", () => {
    render(<ClassicPill license={pillLicense(licensed())} />);
    expect(container.querySelector(".lic")).toBe(null);
  });

  it("is present under prefers-reduced-motion: reduce — it is information, not decoration", () => {
    setReduceMotion(true);
    render(<ClassicPill license={pillLicense(trial(5))} />);
    // The reduced-motion branch really is the one that ran: it parks the
    // waveform at level 0 and schedules nothing further.
    expect(container.querySelector<HTMLElement>(".pill")!.style.getPropertyValue("--level")).toBe("0");
    expect(container.querySelector(".lic-num")!.textContent).toBe("5");
  });

  it("carries the whole sentence in the tooltip and never on the capsule", () => {
    const lic = pillLicense(trial(5));
    render(<ClassicPill license={lic} />);
    const chip = container.querySelector<HTMLElement>(".lic")!;
    expect(chip.title).toBe(lic.title);
    expect(chip.title.length).toBeGreaterThan(3);
    // Only the value is drawn; the sentence is not in the capsule's text.
    expect(chip.textContent).toBe("5d");
  });

  it("shows the ended glyph and no numeral once the trial is over", () => {
    render(<ClassicPill license={pillLicense(required())} />);
    expect(container.querySelector(".lic")!.className).toContain("lic-ended");
    expect(container.querySelector(".lic-num")).toBe(null);
    expect(container.querySelector(".lic-glyph")).not.toBe(null);
  });

  it("draws nothing at all when no license payload has arrived yet", () => {
    render(<ClassicPill license={pillLicense(null)} />);
    expect(container.querySelector(".lic")).toBe(null);
  });
});

describe("YappyPill — Yappy holds the same numeral", () => {
  it("is present for days_left: 5", () => {
    render(<YappyPill license={pillLicense(trial(5))} />);
    const chip = container.querySelector(".kami-license");
    expect(chip).not.toBe(null);
    expect(chip!.querySelector(".kami-license-num")!.textContent).toBe("5");
    expect(chip!.querySelector(".kami-license-unit")!.textContent).toBe("d");
  });

  it("is absent for a licensed Mac", () => {
    render(<YappyPill license={pillLicense(licensed())} />);
    expect(container.querySelector(".kami-license")).toBe(null);
  });

  it("is present under prefers-reduced-motion: reduce", () => {
    setReduceMotion(true);
    render(<YappyPill license={pillLicense(trial(5))} />);
    expect(container.querySelector(".kami-license-num")!.textContent).toBe("5");
  });

  it("says the same thing the classic pill says, because it is the same policy", () => {
    const lic = pillLicense(trial(1));
    render(<YappyPill license={lic} />);
    expect(container.querySelector<HTMLElement>(".kami-license")!.title).toBe(lic.title);
    expect(container.querySelector(".kami-license")!.textContent).toBe("1d");
  });
});

/**
 * NOTHING MOVES. Neither pill may put an animation on the countdown, in any
 * state — no pulse, no bounce, no flash. The chip carries no inline animation
 * and no inline transition, and the stylesheet's `.lic` / `.kami-license` rules
 * declare none (see the note in float.css).
 */
describe("the countdown never moves", () => {
  for (const days of [7, 5, 1, 0]) {
    it(`carries no inline animation at day ${days} on either pill`, () => {
      render(<ClassicPill license={pillLicense(trial(days))} />);
      const classic = container.querySelector<HTMLElement>(".lic")!;
      expect(classic.style.animation).toBe("");
      expect(classic.style.transition).toBe("");
      act(() => root.render(<YappyPill license={pillLicense(trial(days))} />));
      const yappy = container.querySelector<HTMLElement>(".kami-license")!;
      expect(yappy.style.animation).toBe("");
      expect(yappy.style.transition).toBe("");
    });
  }
});

/**
 * Y2-B — all three dock positions. `pill_position` is `bottom | left | right`
 * (lib.rs:408, default "bottom") and the edge rides on `<html data-dock>`
 * (float-main.tsx), which means the dock is PURE CSS: the markup either pill
 * emits must be byte-identical on all three, and the 30px strip is handled by
 * rules in float.css and never by a JS branch.
 *
 * That is worth asserting rather than assuming, because the tempting fix for a
 * narrow strip is a `dock === "left" ? … : …` in the component — which is how
 * one dock out of three silently stops showing the countdown.
 */
describe("every dock shows the same chip, because the dock is CSS", () => {
  const DOCKS = ["bottom", "left", "right"] as const;

  afterEach(() => {
    delete document.documentElement.dataset.dock;
  });

  for (const dock of DOCKS) {
    it(`ClassicPill: the chip is present at the ${dock} dock`, () => {
      document.documentElement.dataset.dock = dock;
      render(<ClassicPill license={pillLicense(trial(5))} />);
      expect(container.querySelector(".lic-num")!.textContent).toBe("5");
    });

    it(`YappyPill: the numeral is present at the ${dock} dock`, () => {
      document.documentElement.dataset.dock = dock;
      render(<YappyPill license={pillLicense(trial(5))} />);
      expect(container.querySelector(".kami-license-num")!.textContent).toBe("5");
    });
  }

  it("emits identical markup on all three docks — no JS branches on the edge", () => {
    const seen = DOCKS.map((dock) => {
      document.documentElement.dataset.dock = dock;
      render(<ClassicPill license={pillLicense(trial(5))} />);
      const html = container.querySelector(".lic")!.outerHTML;
      act(() => root.render(<></>));
      return html;
    });
    expect(new Set(seen).size).toBe(1);
  });
});
