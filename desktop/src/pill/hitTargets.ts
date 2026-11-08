/**
 * yap-1106-fb-02-pill-first-click — every pill button works on the first click.
 *
 * Wilson, 2026-11-06 15:29Z: "the first time i click it the second time doesn't
 * work the third time it works". The float window is a click-THROUGH NSPanel
 * that only took the cursor over the capsule (ARCHITECTURE-PILL: it "ignores
 * clicks except over the capsule"), so a control drawn anywhere else — the
 * meeting stop in the clock slot, Trial ended, a card's buttons — handed its
 * press to the app underneath.
 *
 * This module makes the click-through region the SAME rects the controls draw
 * in. Every clickable control on the pill carries `data-pill-control="<name>"`;
 * the watch below measures each one off the DOM (grown to 24 x 24 pt about its
 * centre, the floor for a pointer target), and publishes the list through
 * drag.ts `extendPillHitboxRects` → `pill_set_hitboxes`. It also:
 *
 *  - holds the panel interactive from pointer-down to pointer-up on a control
 *    (`pill_press_hold`), so no cursor test or hitbox re-publish landing mid
 *    press can hand the mouse-up to the floor (the "hover-state race");
 *  - logs `pill.click {control, handled}` for every click on a control
 *    (`pill_click_log` → one `pill click control=… handled=…` line).
 *
 * The headless proof is scripts/pill-matrix.mjs --clicks: it clicks the centre
 * and the four inner corners of every `[data-pill-control]` once, in every dock
 * position, and asserts exactly one action each.
 */
import { invoke } from "@tauri-apps/api/core";
import { CONTROLS_OWNER, extendPillHitboxRects, type Rect4 } from "./drag";
import { DURATION } from "./motion";

/** The smallest hit area any pill control gets, in logical points. */
export const MIN_HIT_PT = 24;
/** The attribute every clickable pill control carries; its value names it in telemetry. */
export const CONTROL_ATTR = "data-pill-control";
export const CONTROL_SELECTOR = `[${CONTROL_ATTR}]`;
/** How far inside a control's edge the harness probes its four corners. */
export const PROBE_INSET_PT = 1;

/** Grow `r` about its centre until each side is at least `min`. */
export function padToMin(r: Rect4, min: number = MIN_HIT_PT): Rect4 {
  const w = Math.max(r[2], min);
  const h = Math.max(r[3], min);
  return [r[0] - (w - r[2]) / 2, r[1] - (h - r[3]) / 2, w, h];
}

/** The centre and the four inner corners of `r` — the harness's five probes. */
export function probePoints(r: Rect4, inset: number = PROBE_INSET_PT): Array<[number, number]> {
  const [x, y, w, h] = r;
  const i = Math.min(inset, w / 2, h / 2);
  return [
    [x + w / 2, y + h / 2],
    [x + i, y + i],
    [x + w - i, y + i],
    [x + i, y + h - i],
    [x + w - i, y + h - i],
  ];
}

/** Is (x, y) inside any rect? Edges inclusive — the same test float_pill.rs runs. */
export function pointInRects(x: number, y: number, rects: readonly Rect4[]): boolean {
  return rects.some((r) => r[2] > 0 && r[3] > 0 && x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3]);
}

/** The control a pointer event landed on (its `data-pill-control` element), if any. */
export function controlElement(target: EventTarget | null): HTMLElement | null {
  const el = target as Element | null;
  if (!el || typeof el.closest !== "function") return null;
  return el.closest<HTMLElement>(CONTROL_SELECTOR);
}

/** A control that is on screen and can take a pointer. */
function shown(el: HTMLElement): boolean {
  if (el.getClientRects().length === 0) return false;
  const cs = getComputedStyle(el);
  return cs.visibility !== "hidden" && cs.pointerEvents !== "none";
}

/** Every visible control's rect, padded to `MIN_HIT_PT`. */
export function measureControls(root: ParentNode): Rect4[] {
  const out: Rect4[] = [];
  for (const el of Array.from(root.querySelectorAll<HTMLElement>(CONTROL_SELECTOR))) {
    if (!shown(el)) continue;
    const b = el.getBoundingClientRect();
    if (b.width <= 0 || b.height <= 0) continue;
    out.push(padToMin([b.left, b.top, b.width, b.height]));
  }
  return out;
}

