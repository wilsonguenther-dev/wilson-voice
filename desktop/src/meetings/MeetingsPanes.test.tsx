/**
 * yap-1106-meetings-redesign — the two-pane pieces, rendered with
 * renderToStaticMarkup (no DOM), plus the view's source shape: two panes,
 * the three tabs, the hero, the strip, the Undo.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import meetingsView from "../views/Meetings.tsx?raw";
import meetingsCss from "./meetings.css?raw";
import {
  DetailTabs,
  ListSkeleton,
  MeetingFilters,
  PeopleChips,
  PeopleTab,
  PickAMeeting,
  RecordingHero,
  UndoDeleteBar,
  WhoIsTalkingStrip,
} from "./MeetingsPanes";
import { NO_FILTER, PERSON_REASON, type PersonRow } from "./meetingsLayout";

const noop = () => {};
const KINDS = [
  { kind: "virtual", label: "Video call", menuId: "a" },
  { kind: "in_person", label: "In person", menuId: "b" },
];
const SPEAKER: PersonRow = { name: "Speaker", lines: 4, seconds: 90, firstOffset: "00:00:02", named: false, reason: PERSON_REASON.Speaker };
const YOU: PersonRow = { name: "You", lines: 7, seconds: 300, firstOffset: "00:00:00", named: true, reason: PERSON_REASON.You };

describe("list pane", () => {
  it("filters by kind, notes and person, with Clear only while a filter is on", () => {
    const off = renderToStaticMarkup(<MeetingFilters filter={NO_FILTER} kinds={KINDS} people={["You", "Call"]} onChange={noop} />);
    expect(off).toContain('aria-label="Kind of meeting"');
    expect(off).toContain(">Video call</option>");
    expect(off).toContain(">Has notes</option>");
    expect(off).toContain('aria-label="Person"');
    expect(off).not.toContain("Clear filters");
    const on = renderToStaticMarkup(
      <MeetingFilters filter={{ ...NO_FILTER, person: "You" }} kinds={KINDS} people={["You"]} onChange={noop} />,
    );
    expect(on).toContain("Clear filters");
    const nobody = renderToStaticMarkup(<MeetingFilters filter={NO_FILTER} kinds={KINDS} people={[]} onChange={noop} />);
    expect(nobody).not.toContain('aria-label="Person"');
  });

  it("draws people chips, You first in the accent, and nothing for no one", () => {
    const html = renderToStaticMarkup(<PeopleChips people={["You", "Call"]} />);
    expect(html).toContain('class="person-chip is-you">You<');
    expect(html).toContain('class="person-chip">Call<');
    expect(renderToStaticMarkup(<PeopleChips people={[]} />)).toBe("");
  });

  it("loads as six quiet rows, not a spinner", () => {
    const html = renderToStaticMarkup(<ListSkeleton />);
    expect(html.match(/skeleton-row/g)).toHaveLength(6);
    expect(html).toContain('data-loading-state="meetings:list"');
  });
});

describe("detail pane", () => {
  it("asks the person to pick a meeting when none is open", () => {
    const html = renderToStaticMarkup(<PickAMeeting count={3} />);
    expect(html).toContain("Pick a meeting");
    expect(html).toContain("Your 3 meetings are on the left");
    expect(renderToStaticMarkup(<PickAMeeting count={1} />)).toContain("Your meeting is on the left");
  });

  it("has three tabs, Notes | Transcript | People, with one selected and in tab order", () => {
    const html = renderToStaticMarkup(<DetailTabs tab="people" counts={{ transcript: 12, people: 2 }} onTab={noop} />);
    expect(html.match(/role="tab"/g)).toHaveLength(3);
    expect(html.indexOf(">Notes")).toBeLessThan(html.indexOf(">Transcript"));
    expect(html.indexOf(">Transcript")).toBeLessThan(html.indexOf(">People"));
    expect(html).toContain('id="meeting-tab-people" aria-selected="true"');
    expect(html).toContain('aria-controls="meeting-panel-notes"');
    expect(html).toContain('title="⌥⌘3"');
    expect(html).toContain('<span class="meeting-tab-count">12</span>');
  });

  it("shows the Who's talking? strip only while a voice has no name", () => {
    expect(renderToStaticMarkup(<WhoIsTalkingStrip unnamed={[]} onOpenPeople={noop} />)).toBe("");
    const html = renderToStaticMarkup(<WhoIsTalkingStrip unnamed={[SPEAKER]} onOpenPeople={noop} />);
    expect(html).toContain("Who&#x27;s talking?");
    expect(html).toContain("4 lines are labelled “Speaker”");
    expect(html).toContain("See people");
  });

  it("lists people with their lines, talk time and the reason for each label", () => {
    const html = renderToStaticMarkup(<PeopleTab people={[YOU, SPEAKER]} />);
    expect(html).toContain("7 lines · 5 min · first at 00:00:00");
    expect(html).toContain(`Why this label: ${PERSON_REASON.You}`);
    expect(renderToStaticMarkup(<PeopleTab people={[]} />)).toContain('data-empty-state="meetings:people"');
  });
});

describe("recording hero + undo", () => {
  it("is its own card with the clock, the sun, the live transcript and Stop", () => {
    const html = renderToStaticMarkup(
      <RecordingHero
        title="Weekly sync"
        clock="00:30:00"
        elapsedSeconds={1800}
        note="Mic only"
        callAudio={<span>call-audio-line</span>}
        live={<span>live-lines</span>}
        busy={false}
        onStop={noop}
      />,
    );
    expect(html).toContain("data-recording-hero");
    expect(html).toContain("<strong>Weekly sync</strong>");
    expect(html).toContain('style="left:50%"');
    expect(html).toContain("00:30:00");
    expect(html).toContain("live-lines");
    expect(html).toContain("call-audio-line");
    expect(html).toContain(">Stop meeting</button>");
    const confirm = renderToStaticMarkup(
      <RecordingHero title="" clock="00:00:03" elapsedSeconds={3} note={null} callAudio={null} live={null} busy stopLabel="Stop now? Press again" onStop={noop} />,
    );
    expect(confirm).toContain("Recording a meeting");
    expect(confirm).toContain(">Stop now? Press again</button>");
  });

  it("offers Undo with the seconds left, and renders nothing with nothing pending", () => {
    expect(renderToStaticMarkup(<UndoDeleteBar pending={[]} now={0} onUndo={noop} />)).toBe("");
    const html = renderToStaticMarkup(<UndoDeleteBar pending={[{ id: "m1", title: "Retro", at: 0 }]} now={3500} onUndo={noop} />);
    expect(html).toContain("“Retro” deleted.");
    expect(html).toContain("7 s to undo");
    expect(html).toContain(">Undo</button>");
  });
});

describe("the Meetings view source", () => {
  it("is two panes with the list on the left and the detail on the right", () => {
    expect(meetingsView).toContain('className="meetings-list-pane"');
    expect(meetingsView).toContain('className="meetings-detail-pane"');
    expect(meetingsView.indexOf("meetings-list-pane")).toBeLessThan(meetingsView.indexOf("meetings-detail-pane"));
    expect(meetingsCss).toMatch(/\.meetings-panes\s*\{[^}]*grid-template-columns/);
  });

  it("groups the list by date and carries people chips and the state chip per row", () => {
    expect(meetingsView).toContain("groupMeetings(");
    expect(meetingsView).toContain("<PeopleChips people={listPeople(m)} fit />");
    expect(meetingsView).toContain("className={`meeting-state ${m.state}`}");
  });

  it("puts title, date, duration, tracks chip and people chips in the detail header", () => {
    const head = meetingsView.slice(meetingsView.indexOf('className="meeting-detail-head"'), meetingsView.indexOf("<DetailTabs"));
    expect(head).toContain("meeting-title-edit");
    expect(head).toContain("formatTime(openMeeting.meeting.startedAt)");
    expect(head).toContain("formatMeetingDuration(openMeeting.meeting.durationSeconds)");
    expect(head).toContain("data-meeting-tracks");
    expect(head).toContain("<PeopleChips");
  });

  it("puts the Who's talking? strip above the header and the hero above both panes", () => {
    expect(meetingsView.indexOf("<WhoIsTalkingStrip")).toBeLessThan(meetingsView.indexOf('className="meeting-detail-head"'));
    const body = meetingsView.slice(meetingsView.indexOf('data-meetings-pane={paneMode}'));
    expect(body.indexOf("{hero}")).toBeLessThan(body.indexOf('className="meetings-panes"'));
  });

  it("yap-1107: a row's hover/focus changes colour only — no revealed bar, no hidden date, no reflow", () => {
    // No rule anywhere in the Meetings CSS reveals, hides or repositions on row hover/focus.
    expect(meetingsCss).not.toMatch(/\.meeting-row:(hover|focus-within)\s+\.actions/);
    expect(meetingsCss).not.toMatch(/\.meeting-row:(hover|focus-within)\s+\.meeting-row-when/);
    expect(meetingsCss).not.toMatch(/\.meeting-row\s+\.actions/);
    // The one hover rule in the list pane sets a background and nothing else.
    const hover = meetingsCss.match(/\.meetings-list-pane \.meeting-row:hover\s*\{([^}]*)\}/);
    expect(hover).not.toBeNull();
    const props = (hover?.[1] ?? "").split(";").map((d) => d.split(":")[0].trim()).filter(Boolean);
    expect(props).toEqual(["background"]);
    // The row does not wrap (the old hover bar wrapped onto a second line).
    expect(meetingsCss).toMatch(/\.meetings-list-pane \.meeting-row\s*\{[^}]*flex-wrap:\s*nowrap/);
    // The status column is laid out in every state: no opacity/visibility/display/position toggles.
    const status = meetingsCss.match(/\.meeting-row-status\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(status).toContain("flex: none");
    expect(status).not.toMatch(/opacity|visibility|display:\s*none|position/);
  });

  it("yap-1107: the old one-pane title-button styles are gone and the whole row is the open button", () => {
    const title = meetingsCss.match(/\.meeting-row-title\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(title).not.toContain("cursor");
    expect(meetingsCss).not.toMatch(/\.meeting-row-title:hover/);
    expect(meetingsCss).toMatch(/\.meetings-list-pane \.meeting-row-main\s*\{[^}]*align-self:\s*stretch/);
    expect(meetingsCss).toMatch(/\.meetings-list-pane \.meeting-row\s*\{[^}]*padding:\s*0/);
    // Export's list is fixed-position (portalled), never absolute inside a clipping row.
    expect(meetingsCss).toMatch(/\.floating-menu\s*\{[^}]*position:\s*fixed/);
    expect(meetingsCss).not.toContain(".export-menu-list");
  });

  it("deletes through the 10 s Undo, never at once", () => {
    expect(meetingsView).toContain("schedulePendingDelete(");
    expect(meetingsView).toContain("<UndoDeleteBar");
    expect(meetingsView).not.toContain("Delete for good?");
  });
});
