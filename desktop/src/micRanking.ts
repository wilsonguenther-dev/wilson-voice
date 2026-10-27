/**
 * PERM-H — the ranked microphone list, as the Settings screen sees it.
 *
 * The backend owns the order (settings.json `micRanking`, written only by the
 * `mic_rank_*` commands); this module is the pure half the screen needs:
 * the row shape `mic_devices` returns, and the reorder a drag produces.
 */

export type MicDeviceClass = "built_in" | "bluetooth" | "other";

export interface MicDeviceRow {
  name: string;
  /** Attached right now. A hardware fact — says nothing about permission. */
  present: boolean;
  /** 0-based rank, null for an attached device nobody ranked yet. */
  rank: number | null;
  class: MicDeviceClass;
  systemDefault: boolean;
  /** The device the next take opens. */
  wins: boolean;
}

export interface MicDevicesView {
  rows: MicDeviceRow[];
  /** true with the lid closed; null where it cannot be read (desktop Macs). */
  lidClosed: boolean | null;
}

export interface MicNotice {
  kind: "airpods" | "clamshell" | "switched_mid_take" | "parked_mid_take";
  device: string;
  title: string;
  body: string;
}

/** The ranked names, favourite first, in the order the rows show them. */
export function rankedOrder(rows: MicDeviceRow[]): string[] {
  return rows
    .filter((r) => r.rank !== null)
    .sort((a, b) => (a.rank ?? 0) - (b.rank ?? 0))
    .map((r) => r.name);
}

/**
 * Move `name` to position `to` in the ranked order. A name not yet ranked is
 * inserted there (dragging an attached-but-unranked mic into the list ranks
 * it). Out-of-range targets clamp. Returns a new array.
 */
export function moveInRanking(order: string[], name: string, to: number): string[] {
  const without = order.filter((n) => n !== name);
  const at = Math.max(0, Math.min(to, without.length));
  return [...without.slice(0, at), name, ...without.slice(at)];
}

/** The built-in mic to offer instead of AirPods, if one is attached. */
export function builtInAlternative(rows: MicDeviceRow[]): MicDeviceRow | undefined {
  return rows.find((r) => r.class === "built_in" && r.present);
}
