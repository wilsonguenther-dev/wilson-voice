// WF-05 — the pill's "Polished — Reveal" toast. Mounted by float-main.tsx
// beside the Backtrack caught toast. The toast offers Reveal; Reveal opens the
// take's diff (the shared DiffView) with Use original (the Undo AI Edit paste)
// and Keep. Esc closes either face. Unanswered, the toast leaves after
// POLISH_REVEAL_TTL_MS; the opened card waits for an answer. A new recording
// clears it so it never sits over a live take, and the Tips switch in
// Settings › Notifications mutes it (`live.ts` PILL_MESSAGES "polish-reveal").
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import DiffView from "../DiffView";
import type { AppStatus } from "../appTypes";
import { holdPillHitbox } from "./drag";
import { pillMessageAllowed } from "./live";
import {
  LOCAL_ROUTE_LABEL,
  POLISH_REVEAL_EVENT,
  POLISH_REVEAL_TTL_MS,
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

/** Stable default so the listener effect does not re-subscribe every render. */
const NOTHING_MUTED: ReadonlySet<string> = new Set();

/** Paste the take as dictated over the polished one. The ONLY thing Use original does. */
export function pasteOriginal(
  reveal: PolishReveal,
  call: (cmd: string, args: Record<string, unknown>) => Promise<unknown> = invoke,
): Promise<unknown> {
  return call(USE_ORIGINAL_COMMAND, useOriginalArgs(reveal));
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
  onReveal,
  onUseOriginal,
  onKeep,
}: {
  reveal: PolishReveal;
  view: PolishRevealFace;
  route?: string;
  error?: string | null;
  busy?: boolean;
  onReveal: () => void;
  onUseOriginal: () => void;
  onKeep: () => void;
}) {
  const lines = polishRevealLines(reveal);
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
      <DiffView raw={reveal.raw} formatted={reveal.text} stages={reveal.stages} route={route} />
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
    </div>
  );
}

export default function PolishRevealCard({
  forced = null,
  muted = NOTHING_MUTED,
}: {
  /** A dev-only matrix scene (`forcedPolishReveal`); `null` in a release. */
  forced?: { reveal: PolishReveal; view: PolishRevealFace } | null;
  /** WF-17 muted pill categories. This toast is a `tip`, so muting Tips hides it. */
  muted?: ReadonlySet<string>;
} = {}) {
  const [reveal, setReveal] = useState<PolishReveal | null>(forced?.reveal ?? null);
  const [view, setView] = useState<PolishRevealFace>(forced?.view ?? "toast");
  const [route, setRoute] = useState<string>(LOCAL_ROUTE_LABEL);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);
  const isForced = forced !== null;

  const show = useCallback(
    (p: unknown) => {
      if (!polishRevealShows(p, muted)) return;
      setReveal(p);
      setView("toast");
      setError(null);
      setBusy(false);
    },
    [muted],
  );

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
      if (e.payload) setReveal(null);
    }).then(push);
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [show]);

  // Muting Tips while the toast is up takes it down too.
  useEffect(() => {
    if (!pillMessageAllowed("polish-reveal", muted)) setReveal(null);
  }, [muted]);

  const keep = useCallback(() => setReveal(null), []);
  const open = useCallback(() => setView("card"), []);

  const original = useCallback(() => {
    if (!reveal) return;
    setBusy(true);
    setError(null);
    pasteOriginal(reveal)
      .then(() => setReveal(null))
      .catch((e: unknown) => {
        setBusy(false);
        setError(useOriginalError(e));
      });
  }, [reveal]);

  useEffect(() => {
    if (!reveal) return;
    // The float window is click-through outside the reported hitbox; while
    // the toast or card is up it IS the hitbox, so its buttons can be clicked.
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "polish-reveal");
    }
    // Only the toast times out; an opened card or a forced matrix scene waits.
    const ttl =
      view === "toast" && !isForced ? window.setTimeout(() => setReveal(null), POLISH_REVEAL_TTL_MS) : null;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") keep();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      if (ttl !== null) window.clearTimeout(ttl);
      window.removeEventListener("keydown", onKey);
      holdPillHitbox(null, "polish-reveal");
    };
  }, [reveal, view, isForced, keep]);

  if (!reveal) return null;
  return (
    <div ref={ref} className="polish-reveal-wrap" data-view={view}>
      <PolishRevealView
        reveal={reveal}
        view={view}
        route={route}
        error={error}
        busy={busy}
        onReveal={open}
        onUseOriginal={original}
        onKeep={keep}
      />
    </div>
  );
}
