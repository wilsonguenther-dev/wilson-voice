import { invoke } from "@tauri-apps/api/core";

/**
 * A newer Yap release the user has not dismissed (backend `UpdateInfo`, YV44).
 */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

/**
 * Ask the backend whether a newer Yap exists — and only ask. Nothing is
 * downloaded, installed, or staged here; the caller shows a prompt and the
 * install happens on the user's click (`installUpdate`).
 *
 * `null` means "nothing to offer": updates turned off in Settings, already on
 * the latest, this version skipped, or no release manifest published yet (the
 * backend logs that at DEBUG instead of the ERROR the plugin used to write on
 * every launch). A genuine failure — offline, malformed manifest — rejects, so
 * the manual "Check for updates" button can say so; the launch-time check
 * ignores it rather than nagging.
 */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  return (await invoke<UpdateInfo | null>("check_for_update")) ?? null;
}

/**
 * Download + install the pending release. USER-TRIGGERED ONLY — call this from
 * the "Install now" click, never on startup. Resolves with the installed
 * version; the new bundle applies the next time Yap launches.
 */
export async function installUpdate(): Promise<string> {
  return await invoke<string>("install_update");
}
