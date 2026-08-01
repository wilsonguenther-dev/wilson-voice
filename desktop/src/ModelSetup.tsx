import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// YV54 — silent model setup. Yap cannot transcribe without a GGUF model on
// disk, but "which model?" is not a question a first-run user has any way to
// answer: YV33's "Get your model" step made them stop, read two catalog blurbs
// and press Download before onboarding could continue. The choice is now made
// FOR them — the catalog's own top recommendation starts downloading the moment
// a surface that needs a model mounts, in the background, while the user does
// the things that actually need them (permissions, calibration). The only thing
// they ever see is the slim ribbon below.
//
// The picker is not gone, it moved: Settings → Advanced renders the same
// catalog list for power users who DO want to pick.

/** One catalog model as `list_models` reports it (camelCase on the wire). */
export interface ModelEntry {
  id: string;
  name: string;
  description: string;
  architecture: string;
  filename: string;
  quant: string;
  sizeBytes: number;
  downloaded: boolean;
  selected: boolean;
  recommended: boolean;
  recommendedRank: number | null;
}

/** `model_download_progress` payload (plain serde — snake_case on the wire). */
interface DownloadProgress {
  model_id: string;
  downloaded: number;
  total: number;
}

export function fmtSize(bytes: number) {
  const gb = bytes / 1e9;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

/** Catalog order: recommended first, by the catalog's own rank. */
function byRecommendation(a: ModelEntry, b: ModelEntry) {
  if (a.recommended !== b.recommended) return a.recommended ? -1 : 1;
  return (a.recommendedRank ?? 99) - (b.recommendedRank ?? 99);
}

/**
 * What the silent setup grabs when nobody has chosen: a model already on disk
 * (nothing to fetch — selecting it is instantly "ready"), else the catalog's
 * top recommendation. Never a hardcoded id: the catalog is the source of truth.
 */
function autoPick(models: ModelEntry[]): ModelEntry | undefined {
  return models.find((m) => m.downloaded) ?? [...models].sort(byRecommendation)[0];
}

export interface ModelSetup {
  models: ModelEntry[];
  /** A model is downloaded AND selected — dictation can actually run. */
  ready: boolean;
  /** Catalog id currently downloading, or null. */
  downloading: string | null;
  /** Live percentage for the current download (0 while sizing up). */
  pct: number;
  error: string | null;
  /**
   * The one line every surface shows while the engine is still being prepared,
   * or null once it is ready. Progress states carry no action — this is status,
   * not a decision; only the failure state offers a retry (via `retry`).
   */
  ribbon: string | null;
  refresh: () => Promise<void>;
  download: (id: string) => Promise<void>;
  select: (id: string) => Promise<void>;
  retry: () => void;
}

/**
 * Load the catalog, follow `model_download_progress`, and — when `autoDownload`
 * — start the recommended download on mount if nothing usable is on disk yet.
 *
 * `autoDownload` exists so exactly ONE mounted surface ever kicks a download
 * off: the onboarding overlay owns it during first run, and the main app owns
 * it for an already-onboarded install that finds itself with no model (a fresh
 * profile, a deleted file, an update landing on a machine that skipped setup).
 */
export function useModelSetup({
  autoDownload,
}: {
  autoDownload: boolean;
}): ModelSetup {
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Has the catalog been read at least once? Until it has, "no model" is not a
  // fact yet — the ribbon stays quiet rather than flashing on a ready install.
  const [loaded, setLoaded] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setModels(await invoke<ModelEntry[]>("list_models"));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
    setLoaded(true);
  }, []);

  // Downloading is resumable + sha256-verified in Rust, and an already-present
  // file short-circuits, so this is safe to call on a model that is half or
  // fully there. Selecting after it lands is what makes the engine load it.
  const download = useCallback(
    async (id: string) => {
      setDownloading(id);
      setProgress({ model_id: id, downloaded: 0, total: 0 });
      setError(null);
      try {
        await invoke("download_model", { id });
        await invoke("select_model", { id });
      } catch (e) {
        setError(String(e));
      }
      setDownloading(null);
      setProgress(null);
      await refresh();
    },
    [refresh],
  );

  const select = useCallback(
    async (id: string) => {
      try {
        await invoke("select_model", { id });
      } catch (e) {
        setError(String(e));
      }
      await refresh();
    },
    [refresh],
  );

  useEffect(() => {
    refresh();
    // A synchronous cleanup can run before this listen() promise resolves
    // (StrictMode double-mount). A `dead` flag unsubscribes a listener that
    // lands after teardown so no native listener leaks.
    let dead = false;
    const unsubs: Array<() => void> = [];
    listen<DownloadProgress>("model_download_progress", (e) =>
      setProgress(e.payload),
    ).then((u) => (dead ? u() : unsubs.push(u)));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [refresh]);

  const ready = models.some((m) => m.downloaded && m.selected);
  const pct =
    progress && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : 0;

  // One attempt per mount: a download that fails leaves the ribbon in its error
  // state with a retry rather than hammering the mirror in a loop.
  const startedRef = useRef(false);
  useEffect(() => {
    if (!autoDownload || startedRef.current || downloading) return;
    if (ready || models.length === 0) return;
    const pick = autoPick(models);
    if (!pick) return;
    startedRef.current = true;
    void download(pick.id);
  }, [autoDownload, ready, models, downloading, download]);

  const retry = useCallback(() => {
    startedRef.current = false;
    setError(null);
    void refresh();
  }, [refresh]);

  const ribbon = !loaded || ready
    ? null
    : error
      ? `Couldn't prepare your speech engine — ${error}`
      : downloading
        ? progress && progress.total > 0
          ? `Preparing your speech engine… ${pct}%`
          : "Preparing your speech engine…"
        : "Preparing your speech engine…";

  return {
    models,
    ready,
    downloading,
    pct,
    error,
    ribbon,
    refresh,
    download,
    select,
    retry,
  };
}

