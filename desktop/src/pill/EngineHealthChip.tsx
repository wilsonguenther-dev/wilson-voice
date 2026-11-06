/**
 * EngineHealthChip — one small, plain-words engine state for the pill and
 * Diagnostics (yap-1106-engine-owner). `role="status"` so a screen reader
 * hears "Catching up…" once, politely. No animation, so reduce-motion has
 * nothing to turn off.
 */
import { engineHealthLabel, type EngineHealth } from "./engineHealth";

export function EngineHealthChip({ state }: { state: EngineHealth }) {
  return (
    <span className="engine-health" data-state={state} role="status" aria-live="polite">
      {engineHealthLabel(state)}
    </span>
  );
}
