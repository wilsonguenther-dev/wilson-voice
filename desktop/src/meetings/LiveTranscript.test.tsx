/**
 * yap-notetaker-notes — the Meetings detail's live transcript and track turns.
 * Rendered with renderToStaticMarkup, like TranscriptList.test.tsx.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { LiveLines } from "./LiveTranscript";
import TranscriptList from "./TranscriptList";
import { hasLiveLines, NO_LIVE, reduceLive } from "./liveSnapshot";

const CALL = {
  recording: true,
  tracks: [
    { track: 0, speaker: "You", lines: ["  so the plan is friday ", ""] },
    { track: 1, speaker: "Call", lines: ["works for us"] },
  ],
};

describe("live transcript", () => {
  it("draws the last lines under You and Call on a call", () => {
    const html = renderToStaticMarkup(<LiveLines snapshot={reduceLive(CALL, true)} />);
    expect(html).toContain(">You</span>");
    expect(html).toContain(">Call</span>");
    expect(html).toContain("<li>so the plan is friday</li>");
    expect(html).toContain("<li>works for us</li>");
    expect(html).not.toContain("<li></li>");
  });

  it("shows a mic-only meeting's lines with no speaker label", () => {
    const solo = { recording: true, tracks: [{ track: 0, speaker: null, lines: ["room talk"] }] };
    const html = renderToStaticMarkup(<LiveLines snapshot={reduceLive(solo, true)} />);
    expect(html).toContain("<li>room talk</li>");
    expect(html).not.toContain("seg-who");
  });

  it("is replaced at Stop: a stopped snapshot or a stopped meeting clears it", () => {
    expect(reduceLive({ recording: false, tracks: [] }, true)).toEqual(NO_LIVE);
    expect(reduceLive(CALL, false)).toEqual(NO_LIVE);
    expect(hasLiveLines(NO_LIVE)).toBe(false);
    const html = renderToStaticMarkup(<LiveLines snapshot={NO_LIVE} />);
    expect(html).toContain('data-live-transcript="waiting"');
  });
});

describe("track turns in the stored transcript", () => {
  const seg = (id: string, track: number, startSeconds: number, text: string) => ({ id, track, startSeconds, text });

  it("labels a call's tracks You and Call", () => {
    const html = renderToStaticMarkup(
      <TranscriptList kind="virtual" segments={[seg("a", 0, 0, "hi there"), seg("b", 1, 2, "hello back")]} />,
    );
    expect(html).toContain(">You</span>");
    expect(html).toContain(">Call</span>");
  });

  it("shows an in-person meeting's single mic track with no speaker labels", () => {
    const html = renderToStaticMarkup(
      <TranscriptList kind="in_person" segments={[seg("a", 0, 0, "room talk"), seg("b", 0, 4, "more")]} />,
    );
    expect(html).toContain("room talk");
    expect(html).not.toContain("seg-who");
  });
});
