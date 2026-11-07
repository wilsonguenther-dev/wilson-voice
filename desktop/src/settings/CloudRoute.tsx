/**
 * yap-1106-byom-cloud-fast-path — Settings › Models & privacy › "Cloud model
 * (your key)". The mode switch (Cloud when online / Always on this Mac /
 * Cloud only), the key (Keychain only — this view never sees more than its
 * last four), the daily limit with today's and the month's spend, the
 * "Cloud key needs attention" line, and the notes-quality pick.
 *
 * No vendor or model name appears here (HARD rule): the backend's label is
 * "Cloud model (your key)" and model ids live in Diagnostics only.
 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorText } from "../errors";
import { CLOUD_ROUTE_CHANGED_EVENT } from "../meetings/consent";

export type CloudMode = "always_local" | "cloud_when_online" | "cloud_only";

export interface SpendView {
  capUsd: number;
  todayUsd: number;
  monthToDateUsd: number;
  todayFraction: number;
  capReached: boolean;
  warn: boolean;
  pricesAsOf: string;
}

export interface CloudRouteView {
  mode: CloudMode;
  spend: SpendView;
  keyNeedsAttention: boolean;
  keySaved: boolean;
  keyHint: string;
  notesBest: boolean;
  lastRoute: { route: "cloud" | "local"; reason: string; label: string; tooltip: string } | null;
  label: string;
  leavesHost: string | null;
}

/** yap-1107-cloud-first-when-key-present — the line under the mode switch (no vendor names). */
export const CLOUD_FIRST_LINE =
  "With your key in, Yap uses the cloud first. It falls back to this Mac when you are offline, over your daily limit, or the cloud is slow.";

export const MODE_LABEL: Record<CloudMode, string> = {
  cloud_when_online: "Cloud when online (faster)",
  always_local: "Always on this Mac",
  cloud_only: "Cloud only",
};

const MODE_ORDER: CloudMode[] = ["cloud_when_online", "always_local", "cloud_only"];

const usd = (n: number) => `$${n.toFixed(2)}`;

/** The spend line under the limit field. Pure, so a test can pin it. */
export function spendLine(s: SpendView): string {
  return `Today ${usd(s.todayUsd)} of ${usd(s.capUsd)} · Month to date ${usd(s.monthToDateUsd)}`;
}

/** The quiet note at 80 %, the firm one at 100 %, nothing below. */
export function spendNote(s: SpendView): string | null {
  if (s.capReached) return "Cloud limit reached today — Yap is using this Mac until tomorrow.";
  if (s.warn) return "Close to today's cloud limit.";
  return null;
}

