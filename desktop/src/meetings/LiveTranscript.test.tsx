/**
 * yap-notetaker-notes — the Meetings detail's live transcript and track turns.
 * Rendered with renderToStaticMarkup, like TranscriptList.test.tsx.
 */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { LiveLines } from "./LiveTranscript";
import TranscriptList from "./TranscriptList";
import { hasLiveLines, liveRuns, NO_LIVE, reduceLive, SOMEONE_NEW } from "./liveSnapshot";

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

// yap-1106-live-whos-talking — who is talking, live.
describe("live speaker names", () => {
  const voice = (name: string | null, hint: string | null = null) => ({ name, hint });
  const NAMED = {
    recording: true,
    tracks: [
      { track: 0, speaker: "You", lines: ["so the plan is friday"], voices: [voice(null)] },
      {
        track: 1,
        speaker: "Call",
        lines: ["works for us", "", "who else is joining", "not sure yet"],
        voices: [
          voice("Alex", "Probably Alex — voice match 0.82"),
          voice(null),
          voice("Someone new"),
          voice(null),
        ],
      },
    ],
  };

  it("LiveLines renders names: a matched voice, someone new, and the Call fallback", () => {
    const html = renderToStaticMarkup(<LiveLines snapshot={reduceLive(NAMED, true)} />);
    expect(html).toContain(">You</span>");
    expect(html).toContain(">Alex</span>");
    expect(html).toContain('title="Probably Alex — voice match 0.82"');
    expect(html).toContain(`>${SOMEONE_NEW}</span>`);
    expect(html).toContain(">Call</span>");
    // Names sit next to the lines they were heard on.
    expect(html.indexOf(">Alex</span>")).toBeLessThan(html.indexOf("<li>works for us</li>"));
    expect(html.indexOf(`>${SOMEONE_NEW}</span>`)).toBeLessThan(html.indexOf("<li>who else is joining</li>"));
    expect(html.indexOf(">Call</span>")).toBeLessThan(html.indexOf("<li>not sure yet</li>"));
    expect(html).not.toContain("<li></li>");
  });

  it("keeps voices aligned with lines when empty lines are dropped", () => {
    const t = reduceLive(NAMED, true).tracks[1];
    expect(t.lines).toEqual(["works for us", "who else is joining", "not sure yet"]);
    expect(liveRuns(t).map((r) => r.label)).toEqual(["Alex", SOMEONE_NEW, "Call"]);
    expect(liveRuns(t)[0].hint).toBe("Probably Alex — voice match 0.82");
  });

  it("groups consecutive lines of one voice and falls back without a voices list", () => {
    const same = {
      track: 1,
      speaker: "Call",
      lines: ["a", "b"],
      voices: [voice("Alex", "Probably Alex — voice match 0.9"), voice("Alex", "Probably Alex — voice match 0.9")],
    };
    expect(liveRuns(same)).toHaveLength(1);
    const old = reduceLive({ recording: true, tracks: [{ track: 1, speaker: "Call", lines: ["hi"] }] }, true);
    expect(liveRuns(old.tracks[0])).toEqual([{ label: "Call", hint: null, lines: ["hi"] }]);
    const solo = reduceLive({ recording: true, tracks: [{ track: 0, speaker: null, lines: ["room"] }] }, true);
    expect(liveRuns(solo.tracks[0])[0].label).toBeNull();
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