/**
 * The slim, unobtrusive status ribbon — the ONLY thing silent setup shows.
 * No buttons while it is working: there is nothing to decide. It disappears the
 * moment the engine is ready.
 */
export function ModelRibbon({ setup }: { setup: ModelSetup }) {
  if (!setup.ribbon) return null;
  return (
    <div
      className={setup.error ? "model-ribbon bad" : "model-ribbon"}
      role="status"
      aria-live="polite"
    >
      <span
        className="model-ribbon-track"
        role="progressbar"
        aria-valuenow={setup.pct}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-hidden={!!setup.error}
      >
        <span style={{ width: `${setup.error ? 100 : setup.pct}%` }} />
      </span>
      <span className="model-ribbon-text">{setup.ribbon}</span>
      {setup.error && (
        <button className="ghost" onClick={setup.retry}>
          Retry
        </button>
      )}
    </div>
  );
}

function StatusDot({ ok }: { ok: boolean }) {
  return <span className={ok ? "dot-ok" : "dot-bad"} aria-hidden />;
}

/**
 * The catalog picker — YV33's onboarding step, kept verbatim for the people who
 * actually want it, now living in Settings → Advanced. Onboarding never shows
 * it; nothing here is on the first-run path.
 */
export function ModelPicker({ setup }: { setup: ModelSetup }) {
  const rows = [...setup.models].sort(byRecommendation);
  return (
    <>
      <ul className="onboard-models">
        {rows.map((m) => (
          <li key={m.id} className={m.downloaded ? "ok" : "bad"}>
            <StatusDot ok={m.downloaded} />
            <div>
              <strong>
                {m.name}{" "}
                <span className="muted tiny">
                  {fmtSize(m.sizeBytes)} · {m.quant}
                </span>
              </strong>
              <p>{m.description}</p>
              {setup.downloading === m.id ? (
                <div
                  className="onboard-bar"
                  role="progressbar"
                  aria-valuenow={setup.pct}
                  aria-valuemin={0}
                  aria-valuemax={100}
                >
                  <span style={{ width: `${setup.pct}%` }} />
                  <em className="muted tiny">Downloading… {setup.pct}%</em>
                </div>
              ) : m.downloaded && m.selected ? (
                <button disabled>Downloaded ✓ · in use</button>
              ) : m.downloaded ? (
                <button
                  onClick={() => setup.select(m.id)}
                  disabled={!!setup.downloading}
                >
                  Use this model
                </button>
              ) : (
                <button
                  className="primary"
                  onClick={() => setup.download(m.id)}
                  disabled={!!setup.downloading}
                >
                  Download
                </button>
              )}
            </div>
          </li>
        ))}
      </ul>
      {rows.length === 0 && (
        <p className="muted tiny">Loading the model catalog…</p>
      )}
      {setup.error && (
        <p className="onboard-error">
          {setup.error}{" "}
          <button className="ghost" onClick={setup.retry}>
            Retry
          </button>
        </p>
      )}
    </>
  );
}
