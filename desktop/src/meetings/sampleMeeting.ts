/**
 * yap-notetaker-onboard — the bundled SAMPLE meeting the 2-minute tour walks.
 *
 * A read-only fixture that lives only in this file: it is never recorded, never
 * written to the database, and never appears in the Meetings list. The tour
 * renders it with the real `<TranscriptList>`, so what the user sees is the
 * same component a recorded meeting uses.
 */
import type { TranscriptSegment } from "./transcript";

export const SAMPLE_TITLE = "Sample: Weekly product sync";

export const SAMPLE_SEGMENTS: TranscriptSegment[] = [
  { id: "s1", startSeconds: 0, track: 0, text: "Morning. Two things today: the onboarding numbers and the pricing page." },
  { id: "s2", startSeconds: 6, track: 1, text: "Onboarding is up. Most people finish setup in under three minutes now." },
  { id: "s3", startSeconds: 13, track: 0, text: "Great. Then the pricing page: the annual discount is hard to find." },
  { id: "s4", startSeconds: 19, track: 1, text: "Agreed. I'll move it next to the price and ship it Thursday." },
] as TranscriptSegment[];

export const SAMPLE_NOTES = [
  "Onboarding: most people finish setup in under three minutes.",
  "Pricing page: the annual discount is hard to find.",
  "Action: move the discount next to the price — ships Thursday.",
];

/** What Export writes for a meeting: Markdown, on your Mac. */
export const SAMPLE_EXPORT = `# ${SAMPLE_TITLE}

## Notes
${SAMPLE_NOTES.map((n) => `- ${n}`).join("\n")}

## Transcript
**Me** 0:00 — Morning. Two things today…`;
