/**
 * YP2 — reading a rejected `invoke`.
 *
 * Almost every Tauri command in Yap answers `Result<_, String>`, so the UI could
 * get away with `String(e)`. The license gate cannot: "your trial ended" is a
 * different screen from "the mic is busy", so `manual_toggle` and
 * `activate_license` reject with a structured `{ code, message }` instead.
 *
 * `String({code, message})` renders as `[object Object]` — which is exactly the
 * kind of quiet regression that ships. These two helpers are the only way the
 * frontend should read a command rejection.
 */

/** The structured rejection the Rust command layer returns (`CommandError`). */
export type CommandError = { code: string; message: string };

function isCommandError(e: unknown): e is CommandError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as CommandError).code === "string" &&
    typeof (e as CommandError).message === "string"
  );
}

/** The sentence to show a person, whatever shape the rejection arrived in. */
export function errorText(e: unknown): string {
  if (isCommandError(e)) return e.message;
  return String(e);
}

/** The stable machine code, when there is one. `null` for a plain string error. */
export function errorCode(e: unknown): string | null {
  return isCommandError(e) ? e.code : null;
}

/** The trial has run out and no license is installed. NEW dictation only. */
export const LICENSE_REQUIRED = "license_required";

export function isLicenseRequired(e: unknown): boolean {
  return errorCode(e) === LICENSE_REQUIRED;
}

/*
 * Y5-F — the error CATALOGUE.
 *
 * `errorText` turns a rejection into a sentence, but for the common case (a
 * plain `Result<_, String>` rejection) that sentence is whatever Rust wrote —
 * an io error's Debug, a SQLite message — in front of a person. The catalogue
 * is the other half: every CODE Yap can emit maps to one line a person can act
 * on and, where there is one, the action that fixes it. An unknown code or a
 * bare string gets the generic advice, which still says what to do next (send
 * a support bundle) instead of echoing the raw text.
 *
 * Both error surfaces read THIS table: the pill shows `line` in its `error`
 * phase, and the main window's toast adds the `action` button.
 *
 * The keys are matched from Rust (take_error.rs reads this file in a test), so
 * keep each entry on its own line in the `  code: {` shape.
 */

/** What a toast's button does. The shell maps each kind to a handler. */
export type AdviceActionKind = "support" | "permissions" | "models" | "license" | "retry" | "copy-again";

export type AdviceAction = { kind: AdviceActionKind; label: string };

export type ErrorAdvice = { line: string; action: AdviceAction | null };

const SUPPORT: AdviceAction = { kind: "support", label: "Send report" };
const PERMISSIONS: AdviceAction = { kind: "permissions", label: "Open Permissions" };
const MODELS: AdviceAction = { kind: "models", label: "Open Models" };

export const ERROR_CATALOGUE = {
  license_required: { line: "Your Yap trial has ended — add a license to keep dictating.", action: { kind: "license", label: "Get a license" } },
  mic_permission_required: { line: "Yap needs microphone access to hear you.", action: PERMISSIONS },
  silent_capture: { line: "That take recorded silence — check your microphone and its permission.", action: PERMISSIONS },
  cancelled: { line: "Cancelled — the recording is kept in History.", action: null },
  mic_unavailable: { line: "Yap could not open the microphone — check that an input device is connected.", action: PERMISSIONS },
  mic_disconnected: { line: "The microphone disconnected mid-take — the audio was kept, press Retry.", action: null },
  model_missing: { line: "No speech model installed — download one to start dictating.", action: MODELS },
  capture_failed: { line: "Yap could not save that recording. Try again, and send a report if it keeps happening.", action: SUPPORT },
  transcription_failed: { line: "The speech engine could not transcribe that take — the audio was kept, press Retry.", action: null },
  history_write_failed: { line: "Yap transcribed that take but could not save it to History.", action: SUPPORT },
  clipboard_failed: { line: "Yap could not put the text on the clipboard — it is saved in History.", action: null },
  paste_not_confirmed: { line: "The paste may not have landed — the text is on your clipboard.", action: { kind: "copy-again", label: "Copy again" } },
  take_failed: { line: "Yap could not finish that take. Send a report so we can see why.", action: SUPPORT },
} satisfies Record<string, ErrorAdvice>;

export type CatalogueCode = keyof typeof ERROR_CATALOGUE;

/** The advice for anything with no catalogue entry — still says what to do. */
export const GENERIC_ADVICE: ErrorAdvice = {
  line: "Yap could not finish that. If it keeps happening, send a report.",
  action: SUPPORT,
};

function isCatalogued(code: string): code is CatalogueCode {
  return Object.prototype.hasOwnProperty.call(ERROR_CATALOGUE, code);
}

/** The line + action for a code. Unknown or missing codes get GENERIC_ADVICE. */
export function errorAdvice(code: string | null | undefined): ErrorAdvice {
  if (code && isCatalogued(code)) return ERROR_CATALOGUE[code];
  return GENERIC_ADVICE;
}

/**
 * The advice for a REJECTION, whatever shape it arrived in. A coded rejection
 * reads the catalogue; a bare string is never shown — it goes to the console
 * for whoever is debugging, and the person gets `fallbackLine` (what the UI was
 * trying to do, e.g. "Yap could not save that word.") or the generic line, plus
 * the support action either way.
 */
export function adviceFor(e: unknown, fallbackLine?: string): ErrorAdvice {
  const code = errorCode(e);
  if (code && isCatalogued(code)) return ERROR_CATALOGUE[code];
  console.warn("[yap] uncatalogued failure:", e);
  if (fallbackLine) return { line: fallbackLine, action: SUPPORT };
  return GENERIC_ADVICE;
}
