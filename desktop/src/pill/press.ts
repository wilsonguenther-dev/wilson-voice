/**
 * yap-wf-01-click-the-pill-to — WF-01: the gesture classifier for a press on
 * the pill. Pure (no `@tauri-apps/*`), so the click-vs-drag rule is a unit test.
 *
 * A press is a CLICK only when the pointer never travelled 4 logical points
 * from where it went down AND it came back up inside 300 ms. Anything else —
 * it moved, or it was held — is a DRAG, and a drag never toggles a take.
 * Travel is the FURTHEST the pointer got during the press, not where it was
 * released: a wobble out and back is still a drag.
 */

/** Travel (logical points) at or past which a press is a DRAG, not a click. */
export const CLICK_SLOP = 4;
/** Hold (ms) at or past which a press is a DRAG, not a click. */
export const CLICK_MAX_MS = 300;

export type PressKind = "click" | "drag";

/** One press in flight, in screen points and a monotonic ms clock. */
export interface PressTrack {
  x: number;
  y: number;
  t: number;
  /** The furthest travel from the down point seen so far. */
  far: number;
}

export function beginPress(x: number, y: number, t: number): PressTrack {
  return { x, y, t, far: 0 };
}

/** Record a move; returns the travel from the down point at this move. */
export function trackPress(p: PressTrack, x: number, y: number): number {
  const d = Math.hypot(x - p.x, y - p.y);
  if (d > p.far) p.far = d;
  return d;
}

/** The rule itself: under 4 points of travel and under 300 ms is a click. */
export function classifyPress(travel: number, ms: number): PressKind {
  return travel < CLICK_SLOP && ms >= 0 && ms < CLICK_MAX_MS ? "click" : "drag";
}

/** Close a press at its pointer-up point and time. */
export function endPress(p: PressTrack, x: number, y: number, t: number): PressKind {
  trackPress(p, x, y);
  return classifyPress(p.far, t - p.t);
}

// ── yap-1107-pill-click-always-registers: the pressed state is instant ──────

/**
 * pill.html (approved 2026-11-06), motion table: "Press anywhere on the pill —
 * Shell scales to 96 percent while pressed." The scale is driven by this
 * attribute on <html>, written SYNCHRONOUSLY inside the pointer-down handler,
 * so the pressed shell paints on the very next frame. It never waits on the
 * backend: a click that waits on an IPC round trip before showing anything
 * feels dead.
 */
export const PRESSED_ATTR = "data-pill-pressed";
/** The same attribute as a `dataset` key. */
export const PRESSED_DATASET_KEY = "pillPressed";
/** The pressed shell's scale (pill.html: 96 percent). */
export const PRESSED_SCALE = 0.96;
/** Budget, pointer-down to the pressed visual on screen (acceptance 2). */
export const PRESS_VISUAL_BUDGET_MS = 50;

/** Show (true) or clear (false) the pressed shell. Idempotent. */
export function markPressed(root: HTMLElement, on: boolean): void {
  if (on) root.dataset[PRESSED_DATASET_KEY] = "";
  else delete root.dataset[PRESSED_DATASET_KEY];
}

/** Is the pressed shell showing? */
export function isPressed(root: HTMLElement): boolean {
  return root.dataset[PRESSED_DATASET_KEY] !== undefined;
}

/**
 * Milliseconds from the pointer-down event to the frame that painted the
 * pressed shell, or null when a clock is missing or ran backwards. Both are
 * `performance.now()`-based (`Event.timeStamp` shares its origin).
 */
export function pressVisualLatency(downAt: number, paintedAt: number): number | null {
  if (!Number.isFinite(downAt) || !Number.isFinite(paintedAt) || downAt <= 0) return null;
  const ms = paintedAt - downAt;
  return ms >= 0 ? Math.round(ms * 10) / 10 : null;
}

/** The structured log line for one press (no coordinates, no text). */
export function pressLogLine(latencyMs: number | null): string {
  const over = latencyMs !== null && latencyMs > PRESS_VISUAL_BUDGET_MS;
  return `pill press event=pressed_visual latency_ms=${latencyMs ?? "unknown"} budget_ms=${PRESS_VISUAL_BUDGET_MS} over_budget=${over}`;
}
