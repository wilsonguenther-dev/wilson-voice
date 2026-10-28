/**
 * PhaseAnnouncer — the pill's ONE live region (Y5-J).
 *
 * Rendered as the first child of each pill's ROOT (ClassicPill's `.stage`,
 * YappyPill's `.kami-stage`), so whichever character is on screen, the pill
 * root carries exactly one `aria-live="polite"` for the dictation phase.
 * Visually hidden (`.sr-only` in float.css): the eye already has the capsule
 * and the phase strip; this is the same fact for a screen reader.
 *
 * Its text is `phaseAnnouncement(phase)` — a function of the phase alone — so
 * a frame that redraws the waveform, a new mic level or a chunk tick produce
 * the identical string and nothing is re-announced. Never put the amplitude,
 * the progress numeral or Yappy's chatter in here.
 *
 * The meeting badge keeps its own regions (MeetingBadge.tsx): the recording
 * clock is `aria-live="off"`, the post-meeting card is polite. Those report a
 * different state machine and never fire together with a dictation change.
 */
import { phaseAnnouncement, PHASE_ID, type LivePhase } from "./live";

export default function PhaseAnnouncer({ phase, chordLabel }: { phase: LivePhase; chordLabel?: string }) {
  return (
    <span className="sr-only" role="status" aria-live="polite" aria-atomic="true" data-announces={PHASE_ID[phase]}>
      {phaseAnnouncement(phase, chordLabel)}
    </span>
  );
}
