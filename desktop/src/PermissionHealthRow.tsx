import { invoke } from "@tauri-apps/api/core";
import {
  permissionHealth,
  type PermissionGrantRow,
  type PermissionTruth,
} from "./permission";

/**
 * PERM-E — the one permission surface in the main window.
 *
 * INVISIBLE when all four grants are fine, a single calm line when one is not,
 * with the one button that fixes it. Not a dashboard, not four persistent cards:
 * the component returns `null` for the healthy case before it renders anything
 * at all, so a correct install pays no pixels and no attention for it.
 *
 * It renders `report.grants` — the SAME array `permissions.rs` builds for the
 * onboarding checklist — so the two surfaces cannot drift apart.
 */
export function PermissionHealthRow({
  report,
  unproven,
  onTryIt,
}: {
  report?: ({ grants?: PermissionGrantRow[] | null } & PermissionTruth) | null;
  /**
   * Y6-A — `unprovenLine(settings)`: set after a "Skip for now" onboarding
   * finish, until the first take actually pastes. A failing grant still wins
   * the row (it names the cause); this line raises it when every grant reads
   * fine but nothing has ever pasted.
   */
  unproven?: string | null;
  /** Re-open onboarding's try-it flow. */
  onTryIt?: () => void;
}) {
  const health = permissionHealth(report);
  if (!health.visible && unproven) {
    return (
      <div className="perm-health" role="status" data-unproven>
        <div className="perm-health-text">
          <span className="perm-health-line">{unproven}</span>
        </div>
        {onTryIt && (
          <button type="button" className="perm-health-action" onClick={onTryIt}>
            Try it
          </button>
        )}
      </div>
    );
  }
  if (!health.visible) return null;

  return (
    <div className="perm-health" role="status">
      <div className="perm-health-text">
        <span className="perm-health-line">{health.line}</span>
        {/* yap-perm-truth: which binary macOS was asked about, and why a
            toggle that looks ON in System Settings can still be off for it. */}
        {health.notes.map((note) => (
          <span key={note} className="perm-health-note">
            {note}
          </span>
        ))}
      </div>
      {health.action.kind === "settings" && (
        <button
          type="button"
          className="perm-health-action"
          onClick={() => {
            const pane =
              health.action.kind === "settings" ? health.action.pane : "";
            invoke("open_privacy_settings", { pane }).catch((e) =>
              console.error(e),
            );
          }}
        >
          {health.action.label}
        </button>
      )}
    </div>
  );
}
