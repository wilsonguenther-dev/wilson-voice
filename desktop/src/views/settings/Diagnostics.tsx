import { useCallback, useEffect, useState } from "react";
import { useAppCtx } from "../../appShell";
import { errorText } from "../../errors";
import {
  type DiagSnapshot,
  type ExportPaths,
  exportDiagnostics,
  formatEvent,
  formatHealth,
  formatStat,
  levelTone,
  loadDiagnostics,
  sortedStats,
} from "../../diagnostics";

/** Existing tokens.css colours — no new palette. */
const TONE_COLOR = {
  error: "var(--danger-ink)",
  warn: "var(--warn-ink)",
  info: "var(--muted)",
} as const;

/**
 * yap-telemetry-diagnostics — Settings → Diagnostics: the health line, the
 * newest events (newest first), counters/durations, Refresh, and the redacted
 * "Export diagnostics" bundle written to ~/Downloads (nothing is uploaded).
 */
export default function Diagnostics() {
  const { toast, toastError } = useAppCtx();
  const [snap, setSnap] = useState<DiagSnapshot | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [exported, setExported] = useState<ExportPaths | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setSnap(await loadDiagnostics());
      setLoadError(null);
    } catch (e) {
      // Shown in the health line itself; Refresh retries.
      setLoadError(errorText(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runExport = async () => {
    setBusy(true);
    try {
      const paths = await exportDiagnostics();
      setExported(paths);
      setExportError(null);
      toast(`Diagnostics exported → ${paths.txt}`);
    } catch (e) {
      setExported(null);
      setExportError(errorText(e));
      toastError(e);
    } finally {
      setBusy(false);
    }
  };

  const stats = snap ? sortedStats(snap.counters) : [];
  return (
                <section className="settings-panel" data-settings-pane="diagnostics">
                  <div className="panel" data-row>
                    <h3>Health</h3>
                    <p className="settings-hint" data-diag-health style={{ fontFamily: "var(--font-mono, ui-monospace, monospace)", wordBreak: "break-word" }}>
                      {snap ? formatHealth(snap.health) : loadError ?? "Reading…"}
                    </p>
                    <div className="actions wrap">
                      <button type="button" onClick={() => void refresh()}>
                        Refresh
                      </button>
                      <button type="button" disabled={busy} onClick={() => void runExport()}>
                        {busy ? "Exporting…" : "Export diagnostics"}
                      </button>
                    </div>
                    {exported && (
                      <p className="settings-hint" data-diag-export>
                        Saved {exported.json} and {exported.txt}
                      </p>
                    )}
                    {exportError && (
                      <p className="settings-hint" data-diag-export-error style={{ color: TONE_COLOR.error }}>
                        {exportError}
                      </p>
                    )}
                    <p className="settings-hint">
                      The export is redacted (no transcripts, no user name, file names only) and stays on your Mac until you attach it.
                    </p>
                  </div>
                  <div className="panel" data-row>
                    <h3>Counters</h3>
                    {stats.length === 0 ? (
                      <p className="settings-hint">Nothing measured yet this launch.</p>
                    ) : (
                      <ul style={{ margin: 0, paddingLeft: "1rem", fontSize: "var(--text-sm)" }}>
                        {stats.map(([name, s]) => (
                          <li key={name}>{formatStat(name, s)}</li>
                        ))}
                      </ul>
                    )}
                  </div>
                  <div className="panel" data-row>
                    <h3>Recent events</h3>
                    {!snap || snap.events.length === 0 ? (
                      <p className="settings-hint">No events yet this launch.</p>
                    ) : (
                      <ol
                        data-diag-events
                        style={{ margin: 0, paddingLeft: 0, listStyle: "none", maxHeight: "18rem", overflowY: "auto", fontSize: "var(--text-xs)" }}
                      >
                        {snap.events.map((e, i) => (
                          <li key={`${e.tsMs}-${i}`} style={{ color: TONE_COLOR[levelTone(e.level)], wordBreak: "break-word" }}>
                            {formatEvent(e)}
                          </li>
                        ))}
                      </ol>
                    )}
                  </div>
                </section>
  );
}
