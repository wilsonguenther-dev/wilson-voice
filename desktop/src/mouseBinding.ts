/**
 * WF-19 — a mouse button as push-to-talk or hands-free.
 *
 * Pure: the stored setting (`AppSettings.pttMouseBinding`) and the DOM's mouse
 * button numbers, mapped both ways. Mirrors `ptt_macos::MouseBinding::parse`
 * in Rust, which re-validates every save; left and right click are refused on
 * both sides and can never be stored.
 *
 * Stored form: "" (off) | "middle" | "button<N>" (N ≥ 3, the number the
 * mouse's maker prints) — then ":hands_free" for click-to-toggle.
 */

export type MouseMode = "hold" | "hands_free";

export interface MouseBinding {
  /** 1-based maker number: 3 = middle, 4 = back side button, 5 = forward … */
  button: number;
  mode: MouseMode;
}

/** Mirrors Rust `MOUSE_PRIMARY_REFUSED` word for word. */
export const MOUSE_PRIMARY_REFUSED =
  "Left and right click can't be shortcuts — pick the middle button or a side button";

/** Highest maker number macOS reports (CoreGraphics buttons 0…31). */
export const MOUSE_BUTTON_MAX = 32;

/** A parse / capture result: a binding, off (`null`), or the reason it is refused. */
export type MouseParse = { ok: true; binding: MouseBinding | null } | { ok: false; why: string };

/** Parse the stored setting. */
export function parseMouseBinding(stored: string | null | undefined): MouseParse {
  const s = (stored ?? "").trim().toLowerCase();
  if (s === "" || s === "off" || s === "none") return { ok: true, binding: null };
  // Split at the FIRST colon, like Rust's `split_once(':')`.
  const colon = s.indexOf(":");
  const buttonPart = colon < 0 ? s : s.slice(0, colon);
  const modePart = colon < 0 ? "hold" : s.slice(colon + 1);
  let mode: MouseMode;
  if (modePart === "hold" || modePart === "push_to_talk" || modePart === "ptt") mode = "hold";
  else if (modePart === "hands_free" || modePart === "toggle") mode = "hands_free";
  else return { ok: false, why: `Unknown mouse mode "${modePart}" — use hold or hands_free` };
  let button: number;
  if (buttonPart === "left" || buttonPart === "primary") button = 1;
  else if (buttonPart === "right" || buttonPart === "secondary") button = 2;
  else if (buttonPart === "middle") button = 3;
  else {
    const m = /^button(\d+)$/.exec(buttonPart);
    if (!m || Number(m[1]) === 0) {
      return { ok: false, why: `Unknown mouse button "${buttonPart}" — use middle or button4, button5, …` };
    }
    button = Number(m[1]);
  }
  return bindingOf(button, mode);
}

/** Validate a maker-numbered button; left (1) and right (2) never pass. */
export function bindingOf(button: number, mode: MouseMode): MouseParse {
  if (button === 1 || button === 2) return { ok: false, why: MOUSE_PRIMARY_REFUSED };
  if (!Number.isInteger(button) || button < 1 || button > MOUSE_BUTTON_MAX) {
    return {
      ok: false,
      why: `Mouse button ${button} isn't one macOS reports — pick the middle button or a side button`,
    };
  }
  return { ok: true, binding: { button, mode } };
}

/** The canonical stored form (what Rust writes back). */
export function toStored(b: MouseBinding | null): string {
  if (!b) return "";
  const button = b.button === 3 ? "middle" : `button${b.button}`;
  return b.mode === "hands_free" ? `${button}:hands_free` : button;
}

/**
 * A DOM `MouseEvent.button` → maker number. The DOM numbers 0 left, 1 middle,
 * 2 right, 3 back, 4 forward; the maker numbers 1 left, 2 right, 3 middle,
 * 4 back, 5 forward.
 */
export function makerButtonOfDom(domButton: number): number {
  if (domButton === 0) return 1;
  if (domButton === 1) return 3;
  if (domButton === 2) return 2;
  return domButton + 1;
}

/** One captured press in the recorder → the binding it makes, or why not. */
export function captureMouse(domButton: number, mode: MouseMode): MouseParse {
  return bindingOf(makerButtonOfDom(domButton), mode);
}

/** "middle button" / "button 4" — Rust `MouseBinding::button_label`. */
export function buttonLabel(button: number): string {
  return button === 3 ? "middle button" : `button ${button}`;
}

/** The line Settings shows for the stored value. */
export function mouseBindingLine(stored: string | null | undefined): string {
  const p = parseMouseBinding(stored);
  if (!p.ok) return "Off — the saved mouse button isn't valid";
  if (!p.binding) return "Off";
  const name = buttonLabel(p.binding.button);
  return p.binding.mode === "hands_free"
    ? `Click the ${name} to start, click again to stop`
    : `Hold the ${name} to talk, double-click for hands-free`;
}

/** Switch the mode of a stored binding, keeping its button (off stays off). */
export function withMode(stored: string | null | undefined, mode: MouseMode): string {
  const p = parseMouseBinding(stored);
  if (!p.ok || !p.binding) return "";
  return toStored({ ...p.binding, mode });
}
