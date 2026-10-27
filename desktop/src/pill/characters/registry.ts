/**
 * Y5-K — the character registry.
 *
 * The shell resolves the pill-style setting to a registered id ONCE, at its
 * mount point (float-main), through `resolveCharacter`, and an unknown id falls
 * back to `classic` rather than rendering nothing. the setting stays a free
 * string end to end — Rust never learns the list, and neither does a type union.
 */
import type { Character } from "./types";
import { classic } from "./classic";
import { yappy } from "./yappy";

const REGISTRY = new Map<string, Character>();

/** The id every unknown pill-style setting resolves to. */
export const FALLBACK_CHARACTER = "classic";

/**
 * Register a creature. Re-registering the SAME object is a no-op; a different
 * object under a taken id is a programming error, never a silent replacement.
 */
export function registerCharacter(c: Character): Character {
  if (!c || typeof c.id !== "string" || c.id.trim() === "") throw new Error("registerCharacter: a character needs an id");
  const prev = REGISTRY.get(c.id);
  if (prev && prev !== c) throw new Error(`registerCharacter: "${c.id}" is already registered`);
  REGISTRY.set(c.id, c);
  return c;
}

/** Every registered character, in registration order. */
export function characters(): Character[] {
  return [...REGISTRY.values()];
}

/** `pill-style setting` -> a registered character; unknown, empty or missing -> `classic`. */
export function resolveCharacter(id?: string | null): Character {
  return REGISTRY.get(id ?? "") ?? (REGISTRY.get(FALLBACK_CHARACTER) as Character);
}

registerCharacter(classic);
registerCharacter(yappy);
