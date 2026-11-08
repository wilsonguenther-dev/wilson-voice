// WF-06 — the pill's "Backtrack caught" toast. Mounted by float-main.tsx
// beside the language offer. Undo pastes the raw take; Dismiss / Esc closes;
// unanswered, it leaves on its own after BACKTRACK_CAUGHT_TTL_MS, and a new
// recording clears it so it never sits over a live take.
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { holdPillHitbox } from "./drag";
import { pillMessageAllowed } from "./live";
import {
  BACKTRACK_CAUGHT_EVENT,
  BACKTRACK_CAUGHT_TTL_MS,
  BACKTRACK_UNDO_COMMAND,
  backtrackCaughtLines,
  backtrackUndoArgs,
  isBacktrackCaught,
  type BacktrackCaught,
} from "./backtrackCaught";

/** Stable default so the listener effect does not re-subscribe every render. */
const NOTHING_MUTED: ReadonlySet<string> = new Set();

/** Paste the take as dictated. The ONLY thing Undo does. */
export function undoBacktrack(
  caught: BacktrackCaught,
  call: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke,
): Promise<unknown> {
  return call(BACKTRACK_UNDO_COMMAND, backtrackUndoArgs(caught));
}

/** Presentational: no hooks, so a test can render it and press its buttons. */
export function BacktrackCaughtView({
  caught,
  error,
  busy,
  onUndo,
  onDismiss,
}: {
  caught: BacktrackCaught;
  error?: string | null;
  busy?: boolean;
  onUndo: () => void;
  onDismiss: () => void;
}) {
  const { label, headline, undo, dismiss } = backtrackCaughtLines(caught);
  return (
    <div
      className="backtrack-caught"
      role="status"
      aria-label={`${label}. ${headline}`}
      data-backtrack-caught={caught.id}
    >
      <p className="backtrack-caught-label">{label}</p>
      <p className="backtrack-caught-headline">{headline}</p>
      {error && (
        <p className="backtrack-caught-error" role="alert">
          {error}
        </p>
      )}
      <div className="backtrack-caught-actions">
        <button type="button" className="backtrack-caught-undo" data-pill-control="backtrack-caught-undo" disabled={busy} onClick={onUndo}>
          {undo}
        </button>
        <button type="button" className="backtrack-caught-dismiss" data-pill-control="backtrack-caught-dismiss" disabled={busy} onClick={onDismiss}>
          {dismiss}
        </button>
      </div>
    </div>
  );
}

export default function BacktrackCaughtCard({
  forced = null,
  muted = NOTHING_MUTED,
}: {
  /** A dev-only matrix scene's toast (`forcedBacktrackCaught`); `null` in a release. */
  forced?: BacktrackCaught | null;
  /** WF-17 muted pill categories. This toast is `status`, so it is never muted. */
  muted?: ReadonlySet<string>;
} = {}) {
  const [caught, setCaught] = useState<BacktrackCaught | null>(forced);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  const show = useCallback(
    (c: unknown) => {
      if (!isBacktrackCaught(c) || !pillMessageAllowed("backtrack-caught", muted)) return;
      setCaught(c);
      setError(null);
      setBusy(false);
    },
    [muted],
  );

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    listen<BacktrackCaught>(BACKTRACK_CAUGHT_EVENT, (e) => show(e.payload)).then(push);
    // A new take starts: the toast is about the LAST take, so it goes.
    listen<boolean>("recording", (e) => {
      if (e.payload) setCaught(null);
    }).then(push);
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [show]);

  const dismiss = useCallback(() => setCaught(null), []);

  const undo = useCallback(() => {
    if (!caught) return;
    setBusy(true);
    undoBacktrack(caught)
      .then(() => setCaught(null))
      .catch(() => {
        setBusy(false);
        setError(backtrackCaughtLines(caught).undoFailed);
      });
  }, [caught]);

  useEffect(() => {
    if (!caught) return;
    // The float window is click-through outside the reported hitbox; while
    // the toast is up it IS the hitbox, so Undo and Dismiss can be clicked.
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "backtrack-caught");
    }
    // An error stays until answered; a forced matrix scene stays for the photo.
    const ttl = error || forced ? null : window.setTimeout(() => setCaught(null), BACKTRACK_CAUGHT_TTL_MS);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      if (ttl !== null) window.clearTimeout(ttl);
      window.removeEventListener("keydown", onKey);
      holdPillHitbox(null, "backtrack-caught");
    };
  }, [caught, error, forced, dismiss]);

  if (!caught) return null;
  return (
    <div ref={ref} className="backtrack-caught-wrap">
      <BacktrackCaughtView caught={caught} error={error} busy={busy} onUndo={undo} onDismiss={dismiss} />
    </div>
  );
}
