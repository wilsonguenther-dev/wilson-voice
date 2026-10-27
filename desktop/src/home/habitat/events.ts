/**
 * Y5-K — the habitat's event feed. It subscribes to the SAME backend events the
 * pill does (`status`, `recording`, `audio_level`, `transcript`) plus the model
 * download progress ModelSetup already listens to, and turns each into a pure
 * `HabitatEvent` for the director. It never polls: one `get_status` read seeds
 * the initial state, and every change after that is pushed.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { eventForStatus, modelReadyFrom, type HabitatEvent } from "./habitat";

interface StatusPayload {
  recording?: boolean;
  busy?: boolean;
}
interface DownloadProgress {
  model_id?: string;
  downloaded?: number;
  total?: number;
}

/** Subscribe; returns the unsubscribe. Safe under StrictMode double-mount. */
export function subscribeHabitat(on: (ev: HabitatEvent) => void): () => void {
  let dead = false;
  const unsubs: Array<() => void> = [];
  const keep = (p: Promise<() => void>) => {
    // A listener that cannot attach leaves the habitat on its clock routine:
    // ambient decoration, never a failure the user can act on.
    p.then((u) => (dead ? u() : unsubs.push(u))).catch(() => undefined);
  };
  const seen = new Set<string>();
  invoke<StatusPayload>("get_status")
    .then((s) => {
      if (!dead) on(eventForStatus(s));
    })
    .catch(() => undefined);
  keep(listen<StatusPayload>("status", (e) => on(eventForStatus(e.payload))));
  keep(listen<boolean>("recording", (e) => (e.payload ? on({ kind: "take-start" }) : undefined)));
  keep(listen<number>("audio_level", (e) => on({ kind: "level", value: typeof e.payload === "number" ? e.payload : 0 })));
  // `transcript` is emitted after the text was inserted: the paste LANDED.
  keep(listen("transcript", () => on({ kind: "paste" })));
  keep(
    listen<DownloadProgress>("model_download_progress", (e) => {
      const ev = modelReadyFrom(e.payload, seen);
      if (ev) on(ev);
    }),
  );
  return () => {
    dead = true;
    unsubs.forEach((u) => u());
  };
}
