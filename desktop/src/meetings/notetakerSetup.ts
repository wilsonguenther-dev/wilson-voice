/**
 * yap-notetaker-onboard — the Set up Notetaker card's state, kept out of the
 * component so the rules are testable without a DOM.
 *
 * The truth lives in Rust: `settings_kv` row `notetaker_setup_v1` (JSON), read
 * by `notetaker_setup_state` and written by `save_notetaker_setup`. The
 * calendar and import steps are ALSO marked done by Rust in the same command
 * that did the work (`connect_calendar`, `import_meetings`), so a dot can never
 * disagree with what actually happened. Help → "Set up Notetaker" clears
 * `dismissed` in Rust and emits `notetaker-setup` with the new state.
 */
import { invoke } from "@tauri-apps/api/core";

export type NotetakerSetup = {
  tour: boolean;
  calendar: boolean;
  import: boolean;
  dismissed: boolean;
};

export type WorkingStep = "tour" | "calendar" | "import";

export type SetupRow = {
  id: WorkingStep | "mcp";
  title: string;
  /** One line under the title. */
  detail: string;
  /** The plain verb on the row's button; `null` = no action (Coming). */
  verb: string | null;
  /** `null` for a Coming row: it has no dot to fill. */
  done: boolean | null;
};

export const EMPTY_SETUP: NotetakerSetup = {
  tour: false,
  calendar: false,
  import: false,
  dismissed: false,
};

/** The event Rust emits when Help → Set up Notetaker reopens the card. */
export const NOTETAKER_SETUP_EVENT = "notetaker-setup";

/** Exactly the four rows, in the order Wilson named them. */
export function setupRows(s: NotetakerSetup): SetupRow[] {
  return [
    {
      id: "tour",
      title: "Take the 2-minute tour",
      detail: "Walk through a sample meeting: start, live meters, stop, transcript, notes, export.",
      verb: s.tour ? "Replay" : "Start",
      done: s.tour,
    },
    {
      id: "calendar",
      title: "Connect calendar",
      detail:
        "Apple Calendar, read-only, on this Mac. It also sees Google or Exchange accounts already added to macOS Calendar. Google and Microsoft sign-in: Coming.",
      verb: s.calendar ? null : "Connect",
      done: s.calendar,
    },
    {
      id: "import",
      title: "Import from Otter",
      detail:
        "Pick Otter .txt or .srt exports, or a folder of them. Files only — Yap never signs in to Otter. Granola: Coming (its CSV export has no published column layout yet).",
      verb: "Import",
      done: s.import,
    },
    {
      id: "mcp",
      title: "Set up MCP",
      detail: "Let other AI apps read your meetings, with your OK per meeting.",
      verb: null,
      done: null,
    },
  ];
}

/** Every working step finished. The MCP row is Coming and never counts. */
export function allDone(s: NotetakerSetup): boolean {
  return s.tour && s.calendar && s.import;
}

/** The card sits at the top of Meetings until done or dismissed. */
export function shouldShow(s: NotetakerSetup | null): boolean {
  return s !== null && !s.dismissed && !allDone(s);
}

export function markDone(s: NotetakerSetup, step: WorkingStep): NotetakerSetup {
  return { ...s, [step]: true };
}

export function dismiss(s: NotetakerSetup): NotetakerSetup {
  return { ...s, dismissed: true };
}

/** Help → Set up Notetaker. Progress is kept; only the X is undone. */
export function reopen(s: NotetakerSetup): NotetakerSetup {
  return { ...s, dismissed: false };
}

export function loadSetup(): Promise<NotetakerSetup> {
  return invoke<NotetakerSetup>("notetaker_setup_state");
}

export function saveSetup(setup: NotetakerSetup): Promise<NotetakerSetup> {
  return invoke<NotetakerSetup>("save_notetaker_setup", { setup });
}

/** What `connect_calendar` returns. */
export type CalendarAccess = {
  status: string;
  calendars: number;
  connected: boolean;
  message: string | null;
};

/** What `import_meetings` returns per file. */
export type ImportOutcome = {
  name: string;
  meetingId: string | null;
  segments: number;
  error: string | null;
};

/** The extensions this build can import (Rust's `meeting_import::format_for`). */
export const IMPORTABLE = [".txt", ".srt"] as const;

export function isImportable(name: string): boolean {
  const lower = name.toLowerCase();
  return IMPORTABLE.some((ext) => lower.endsWith(ext));
}

/** One short line for the card after an import. */
export function importSummary(outcomes: ImportOutcome[]): string {
  const ok = outcomes.filter((o) => o.meetingId);
  const bad = outcomes.filter((o) => !o.meetingId);
  if (outcomes.length === 0) return "No Otter .txt or .srt files found there.";
  if (bad.length === 0) return `Imported ${ok.length} meeting${ok.length === 1 ? "" : "s"}.`;
  const first = bad[0];
  const lead = ok.length ? `Imported ${ok.length}; ` : "";
  return `${lead}${first.name}: ${first.error ?? "not imported"}${bad.length > 1 ? ` (+${bad.length - 1} more)` : ""}`;
}
