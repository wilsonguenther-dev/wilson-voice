/**
 * yap-1106-byom-cloud-fast-path — the bar's route chip ("Cloud" / "On this
 * Mac"). Rendered inside the capsule like the language chip; it only listens
 * to the `status` event the pill already receives after every take, so there
 * is no polling and no second channel. Model in `cloudChip.ts`.
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { AppStatus } from "../appTypes";
import { cloudChip, type CloudChipModel } from "./cloudChip";

export default function RouteChip() {
  const [model, setModel] = useState<CloudChipModel | null>(null);
  useEffect(() => {
    let dead = false;
    let unlisten: (() => void) | null = null;
    listen<AppStatus>("status", (e) => setModel(cloudChip(e.payload))).then((u) => {
      if (dead) u();
      else unlisten = u;
    });
    return () => {
      dead = true;
      unlisten?.();
    };
  }, []);
  if (!model) return null;
  return (
    <span
      className="pill-route"
      data-route={model.route}
      data-tone={model.tone}
      title={model.title}
      aria-label={`${model.label} — ${model.title}`}
      role="status"
    >
      {model.label}
    </span>
  );
}
