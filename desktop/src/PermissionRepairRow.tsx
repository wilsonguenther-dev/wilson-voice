import { useEffect, useState } from "react";
import { adviceFor } from "./errors";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  IDLE,
  REPAIR_EVENT,
  nameProduct,
  offersRelaunch,
  reduceRepair,
  relaunchApp,
  repairRows,
  requestRepair,
  serviceCopy,
  type ProbeReport,
  type RepairEvent,
  type RepairInput,
  type RepairService,
  type RepairView,
} from "./permissionRepair";

/**
 * yap-p0-perm-repair — one row per denied or stale grant, each with Repair.
 *
 * Rendered by the main-window banner AND the onboarding permissions step from
 * the SAME `PermissionReport.probe`, so the two can never disagree. Returns
 * null when nothing needs repairing and no repair is in flight.
 */
export function PermissionRepair({
  probe,
  onChanged,
  productName,
}: {
  probe?: ProbeReport | null;
  onChanged?: () => void;
  /** `PermissionReport.productName` — "Yap Test" on the test channel. */
  productName?: string | null;
}) {
  const [views, setViews] = useState<Partial<Record<RepairService, RepairView>>>({});

  const apply = (service: RepairService, input: RepairInput) =>
    setViews((v) => ({ ...v, [service]: reduceRepair(v[service] ?? IDLE, input) }));

  useEffect(() => {
    const un = listen<RepairEvent>(REPAIR_EVENT, (e) => {
      apply(e.payload.service, { kind: "event", event: e.payload });
      if (e.payload.phase !== "waiting") onChanged?.();
    });
    return () => {
      void un.then((f) => f());
    };
  }, [onChanged]);

  async function repair(service: RepairService) {
    apply(service, { kind: "started", product: productName });
    try {
      const outcome = await requestRepair(service);
      apply(service, { kind: "outcome", outcome });
      // After a reset the microphone is "not determined": draw the prompt.
      if (outcome.ok && service === "microphone") {
        await invoke("request_microphone").catch((e) => console.error(e));
      }
    } catch (e) {
      apply(service, { kind: "error", message: adviceFor(e, nameProduct("Yap could not repair that permission — open System Settings and toggle Yap off and on.", productName)).line });
    }
  }

  const rows = repairRows(probe);
  const active = (Object.keys(views) as RepairService[]).filter(
    (s) => views[s] && views[s]!.phase !== "idle" && !rows.some((r) => r.service === s),
  );
  if (rows.length === 0 && active.length === 0) return null;

  const all = [
    ...rows.map((r) => ({ service: r.service, copy: serviceCopy(r, productName) })),
    ...active.map((s) => ({ service: s, copy: null })),
  ];

  return (
    <div className="perm-repair" role="status">
      {all.map(({ service, copy }) => {
        const view = views[service] ?? IDLE;
        const busy = view.phase === "running" || view.phase === "waiting";
        return (
          <div key={service} className="perm-health" data-repair-service={service}>
            <div className="perm-health-text">
              {copy && <span className="perm-health-line">{copy.title}</span>}
              {copy && view.phase === "idle" && (
                <span className="perm-health-note">{copy.body}</span>
              )}
              {view.line && <span className="perm-health-line">{nameProduct(view.line, productName)}</span>}
              {view.detail && <span className="perm-health-note">{view.detail}</span>}
            </div>
            {offersRelaunch(service, view) ? (
              <button
                type="button"
                className="perm-health-action"
                onClick={() => relaunchApp().catch((e) => console.error(e))}
              >
                Relaunch
              </button>
            ) : (
              copy &&
              view.phase !== "fixed" && (
                <button
                  type="button"
                  className="perm-health-action"
                  disabled={busy}
                  onClick={() => void repair(service)}
                >
                  Repair
                </button>
              )
            )}
          </div>
        );
      })}
    </div>
  );
}
