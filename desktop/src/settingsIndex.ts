/**
 * yap-1106-settings-redesign (§14, docs/design/YAP-UI-SPEC-2026-11.md §1.4) —
 * the Settings information architecture as DATA, so the screen, the search and
 * the tests all read one table.
 *
 * Five sections in the slim left list (General, Dictation, Meetings, Yappy &
 * pill, Models & privacy) plus "Account & help", which is a footer link and not
 * a list row. Each section is an ordered list of BLOCKS; a block is one pane (or
 * one part of a pane) and appears in exactly one section, so every control is
 * shown exactly once.
 *
 * The shell still keeps `settingsTab` (the tray's "Keyboard Shortcuts…" event
 * and the License deep links send one of the old tab ids). The visible section
 * is always DERIVED from it through TAB_SECTION, so an old deep link lands on
 * the section that now holds that pane.
 */
import type { SettingsTab } from "./appTypes";

export type SettingsSection = "general" | "dictation" | "meetings" | "yappy" | "models" | "account";

export type SettingsBlock =
  | "shortcut"
  | "microphone"
  | "language"
  | "startup"
  | "dictation"
  | "audio-cleanup"
  | "snippets"
  | "dictionary-link"
  | "meetings"
  | "meeting-kinds"
  | "meeting-retention"
  | "speakers"
  | "meeting-exports"
  | "companion"
  | "notifications"
  | "news"
  | "speech-models"
  | "providers"
  | "privacy"
  | "license"
  | "diagnostics";

export interface SectionDef {
  id: SettingsSection;
  label: string;
  /** One plain-language line under the section title. */
  hint: string;
  blocks: readonly SettingsBlock[];
}

/** The left list, in order. Account & help is last and rendered in the footer. */
export const SETTINGS_SECTIONS: readonly SectionDef[] = [
  {
    id: "general",
    label: "General",
    hint: "The key you hold to talk, your microphone, your language, and how Yap starts.",
    blocks: ["shortcut", "microphone", "language", "startup"],
  },
  {
    id: "dictation",
    label: "Dictation",
    hint: "How your speech becomes clean text, and the words Yap expands for you.",
    blocks: ["dictation", "audio-cleanup", "snippets", "dictionary-link"],
  },
  {
    id: "meetings",
    label: "Meetings",
    hint: "Call audio, meeting kinds, how long audio is kept, who is talking, and where the notes go.",
    // spec §1.4: call audio (with detection and calendar), kinds, retention,
    // Speakers / voiceprints, destinations — in that order.
    blocks: ["meetings", "meeting-kinds", "meeting-retention", "speakers", "meeting-exports"],
  },
  {
    id: "yappy",
    label: "Yappy & pill",
    hint: "Yappy's voice, where the pill sits, which little messages it shows, and Yappy News.",
    blocks: ["companion", "notifications", "news"],
  },
  {
    id: "models",
    label: "Models & privacy",
    hint: "Where speech, notes and polish run, and what stays on this Mac.",
    blocks: ["speech-models", "providers", "privacy"],
  },
  {
    id: "account",
    label: "Account & help",
    hint: "Your license, and the health of Yap when something feels off.",
    blocks: ["license", "diagnostics"],
  },
];

/** The five rows of the left list (Account & help lives in the footer). */
export const LIST_SECTIONS: readonly SectionDef[] = SETTINGS_SECTIONS.filter((s) => s.id !== "account");

/** Which section now holds each of the old tab ids. Exhaustive by type. */
export const TAB_SECTION: Record<SettingsTab, SettingsSection> = {
  shortcut: "general",
  dictation: "dictation",
  snippets: "dictation",
  audio: "dictation",
  meetings: "meetings",
  companion: "yappy",
  notifications: "yappy",
  news: "yappy",
  advanced: "models",
  privacy: "models",
  license: "account",
  diagnostics: "account",
};

/**
 * The tab the shell is set to when a section is picked. Models & privacy uses
 * "privacy" so the shell's engine-status refresh (appShell) still fires.
 */
export const SECTION_HOME_TAB: Record<SettingsSection, SettingsTab> = {
  general: "shortcut",
  dictation: "dictation",
  meetings: "meetings",
  yappy: "companion",
  models: "privacy",
  account: "license",
};

