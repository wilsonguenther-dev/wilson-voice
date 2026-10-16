/**
 * yap-format-structure — the pure state behind Settings > Dictation > Lists.
 *
 * The backend decides the list syntax per paste target
 * (`structure::structure_format_for`); these helpers only read and edit the two
 * settings that steer it, so the view stays a thin renderer and the edits are
 * testable without a DOM.
 */

/** What a per-app override can pick. Mirrors the backend's accepted values. */
export type StructureChoice = "markdown" | "plain" | "off";

/** The choices in the order the picker shows them, in plain words (Y4-H). */
export const STRUCTURE_CHOICES: ReadonlyArray<readonly [StructureChoice, string]> = [
  ["markdown", "Bullets and headings"],
  ["plain", "Simple bullets"],
  ["off", "Leave it as I said it"],
];

/** The switch ships ON: an absent value is on. */
export function structureEnabled(settings: { structureEnabled?: boolean }): boolean {
  return settings.structureEnabled ?? true;
}

/** The overrides as sorted rows, so the list renders in a stable order. */
export function structureOverrideRows(
  overrides: Record<string, string> | undefined,
): Array<[string, StructureChoice]> {
  return Object.entries(overrides ?? {})
    .filter((row): row is [string, StructureChoice] =>
      STRUCTURE_CHOICES.some(([id]) => id === row[1]),
    )
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
}

/**
 * Set (or, with `null`, remove) one app's override. Returns a NEW map; a blank
 * app id changes nothing.
 */
export function withStructureOverride(
  overrides: Record<string, string> | undefined,
  appId: string,
  choice: StructureChoice | null,
): Record<string, string> {
  const id = appId.trim();
  const next = { ...(overrides ?? {}) };
  if (!id) return next;
  if (choice === null) delete next[id];
  else next[id] = choice;
  return next;
}
