/**
 * yap-1106-partial-transcript-stream — the hook half: listens for SP-05's
 * `transcript_partial` and hands the pill the newest words, throttled.
 *
 * Display state ONLY: this hook never invokes a command and never stores the
 * words anywhere but React state, so a partial can never reach history (the
 * history row is still written from the final `transcript`, unchanged).
 * Energy: no polling and no interval. A redraw is one setState, at most once
 * every PARTIAL_RENDER_MIN_MS, scheduled only when a partial actually arrives.
 */
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { GovernorView } from "./governor";
import {
  governorAllowsPartials, nextRenderDelay, PARTIAL_EMPTY, PARTIAL_RENDER_MIN_MS, partialTakeLogLine,
  reducePartial, TRANSCRIPT_PARTIAL_EVENT, type PartialEvent, type PartialState, type PartialTakeStats,
} from "./partialTranscript";

/** The newest live words for the current take ("" = none). */
export function usePartialTranscript(governor: GovernorView | null): string {
  const [text, setText] = useState("");
  const state = useRef<PartialState>(PARTIAL_EMPTY);
  const lastRender = useRef<number | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const takeStart = useRef<number | null>(null);
  const lastPartialAt = useRef<number | null>(null);
  const stats = useRef<PartialTakeStats | null>(null);
  // The governor is read inside the listener; a ref keeps the listener mounted once.
  const governorRef = useRef<GovernorView | null>(governor);
  governorRef.current = governor;

  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));

    const flush = () => {
      timer.current = null;
      if (dead) return;
      lastRender.current = performance.now();
      setText(state.current.text);
    };
    const schedule = () => {
      if (timer.current !== null) return; // a trailing redraw is already due: it reads the newest state
      const wait = nextRenderDelay(lastRender.current, performance.now(), PARTIAL_RENDER_MIN_MS);
      if (wait === 0) flush();
      else timer.current = setTimeout(flush, wait);
    };
    const clearNow = () => {
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
      lastRender.current = null;
      setText("");
    };
    const endTake = () => {
      const s = stats.current;
      if (s && s.updates > 0) console.info(partialTakeLogLine(s));
      stats.current = null;
      lastPartialAt.current = null;
    };
    const apply = (e: PartialEvent) => {
      const before = state.current;
      state.current = reducePartial(before, e);
      if (e.type === "partial") {
        if (state.current === before) return; // stale or malformed: dropped
        const now = performance.now();
        const take = state.current.take ?? 0;
        if (!stats.current || stats.current.take !== take) {
          endTake();
          stats.current = {
            take,
            updates: 0,
            firstMs: takeStart.current === null ? null : now - takeStart.current,
            maxGapMs: 0,
            hiddenByGovernor: 0,
          };
        }
        const s = stats.current;
        s.updates += 1;
        if (lastPartialAt.current !== null) s.maxGapMs = Math.max(s.maxGapMs, now - lastPartialAt.current);
        lastPartialAt.current = now;
        if (!governorAllowsPartials(governorRef.current)) {
          // Off under load: count it, draw nothing, keep no pending redraw.
          s.hiddenByGovernor += 1;
          return;
        }
        schedule();
        return;
      }
      if (e.type === "recording" && e.recording) {
        endTake();
        takeStart.current = performance.now();
        clearNow();
      } else if (e.type === "end") {
        endTake();
        takeStart.current = null;
        clearNow();
      }
    };

    listen<unknown>(TRANSCRIPT_PARTIAL_EVENT, (e) => apply({ type: "partial", payload: e.payload })).then(push);
    listen<boolean>("recording", (e) => apply({ type: "recording", recording: e.payload === true })).then(push);
    listen<unknown>("transcript", () => apply({ type: "end" })).then(push);
    listen<unknown>("transcript_error", () => apply({ type: "end" })).then(push);
    listen<unknown>("take_cancelled", () => apply({ type: "end" })).then(push);
    return () => {
      dead = true;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
      unsubs.forEach((u) => u());
    };
  }, []);

  return text;
}
