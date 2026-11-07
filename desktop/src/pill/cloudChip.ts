/**
 * yap-1106-byom-cloud-fast-path — the pill's route chip: **Cloud** or
 * **On this Mac**, updated per take, with the reason in its tooltip.
 *
 * Pure: the model from `AppStatus.cloudRoute` (set by the Rust route after
 * every polish / notes job). The chip never names a vendor or a model — the
 * Rust side writes the label and tooltip, and `vendorStrings.test.ts` sweeps
 * this file like every other user-visible surface.
 *
 * Hidden when there is nothing to say: a default install ("Always on this
 * Mac", no key) keeps the bar exactly as it was.
 */
import type { AppStatus, CloudRouteChip } from "../appTypes";

export interface CloudChipModel {
  route: "cloud" | "local";
  reason: CloudRouteChip["reason"];
  label: string;
  title: string;
  /** `limit` when the cap is reached (the chip carries the warning tone). */
  tone: "cloud" | "local" | "limit";
}

/** The cap line the pill shows while the route is local for the day. */
export const CAP_REACHED_LINE = "Cloud limit reached today, using this Mac";

export function cloudChip(
  status: Pick<AppStatus, "cloudRoute"> | null | undefined,
): CloudChipModel | null {
  const r = status?.cloudRoute;
  if (!r) return null;
  // Nothing to say: the user chose this Mac, or never entered a key.
  if (r.route === "local" && (r.reason === "user" || r.reason === "no_key")) return null;
  const label = r.route === "cloud" ? "Cloud" : "On this Mac";
  const title = r.reason === "cap" ? CAP_REACHED_LINE : r.tooltip || label;
  return {
    route: r.route,
    reason: r.reason,
    label,
    title,
    tone: r.reason === "cap" ? "limit" : r.route,
  };
}

/** The meeting-notes header line for a stored route (`null` = no line). */
export function notesRouteLine(route: string | null | undefined): string | null {
  if (route === "cloud") return "Notes written in the cloud";
  if (route === "local") return "Notes written on this Mac";
  return null;
}
