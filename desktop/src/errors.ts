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
