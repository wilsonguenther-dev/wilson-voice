/**
 * Y5-K — the character registry.
 *
 * The shell resolves the pill-style setting to a registered id ONCE, at its
 * mount point (float-main), through `resolveCharacter`, and an unknown id falls
 * back to `yappy` rather than rendering nothing. the setting stays a free
 * string end to end — Rust never learns the list, and neither does a type union.
 *
 * yap-1106-retire-classic-pill — the Classic pill is RETIRED as a choice: it is
 * no longer registered, so a stale `"classic"` setting resolves to Yappy like
 * any other unknown id (the settings migration rewrites it on disk too). Its
 * module lives on ONLY as the shell's neutral body painter (`shellBody`): what
 * `PillShell` draws when it is mounted with no character, and what paints a
 * frame a character has no art for. It is never offered, listed or resolved.
 */
import type { Character } from "./types";
import { classic } from "./classic";
import { yappy } from "./yappy";

const REGISTRY = new Map<string, Character>();

/** The id every unknown, empty, missing or retired pill-style setting resolves to. */
export const FALLBACK_CHARACTER = "yappy";

/** Pill-style ids that once shipped and were retired; each resolves to the fallback. */
export const RETIRED_CHARACTERS: readonly string[] = ["classic"];

/**
 * The shell's neutral body painter (the retired Classic capsule's art). Not a
 * registered character: `characters()` never lists it and `resolveCharacter`
 * never returns it.
 */
export function shellBody(): Character {
  return classic;
}

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

/** `pill-style setting` -> a registered character; unknown, empty, missing or retired -> `yappy`. */
export function resolveCharacter(id?: string | null): Character {
  return REGISTRY.get(id ?? "") ?? (REGISTRY.get(FALLBACK_CHARACTER) as Character);
}

registerCharacter(yappy);