export function sectionForTab(tab: SettingsTab | null | undefined): SettingsSection {
  return (tab && TAB_SECTION[tab]) || "dictation";
}

export function sectionDef(id: SettingsSection): SectionDef {
  return SETTINGS_SECTIONS.find((s) => s.id === id) ?? SETTINGS_SECTIONS[0];
}

export function sectionOfBlock(block: SettingsBlock): SettingsSection {
  const owner = SETTINGS_SECTIONS.find((s) => s.blocks.includes(block));
  return owner ? owner.id : "general";
}

/**
 * One searchable control. `label` is the words the control shows (the jump
 * finds the element with that text and highlights it); `help` is its hint.
 */
export interface SettingsEntry {
  label: string;
  help: string;
  block: SettingsBlock;
}

/**
 * Every control's label and help text, by block. Kept beside the IA so a new
 * control is one row here; settingsIndex.test.ts checks every block is indexed
 * and every label is unique.
 */
export const SETTINGS_INDEX: readonly SettingsEntry[] = [
  // General
  { block: "shortcut", label: "Hotkeys", help: "Keys that start dictation, hands-free and the scratchpad." },
  { block: "shortcut", label: "Hold-to-talk key", help: "The key you hold while you talk; let go to paste." },
  { block: "shortcut", label: "Command mode", help: "Say a command instead of text, like rewriting a selection." },
  { block: "microphone", label: "Microphone", help: "Which input Yap listens to, and the order to fall back to." },
  { block: "microphone", label: "If a mic disconnects", help: "The next microphone Yap switches to." },
  { block: "language", label: "Language", help: "The languages you speak; Yap listens for these." },
  { block: "startup", label: "Launch Yap at login", help: "Start at login so your hold-to-talk key always works." },
  { block: "startup", label: "Check for updates", help: "Look for a new Yap at launch; nothing installs until you say so." },
  // Dictation
  { block: "dictation", label: "Dictation mode", help: "How Yap listens: hold to talk or hands-free." },
  { block: "dictation", label: "How much should Yap clean up?", help: "Formatting: filler words, punctuation, polish." },
  { block: "dictation", label: "How long may the model think?", help: "Trade a little speed for cleaner text." },
  { block: "dictation", label: "How should Yap sound in each app?", help: "A writing tone per app." },
  { block: "dictation", label: "Should Yap sign off for you?", help: "A signature added to emails and messages." },
  { block: "dictation", label: "Lists", help: "Turn spoken lists into bullets or numbers, per app." },
  { block: "dictation", label: "Paste", help: "Paste my words automatically when I finish talking. Pastes when you pause." },
  { block: "audio-cleanup", label: "Denoise", help: "Remove fans, hum and keyboard clatter before transcribing." },
  { block: "audio-cleanup", label: "Mute the Mac while dictating", help: "Your exact volume comes back when you stop." },
  { block: "audio-cleanup", label: "Sounds", help: "A short chirp when you start, stop and paste." },
  { block: "snippets", label: "New snippet", help: "A spoken trigger that expands to saved text." },
  { block: "snippets", label: "Where triggers fire", help: "Which apps snippets expand in." },
  { block: "dictionary-link", label: "Dictionary", help: "Words and names Yap should always spell your way." },
  // Meetings
  { block: "meetings", label: "Meeting detection", help: "Offer to record a call Yap notices. It never records on its own." },
  { block: "meetings", label: "Calls in a browser", help: "Notice calls in a browser tab too." },
  { block: "meetings", label: "Screen sharing", help: "What Yap does while you share your screen." },
  { block: "meetings", label: "Never offer for", help: "Apps Yap never offers to record." },
  { block: "meetings", label: "Call audio", help: "Record the other side of the call as well as your microphone." },
  { block: "meeting-kinds", label: "Meeting kinds", help: "In person, Virtual or Not sure: how Yap tells the speakers apart." },
  { block: "meeting-kinds", label: "New meetings start as", help: "The kind a new meeting begins with; change it before you press Record." },
  { block: "meeting-retention", label: "Meeting audio retention", help: "How long meeting audio is kept: 7 days, or 30 when a transcript is incomplete." },
  { block: "speakers", label: "Speakers", help: "How Yap labels who is talking: You, Call, Speaker 1, Speaker 2." },
  { block: "speakers", label: "Voiceprints", help: "Saved voices: none. Yap keeps no voiceprint of anyone." },
  { block: "meeting-exports", label: "Meeting exports", help: "Destinations: the folder finished meeting notes are exported to." },
  // Yappy & pill
  { block: "companion", label: "Pill style", help: "The one pill: Yappy." },
  { block: "companion", label: "Screen position", help: "Where the pill docks on your screen." },
  { block: "companion", label: "Yappy's voice", help: "Best friend, Badass or Professional; how Yappy talks to you." },
  { block: "companion", label: "Always on screen", help: "Keep the pill visible even when you are not talking." },
  { block: "notifications", label: "On the pill", help: "Tips, feature news and milestones the pill may show." },
  { block: "news", label: "Yappy News", help: "A short briefing on Home: on or off, your topics, and when it last refreshed." },
  { block: "news", label: "News topics", help: "The topics and tickers Yappy's briefing follows." },
  // Models & privacy
  { block: "speech-models", label: "Speech model", help: "Speech engine, on this Mac: smaller is faster, larger is more accurate." },
  { block: "speech-models", label: "Power and heat", help: "Pause heavy work on battery below a charge you pick." },
  { block: "speech-models", label: "Keep the model loaded from launch", help: "Every take starts instantly; uses more memory." },
  { block: "providers", label: "Model providers", help: "Where notes and polish run: Cloud or On this Mac." },
  { block: "providers", label: "Your key", help: "Your own cloud key: faster cleanup and notes when online, a daily limit, and where they run." },
  { block: "privacy", label: "Your data", help: "Recordings, history and logs live on your Mac." },
  { block: "privacy", label: "Recording other people", help: "Consent before you record a call." },
  { block: "privacy", label: "Engine", help: "What is running right now, and whether it is ready." },
  { block: "privacy", label: "Stability", help: "Crash reports and recovery." },
  { block: "privacy", label: "Support", help: "Send a redacted support bundle." },
  // Account & help
  { block: "license", label: "License", help: "Your trial, purchase and activation key." },
  { block: "diagnostics", label: "Health", help: "Diagnostics: one line on how Yap is doing." },
  { block: "diagnostics", label: "Counters", help: "How many takes, pastes and failures." },
  { block: "diagnostics", label: "Recent events", help: "The last things Yap did, with an export." },
];

