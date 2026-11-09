// yap-1107-parity-companions — the visiting friend stands INSIDE the pill window
// (companions.html s09, line 289-297), whole at 1:1, never cropped, never twice.
import { afterEach, describe, expect, it, vi } from "vitest";
import { Raster } from "../sprites/raster";
import { friendManifest } from "../sprites/friends";
import { milestoneLine } from "./friendVisits";
import { forcedPill, matrixFriendVisitWindowScene } from "./forced";
import {
  VISITOR_DX,
  cameraView,
  currentPillVisitor,
  paintPillVisitor,
  placementLogLine,
  publishPillVisitor,
  rectInside,
  resetPillVisitorForTests,
  setVisitorInWindow,
  subscribePillVisitor,
  subscribeVisitorInWindow,
  visitorInWindow,
  visitorSegment,
  visitorStamp,
  type ArtRect,
} from "./visitorInWindow";
import FRIEND_VISIT_SRC from "./FriendVisit.tsx?raw";
import YAPPY_PILL_SRC from "./YappyPill.tsx?raw";
import FLOAT_CSS from "../float.css?raw";

/** The painted (alpha > 0) box of a raster, or null when nothing was painted. */
function paintedBox(r: Raster): ArtRect | null {
  let top = r.height, bottom = -1, left = r.width, right = -1;
  for (let y = 0; y < r.height; y++) {
    for (let x = 0; x < r.width; x++) {
      if (r.data[(y * r.width + x) * 4 + 3] > 0) {
        top = Math.min(top, y); bottom = Math.max(bottom, y);
        left = Math.min(left, x); right = Math.max(right, x);
      }
    }
  }
  return bottom < 0 ? null : { x: left, y: top, w: right - left + 1, h: bottom - top + 1 };
}

const CX = 70, FEET = 50;

afterEach(() => resetPillVisitorForTests());

describe("visitorInWindow: where the visitor stands (companions.html line 289)", () => {
  it("parity_visitor_stands_26_px_right_of_the_occupant_on_its_feet_line", () => {
    expect(VISITOR_DX).toBe(26);
    const s = visitorStamp("yappy", CX, FEET)!;
    expect(s.vx).toBe(CX + 26);
    expect([s.ox, s.oy]).toEqual([CX + 26, FEET]);
    const r = new Raster(160, 80);
    expect(paintPillVisitor(r, "yappy", CX, FEET)).toBe(true);
    const b = paintedBox(r)!;
    // every painted pixel of Yappy is inside the stamp the no-crop check uses
    expect(rectInside(b, s)).toBe(true);
    // centred on cx + 26 (his art is a px wider on the tail side)
    expect(Math.abs(b.x + b.w / 2 - (CX + 26))).toBeLessThanOrEqual(1.5);
    // standing on the occupant's feet line: toes/shadow end within 4 rows below it
    expect(b.y + b.h - 1).toBeGreaterThanOrEqual(FEET);
    expect(b.y + b.h - 1).toBeLessThanOrEqual(FEET + 4);
  });

  it("parity_visiting_friends_are_stamped_whole_at_1x_inside_their_stamp", () => {
    for (const id of ["tidy", "gulp", "shelly", "hoot", "sal", "mochi", "cinder", "ember"] as const) {
      const m = friendManifest(id);
      if (!m) continue;
      const s = visitorStamp(id, CX, FEET)!;
      // 1:1, never scaled: the stamp is the sprite's own cell
      expect([s.w, s.h], id).toEqual([m.w, m.h]);
      expect(s.x, id).toBe(CX + 26 - Math.floor(m.w / 2));
      expect(s.y + s.h - 2, id).toBe(FEET);
      const r = new Raster(220, 120);
      expect(paintPillVisitor(r, id, CX + 40, FEET + 30), id).toBe(true);
      const b = paintedBox(r);
      expect(b, id).not.toBeNull();
      expect(rectInside(b!, visitorStamp(id, CX + 40, FEET + 30)!), id).toBe(true);
    }
    expect(visitorStamp("nobody" as never, CX, FEET)).toBeNull();
  });

  it("parity_page_example_hoot_plus_yappy_fits_one_open_196x44_window_whole", () => {
    // Hoot sits in the pill: the page stamps him at (cx - 20, feet - 38); his
    // painted rows start at sprite row 7 (headband) and end at row 38 (shadow).
    const hoot = new Raster(40, 40);
    expect(paintPillVisitor(hoot, "hoot", 20 - 26, 38)).toBe(true);
    const hb = paintedBox(hoot)!;
    const hootBox = { x: CX - 20 + hb.x, y: FEET - 38 + hb.y, w: hb.w, h: hb.h };
    const v = visitorStamp("yappy", CX, FEET)!;
    const left = Math.min(hootBox.x, v.x), right = Math.max(hootBox.x + hootBox.w, v.x + v.w);
    const top = Math.min(hootBox.y, v.y), bottom = Math.max(hootBox.y + hootBox.h, v.y + v.h);
    // pill.html's open window with a segment is 196 x 44 (the page draws 200 x 44)
    expect(right - left).toBeLessThanOrEqual(196);
    expect(bottom - top).toBeLessThanOrEqual(44);
    // the visitor stands BESIDE the occupant: his centre is right of Hoot's painted box
    expect(v.vx).toBeGreaterThan(hootBox.x + hootBox.w);
  });

  it("parity_visitor_outside_the_camera_view_is_not_painted_in_the_window", () => {
    const s = visitorStamp("yappy", CX, FEET)!;
    // an open window's view (196 x 44 art px centred on the occupant)
    expect(rectInside(s, { x: CX - 98, y: FEET - 34, w: 196, h: 44 })).toBe(true);
    // the 64 x 20 resting window (head framing) cannot hold him whole
    expect(rectInside(s, { x: CX - 32, y: FEET - 30, w: 64, h: 20 })).toBe(false);
    // the camera's view is the CLIP mapped back to art px, not the wider src:
    // cell 2, the 392 x 88 device-px window drawn from src x 10 at dst x -20
    const blit = { cell: 2, clip: { x: 0, y: 0, w: 392, h: 88 }, src: { x: 10, y: 4, w: 220, h: 50 }, dst: { x: -20, y: -8, w: 440, h: 100 } };
    expect(cameraView(blit)).toEqual({ x: 20, y: 8, w: 196, h: 44 });
    // the rounded ends come off the left and right (radius in device px)
    expect(cameraView(blit, 28)).toEqual({ x: 34, y: 8, w: 168, h: 44 });
    // touching the edge is inside; one px over is not
    expect(rectInside({ x: 0, y: 0, w: 10, h: 10 }, { x: 0, y: 0, w: 10, h: 10 })).toBe(true);
    expect(rectInside({ x: 0, y: 0, w: 11, h: 10 }, { x: 0, y: 0, w: 10, h: 10 })).toBe(false);
  });
});

