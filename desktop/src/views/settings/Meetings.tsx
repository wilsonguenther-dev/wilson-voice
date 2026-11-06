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

/** yap-1106 — mirrors `MeetingExportLocation` in lib.rs. */
interface MeetingExportLocation {
  path: string;
  isDefault: boolean;
  defaultPath: string;
}

export default function Meetings() {
  const { toast, toastError } = useAppCtx();
  const [view, setView] = useState<DetectSettingsView | null>(null);
  // yap-1106 — the export folder: shown, openable, changeable.
  const [exportLoc, setExportLoc] = useState<MeetingExportLocation | null>(null);
  const [draftDir, setDraftDir] = useState("");
  useEffect(() => {
    let dead = false;
    invoke<MeetingExportLocation>("meeting_export_location")
      .then((v) => {
        if (dead) return;
        setExportLoc(v);
        setDraftDir(v.path);
      })
      .catch((e) => toastError(e, "Couldn't read the export folder setting."));
    return () => { dead = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  async function saveExportDir(path: string | null) {
    try {
      const v = await invoke<MeetingExportLocation>("set_meeting_export_dir", { path });
      setExportLoc(v);
      setDraftDir(v.path);
      toast(v.isDefault ? "Exports go to the default folder" : "Export folder saved");
    } catch (e) {
      toastError(e, "That folder couldn't be used. Pick one you can write to.");
    }
  }
  async function openExportDir() {
    try {
      await invoke<string>("open_meeting_export_dir");
    } catch (e) {
      toastError(e, "Couldn't open the meetings folder.");
    }
  }
  useEffect(() => {
    let dead = false;
    invoke<DetectSettingsView>("meeting_detect_settings")
      .then((v) => { if (!dead) setView(v); })
      .catch((e) => toastError(e));
    return () => { dead = true; };
    // Read once per mount; `toast` is not a reason to read again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function save(patch: { offerDetected?: boolean; useCalendar?: boolean }) {
    try {
      setView(await invoke<DetectSettingsView>("set_meeting_detect_settings", patch));
    } catch (e) {
      toastError(e);
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
      {/* yap-1106-call-audio-truth — the one sentence that explains the
          mechanism: a tap on this Mac's own sound, not a bot, not a plug-in. */}
      <div className="panel" data-row data-settings-row="call-audio">
        <h3>Call audio</h3>
        <p className="settings-hint">
          Yap records the call's sound from your Mac. No bot joins your call,
          nothing to install. While a meeting records, the recording bar and
          the pill show whether call audio is on, silent, or off.
        </p>
      </div>
      <div className="panel" data-row data-meeting-exports>
        <h3>Meeting exports</h3>
        <p className="settings-hint">
          Export Markdown and Show in Finder use this folder. Files are named
          by date and title. Everything stays on this Mac.
        </p>
        <label className="field">
          <span>Folder</span>
          <input
            type="text"
            value={draftDir}
            placeholder={exportLoc?.defaultPath ?? "~/Documents/Yap/Meetings"}
            aria-label="Export folder"
            onChange={(e) => setDraftDir(e.target.value)}
          />
        </label>
        <div className="actions">
          <button type="button" onClick={() => void openExportDir()}>
            Open meetings folder
          </button>
          <button
            type="button"
            className="ghost"
            disabled={!exportLoc || draftDir.trim() === exportLoc.path}
            onClick={() => void saveExportDir(draftDir)}
          >
            Save folder
          </button>
          {exportLoc && !exportLoc.isDefault && (
            <button type="button" className="ghost" onClick={() => void saveExportDir(null)}>
              Use default
            </button>
          )}
        </div>
      </div>
    </section>
  );
}
