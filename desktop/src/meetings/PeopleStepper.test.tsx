/**
 * yap-1106-fb-23 — the People stepper: Auto, bounds 1..12, keyboard ±, and
 * the calendar suggestion with its reason. No DOM: pure model +
 * `renderToStaticMarkup` + source text, like the other meeting tests.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import libRs from "../../src-tauri/src/lib.rs?raw";
import badgeSrc from "../pill/MeetingBadge.tsx?raw";
import whoSrc from "./WhoIsTalking.tsx?raw";
import PeopleStepper, {
  PEOPLE_MAX,
  PEOPLE_MIN,
  calendarSuggestion,
  clampPeople,
  keyToPeople,
  peopleLabel,
  stepPeople,
} from "./PeopleStepper";

describe("PeopleStepper model", () => {
  it("Auto is the default and reads as Auto", () => {
    expect(peopleLabel(null)).toBe("Auto");
    const html = renderToStaticMarkup(<PeopleStepper value={null} onChange={() => {}} />);
    expect(html).toContain('data-people="Auto"');
    expect(html).toContain('aria-pressed="true"');
    expect(html).toContain("Auto, the voices decide");
  });

  it("stays inside 1..12", () => {
    expect(PEOPLE_MIN).toBe(1);
    expect(PEOPLE_MAX).toBe(12);
    expect(stepPeople(1, -1)).toBe(1);
    expect(stepPeople(12, 1)).toBe(12);
    expect(stepPeople(4, 1)).toBe(5);
    expect(stepPeople(4, -1)).toBe(3);
    expect(clampPeople(40)).toBe(12);
    expect(clampPeople(0)).toBe(1);
    const min = renderToStaticMarkup(<PeopleStepper value={1} onChange={() => {}} />);
    expect(min).toMatch(/aria-label="One fewer person"[^>]*disabled/);
    const max = renderToStaticMarkup(<PeopleStepper value={12} onChange={() => {}} />);
    expect(max).toMatch(/aria-label="One more person"[^>]*disabled/);
  });

  it("leaving Auto starts at the calendar suggestion, else 2", () => {
    expect(stepPeople(null, 1)).toBe(2);
    expect(stepPeople(null, -1, 4)).toBe(4);
  });

  it("keyboard: arrows and +/− step, Home is Auto, other keys are not ours", () => {
    expect(keyToPeople("ArrowUp", 4)).toBe(5);
    expect(keyToPeople("+", 4)).toBe(5);
    expect(keyToPeople("ArrowDown", 4)).toBe(3);
    expect(keyToPeople("-", 4)).toBe(3);
    expect(keyToPeople("ArrowDown", 1)).toBe(1);
    expect(keyToPeople("ArrowUp", 12)).toBe(12);
    expect(keyToPeople("Home", 4)).toBeNull();
    expect(keyToPeople("Enter", 4)).toBeUndefined();
  });

  it("the calendar count is a suggestion with its reason, never applied on its own", () => {
    expect(calendarSuggestion(4, null)).toEqual({ count: 4, reason: "4 on the invite" });
    expect(calendarSuggestion(4, 4)).toBeNull();
    expect(calendarSuggestion(null, null)).toBeNull();
    expect(calendarSuggestion(30, null)).toEqual({ count: 12, reason: "12 on the invite" });
    const html = renderToStaticMarkup(<PeopleStepper value={null} attendees={4} onChange={() => {}} />);
    expect(html).toContain("4 on the invite");
    expect(html).toContain('data-people="Auto"');
  });

  it("the after-meeting header reads '4 voices · People − 4 +'", () => {
    const html = renderToStaticMarkup(<PeopleStepper value={4} voices={4} onChange={() => {}} />);
    expect(html).toContain("4 voices · ");
    expect(html).toContain('data-people="4"');
    expect(html).toContain('aria-valuenow="4"');
  });

  it("the pill variant never takes keyboard focus from the call", () => {
    const html = renderToStaticMarkup(<PeopleStepper value={4} compact onChange={() => {}} />);
    expect(html).not.toMatch(/tabindex="0"/);
    expect(html).toContain("is-compact");
  });

  it("is mounted in the pill and the cards header, backed by registered commands", () => {
    expect(badgeSrc).toContain("<PeopleStepper");
    expect(badgeSrc).toContain("meeting_set_expected_speakers");
    expect(badgeSrc).toContain("!screenSharing");
    expect(whoSrc).toContain("<PeopleStepper");
    expect(whoSrc).toContain("meeting_recap_speakers");
    expect(libRs).toContain("live_transcript::meeting_set_expected_speakers");
    expect(libRs).toContain("live_transcript::meeting_recap_speakers");
  });
});
