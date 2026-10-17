// yap-p0-hotkeys-no-permission — the truthful hotkey status, read by the home
// banner, Permissions and Insights. Mirrors `hotkeys::HotkeysStatus` in Rust.
import type { AppStatus } from "./appTypes";

/** True only when the Carbon register call returned Ok. */
export function carbonRegistered(status: Pick<AppStatus, "hotkeys">): boolean {
  return status.hotkeys?.carbon?.state === "registered";
}

/** True only after the fn tap delivered a real key event. */
export function fnDelivering(status: Pick<AppStatus, "hotkeys">): boolean {
  return status.hotkeys?.fnTap?.state === "delivering";
}

/** The dictate chord's label, e.g. ⌃⌥⌘Space. */
export function dictateLabel(status: Pick<AppStatus, "dictateToggleLabel">): string {
  return status.dictateToggleLabel || "⌃⌥⌘Space";
}

/** Which System Settings pane the Repair button opens for this fn state. */
export function repairPane(status: Pick<AppStatus, "hotkeys">): string | null {
  const s = status.hotkeys?.fnTap?.state;
  if (s === "secureInput" || s === "delivering") return null;
  return "InputMonitoring";
}