describe("visitorInWindow: the milestone the page shows with the visit (line 297)", () => {
  it("parity_milestone_segment_reads_100th_take_and_the_bubble_the_hundred_line", () => {
    expect(visitorSegment("milestone", 100)).toBe("100th take");
    expect(visitorSegment("milestone", 1000)).toBe("1000th take");
    expect(visitorSegment("backtrack-caught", 100)).toBeNull();
    expect(visitorSegment(null, 100)).toBeNull();
    expect(milestoneLine("milestone", 100)).toBe("that's a hundred! — Yappy");
  });
});

describe("visitorInWindow: the hand-off between FriendVisit and the pill window", () => {
  it("publishes changes only, and a gone visitor leaves the window", () => {
    const seen: unknown[] = [];
    const unsub = subscribePillVisitor((v) => seen.push(v));
    const v = { friend: "yappy" as const, event: "milestone", line: "that's a hundred! — Yappy", segment: "100th take" };
    publishPillVisitor(v);
    publishPillVisitor({ ...v });
    expect(seen).toHaveLength(1);
    expect(currentPillVisitor()).toEqual(v);
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const flags: boolean[] = [];
    const unsubW = subscribeVisitorInWindow((on) => flags.push(on));
    setVisitorInWindow(true);
    setVisitorInWindow(true);
    expect(visitorInWindow()).toBe(true);
    expect(info).toHaveBeenCalledTimes(1);
    expect(info).toHaveBeenCalledWith(placementLogLine("yappy", true));
    publishPillVisitor(null);
    expect(visitorInWindow()).toBe(false);
    expect(flags).toEqual([true, false]);
    expect(seen).toEqual([v, null]);
    unsub(); unsubW();
    publishPillVisitor(v);
    expect(seen).toHaveLength(2);
    info.mockRestore();
  });

  it("placement log line names the place and the friend, nothing else", () => {
    expect(placementLogLine("tidy", false)).toBe("pill friend_visit placed=corner friend=tidy");
    expect(placementLogLine(null, true)).toBe("pill friend_visit placed=window friend=none");
  });

  it("the pill paints the visitor in the window and the corner box steps aside", () => {
    expect(YAPPY_PILL_SRC).toContain("paintPillVisitor(octx, visitorOn.friend, cx, feet");
    // the stadium's rounded ends (radius (capH - 2) / 2 CSS px) come off the view
    expect(YAPPY_PILL_SRC).toContain("rectInside(vStamp, cameraView(cam.blit, ((capH - 2) / 2) * DPR))");
    expect(YAPPY_PILL_SRC).toContain("drawScene({ ...scenePose, lift: cam.lift }, vOn)");
    expect(FRIEND_VISIT_SRC).toContain("publishPillVisitor(who ?");
    expect(FRIEND_VISIT_SRC).toContain('data-in-window={inWindow ? "true" : undefined}');
    expect(FLOAT_CSS).toMatch(/\.friend-visit\[data-in-window="true"\]\s*\{\s*display:\s*none;\s*\}/);
  });
});

describe("visitorInWindow: the matrix row the parity sheet's app column comes from", () => {
  it("matrixFriendVisitWindowScene is the page's visit: done, Hoot in the pill, Yappy visiting, bottom dock", () => {
    const row = matrixFriendVisitWindowScene();
    expect(row.id).toBe("friend-visit-window");
    expect(row.phase).toBe("done");
    expect(row.query).toBe("friend=yappy");
    expect(row.characters).toEqual(["hoot"]);
    expect(row.docks).toEqual(["bottom"]);
    // none = the visitor in the window; recording = the quiet rule (no visitor)
    expect(row.meetings).toEqual(["none", "recording"]);
    const f = forcedPill("?phase=done&pill=hoot&dock=bottom&meeting=none&friend=yappy");
    expect(f?.phase).toBe("done");
  });
});
