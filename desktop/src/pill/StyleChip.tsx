/**
 * WF-20 — the per-app style chip the pill shows while listening: "Slack · casual".
 * A click moves THIS app to the next app-aware mode for 15 minutes (the backend,
 * `pill_cycle_style`, owns the clock and the per-app scoping); the chip then
 * shows the new mode. Hidden when app-aware modes are off, outside listening,
 * and when no app could be read (see `chipShows`).
 *
 * It sits in the hint box the one layout already reserves (pill/state.ts): the
 * plain listening phase shows no hint line, so the chip takes that row. It is a
 * real <button> (Enter/Space work), and it widens the click-through hitbox under
 * its own owner so the capsule stays grabbable beside it.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { extendPillHitbox } from "./drag";
import type { LivePhase } from "./live";
import {
  chipAccessibleName, chipShows, CYCLE_STYLE_COMMAND, HIDDEN_CHIP, parseChip, STYLE_CHIP_EVENT,
  type StyleChip as Chip,
} from "./styleChip";

const HITBOX_OWNER = "style-chip";

export default function StyleChip({ phase, forced = null }: { phase: LivePhase; forced?: Chip | null }) {
  const [chip, setChip] = useState<Chip>(forced ?? HIDDEN_CHIP);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (forced) return;
    let dead = false;
    const offs: Array<() => void> = [];
    const push = (off: () => void) => { if (dead) off(); else offs.push(off); };
    listen<unknown>(STYLE_CHIP_EVENT, (e) => setChip(parseChip(e.payload))).then(push).catch(() => {});
    // A take that ends takes its chip with it; the next take sends its own.
    listen<boolean>("recording", (e) => { if (!e.payload) setChip(HIDDEN_CHIP); }).then(push).catch(() => {});
    return () => { dead = true; offs.forEach((off) => off()); };
  }, [forced]);

  const shows = chipShows(chip, phase);

  useEffect(() => {
    const el = ref.current;
    if (!shows || !el) {
      extendPillHitbox(null, HITBOX_OWNER);
      return;
    }
    const r = el.getBoundingClientRect();
    extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, HITBOX_OWNER);
    return () => extendPillHitbox(null, HITBOX_OWNER);
  }, [shows, chip.label]);

  const cycle = useCallback(() => {
    if (busy || forced) return;
    setBusy(true);
    invoke<unknown>(CYCLE_STYLE_COMMAND)
      .then((next) => setChip(parseChip(next)))
      .catch((err) => {
        // The backend refused (the take ended, app-aware modes went off, or no
        // app was read) and logged why; the chip has nothing true to show.
        console.warn("style_chip cycle failed:", err);
        setChip(HIDDEN_CHIP);
      })
      .finally(() => setBusy(false));
  }, [busy, forced]);

  if (!shows) return null;
  const name = chipAccessibleName(chip);
  return (
    <button
      ref={ref}
      type="button"
      className="style-chip"
      data-style-chip={chip.mode}
      data-overridden={chip.overridden ? "true" : "false"}
      aria-label={name}
      title={name}
      aria-busy={busy}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={cycle}
    >
      <span className="style-chip-app">{chip.app}</span>
      <span className="style-chip-sep" aria-hidden="true">{"·"}</span>
      <span className="style-chip-mode">{chip.mode}</span>
    </button>
  );
}
