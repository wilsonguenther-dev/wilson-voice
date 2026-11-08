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
import floatCss from "../float.css?raw";
import PeopleStepper, {
  PEOPLE_MAX,
  PEOPLE_MIN,
  PILL_PEOPLE_MAX_W,
  calendarSuggestion,
  clampPeople,
  compactTitle,
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

/**
 * Batch 5 bounce: inline in the 208 px clock row, the stepper pushed the
 * centred, overflow-hidden badge past its width and clipped the clock and
 * Stop at every dock. It now shares the chip's ONE column and never adds width.
 */
describe("PeopleStepper in the pill never crowds the clock or Stop", () => {
  it("the compact variant is just − value +: no Auto chip, no suggestion chip", () => {
    const auto = renderToStaticMarkup(<PeopleStepper value={null} attendees={4} compact onChange={() => {}} />);
    expect(auto).not.toContain("people-auto");
    expect(auto).not.toContain("people-suggest");
    expect(auto).toContain('class="people-value is-auto"');
    expect(auto).toContain(">Auto</button>");
    // The suggestion and its reason ride the title, never applied on their own.
    expect(auto).toContain("Suggested 4 (4 on the invite)");
    expect(auto).toContain('data-people="Auto"');
    const four = renderToStaticMarkup(<PeopleStepper value={4} compact onChange={() => {}} />);
    expect(four).toContain(">4</button>");
    expect(four).toContain("4 people. Back to Auto");
    expect((four.match(/<button/g) ?? []).length).toBe(3);
    expect(four).not.toMatch(/tabindex="0"/);
  });

  it("the full variant (cards header) keeps Auto and the suggestion chip", () => {
    const html = renderToStaticMarkup(<PeopleStepper value={null} attendees={4} onChange={() => {}} />);
    expect(html).toContain("people-auto is-on");
    expect(html).toContain("people-suggest");
    expect(html).toContain("4 on the invite");
  });

  it("compactTitle explains the count and the calendar suggestion", () => {
    expect(compactTitle(null, null)).toBe("People in this meeting: Auto, the voices decide");
    expect(compactTitle(3, { count: 4, reason: "4 on the invite" })).toBe(
      "People in this meeting: 3, counting you · Suggested 4 (4 on the invite)",
    );
  });

  it("the stepper shares the speaker / call-audio column instead of adding one", () => {
    const swap = badgeSrc.indexOf('className="rec-swap"');
    expect(swap).toBeGreaterThan(-1);
    const close = badgeSrc.indexOf("</span>", swap);
    const inside = badgeSrc.slice(swap, close);
    expect(inside).toContain('speakerSlot("row", audioChip)');
    expect(inside).toContain("<MeetingPeopleSlot");
    // Exactly one mount in the pill, and it is the one inside the swap.
    expect(badgeSrc.split("<MeetingPeopleSlot").length).toBe(2);
  });

  it("float.css: at rest the swap is display: contents; shown, it is capped and the only thing that shrinks", () => {
    expect(PILL_PEOPLE_MAX_W).toBe(46);
    // 208 = 8 + scene 112 + 7 + column + 7 + Stop 20 + 8 → column 46.
    expect(8 + 112 + 7 + PILL_PEOPLE_MAX_W + 7 + 20 + 8).toBe(208);
    expect(floatCss).toMatch(/\.rec-swap\s*\{\s*display:\s*contents;/);
    expect(floatCss).toMatch(/\.rec-swap > \.rec-people\s*\{\s*display:\s*none;/);
    const shown = floatCss.slice(floatCss.indexOf(".meeting-badge:is(:hover, :focus-within) > .rec-swap:has(> .rec-people) {"));
    const rule = shown.slice(0, shown.indexOf("}"));
    expect(rule).toContain(`max-width: ${PILL_PEOPLE_MAX_W}px;`);
    expect(rule).toContain("min-width: 0;");
    expect(rule).toContain("overflow: hidden;");
    // The scene and Stop never shrink, so neither can be clipped.
    expect(floatCss).toMatch(/\.meeting-scene\s*\{[^}]*flex:\s*none;/);
    expect(floatCss).toMatch(/\.meeting-badge > \.rec-stop\s*\{\s*flex:\s*none;/);
  });
});