export interface SettingsHit extends SettingsEntry {
  section: SettingsSection;
}

const norm = (s: string) => s.toLowerCase().normalize("NFKD").replace(/[̀-ͯ]/g, "");

/**
 * Search labels and help text across every section. Every word of the query
 * must appear (in the label, the help, or the section name); label matches rank
 * first, then the IA order. An empty or blank query returns no hits.
 */
export function searchSettings(query: string, limit = 12): SettingsHit[] {
  const words = norm(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const scored: { hit: SettingsHit; score: number; order: number }[] = [];
  SETTINGS_INDEX.forEach((e, order) => {
    const section = sectionOfBlock(e.block);
    const label = norm(e.label);
    const hay = `${label} ${norm(e.help)} ${norm(sectionDef(section).label)}`;
    if (!words.every((w) => hay.includes(w))) return;
    const score = words.every((w) => label.includes(w)) ? 0 : label.includes(words[0]) ? 1 : 2;
    scored.push({ hit: { ...e, section }, score, order });
  });
  scored.sort((a, b) => a.score - b.score || a.order - b.order);
  return scored.slice(0, Math.max(0, limit)).map((s) => s.hit);
}

/** ↑/↓ in the section list: the next list section, wrapping. */
export function stepSection(current: SettingsSection, delta: 1 | -1): SettingsSection {
  const ids = LIST_SECTIONS.map((s) => s.id);
  const at = ids.indexOf(current);
  if (at < 0) return ids[delta > 0 ? 0 : ids.length - 1];
  return ids[(at + delta + ids.length) % ids.length];
}
