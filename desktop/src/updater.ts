import { check } from "@tauri-apps/plugin-updater";

/**
 * Best-effort auto-update check against the GitHub Releases updater manifest
 * (endpoints + pubkey are configured in tauri.conf.json under plugins.updater).
 *
 * Fully guarded: any failure (offline, no manifest, denied capability, dev
 * build) is swallowed so it can never block app startup. When an update is
 * found it is downloaded and staged; it applies on the next relaunch.
 */
export async function checkForUpdates(): Promise<void> {
  try {
    const update = await check();
    if (update?.available) {
      console.info(`Yap update available: ${update.version} — downloading`);
      await update.downloadAndInstall();
      console.info("Yap update installed — will apply on next launch");
    }
  } catch (err) {
    // Non-fatal: updates are a convenience, never a startup dependency.
    console.warn("update check skipped:", err);
  }
}
