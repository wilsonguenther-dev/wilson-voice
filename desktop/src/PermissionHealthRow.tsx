import { invoke } from "@tauri-apps/api/core";
import { permissionHealth, type PermissionGrantRow } from "./permission";

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
  grants,
}: {
  grants?: PermissionGrantRow[] | null;
}) {
  const health = permissionHealth({ grants });
  if (!health.visible) return null;

  return (
    <div className="perm-health" role="status">
      <span className="perm-health-line">{health.line}</span>
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
