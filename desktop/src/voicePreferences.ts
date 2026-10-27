// Y9-C — spoken preference rules: the pure half of the Settings panel.
//
// A rule is said once ("never use exclamation marks in email"), derived on
// this Mac by the polish model into {filter, rule}, shown back, and only takes
// effect when the user presses Apply. The rows are append-only: a retracted
// rule stays in the history so the user can read what they said.

/** One row of the append-only `voice_preferences` table (Rust: voice_prefs::VoicePreference). */
export type VoicePreference = {
  id: string;
  spoken: string;
  filter: string;
  rule: string;
  createdAt: string;
  active: boolean;
  appliedAt: string | null;
  retractedAt: string | null;
};

export type PreferenceStatus = "pending" | "active" | "retracted" | "discarded";

/** Where a row is in its one-way life: pending → active → retracted, or pending → discarded. */
export function preferenceStatus(p: VoicePreference): PreferenceStatus {
  if (p.retractedAt) return p.appliedAt ? "retracted" : "discarded";
  if (p.active && p.appliedAt) return "active";
  return "pending";
}

const APP_NAMES: Record<string, string> = {
  "com.apple.mail": "Mail",
  "com.tinyspeck.slackmacgap": "Slack",
  "com.apple.mobilesms": "Messages",
  "com.microsoft.outlook": "Outlook",
  "com.apple.notes": "Notes",
};

const LANGUAGE_NAMES: Record<string, string> = {
  en: "English",
  es: "Spanish",
  fr: "French",
  de: "German",
  ht: "Haitian Creole",
  it: "Italian",
  pt: "Portuguese",
};

/** The human name of an allowlisted filter: "email", "Slack", "German", "everything I dictate". */
export function describeFilter(filter: string): string {
  if (filter === "*") return "everything I dictate";
  const [kind, value = ""] = filter.split(/:(.*)/s);
  if (kind === "mode") return value;
  if (kind === "app") return APP_NAMES[value.toLowerCase()] ?? value;
  if (kind === "lang") return LANGUAGE_NAMES[value] ?? value;
  return filter;
}

/** The confirmation line the Apply step shows, before anything takes effect. */
export function heardLine(p: Pick<VoicePreference, "rule" | "filter">): string {
  const rule = p.rule.replace(/[.!]+$/, "");
  return `I heard: ${rule.charAt(0).toLowerCase()}${rule.slice(1)}. I'll apply it to: ${describeFilter(p.filter)}`;
}
