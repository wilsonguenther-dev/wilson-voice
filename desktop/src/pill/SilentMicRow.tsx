/**
 * WF-13 — the silent-mic row: "I can't hear <mic>" + a Switch mic button, in
 * the hint row while a take is recording (see `silentMic.ts` for the rules).
 *
 * Two rows: the line (fitted to the row, never cut off) over the button.
 * The take never stops: this row is advice, not an error. It is a real
 * <button> (Enter/Space work) and widens the click-through hitbox under its own
 * owner, exactly like WF-20's style chip, which it replaces in that row while it
 * is up (float.css `html[data-silent-mic="true"] .style-chip`).
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { extendPillHitbox } from "./drag";
import type { LivePhase } from "./live";
import { PILL_LAYOUT_EVENT } from "./state";
import {
  LIST_MICROPHONES_COMMAND, NO_OTHER_MIC_LINE, nextMicrophone, SET_MICROPHONE_COMMAND,
  silentMicAccessibleName, silentMicLine, silentMicShows, SWITCH_FAILED_LINE, SWITCH_MIC_LABEL, switchedLine,
  type SilentMicList,
} from "./silentMic";

const HITBOX_OWNER = "silent-mic";

export default function SilentMicRow({
  warning,
  phase,
  forced = false,
}: {
  warning: string | null;
  phase: LivePhase;
  forced?: boolean;
}) {
  const [after, setAfter] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const shows = silentMicShows(warning, phase);

  // A new warning (a new take) starts with the button, not last take's answer.
  // One line per warning: whether the drawn line had to be shortened to fit.
  useEffect(() => {
    setAfter(null);
    if (warning !== null) console.info("silent_mic row shown fitted=" + (silentMicLine(warning) === warning ? "0" : "1"));
  }, [warning]);

  useEffect(() => {
    document.documentElement.dataset.silentMic = shows ? "true" : "false";
    const el = ref.current;
    if (!shows || !el) {
      extendPillHitbox(null, HITBOX_OWNER);
      return;
    }
    // Re-take the hitbox after every layout too: the layout grows the hint
    // slot for this row after it mounts and MOVES it (a move is not a resize).
    const take = () => {
      const box = ref.current;
      if (!box || !box.isConnected) return;
      const r = box.getBoundingClientRect();
      extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, HITBOX_OWNER);
    };
    take();
    window.addEventListener(PILL_LAYOUT_EVENT, take);
    return () => {
      window.removeEventListener(PILL_LAYOUT_EVENT, take);
      extendPillHitbox(null, HITBOX_OWNER);
    };
  }, [shows, warning, after]);

  const switchMic = useCallback(() => {
    if (busy || forced) return;
    setBusy(true);
    invoke<SilentMicList>(LIST_MICROPHONES_COMMAND)
      .then((list) => {
        const next = nextMicrophone(list);
        if (!next) {
          console.info("silent_mic switch none_available");
          setAfter(NO_OTHER_MIC_LINE);
          return;
        }
        return invoke<SilentMicList>(SET_MICROPHONE_COMMAND, { id: next.id, name: next.name })
          .then(() => {
            console.info("silent_mic switch ok virtual=" + (next.isVirtual ? "1" : "0"));
            setAfter(switchedLine(next.name));
          });
      })
      .catch((err) => {
        console.warn("silent_mic switch failed:", err);
        setAfter(SWITCH_FAILED_LINE);
      })
      .finally(() => setBusy(false));
  }, [busy, forced]);

  if (!shows || warning === null) return null;
  return (
    <div
      ref={ref}
      className="silent-mic-row"
      data-silent-mic-row={after ? "switched" : "warning"}
      role="status"
      aria-label={after ?? silentMicAccessibleName(warning)}
      onPointerDown={(e) => e.stopPropagation()}
    >
      {/* The drawn line is fitted to the row (silentMic.ts silentMicLine);
          the tooltip and the accessible name keep the full warning. */}
      <span className="silent-mic-line" title={after ?? warning}>{after ?? silentMicLine(warning)}</span>
      {after === null && (
        <button
          type="button"
          className="silent-mic-switch"
          aria-busy={busy}
          onClick={switchMic}
        >
          {SWITCH_MIC_LABEL}
        </button>
      )}
    </div>
  );
}
