// WF-09 — Settings › Audio › Microphone. The same list the menu bar's
// "Microphone ▸" submenu draws (both come from `mic_select::list`), with
// "System Default" first. A pick is saved on this Mac and used by the next
// take; the list redraws on its own when a mic is plugged in or unplugged
// (`microphones-changed`, published by the backend device watcher).
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useAppCtx } from "../../appShell";

/** Mirrors `mic_select::MicDevice`. */
export interface MicDevice {
  id: string;
  name: string;
  isSystemDefault: boolean;
}

/** Mirrors `mic_select::MicList`. */
export interface MicList {
  devices: MicDevice[];
  chosenId: string | null;
  chosenName: string | null;
  chosenMissing: boolean;
  inUseId: string | null;
  inUseName: string | null;
}

/** Mirrors `mic_select::CHANGED_EVENT`. */
export const MICROPHONES_CHANGED = "microphones-changed";

/** The `<select>` value for "System Default" (a device id is never empty). */
export const SYSTEM_DEFAULT_VALUE = "";

/** The label of the first option, with the device it currently means. */
export function systemDefaultLabel(list: MicList): string {
  const d = list.devices.find((x) => x.isSystemDefault);
  return d && d.name ? `System Default (${d.name})` : "System Default";
}

/** The one sentence under the picker — what the next take will use. */
export function micHint(list: MicList): string {
  if (list.devices.length === 0) {
    return "No microphone is connected. Plug one in and it shows up here.";
  }
  if (list.chosenMissing) {
    const name = list.chosenName || "Your saved microphone";
    return `${name} isn't connected, so Yap uses the system default until it's back.`;
  }
  return list.inUseName
    ? `Your next take records from ${list.inUseName}.`
    : "Your next take records from the system default.";
}

type Load =
  | { kind: "loading" }
  | { kind: "error" }
  | { kind: "ready"; list: MicList };

export default function MicrophonePicker() {
  const { toast, toastError } = useAppCtx();
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [saving, setSaving] = useState(false);

  const refresh = useCallback(() => {
    setLoad({ kind: "loading" });
    invoke<MicList>("list_microphones")
      .then((list) => setLoad({ kind: "ready", list }))
      .catch(() => setLoad({ kind: "error" }));
  }, []);

  useEffect(() => {
    let dead = false;
    invoke<MicList>("list_microphones")
      .then((list) => {
        if (!dead) setLoad({ kind: "ready", list });
      })
      .catch(() => {
        if (!dead) setLoad({ kind: "error" });
      });
    const un = listen<MicList>(MICROPHONES_CHANGED, (e) => {
      if (!dead) setLoad({ kind: "ready", list: e.payload });
    });
    return () => {
      dead = true;
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const pick = (value: string) => {
    if (load.kind !== "ready") return;
    const device = load.list.devices.find((d) => d.id === value);
    const id = value === SYSTEM_DEFAULT_VALUE ? null : value;
    setSaving(true);
    invoke<MicList>("set_microphone", { id, name: device?.name ?? null })
      .then((list) => {
        setLoad({ kind: "ready", list });
        toast(id ? `Next take uses ${device?.name || "that microphone"}` : "Next take uses the system default");
      })
      .catch((e) => toastError(e, "Yap could not save that microphone. Try again."))
      .finally(() => setSaving(false));
  };

  return (
    <div className="panel" data-row data-mic-picker>
      <h3>Microphone</h3>
      {load.kind === "loading" && (
        <p className="settings-hint" role="status">Looking for microphones…</p>
      )}
      {load.kind === "error" && (
        <>
          <p className="settings-hint" role="alert">
            Yap couldn't list your microphones.
          </p>
          <div className="actions">
            <button type="button" className="ghost" onClick={refresh}>
              Try again
            </button>
          </div>
        </>
      )}
      {load.kind === "ready" && (
        <>
          <label className="field">
            <span>Record from</span>
            <select
              aria-label="Microphone"
              aria-describedby="mic-picker-hint"
              value={load.list.chosenId ?? SYSTEM_DEFAULT_VALUE}
              disabled={saving}
              onChange={(e) => pick(e.target.value)}
            >
              <option value={SYSTEM_DEFAULT_VALUE}>{systemDefaultLabel(load.list)}</option>
              {load.list.devices.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.name || "Unnamed input"}
                </option>
              ))}
              {load.list.chosenMissing && load.list.chosenId && (
                <option value={load.list.chosenId} disabled>
                  {(load.list.chosenName || "Saved microphone") + " — not connected"}
                </option>
              )}
            </select>
          </label>
          <p className="settings-hint" id="mic-picker-hint" aria-live="polite">
            {micHint(load.list)}
          </p>
        </>
      )}
    </div>
  );
}
