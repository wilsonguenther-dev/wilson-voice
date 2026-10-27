// yap-notetaker-detect — Settings › Meetings. Two toggles, no auto-start
// setting (there is none, by design: Yap only ever OFFERS to record).
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppCtx } from "../../appShell";

/** Mirrors `meeting_detect::DetectSettingsView`. */
interface DetectSettingsView {
  offerDetected: boolean;
  useCalendar: boolean;
  calendarAccess: string;
}

export default function Meetings() {
  const { toast } = useAppCtx();
  const [view, setView] = useState<DetectSettingsView | null>(null);
  useEffect(() => {
    let dead = false;
    invoke<DetectSettingsView>("meeting_detect_settings")
      .then((v) => { if (!dead) setView(v); })
      .catch((e) => toast(String(e)));
    return () => { dead = true; };
    // Read once per mount; `toast` is not a reason to read again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function save(patch: { offerDetected?: boolean; useCalendar?: boolean }) {
    try {
      setView(await invoke<DetectSettingsView>("set_meeting_detect_settings", patch));
    } catch (e) {
      toast(String(e));
    }
  }

  if (!view) return null;
  const calendarReady = view.calendarAccess === "full_access";
  return (
    <section className="settings-panel" data-settings-pane="meetings">
      <div className="panel" data-row>
        <h3>Meeting detection</h3>
        <label className="toggle">
          <input
            type="checkbox"
            checked={view.offerDetected}
            onChange={(e) => void save({ offerDetected: e.target.checked })}
          />
          <span>Offer to record detected meetings</span>
        </label>
        <label className="toggle">
          <input
            type="checkbox"
            checked={view.useCalendar}
            disabled={!view.useCalendar && !calendarReady}
            onChange={(e) => void save({ useCalendar: e.target.checked })}
          />
          <span>
            Use my calendar
            {!calendarReady && !view.useCalendar && (
              <> — connect your calendar from the Meetings setup card first</>
            )}
          </span>
        </label>
        <p className="settings-hint">
          Yap can notice a meeting and ask if you want to record it. It never
          starts recording on its own.
        </p>
      </div>
    </section>
  );
}
