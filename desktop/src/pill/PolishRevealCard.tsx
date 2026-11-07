// WF-05 — the pill's "Polished — Reveal" toast. Mounted by float-main.tsx
// beside the Backtrack caught toast. The toast offers Reveal; Reveal opens the
// take's diff (the shared DiffView) with Use original (the Undo AI Edit paste)
// and Keep. yap-1107-reveal-stays-until-read: unanswered, the toast stays at
// least 8 s, never leaves while the pointer is on it or within 2 s of its last
// move there, and never leaves on its own with "Keep the reveal open until I
// close it" on (`revealDwell.ts`); the opened card waits for an answer. Esc
// (read by the backend, the webview keydown is a fallback), a click outside
// (backend), the close button, Keep / Dismiss and a new recording close either
// face at once. The Tips switch in Settings › Notifications mutes it
// (`live.ts` PILL_MESSAGES "polish-reveal").
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import DiffView from "../DiffView";
import type { DiffLayout } from "../diff";
import type { AppStatus } from "../appTypes";
import { holdPillHitbox } from "./drag";
import { PILL_HOVER_OUT_EVENT } from "./hitbox";
import { pillMessageAllowed } from "./live";
import {
  LOCAL_ROUTE_LABEL,
  POLISH_REVEAL_CLOSE_EVENT,
  POLISH_REVEAL_EVENT,
  REVEAL_STATE_COMMAND,
  USE_ORIGINAL_COMMAND,
  isPolishReveal,
  polishRevealLines,
  resultRouteLabel,
  revealRouteLabel,
  useOriginalArgs,
  useOriginalError,
  type PolishReveal,
  type PolishRevealFace,
} from "./polishReveal";
import { POLISH_RESULT_EVENT, isPolishResult } from "./polishResult";
import { createRevealDwell, type RevealCloseReason, type RevealDwell } from "./revealDwell";

/** Stable default so the listener effect does not re-subscribe every render. */
const NOTHING_MUTED: ReadonlySet<string> = new Set();

/** Paste the take as dictated over the polished one. The ONLY thing Use original does. */
export function pasteOriginal(
  reveal: PolishReveal,
  call: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke,
): Promise<unknown> {
  return call(USE_ORIGINAL_COMMAND, useOriginalArgs(reveal));
}

/**
 * Tell the backend the reveal opened ("shown") or closed (why, and how long it
 * was up): it arms / disarms the Esc + click-outside watch and writes the one
 * telemetry line. Never blocks the card; a missing backend (the headless
 * matrix) is ignored.
 */
export function reportRevealState(
  open: boolean,
  reason: RevealCloseReason | "shown",
  dwellMs: number,
  call: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke,
): Promise<unknown> {
  return call(REVEAL_STATE_COMMAND, { open, reason, dwellMs: Math.round(dwellMs) }).catch(() => undefined);
}

/** The backend's close event payload, if it is one of the two it may send. */
export function backendCloseReason(payload: unknown): "esc" | "click_outside" | null {
  return payload === "esc" || payload === "click_outside" ? payload : null;
}

/** The close button: an X glyph with an accessible name. */
function CloseButton({ label, busy, onClick }: { label: string; busy?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      className="polish-reveal-close"
      aria-label={label}
      title={label}
      disabled={busy}
      onClick={onClick}
    >
      <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true" focusable="false">
        <path d="M2 2l6 6M8 2l-6 6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" fill="none" />
      </svg>
    </button>
  );
}

/** Should a `polish_reveal` payload reach the pill? The payload check plus the WF-17 mute gate. */
export function polishRevealShows(payload: unknown, muted: ReadonlySet<string>): payload is PolishReveal {
  return isPolishReveal(payload) && pillMessageAllowed("polish-reveal", muted);
}