/** Did this click reach a live action? A disabled control's click is logged `handled=false`. */
export function clickHandled(el: HTMLElement): boolean {
  return !(el as HTMLButtonElement).disabled && el.getAttribute("aria-disabled") !== "true";
}

/** `pill.click {control, handled}` — one structured backend log line per click. */
export function logPillClick(control: string, handled: boolean): void {
  invoke("pill_click_log", { control, handled }).catch(() => {});
}

/**
 * The elements whose size moves a control: each control and its siblings. A
 * pure :hover swap (yap-1106-fb-23's People stepper replacing the call-app name
 * in the centred clock row) slides the meeting stop without resizing it and
 * without a DOM mutation; the sibling that appears or collapses is what the
 * ResizeObserver sees, so the panel's rect follows the stop before the press.
 */
export function observedForControls(controls: readonly HTMLElement[]): HTMLElement[] {
  const out = new Set<HTMLElement>();
  for (const el of controls) {
    out.add(el);
    const parent = el.parentElement;
    if (!parent) continue;
    // `children` holds Elements only; no HTMLElement global needed (node tests).
    for (const sib of Array.from(parent.children)) out.add(sib as HTMLElement);
  }
  return Array.from(out);
}

/**
 * Start publishing every control's rect and the press/click wiring. Returns the
 * teardown. Coalesced to one measure per ~frame; re-measures on DOM changes
 * (a control mounting, swapping, relabelling), resizes, the dock flipping and
 * the end of any transition/animation (controls slide without resizing).
 */
export function startPillControlTargets(doc: Document = document): () => void {
  const win = doc.defaultView ?? window;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let observed: HTMLElement[] = [];
  const ro = new ResizeObserver(() => schedule());
  const push = () => {
    timer = null;
    for (const el of observed) ro.unobserve(el);
    observed = observedForControls(Array.from(doc.querySelectorAll<HTMLElement>(CONTROL_SELECTOR)));
    for (const el of observed) ro.observe(el);
    extendPillHitboxRects(measureControls(doc), CONTROLS_OWNER);
  };
  function schedule() {
    if (timer === null) timer = setTimeout(push, DURATION.measureCoalesce);
  }
  const mo = new MutationObserver(() => schedule());
  mo.observe(doc.documentElement, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["class", "style", "hidden", "disabled", "data-dock", CONTROL_ATTR],
  });

  let held = false;
  const release = () => {
    if (!held) return;
    held = false;
    invoke("pill_press_hold", { held: false }).catch(() => {});
  };
  const onDown = (e: PointerEvent) => {
    if (e.button !== 0 || !controlElement(e.target)) return;
    held = true;
    invoke("pill_press_hold", { held: true }).catch(() => {});
  };
  const onClick = (e: MouseEvent) => {
    const el = controlElement(e.target);
    if (!el) return;
    logPillClick(el.getAttribute(CONTROL_ATTR) || "unknown", clickHandled(el));
  };
  doc.addEventListener("pointerdown", onDown, true);
  win.addEventListener("pointerup", release, true);
  win.addEventListener("pointercancel", release, true);
  win.addEventListener("blur", release);
  doc.addEventListener("click", onClick, true);
  doc.addEventListener("transitionend", schedule, true);
  doc.addEventListener("animationend", schedule, true);
  win.addEventListener("resize", schedule);
  push();
  return () => {
    if (timer !== null) clearTimeout(timer);
    mo.disconnect();
    ro.disconnect();
    release();
    doc.removeEventListener("pointerdown", onDown, true);
    win.removeEventListener("pointerup", release, true);
    win.removeEventListener("pointercancel", release, true);
    win.removeEventListener("blur", release);
    doc.removeEventListener("click", onClick, true);
    doc.removeEventListener("transitionend", schedule, true);
    doc.removeEventListener("animationend", schedule, true);
    win.removeEventListener("resize", schedule);
    extendPillHitboxRects(null, CONTROLS_OWNER);
  };
}
