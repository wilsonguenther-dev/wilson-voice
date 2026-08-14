/**
 * YV98 — the crash-report button's frontend contract.
 *
 * Two invokes, in order: `preview_support_bundle` builds the zip in memory and
 * returns what is in it; `send_support_bundle` writes those same bytes and
 * either opens a compose window or reveals the file. The sheet in between is
 * not a confirmation dialog — it is the whole privacy story of this feature,
 * because it is the only moment the user gets to read what they are about to
 * send before it exists as a file.
 *
 * Everything here is pure so it can be tested without a webview, and so the
 * copy that describes what the button will do lives in one place rather than
 * being invented twice (once for the sheet, once for the toast).
 */

/** One file inside the bundle, with a prefix of its REAL redacted contents. */
export interface SupportBundleEntry {
  name: string;
  bytes: number;
  lines: number;
  excerpt: string;
  truncated: boolean;
}

export interface SupportBundlePreview {
  fileName: string;
  recipient: string;
  /** AppKit's `canPerformWithItems:` said a compose window is achievable. */
  mailAvailable: boolean;
  totalBytes: number;
  entries: SupportBundleEntry[];
}

export interface SupportSendOutcome {
  /** `compose` — a mail window is open. `reveal` — the file is in Finder. */
  method: "compose" | "reveal";
  path: string;
  recipient: string;
  message: string;
}

/** `12.4 KB`. Bundles are text; nothing here is ever megabytes. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const kb = bytes / 1024;
  if (kb < 1024) return `${kb.toFixed(kb < 10 ? 1 : 0)} KB`;
  return `${(kb / 1024).toFixed(1)} MB`;
}

/**
 * What the button is about to do, said before it is pressed.
 *
 * The plan's rule is "never a dead end", and the way to honour it in copy is to
 * stop pretending both paths are the same path. If `canPerformWithItems:`
 * already said no — Outlook, Airmail, no mail client at all — the sheet says so
 * up front rather than opening nothing and leaving the user to guess.
 */
export function actionLabel(preview: SupportBundlePreview | null): string {
  if (!preview) return "Send crash report to Wilson";
  return preview.mailAvailable
    ? "Open the email with this attached"
    : "Save it and show me the file";
}

/** The one sentence under the action button. */
export function actionExplainer(preview: SupportBundlePreview): string {
  return preview.mailAvailable
    ? `Yap will save ${preview.fileName} to your Desktop and open a message to ${preview.recipient} with it attached. Nothing is sent until you press send.`
    : `Yap could not find a mail app it can drive, so it will save ${preview.fileName} to your Desktop, show it to you in Finder, and copy ${preview.recipient} to your clipboard for you to attach it yourself.`;
}

/**
 * The plain-language line for one entry. Names are paths inside the zip
 * (`logs/yap.log.2`), which is not what a person calls the thing.
 */
export function describeEntry(name: string): string {
  if (name === "README.txt") return "What this file is, and what is not in it";
  if (name === "environment.txt") return "App version, bundle id and macOS version";
  if (name === "crash-summary.txt") return "The crashes Yap recorded on this Mac";
  if (name === "permissions.txt") return "Which permissions are granted";
  if (name === "models.txt") return "Which speech models are downloaded";
  if (name === "logs/yap.log") return "The current log — redacted";
  if (name.startsWith("logs/yap.log.")) {
    return `Rotated log ${name.slice("logs/yap.log.".length)} — redacted`;
  }
  return name;
}

/** Entries whose contents went through the redactor, for the sheet's badge. */
export function isRedacted(name: string): boolean {
  return name.startsWith("logs/");
}