/** Presentational: no hooks, so a test can render it and press its buttons. */
export function PolishRevealView({
  reveal,
  view,
  route = LOCAL_ROUTE_LABEL,
  error,
  busy,
  layout = null,
  onLayoutChange,
  onReveal,
  onUseOriginal,
  onKeep,
  onClose,
}: {
  reveal: PolishReveal;
  view: PolishRevealFace;
  route?: string;
  error?: string | null;
  busy?: boolean;
  /**
   * yap-1107-polish-diff-readable — the "Show as before/after" choice for this
   * card session; null = the diff's own verdict (blocks when changes are dense).
   */
  layout?: DiffLayout | null;
  onLayoutChange?: (layout: DiffLayout) => void;
  onReveal: () => void;
  onUseOriginal: () => void;
  onKeep: () => void;
  /** The close button (top right of either face). Defaults to `onKeep`. */
  onClose?: () => void;
}) {
  const lines = polishRevealLines(reveal);
  const close = onClose ?? onKeep;
  if (view === "toast") {
    return (
      <div className="polish-reveal polish-reveal-toast" role="status" aria-label={lines.aria} data-polish-reveal={reveal.id}>
        <p className="polish-reveal-label">{lines.label}</p>
        <div className="polish-reveal-actions">
          <button type="button" className="polish-reveal-open" onClick={onReveal}>
            {lines.reveal}
          </button>
          <button type="button" className="polish-reveal-dismiss" onClick={onKeep}>
            {lines.dismiss}
          </button>
          <CloseButton label={lines.close} onClick={close} />
        </div>
      </div>
    );
  }
  return (
    <div
      className="polish-reveal polish-reveal-card"
      role="dialog"
      aria-label={lines.title}
      data-polish-reveal={reveal.id}
    >
      <p className="polish-reveal-title">{lines.title}</p>
      <DiffView
        raw={reveal.raw}
        formatted={reveal.text}
        stages={reveal.stages}
        route={route}
        layout={layout}
        onLayoutChange={onLayoutChange}
      />
      {error && (
        <p className="polish-reveal-error" role="alert">
          {error}
        </p>
      )}
      <div className="polish-reveal-actions">
        <button type="button" className="polish-reveal-original" disabled={busy} onClick={onUseOriginal}>
          {lines.useOriginal}
        </button>
        <button type="button" className="polish-reveal-keep" disabled={busy} onClick={onKeep}>
          {lines.keep}
        </button>
      </div>
      <CloseButton label={lines.close} busy={busy} onClick={close} />
    </div>
  );
}