export function CloudRouteSettingsView({
  view,
  keyDraft,
  capDraft,
  error,
  onKeyDraft,
  onSaveKey,
  onRemoveKey,
  onMode,
  onCapDraft,
  onSaveCap,
  onNotesBest,
}: {
  view: CloudRouteView;
  keyDraft: string;
  capDraft: string;
  error: string | null;
  onKeyDraft: (v: string) => void;
  onSaveKey: () => void;
  onRemoveKey: () => void;
  onMode: (m: CloudMode) => void;
  onCapDraft: (v: string) => void;
  onSaveCap: () => void;
  onNotesBest: (best: boolean) => void;
}) {
  const note = spendNote(view.spend);
  return (
    <section className="settings-block" data-block="cloud-route" aria-labelledby="cloud-route-h">
      <h3 id="cloud-route-h">{view.label}</h3>
      <p className="settings-help">
        With your own key, cleanup, formatting and meeting notes run in the cloud when you are online —
        much faster than the bundled model. Offline, over the limit, or if the cloud is slow, the same
        job runs on this Mac. Leaves this Mac: {view.keySaved && view.leavesHost ? `Yes — ${view.leavesHost}` : "No (no key saved)"}
      </p>
      <fieldset className="field" data-field="cloud-mode">
        <legend>Where cleanup and notes run</legend>
        {MODE_ORDER.map((m) => (
          <label key={m} className="radio">
            <input
              type="radio"
              name="cloud-mode"
              value={m}
              checked={view.mode === m}
              onChange={() => onMode(m)}
            />
            <span>{MODE_LABEL[m]}</span>
          </label>
        ))}
      </fieldset>
      {/* yap-1107-cloud-first-when-key-present (D2): one plain line under the switch. */}
      <p className="settings-help" data-line="cloud-first">
        {CLOUD_FIRST_LINE}
      </p>
      <label className="field" data-field="cloud-key">
        <span>Your key {view.keySaved ? `(saved ••••${view.keyHint})` : "(not saved)"}</span>
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={keyDraft}
          placeholder={view.keySaved ? "Paste a new key to replace it" : "Paste your key"}
          onChange={(e) => onKeyDraft(e.target.value)}
        />
      </label>
      <div className="row">
        <button type="button" onClick={onSaveKey} disabled={!keyDraft.trim()}>
          Save key
        </button>
        {view.keySaved ? (
          <button type="button" className="danger" onClick={onRemoveKey}>
            Remove key
          </button>
        ) : null}
      </div>
      {view.keyNeedsAttention ? (
        <p className="settings-warning" role="alert" data-line="key-attention">
          Cloud key needs attention — the cloud rejected it, so Yap is using this Mac. Paste a valid key to resume.
        </p>
      ) : null}
      <label className="field" data-field="cloud-cap">
        <span>Daily cloud limit (USD)</span>
        <input
          type="number"
          inputMode="decimal"
          min={0.25}
          step={0.25}
          value={capDraft}
          onChange={(e) => onCapDraft(e.target.value)}
          onBlur={onSaveCap}
        />
      </label>
      <p className="settings-help" data-line="spend">{spendLine(view.spend)}</p>
      {note ? <p className="settings-note" data-line="spend-note">{note}</p> : null}
      <label className="field checkbox" data-field="notes-best">
        <input type="checkbox" checked={view.notesBest} onChange={(e) => onNotesBest(e.target.checked)} />
        <span>Best quality for meeting notes (slower, costs more)</span>
      </label>
      {error ? <p className="settings-error" role="alert">{error}</p> : null}
    </section>
  );
}

export default function CloudRouteSettings() {
  const [view, setView] = useState<CloudRouteView | null>(null);
  const [keyDraft, setKeyDraft] = useState("");
  const [capDraft, setCapDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const apply = useCallback((v: CloudRouteView) => {
    setView(v);
    setCapDraft(v.spend.capUsd.toFixed(2));
    setError(null);
  }, []);
  const call = useCallback(
    (cmd: string, args?: Record<string, unknown>) =>
      invoke<CloudRouteView>(cmd, args)
        .then((v) => {
          apply(v);
          // fix-l3b3-1 — a key or mode change moves what leaves this Mac;
          // Model providers re-reads its "Leaves this Mac" summary.
          if (cmd !== "cloud_route_view") {
            window.dispatchEvent(new Event(CLOUD_ROUTE_CHANGED_EVENT));
          }
        })
        .catch((e) => setError(errorText(e))),
    [apply],
  );
  useEffect(() => {
    call("cloud_route_view");
  }, [call]);
  if (!view) return null;
  return (
    <CloudRouteSettingsView
      view={view}
      keyDraft={keyDraft}
      capDraft={capDraft}
      error={error}
      onKeyDraft={setKeyDraft}
      onSaveKey={() => {
        const key = keyDraft;
        setKeyDraft("");
        call("save_cloud_key", { key });
      }}
      onRemoveKey={() => call("save_cloud_key", { key: "" })}
      onMode={(mode) => call("set_cloud_mode", { mode })}
      onCapDraft={setCapDraft}
      onSaveCap={() => {
        const usdValue = Number(capDraft);
        if (!Number.isFinite(usdValue) || usdValue <= 0) {
          setError("The daily limit must be a positive amount.");
          return;
        }
        call("set_cloud_daily_cap", { usd: usdValue });
      }}
      onNotesBest={(best) => call("set_cloud_notes_model", { best })}
    />
  );
}
