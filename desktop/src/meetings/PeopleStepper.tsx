/**
 * yap-1106-fb-23 — "How many people are in this meeting?"
 *
 * A small stepper, "People: Auto · − 4 +", 1 to 12 plus Auto (it counts
 * you). Auto is today's behaviour: the voices decide. A number is a CEILING
 * on the voices the meeting shows, never a target — if only 3 people talk,
 * there are 3 speakers. Shown in the meeting pill while recording (never
 * during a screen share, and it never takes focus from the call) and in the
 * after-meeting cards header ("4 voices · People − 4 +").
 *
 * When the calendar is on, the attendee count is offered as a suggestion with
 * its reason ("4 on the invite"); it is never applied without a click.
 */
import type { KeyboardEvent } from "react";

export const PEOPLE_MIN = 1;
export const PEOPLE_MAX = 12;

/** `null` = Auto. */
export type People = number | null;

/** One step: from Auto, − starts at the suggestion or 2, + at the suggestion or 2. */
export function stepPeople(value: People, delta: 1 | -1, suggestion?: number | null): People {
  if (value === null) {
    const start = clampPeople(suggestion ?? 2);
    return start;
  }
  const next = value + delta;
  if (next < PEOPLE_MIN) return PEOPLE_MIN;
  if (next > PEOPLE_MAX) return PEOPLE_MAX;
  return next;
}

export function clampPeople(n: number): number {
  if (!Number.isFinite(n)) return PEOPLE_MIN;
  return Math.min(PEOPLE_MAX, Math.max(PEOPLE_MIN, Math.round(n)));
}

/** What the value reads as: "Auto" or the number. */
export function peopleLabel(value: People): string {
  return value === null ? "Auto" : String(value);
}

/**
 * The calendar's suggestion: the invite's attendee count, inside 1..12, with
 * its reason. `null` when the calendar is off, the count is unknown, or it
 * already matches the value.
 */
export function calendarSuggestion(attendees: number | null | undefined, value: People): { count: number; reason: string } | null {
  if (attendees === null || attendees === undefined || attendees < PEOPLE_MIN) return null;
  const count = clampPeople(attendees);
  if (value === count) return null;
  return { count, reason: `${count} on the invite` };
}

/** Arrow keys / +/− / Home (Auto) on the stepper; `undefined` = not ours. */
export function keyToPeople(key: string, value: People, suggestion?: number | null): People | undefined {
  switch (key) {
    case "ArrowUp":
    case "ArrowRight":
    case "+":
    case "=":
      return stepPeople(value, 1, suggestion);
    case "ArrowDown":
    case "ArrowLeft":
    case "-":
      return stepPeople(value, -1, suggestion);
    case "Home":
    case "a":
    case "A":
      return null;
    default:
      return undefined;
  }
}

interface Props {
  value: People;
  onChange: (next: People) => void;
  /** Calendar attendee count, when the calendar is on. */
  attendees?: number | null;
  /** "4 voices" before the stepper, in the after-meeting header. */
  voices?: number | null;
  disabled?: boolean;
  /** Compact pill variant: no buttons take focus away from the call. */
  compact?: boolean;
}

export default function PeopleStepper({ value, onChange, attendees, voices, disabled = false, compact = false }: Props) {
  const suggestion = calendarSuggestion(attendees, value);
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const next = keyToPeople(e.key, value, suggestion?.count ?? null);
    if (next === undefined || disabled) return;
    e.preventDefault();
    if (next !== value) onChange(next);
  };
  const atMin = value !== null && value <= PEOPLE_MIN;
  const atMax = value !== null && value >= PEOPLE_MAX;
  // In the pill the buttons are pointer targets only (tabIndex -1): the pill
  // is non-activating and never steals keyboard focus from the call.
  const tab = compact ? -1 : undefined;
  return (
    <div
      className={`people-stepper${compact ? " is-compact" : ""}`}
      role="spinbutton"
      aria-label="People in this meeting"
      aria-valuemin={PEOPLE_MIN}
      aria-valuemax={PEOPLE_MAX}
      aria-valuenow={value ?? undefined}
      aria-valuetext={value === null ? "Auto, the voices decide" : `${value} people, counting you`}
      aria-disabled={disabled || undefined}
      tabIndex={compact || disabled ? -1 : 0}
      onKeyDown={onKey}
      data-people={peopleLabel(value)}
    >
      {voices !== null && voices !== undefined && (
        <span className="people-voices">
          {voices} {voices === 1 ? "voice" : "voices"} ·{" "}
        </span>
      )}
      <span className="people-label">People</span>
      <button
        type="button"
        className={`people-auto${value === null ? " is-on" : ""}`}
        aria-pressed={value === null}
        tabIndex={tab}
        disabled={disabled}
        onClick={() => value !== null && onChange(null)}
      >
        Auto
      </button>
      <button
        type="button"
        className="people-step"
        aria-label="One fewer person"
        tabIndex={tab}
        disabled={disabled || atMin}
        onClick={() => onChange(stepPeople(value, -1, suggestion?.count ?? null))}
      >
        −
      </button>
      <span className="people-value" aria-hidden>
        {value === null ? "·" : value}
      </span>
      <button
        type="button"
        className="people-step"
        aria-label="One more person"
        tabIndex={tab}
        disabled={disabled || atMax}
        onClick={() => onChange(stepPeople(value, 1, suggestion?.count ?? null))}
      >
        +
      </button>
      {suggestion && (
        <button
          type="button"
          className="people-suggest"
          tabIndex={tab}
          disabled={disabled}
          title="A suggestion from your calendar"
          onClick={() => onChange(suggestion.count)}
        >
          {suggestion.count}? <span className="people-suggest-reason">{suggestion.reason}</span>
        </button>
      )}
    </div>
  );
}
