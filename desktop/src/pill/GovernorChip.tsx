/**
 * GovernorChip — yap-1106-resource-governor's small state in the pill:
 * "Saving battery: notes after you plug in", with "Run anyway" when a job is
 * actually waiting. Rendered by BOTH pill styles from the same component.
 *
 * Energy: nothing here polls. The current view is read once on mount and the
 * backend emits `governor` on every transition (its own 30 s tick decides).
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { GOVERNOR_EVENT, governorChip, type GovernorView } from "./governor";

export function useGovernor(): GovernorView | null {
  const [view, setView] = useState<GovernorView | null>(null);
  useEffect(() => {
    let dead = false;
    const unsubs: Array<() => void> = [];
    const push = (u: () => void) => (dead ? u() : unsubs.push(u));
    invoke<GovernorView>("governor_status")
      .then((v) => { if (!dead) setView(v); })
      .catch(() => {});
    listen<GovernorView>(GOVERNOR_EVENT, (e) => setView(e.payload)).then(push);
    return () => { dead = true; unsubs.forEach((u) => u()); };
  }, []);
  return view;
}

/** "Run anyway" for every job the governor is holding right now. */
export async function runWaitingJobsAnyway(): Promise<GovernorView | null> {
  const fresh = await invoke<GovernorView>("governor_status");
  let last: GovernorView = fresh;
  for (const job of fresh.waitingJobs) {
    last = await invoke<GovernorView>("governor_run_anyway", { job });
  }
  return last;
}

export default function GovernorChip() {
  const view = useGovernor();
  const [busy, setBusy] = useState(false);
  const chip = governorChip(view);
  const onRunAnyway = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    setBusy(true);
    try {
      await runWaitingJobsAnyway();
    } catch {
      // The backend logs the refusal; the chip simply stays.
    } finally {
      setBusy(false);
    }
  }, []);
  if (!chip) return null;
  return (
    <div className={`governor-chip ${chip.tone}`} role="status" aria-live="polite" data-governor={chip.tone}>
      <span className="governor-dot" aria-hidden />
      <span className="governor-label">{chip.label}</span>
      {chip.runAnyway && (
        <button
          type="button"
          className="governor-run"
          disabled={busy}
          onClick={(e) => void onRunAnyway(e)}
          title="Transcribe now, on battery"
        >
          Run anyway
        </button>
      )}
    </div>
  );
}