export default function PolishRevealCard({
  forced = null,
  muted = NOTHING_MUTED,
  keepOpen = false,
}: {
  /** A dev-only matrix scene (`forcedPolishReveal`); `null` in a release. */
  forced?: { reveal: PolishReveal; view: PolishRevealFace; layout?: DiffLayout; hover?: true } | null;
  /** WF-17 muted pill categories. This toast is a `tip`, so muting Tips hides it. */
  muted?: ReadonlySet<string>;
  /** Settings › Notifications "Keep the reveal open until I close it" (`keepPolishRevealOpen`). */
  keepOpen?: boolean;
} = {}) {
  const [reveal, setReveal] = useState<PolishReveal | null>(forced?.reveal ?? null);
  const [view, setView] = useState<PolishRevealFace>(forced?.view ?? "toast");
  const [route, setRoute] = useState<string>(LOCAL_ROUTE_LABEL);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [pointerIn, setPointerIn] = useState<boolean>(forced?.hover === true);
  // The toggle's choice lives for the card session: kept while this take's
  // toast/card is up, cleared when the next take arrives.
  const [layout, setLayout] = useState<DiffLayout | null>(forced?.layout ?? null);
  const ref = useRef<HTMLDivElement | null>(null);

  // The dwell rule + timer (`revealDwell.ts`). One per mount; every close path
  // goes through `closeWith`, so each close is logged exactly once.
  const dwellRef = useRef<RevealDwell | null>(null);
  if (dwellRef.current === null) {
    dwellRef.current = createRevealDwell(keepOpen, {
      onClose: (reason, dwellMs) => {
        setReveal(null);
        setPointerIn(false);
        void reportRevealState(false, reason, dwellMs);
      },
    });
  }
  const closeWith = useCallback((reason: RevealCloseReason) => {
    const d = dwellRef.current;
    if (d && d.isOpen()) d.close(reason);
    else setReveal(null);
  }, []);

  const show = useCallback(
    (p: unknown) => {
      if (!polishRevealShows(p, muted)) return;
      setReveal(p);
      setView("toast");
      setLayout(null);
      setError(null);
      setBusy(false);
      setPointerIn(false);
      dwellRef.current?.show("toast");
      void reportRevealState(true, "shown", 0);
    },
    [muted],
  );

  useEffect(() => {
    dwellRef.current?.setKeepOpen(keepOpen);
  }, [keepOpen]);

  useEffect(() => {
    const d = dwellRef.current;
    return () => d?.dispose();
  }, []);

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<PolishReveal>(POLISH_REVEAL_EVENT, (e) => show(e.payload)).then(push);
    // The route chip's words for where polish ran ("Cloud" / "On this Mac").
    const onStatus = (s: Pick<AppStatus, "cloudRoute"> | null | undefined) => setRoute(revealRouteLabel(s));
    invoke<Pick<AppStatus, "cloudRoute">>("get_status").then(onStatus).catch(() => {});
    listen<Pick<AppStatus, "cloudRoute">>("status", (e) => onStatus(e.payload)).then(push);
    // yap-1106-byom — the take's own route (`polish_result`, emitted right
    // before `polish_reveal`), so the card names where THIS polish ran.
    listen<unknown>(POLISH_RESULT_EVENT, (e) => {
      if (isPolishResult(e.payload)) setRoute(resultRouteLabel(e.payload));
    }).then(push);
    // A new take starts: the toast is about the LAST take, so it goes.
    listen<boolean>("recording", (e) => {
      if (e.payload) closeWith("new_recording");
    }).then(push);
    // Esc / a click outside, read by the backend hover watch (the panel is
    // normally never key, so the webview keydown below rarely fires).
    listen<unknown>(POLISH_REVEAL_CLOSE_EVENT, (e) => {
      const why = backendCloseReason(e.payload);
      if (why) closeWith(why);
    }).then(push);
    // The panel went click-through again: the pointer left the reveal, and
    // the webview may never see a pointerleave.
    listen<unknown>(PILL_HOVER_OUT_EVENT, () => {
      dwellRef.current?.pointerLeave();
      setPointerIn(false);
    }).then(push);
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [show, closeWith]);

  // Muting Tips while the toast is up takes it down too.
  useEffect(() => {
    if (!pillMessageAllowed("polish-reveal", muted)) closeWith("muted");
  }, [muted, closeWith]);

  const keep = useCallback(() => closeWith("keep"), [closeWith]);
  const closeButton = useCallback(() => closeWith("close_button"), [closeWith]);
  const open = useCallback(() => {
    setView("card");
    dwellRef.current?.setFace("card");
  }, []);
  const pointerEnter = useCallback(() => {
    dwellRef.current?.pointerEnter();
    setPointerIn(true);
  }, []);
  const pointerMove = useCallback(() => dwellRef.current?.pointerMove(), []);
  const pointerLeave = useCallback(() => {
    dwellRef.current?.pointerLeave();
    setPointerIn(false);
  }, []);

  const original = useCallback(() => {
    if (!reveal) return;
    setBusy(true);
    setError(null);
    pasteOriginal(reveal)
      .then(() => closeWith("use_original"))
      .catch((e: unknown) => {
        setBusy(false);
        setError(useOriginalError(e));
      });
  }, [reveal, closeWith]);

  useEffect(() => {
    if (!reveal) return;
    // The float window is click-through outside the reported hitbox; while
    // the toast or card is up it IS the hitbox, so its buttons can be clicked.
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "polish-reveal");
    }
    // The timer is `revealDwell.ts` (a forced matrix scene never starts it).
    // Esc here is only the fallback: the panel is normally never key, so the
    // backend reads Esc and sends POLISH_REVEAL_CLOSE_EVENT.
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeWith("esc");
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      holdPillHitbox(null, "polish-reveal");
    };
  }, [reveal, view, layout, closeWith]);

  if (!reveal) return null;
  return (
    <div
      ref={ref}
      className="polish-reveal-wrap"
      data-view={view}
      data-pointer={pointerIn ? "inside" : undefined}
      data-keep-open={keepOpen ? "true" : undefined}
      onPointerEnter={pointerEnter}
      onPointerMove={pointerMove}
      onPointerLeave={pointerLeave}
    >
      <PolishRevealView
        reveal={reveal}
        view={view}
        route={route}
        error={error}
        busy={busy}
        layout={layout}
        onLayoutChange={setLayout}
        onReveal={open}
        onUseOriginal={original}
        onKeep={keep}
        onClose={closeButton}
      />
    </div>
  );
}
