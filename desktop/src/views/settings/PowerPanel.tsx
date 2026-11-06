// yap-1106-resource-governor — Settings › Advanced › "Power and heat".
// One sentence about what the Mac can afford right now, the one number the
// user can move, and "Run anyway" when something is being held.
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppCtx } from "../../appShell";
import { useGovernor, runWaitingJobsAnyway } from "../../pill/GovernorChip";
import { governorSentence, pauseBelowFromInput, type GovernorView } from "../../pill/governor";

export default function PowerPanel() {
  const { toastError } = useAppCtx();
  const live = useGovernor();
  const [local, setLocal] = useState<GovernorView | null>(null);
  const view = local ?? live;
  const pauseBelow = view?.pauseBelowPercent ?? 20;
  const [draft, setDraft] = useState<string | null>(null);

  async function commit(raw: string) {
    setDraft(null);
    const percent = pauseBelowFromInput(raw, pauseBelow);
    if (percent === pauseBelow) return;
    try {
      setLocal(await invoke<GovernorView>("set_governor_pause_below", { percent }));
    } catch (e) {
      toastError(e);
    }
  }

  async function runAnyway() {
    try {
      const next = await runWaitingJobsAnyway();
      if (next) setLocal(next);
    } catch (e) {
      toastError(e);
    }
  }

  const waiting = view?.waitingJobs.length ?? 0;
  return (
    <div className="panel" data-row data-governor-panel>
      <h3>Power and heat</h3>
      <p className="settings-hint" data-governor-sentence>
        {governorSentence(view)}
      </p>
      <label className="toggle">
        <span>Pause heavy work on battery below</span>
        <input
          type="number"
          min={1}
          max={99}
          step={1}
          value={draft ?? String(pauseBelow)}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={(e) => void commit(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          }}
          aria-label="Pause heavy work on battery below this percent"
          style={{ width: "4.5em", marginLeft: 8 }}
        />
        <span>%</span>
      </label>
      {view?.heavyWorkDeferred && waiting > 0 && (
        <button type="button" className="secondary" onClick={() => void runAnyway()}>
          Run anyway ({waiting})
        </button>
      )}
      <p className="settings-hint">
        Dictation always runs. Meeting transcription, notes and speaker labels wait
        for wall power under this percent, and polish steps aside when the Mac is
        hot, in Low Power Mode or low on memory.
      </p>
    </div>
  );
}
