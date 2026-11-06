// WF-09 — Settings › Audio › Microphone. The same list the menu bar's
// "Microphone ▸" submenu draws (both come from `mic_select::list`), with
// "System Default" first. A pick is saved on this Mac and used by the next
// take; the list redraws on its own when a mic is plugged in or unplugged
// (`microphones-changed`, published by the backend device watcher; the shell
// in appShell.ts owns that subscription and hands the list down as
// `microphones`, so this pane registers no listener of its own).
//
// WF-10 — below the picker, the fallback order: drag (or Move up / Move down)
// to rank the mics. When the chosen mic disconnects — even mid-take — Yap
// continues on the next one plugged in. Virtual and loopback inputs (decided by
// the backend from the CoreAudio transport type) sit under "Other devices" and
// are never picked automatically, only by an explicit choice.
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppCtx } from "../../../appShell";

/** Mirrors `mic_select::MicDevice`. */
export interface MicDevice {
  id: string;
  name: string;
  isSystemDefault: boolean;
  /** WF-10 — virtual / loopback / aggregate input; never auto-picked. */
  isVirtual?: boolean;
}

/** Mirrors `mic_select::RankedMic`. */
export interface RankedMic {
  id: string;
  name: string;
}

/** Mirrors `mic_select::MicList`. */
export interface MicList {
  devices: MicDevice[];
  chosenId: string | null;
  chosenName: string | null;
  chosenMissing: boolean;
  inUseId: string | null;
  inUseName: string | null;
  /** WF-10 — the fallback ranking, best first (may name unplugged mics). */
  order?: RankedMic[];
}

/** One row of the fallback-order list. */
export interface RankRow {
  id: string;
  name: string;
  connected: boolean;
}

/** Physical mics first; virtual / loopback inputs under "Other devices". */
export function splitDevices(list: MicList): { mics: MicDevice[]; other: MicDevice[] } {
  return {
    mics: list.devices.filter((d) => !d.isVirtual),
    other: list.devices.filter((d) => d.isVirtual),
  };
}

/**
 * The fallback-order rows: the saved ranking first (an unplugged mic keeps its
 * place and says so), then every connected physical mic not ranked yet. A
 * virtual input is never in this list — it is never a fallback.
 */
export function rankedRows(list: MicList): RankRow[] {
  const virtualIds = new Set(list.devices.filter((d) => d.isVirtual).map((d) => d.id));
  const rows: RankRow[] = [];
  const seen = new Set<string>();
  for (const r of list.order ?? []) {
    if (!r.id || seen.has(r.id) || virtualIds.has(r.id)) continue;
    seen.add(r.id);
    const live = list.devices.find((d) => d.id === r.id);
    rows.push({ id: r.id, name: live?.name || r.name || "Unnamed input", connected: !!live });
  }
  for (const d of list.devices) {
    if (d.isVirtual || seen.has(d.id)) continue;
    seen.add(d.id);
    rows.push({ id: d.id, name: d.name || "Unnamed input", connected: true });
  }
  return rows;
}

/** Move the row at `from` to `to` (both clamped); returns the new id order. */
export function moveRank(ids: string[], from: number, to: number): string[] {
  const next = ids.slice();
  if (from < 0 || from >= next.length) return next;
  const target = Math.max(0, Math.min(next.length - 1, to));
  const [row] = next.splice(from, 1);
  next.splice(target, 0, row);
  return next;
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
    const inUse = list.devices.find((d) => d.id === list.inUseId);
    if (inUse && !inUse.isSystemDefault && list.inUseName) {
      return `${name} isn't connected, so Yap uses ${list.inUseName}, next in your order, until it's back.`;
    }
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
  const { toast, toastError, microphones } = useAppCtx();
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [saving, setSaving] = useState(false);
  const [dragFrom, setDragFrom] = useState<number | null>(null);

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
    return () => {
      dead = true;
    };
  }, []);

  // The backend device watcher's latest list (plug / unplug), via the shell.
  useEffect(() => {
    if (microphones && Array.isArray(microphones.devices)) {
      setLoad({ kind: "ready", list: microphones });
    }
  }, [microphones]);

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

  const saveOrder = (ids: string[]) => {
    if (load.kind !== "ready") return;
    setSaving(true);
    invoke<MicList>("set_microphone_order", { ids })
      .then((list) => setLoad({ kind: "ready", list }))
      .catch((e) => toastError(e, "Yap could not save the microphone order. Try again."))
      .finally(() => setSaving(false));
  };

  const move = (rows: RankRow[], from: number, to: number) => {
    if (to < 0 || to >= rows.length || from === to) return;
    saveOrder(moveRank(rows.map((r) => r.id), from, to));
  };

  const ready = load.kind === "ready" ? load.list : null;
  const split = ready ? splitDevices(ready) : { mics: [], other: [] };
  const rows = ready ? rankedRows(ready) : [];

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
              {split.mics.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.name || "Unnamed input"}
                </option>
              ))}
              {split.other.length > 0 && (
                <optgroup label="Other devices" data-mic-other>
                  {split.other.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.name || "Unnamed input"}
                    </option>
                  ))}
                </optgroup>
              )}
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
          {rows.length > 1 && (
            <div className="mic-order" data-mic-order>
              <h4 id="mic-order-title">If a mic disconnects</h4>
              <p className="settings-hint" id="mic-order-hint">
                Yap keeps recording on the next mic in this order. Drag to rearrange.
                {split.other.length > 0 && " Other devices, like virtual and loopback inputs, are never picked on their own."}
              </p>
              <ol className="mic-order-list" aria-labelledby="mic-order-title" aria-describedby="mic-order-hint">
                {rows.map((r, i) => (
                  <li
                    key={r.id}
                    data-mic-rank={i + 1}
                    draggable={!saving}
                    onDragStart={() => setDragFrom(i)}
                    onDragOver={(e) => e.preventDefault()}
                    onDrop={(e) => {
                      e.preventDefault();
                      if (dragFrom !== null) move(rows, dragFrom, i);
                      setDragFrom(null);
                    }}
                    onDragEnd={() => setDragFrom(null)}
                  >
                    <span className="mic-order-rank">{i + 1}</span>
                    <span className="mic-order-name">
                      {r.name}
                      {!r.connected && <span className="settings-hint"> — not connected</span>}
                    </span>
                    <button
                      type="button"
                      className="ghost"
                      aria-label={`Move ${r.name} up`}
                      disabled={saving || i === 0}
                      onClick={() => move(rows, i, i - 1)}
                    >
                      Up
                    </button>
                    <button
                      type="button"
                      className="ghost"
                      aria-label={`Move ${r.name} down`}
                      disabled={saving || i === rows.length - 1}
                      onClick={() => move(rows, i, i + 1)}
                    >
                      Down
                    </button>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </>
      )}
    </div>
  );
}
