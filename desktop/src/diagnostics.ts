/**
 * yap-telemetry-diagnostics — the Diagnostics panel's types, its two invoke
 * wrappers, and the pure formatters the panel (and its tests) use.
 *
 * Mirrors `desktop/src-tauri/src/diagnostics.rs`: `DiagSnapshot` is what
 * `diagnostics_snapshot` returns, `ExportPaths` what `export_diagnostics`
 * returns. Failures are rejected invokes; the panel reads them with
 * errorText() and the Rust command has already logged them.
 */
import { invoke } from "@tauri-apps/api/core";

export interface DiagEvent {
  tsMs: number;
  level: string;
  module: string;
  msg: string;
  fields: Record<string, string>;
}

export interface DiagStat {
  last: number;
  count: number;
  max: number;
}

export interface DiagHealth {
  permissions: Record<string, string>;
  model: { selected: string; loaded: boolean; loadedModel: string | null };
  hotkey: { registered: boolean; chord: string; fnTap: string };
  meeting: { state: string; lastStage: string };
  insert: { lastPath: string; lastOutcome: string };
}

export interface DiagSnapshot {
  generatedAtMs: number;
  /** Newest first. */
  events: DiagEvent[];
  counters: Record<string, DiagStat>;
  health: DiagHealth;
}

export interface ExportPaths {
  json: string;
  txt: string;
}

export function loadDiagnostics(): Promise<DiagSnapshot> {
  return invoke<DiagSnapshot>("diagnostics_snapshot");
}

export function exportDiagnostics(): Promise<ExportPaths> {
  return invoke<ExportPaths>("export_diagnostics");
}

const yn = (b: boolean) => (b ? "yes" : "no");

/** The one health line — the same shape as `health_line` in diagnostics.rs. */
export function formatHealth(h: DiagHealth): string {
  const perm = (k: string) => h.permissions[k] ?? "?";
  return [
    `mic=${perm("microphone")} ax=${perm("accessibility")} input=${perm("input_monitoring")} post_event=${perm("post_event")}`,
    `model=${h.model.selected || "none"} loaded=${yn(h.model.loaded)}`,
    `hotkey=${h.hotkey.chord} registered=${yn(h.hotkey.registered)}`,
    `meeting=${h.meeting.state} stage=${h.meeting.lastStage}`,
    `insert=${h.insert.lastPath} outcome=${h.insert.lastOutcome}`,
  ].join(" · ");
}

/** `HH:MM:SS.mmmZ` (UTC, so a pasted line matches the export and yap.log). */
export function formatTimestamp(ms: number): string {
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return String(ms);
  return d.toISOString().slice(11, 23) + "Z";
}

/** `HH:MM:SS.mmmZ LEVEL module msg k=v k=v` — fields in key order. */
export function formatEvent(e: DiagEvent): string {
  const fields = Object.keys(e.fields)
    .sort()
    .map((k) => `${k}=${e.fields[k]}`);
  return [formatTimestamp(e.tsMs), e.level.toUpperCase(), e.module, e.msg, ...fields].join(" ");
}

export type LevelTone = "error" | "warn" | "info";

/** Which colour a level gets: ERROR → danger, WARN → caution, else muted. */
export function levelTone(level: string): LevelTone {
  const l = level.toUpperCase();
  if (l === "ERROR") return "error";
  if (l === "WARN") return "warn";
  return "info";
}

/** `paste.insert_ms  last=12 count=3 max=40`. */
export function formatStat(name: string, s: DiagStat): string {
  return `${name}  last=${s.last} count=${s.count} max=${s.max}`;
}

/** Counters in name order. */
export function sortedStats(counters: Record<string, DiagStat>): [string, DiagStat][] {
  return Object.keys(counters)
    .sort()
    .map((k) => [k, counters[k]]);
}
