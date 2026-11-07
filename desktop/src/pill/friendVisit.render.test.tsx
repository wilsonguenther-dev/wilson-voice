// yap-1106-friend-visits — the shipping view, rendered without a DOM
// (renderToStaticMarkup, like backtrackCaught.test.tsx). Effects do not run
// here, so this proves what the markup is for a visitor on screen and that the
// quiet rule hides even a pinned one; WHEN a visit happens is friendVisits.test.ts.
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FriendVisit } from "./FriendVisit";
import { matrixFriendVisitScene } from "./forced";
import { forcedFriendVisit } from "./friendVisits";

describe("friend visit — the view", () => {
  it("draws nothing when nobody is visiting", () => {
    const html = renderToStaticMarkup(
      <FriendVisit meetingRecording={false} meetingNotesReady={false} occupant="yappy" />,
    );
    expect(html).toBe("");
  });

  it("draws the visitor decoratively, with no hitbox", () => {
    const html = renderToStaticMarkup(
      <FriendVisit meetingRecording={false} meetingNotesReady={false} occupant="yappy" forced="tidy" />,
    );
    expect(html).toContain('data-friend-visit="tidy"');
    expect(html).toContain('aria-hidden="true"');
    expect(html).toContain("<canvas");
  });

  it("hides even a pinned visitor while a meeting records", () => {
    const html = renderToStaticMarkup(
      <FriendVisit meetingRecording={true} meetingNotesReady={false} occupant="yappy" forced="tidy" />,
    );
    expect(html).toBe("");
  });

  it("the matrix row pins Tidy on an idle pill", () => {
    const row = matrixFriendVisitScene();
    expect(row.phase).toBe("idle");
    expect(row.id).toBe("friend-visit");
    expect(forcedFriendVisit("?" + row.query)).toBe("tidy");
  });
});
