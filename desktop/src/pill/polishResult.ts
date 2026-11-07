// yap-1106-byom-cloud-fast-path — the polish result a take carries out of
// Rust: {raw, polished, route, reason} (`lib.rs` POLISH_RESULT_EVENT). The
// reveal-edit card itself is owned by yap-wf-05 (it covers local AND cloud
// polish); this file is the contract it reads, plus the two actions it must
// get exactly right. Pure model: no hooks, no Tauri imports.

/** Mirrors `lib.rs` `POLISH_RESULT_EVENT`. */
export const POLISH_RESULT_EVENT = "polish_result";
/**
 * "Polished · Reveal edit" stays on the pill at least this long
 * (yap-1107-reveal-stays-until-read: 8 s minimum, never under the pointer;
 * the full rule is `revealDwell.ts`).
 */
export const REVEAL_EDIT_TTL_MS = 8_000;

export type PolishRoute = "cloud" | "local";

/** The `polish_result` payload. Webview only: never logged. */
export interface PolishResult {
  /** The take as dictated (ASR + dictionary), before any polish. */
  raw: string;
  /** What was pasted. */
  polished: string;
  /** Where the polish ran. */
  route: PolishRoute;
  /** Why it ran there (`cloud_route.rs` Reason tag). */
  reason: string;
}

const isText = (v: unknown): v is string => typeof v === "string";

export function isPolishResult(v: unknown): v is PolishResult {
  if (!v || typeof v !== "object") return false;
  const o = v as Record<string, unknown>;
  return (
    isText(o.raw) &&
    isText(o.polished) &&
    (o.route === "cloud" || o.route === "local") &&
    isText(o.reason)
  );
}

/** Only a take whose polish actually changed the words offers "Reveal edit". */
export function hasRevealableEdit(r: PolishResult): boolean {
  return r.raw.trim().length > 0 && r.raw.trim() !== r.polished.trim();
}

/** The pill line after a polished paste. */
export function revealEditLine(r: PolishResult): string | null {
  return hasRevealableEdit(r) ? "Polished · Reveal edit" : null;
}

export type RevealEditChoice = "keep" | "use_original";

/** What the card does for a choice. */
export type RevealEditEffect =
  | { kind: "none" }
  | { kind: "paste"; text: string }
  | { kind: "copy"; text: string; notice: string };

/**
 * "Keep" leaves the paste alone. "Use original" re-pastes the raw take, or,
 * when the field changed since the paste (the user typed, or focus moved),
 * copies it instead so nothing is overwritten.
 */
export function revealEditEffect(
  r: PolishResult,
  choice: RevealEditChoice,
  fieldUnchanged: boolean,
): RevealEditEffect {
  if (choice === "keep" || !hasRevealableEdit(r)) return { kind: "none" };
  if (fieldUnchanged) return { kind: "paste", text: r.raw };
  return { kind: "copy", text: r.raw, notice: "The field changed, so the original is on your clipboard." };
}
