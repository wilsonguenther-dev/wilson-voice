/**
 * fix-l3b4-1-pill-shows-live-speaker — the pill's meeting surface draws who is
 * talking from a live snapshot: the word in the clock row with the hint as its
 * title, and nothing extra when there is no live snapshot.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import MeetingBadge from "./MeetingBadge";
import { IDLE_MEETING, type MeetingStatus } from "./meeting";
import { SOMEONE_NEW, type LiveSnapshot } from "../meetings/liveSnapshot";

const RECORDING: MeetingStatus = {
  ...IDLE_MEETING,
  recording: true,
  id: "m1",
  title: "Standup",
  elapsedSeconds: 724,
  elapsedLabel: "00:12:04",
  captureAvailable: true,
};

const HINT = "Probably Alex — voice match 0.82";

const ALEX_LIVE: LiveSnapshot = {
  recording: true,
  tracks: [
    { track: 0, speaker: "You", lines: [], voices: [] },
    {
      track: 1,
      speaker: "Call",
      lines: ["can everyone hear me", "the budget looks fine"],
      voices: [
        { name: null, hint: null },
        { name: "Alex", hint: HINT },
      ],
    },
  ],
};

describe("fix-l3b4-1 — MeetingBadge shows the live speaker", () => {
  it("shows 'Alex' with the hint as its title for a fixture live snapshot", () => {
    const html = renderToStaticMarkup(<MeetingBadge status={RECORDING} live={ALEX_LIVE} />);
    expect(html).toContain('data-live-speaker="name"');
    expect(html).toContain(`title="${HINT}"`);
    expect(html).toContain(`aria-label="Talking now: ${HINT}"`);
    expect(html).toMatch(/class="rec-speaker is-name"[^>]*>Alex<\/span>/);
    // Still the one meeting surface, with its one Stop.
    expect(html.match(/data-pill-slot="clock"/g)).toHaveLength(1);
    expect(html.match(/class="rec-stop"/g)).toHaveLength(1);
  });

  it("shows no speaker element without a live snapshot", () => {
    const plain = renderToStaticMarkup(<MeetingBadge status={RECORDING} />);
    expect(plain).not.toContain("data-live-speaker");
    expect(plain).not.toContain("rec-speaker");
    expect(plain).not.toContain("rec-side");
    // A stopped snapshot, or an empty one, draws exactly the plain badge.
    expect(renderToStaticMarkup(<MeetingBadge status={RECORDING} live={{ recording: false, tracks: [] }} />)).toBe(plain);
    expect(renderToStaticMarkup(<MeetingBadge status={RECORDING} live={null} />)).toBe(plain);
  });

  it("a mic-only meeting with no match shows nothing extra", () => {
    const micOnly: LiveSnapshot = { recording: true, tracks: [{ track: 0, speaker: null, lines: ["just me"], voices: [{ name: null, hint: null }] }] };
    const plain = renderToStaticMarkup(<MeetingBadge status={RECORDING} />);
    expect(renderToStaticMarkup(<MeetingBadge status={RECORDING} live={micOnly} />)).toBe(plain);
  });

  it("Someone new and the track word render as words too", () => {
    const someone: LiveSnapshot = { recording: true, tracks: [{ track: 1, speaker: "Call", lines: ["hi"], voices: [{ name: SOMEONE_NEW, hint: null }] }] };
    expect(renderToStaticMarkup(<MeetingBadge status={RECORDING} live={someone} />)).toMatch(/data-live-speaker="someone_new"[^>]*>Someone new<\/span>/);
    const call: LiveSnapshot = { recording: true, tracks: [{ track: 1, speaker: "Call", lines: ["hi"] }] };
    expect(renderToStaticMarkup(<MeetingBadge status={RECORDING} live={call} />)).toMatch(/data-live-speaker="track"[^>]*>Call<\/span>/);
  });

  it("mid-take (the 96 px stack) the speaker rides the stack tooltip and its screen-reader line", () => {
    const html = renderToStaticMarkup(<MeetingBadge status={RECORDING} live={ALEX_LIVE} stacked />);
    expect(html).toContain('data-live-speaker="name"');
    expect(html).toContain(`Talking now: ${HINT}`);
    expect(html).not.toContain("rec-speaker");
    const plain = renderToStaticMarkup(<MeetingBadge status={RECORDING} stacked />);
    expect(plain).not.toContain("data-live-speaker");
    expect(plain).not.toContain("Talking now");
  });

  it("a meeting that is not recording draws nothing, snapshot or not", () => {
    expect(renderToStaticMarkup(<MeetingBadge status={IDLE_MEETING} live={ALEX_LIVE} />)).toBe("");
  });
});
