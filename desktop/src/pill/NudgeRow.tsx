/**
 * Y8-D — the coaching nudge, drawn. The `growthNudge` bar state: one row,
 * fit-content x 32, above the capsule on the bottom dock and hugging the edge
 * on a side dock (float.css `.nudge-row`). The DECISION is `nudge.ts`'s; this
 * component only feeds it what the app knows and shows what it returns.
 *
 *   - evaluated only once the pill is back at REST after a take, so a nudge is
 *     never drawn while a take is live, and a take starting hides it for good;
 *   - one click anywhere on the row dismisses it, and it leaves on its own after
 *     NUDGE_TTL_MS so it never holds the window's hitbox for long;
 *   - a take that raised the Copy card does not also get a nudge: the card is
 *     already the lesson, and the two would fight over the same hitbox.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  NUDGE_TTL_MS,
  TAKE_CONTEXT_EVENT,
  daysSince,
  isTakeContext,
  nextNudge,
  type Feature,
  type Nudge,
  type TakeContextPayload,
} from "./nudge";
import { INSERT_COPY_CARD_EVENT } from "./copyCardModel";
import { holdPillHitbox } from "./drag";
import type { ChatTone, LivePhase } from "./live";

/** nudge.rs `NudgeLedger` (serde camelCase). */
interface LedgerPayload {
  firstSeenMs: number;
  shown: string[];
  successfulTakes: number;
  meetingsRecorded: number;
}

/** The phases a nudge may appear in: the pill at rest. */
const REST: ReadonlySet<LivePhase> = new Set<LivePhase>(["idle", "sleepy"]);

export default function NudgeRow({ phase, tone }: { phase: LivePhase; tone: ChatTone }) {
  const [ledger, setLedger] = useState<LedgerPayload | null>(null);
  const [active, setActive] = useState<Nudge | null>(null);
  // Mutable bookkeeping that must not re-render the float on every take.
  const ever = useRef<Set<string>>(new Set());
  const thisSession = useRef(0);
  const sessionTakes = useRef(0);
  const lastTake = useRef<TakeContextPayload | null>(null);
  const pending = useRef(false);
  const cardThisTake = useRef(false);
  const ref = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    invoke<LedgerPayload>("nudge_ledger")
      .then((l) => {
        if (dead || !l) return;
        (l.shown ?? []).forEach((id) => ever.current.add(id));
        setLedger(l);
      })
      .catch(() => {});
    listen<unknown>(TAKE_CONTEXT_EVENT, (e) => {
      if (!isTakeContext(e.payload)) return;
      lastTake.current = e.payload;
      sessionTakes.current += 1;
      cardThisTake.current = false;
      pending.current = true;
    }).then((u) => (dead ? u() : unsubs.push(u)));
    listen(INSERT_COPY_CARD_EVENT, () => {
      cardThisTake.current = true;
    }).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, []);

  const dismiss = useCallback(() => setActive(null), []);

  // Decide, once per take, when the pill is back at rest.
  useEffect(() => {
    const atRest = REST.has(phase);
    if (!atRest) {
      // A take is live: never draw over it, and a hidden lesson stays spent.
      setActive(null);
      return;
    }
    if (!pending.current || !ledger) return;
    pending.current = false;
    if (cardThisTake.current) return;
    const unused: Feature[] = ledger.meetingsRecorded > 0 ? [] : ["meetings"];
    const n = nextNudge(
      {
        takeLive: !atRest,
        lastTake: lastTake.current,
        // The ledger's count was read at mount; this session's takes are on top.
        takesSoFar: ledger.successfulTakes + sessionTakes.current,
        daysSinceInstall: daysSince(ledger.firstSeenMs, Date.now()),
        unusedFeatures: unused,
      },
      { thisSession: thisSession.current, ever: ever.current },
    );
    if (!n) return;
    thisSession.current += 1;
    ever.current.add(n.id);
    invoke("nudge_mark_shown", { condition: n.id }).catch(() => {});
    setActive(n);
  }, [phase, ledger]);

  // While up, the row IS the window's hitbox (the float is click-through
  // elsewhere), exactly as the Copy card does; it leaves on its own.
  useEffect(() => {
    document.documentElement.dataset.growthNudge = active?.id ?? "";
    if (!active) return;
    const el = ref.current;
    if (el) {
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "growth-nudge");
    }
    const t = setTimeout(dismiss, NUDGE_TTL_MS);
    return () => {
      clearTimeout(t);
      holdPillHitbox(null, "growth-nudge");
    };
  }, [active, dismiss]);

  if (!active) return null;
  return (
    <div
      ref={ref}
      className="nudge-row"
      data-nudge={active.id}
      role="note"
      aria-label={`Tip: ${active.copy[tone]}`}
    >
      {/* Y5-J: the action is the × button; a clickable <div> has no keyboard path. */}
      <span className="nudge-text">{active.copy[tone]}</span>
      <button type="button" className="nudge-close" aria-label="Dismiss tip" onClick={dismiss}>
        ×
      </button>
    </div>
  );
}
