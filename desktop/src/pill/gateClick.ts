/**
 * yap-1106-pill-trial-clickable — the "Trial ended" pill is a BUTTON to the fix.
 *
 * When the trial is over and a press is refused, the whole pill (canvas
 * capsule and its overlay chip alike) opens the drivia.consulting Yap page in
 * the browser. Hover says what a click will do ("Buy a license") and where it
 * goes ("Opens drivia.consulting in your browser"). The refusal holds for
 * [`GATE_HOLD_MS`] and for as long as the pointer rests on it; afterwards the
 * resting nub keeps a small lock that opens the same page.
 *
 * ONE PURE MODULE, NO TIMERS. `float-main` arms the settle timer from
 * [`gateSettleDueIn`]; YappyPill reports hover through [`setGateHover`]; the
 * route is [`gateClickRoute`]. All three are unit-tested without a webview.
 *
 * THE PILL NAMES NO URL. A click invokes [`GATE_CLICK_COMMAND`] with a SURFACE
 * name only; the backend logs `pill gate.click` and hands its own compile-time
 * constant to the opener. The float webview is the least-trusted window in the
 * app and it never gets to say where a browser goes.
 *
 * A PAYING CUSTOMER IS NEVER SOLD TO (Y2-F). A stored key that grants nothing
 * (`problem` tone) routes to Settings → License, never to the purchase page.
 */
import type { PillLicense } from "./license";
import { DURATION } from "./motion";

/** What hover says the click does. */
export const GATE_BUY_LABEL = "Buy a license";

/** Where the click goes, as the hover tooltip says it. */
export const GATE_BUY_TOOLTIP = "Opens drivia.consulting in your browser";

/** The nub lock's accessible name: the action, then the destination. */
export const GATE_LOCK_LABEL = `${GATE_BUY_LABEL}. ${GATE_BUY_TOOLTIP}`;

/**
 * The Rust command a gated click reaches (lib.rs `pill_gate_click`). It takes a
 * surface and a target NAME, never a URL.
 */
export const GATE_CLICK_COMMAND = "pill_gate_click";

/** How long the refusal stays up when nobody touches it. */
export const GATE_HOLD_MS: number = DURATION.gateHold;

/**
 * After the pointer LEAVES a refusal that is already past its hold, how long it
 * stays before settling, so moving off it by a pixel does not snap it away
 * under the cursor.
 */
export const GATE_HOVER_GRACE_MS: number = DURATION.gateHoverGrace;

/** Which part of the pill was clicked. Logged; never a destination. */
export type GateSurface = "pill" | "chip" | "nub";

/** Where a gated click goes. */
export type GateTarget = "purchase_page" | "license_tab";

/**
 * The route for a gated click. A stored key in trouble (Y2-F `problem`) is
 * somebody who PAID: they get the re-activate box. Everyone else — an ended
 * trial, or no payload yet — gets the purchase page, which says "not open yet"
 * on its own while checkout is closed.
 */
export function gateClickRoute(license: PillLicense | null | undefined): GateTarget {
  return license?.show === true && license.tone === "problem" ? "license_tab" : "purchase_page";
}

/**
 * The resting nub's lock: shown once the refusal has settled, while the trial
 * is over. Never on a licensed pill, never during a running trial, never over a
 * stored-key problem (that chip already routes to the license box), and never
 * while the full refusal or the mic refusal is on screen.
 */
export function nubLockVisible(
  license: PillLicense | null | undefined,
  gated: boolean,
  blocked: boolean,
): boolean {
  return !gated && !blocked && license?.show === true && license.tone === "ended";
}

/** The command's arguments, exactly as `pill_gate_click` reads them. */
export function gateClickArgs(
  license: PillLicense | null | undefined,
  surface: GateSurface,
): { surface: GateSurface; target: GateTarget } {
  return { surface, target: gateClickRoute(license) };
}

// ── the hold: 6 s, longer while hovered ────────────────────────────────────

export interface GateHold {
  /** When the refusal went up (ms, any monotonic clock); null = not showing. */
  shownAt: number | null;
  /** Is the pointer (or keyboard focus) resting on the pill right now? */
  hovered: boolean;
  /** When the pointer last left; null = never left since it was shown. */
  leftAt: number | null;
}

export const GATE_HOLD_IDLE: GateHold = { shownAt: null, hovered: false, leftAt: null };

export type GateHoldEvent =
  | { type: "shown"; at: number }
  | { type: "hover"; on: boolean; at: number }
  | { type: "settled" };

export function reduceGateHold(s: GateHold, ev: GateHoldEvent): GateHold {
  switch (ev.type) {
    case "shown":
      // A fresh refused press restarts the clock; a pointer already resting on
      // the pill keeps holding it.
      return { shownAt: ev.at, hovered: s.hovered, leftAt: null };
    case "hover":
      if (ev.on === s.hovered) return s;
      return ev.on ? { ...s, hovered: true } : { ...s, hovered: false, leftAt: ev.at };
    case "settled":
      return { ...GATE_HOLD_IDLE, hovered: s.hovered };
  }
}

/**
 * How long until the refusal should settle: `null` while it is not showing or
 * while it is hovered (no timer at all), else milliseconds (0 = settle now).
 * The deadline is the later of "6 s after it went up" and "a short grace after
 * the pointer left".
 */
export function gateSettleDueIn(s: GateHold, now: number): number | null {
  if (s.shownAt === null || s.hovered) return null;
  const hold = s.shownAt + GATE_HOLD_MS;
  const grace = s.leftAt === null ? -Infinity : s.leftAt + GATE_HOVER_GRACE_MS;
  return Math.max(0, Math.max(hold, grace) - now);
}

// ── hover bus: YappyPill → float-main, one webview ─────────────────────────

type HoverListener = (on: boolean) => void;
const hoverListeners = new Set<HoverListener>();
let hoverNow = false;

/** YappyPill: the pointer entered (true) or left (false) the gated pill. */
export function setGateHover(on: boolean): void {
  if (on === hoverNow) return;
  hoverNow = on;
  hoverListeners.forEach((fn) => fn(on));
}

/** float-main: follow hover; returns the unsubscribe. */
export function onGateHover(fn: HoverListener): () => void {
  hoverListeners.add(fn);
  return () => {
    hoverListeners.delete(fn);
  };
}

/** Is the gated pill hovered right now? (for a refusal that arrives mid-hover) */
export function gateHovered(): boolean {
  return hoverNow;
}

// ── the hop: a deep link back from the site activated a license ────────────

/** Backend → float webview: a license arrived by deep link; Yappy hops. */
export const YAPPY_HOP_EVENT = "yappy-hop";

/** How long the hop class stays on the stage (one hop, then rest). */
export const YAPPY_HOP_MS: number = DURATION.yappyHop;
