/**
 * yap-1106-pill-vertical-dock-world (+ FB-20) — the side dock is a tall room,
 * never wider than 64 px, and it never crops Yappy or the room.
 *
 * Walks every pill state x left/right x meeting x display scale (true size and
 * 2x, plus 1.5x) through the same functions YappyPill draws with, and asserts
 * the shell width, the flush edge, the 56 x 180 scene / 30 x 40 tab sizes,
 * Yappy's whole bounding box inside the visible (rounded) window, the room's
 * parts inside the scene on both edges, and the bubble opening inward.
 */
import { describe, expect, it } from "vitest";
import { type ChickPose } from "../sprites/art";
import { Raster } from "../sprites/raster";
import { CHICK_EXTENT, type FitBox } from "./characterFit";
import { ALL_LIVE_PHASES } from "./live";
import { skyLook } from "./sky";
import { RESTING_PHASES, boxesOverlap, layoutProblems, pillLayout, type MeetingMode } from "./state";
import {
  LADDER_CX, TAB_ART, VDOCK_MAX_W, VDOCK_OPEN, VDOCK_POSE, VDOCK_RADIUS, VDOCK_ROOM, VDOCK_SCENE, VDOCK_TAB, VDOCK_TAB_WINDOW,
  drawTabChick, drawTallRoom, mirrorBox, roomParts, roundRectContains, sideDock, tabCell, vChickAt, vChickBox, vInnerRect,
  vShellRect, vTierFor, vdockCropWarning, vdockFrame, type SideDock, type VProp,
  VDOCK_STOP, VDOCK_STOP_ROW, vStopRect,
} from "./vdock";

const SIDES: readonly SideDock[] = ["left", "right"];
const DPRS = [1, 1.5, 2] as const;
const MEETINGS: readonly MeetingMode[] = ["none", "recording", "after"];
const PROPS: readonly VProp[] = ["none", "pad", "desk"];
const inside = (b: FitBox, o: FitBox) => b.x >= o.x - 1e-6 && b.y >= o.y - 1e-6 && b.x + b.w <= o.x + o.w + 1e-6 && b.y + b.h <= o.y + o.h + 1e-6;
const SCENE: FitBox = { x: 0, y: 0, w: VDOCK_SCENE.w, h: VDOCK_SCENE.h };
const POSE: ChickPose = { hop: 0, sx: 0, sy: 0, blink: 0, mood: 0, beakF: 0, sway: 0, flap: 0, look: "fwd", glasses: false, pencil: false, droop: false };

describe("yap-1106-pill-vertical-dock-world: the sizes", () => {
  it("open_is_a_56x180_scene_in_a_64px_shell_and_rest_is_a_30x40_tab", () => {
    expect(VDOCK_SCENE).toEqual({ w: 56, h: 180 });
    expect(VDOCK_OPEN.w).toBe(64);
    expect(VDOCK_OPEN.w).toBeLessThanOrEqual(VDOCK_MAX_W);
    expect(VDOCK_TAB).toEqual({ w: 30, h: 40 });
    expect(VDOCK_TAB_WINDOW).toEqual({ w: 22, h: 32 });
    const open = vShellRect("left", 1, { x: 0, y: 0, w: VDOCK_OPEN.w, h: VDOCK_OPEN.h });
    const scene = vInnerRect(open);
    expect([scene.w, scene.h]).toEqual([56, 180]);
    const tab = vShellRect("right", 0, { x: 0, y: 0, w: VDOCK_TAB.w, h: VDOCK_TAB.h });
    expect([tab.w, tab.h]).toEqual([30, 40]);
  });

  // yap-1107-parity-pill — pill.html #vstage .vshell.l/.r measure 64 x 216:
  // pad 4, the 56 x 180 room, gap 4, the 24 px stop, pad 4.
  it("parity_open_side_shell_is_64x216_with_the_stop_under_the_room", () => {
    expect(VDOCK_OPEN).toEqual({ w: 64, h: 216 });
    expect(VDOCK_STOP).toBe(24);
    expect(VDOCK_STOP_ROW).toBe(28);
    for (const side of SIDES) {
      const open = vShellRect(side, 1, { x: 0, y: 0, w: VDOCK_OPEN.w, h: VDOCK_OPEN.h });
      const room = vInnerRect(open);
      const stop = vStopRect(open);
      expect([room.w, room.h]).toEqual([56, 180]);
      expect(stop.y).toBe(room.y + room.h + 4); // the grid gap
      expect(stop.y + stop.h).toBe(open.y + open.h - 4); // the bottom padding
      expect(stop.x + stop.w / 2).toBe(open.x + open.w / 2); // centred (justify-items: center)
      // the room sprite at 2x is whole and inside its window
      for (const dpr of DPRS) expect(vdockFrame(side, 1, "none", 1, dpr, open).fits, `${side}@${dpr}`).toBe(true);
    }
    // the tab has no stop row: its window stays 22 x 32
    const tab = vInnerRect(vShellRect("left", 0, { x: 0, y: 0, w: VDOCK_TAB.w, h: VDOCK_TAB.h }));
    expect([tab.w, tab.h]).toEqual([22, 32]);
  });

  it("side_dock_reads_only_left_and_right", () => {
    expect(sideDock("left")).toBe("left");
    expect(sideDock("right")).toBe("right");
    for (const d of ["bottom", "top", "", undefined, null, "LEFT"]) expect(sideDock(d as string | null | undefined)).toBeNull();
  });
});

describe("yap-1106-pill-vertical-dock-world: Yappy's place in the room", () => {
  it("quick_takes_stay_on_the_floor_notes_climb_to_the_notepad_desk_tiers_work_at_the_typewriter", () => {
    expect(vTierFor("none")).toBe("floor");
    expect(vTierFor("pad")).toBe("notepad");
    expect(vTierFor("desk")).toBe("typewriter");
    expect(vChickAt("none", 1)).toEqual(VDOCK_POSE.floor);
    expect(vChickAt("pad", 1)).toEqual(VDOCK_POSE.notepad);
    expect(vChickAt("desk", 1)).toEqual(VDOCK_POSE.typewriter);
    expect(vChickAt("pad", 0)).toEqual(VDOCK_POSE.floor);
    expect(vChickAt("desk", Number.NaN)).toEqual(VDOCK_POSE.floor);
    // feet on the shelf he works at
    expect(VDOCK_POSE.notepad.feetY).toBe(VDOCK_ROOM.notepadShelf.y);
    expect(VDOCK_POSE.typewriter.feetY).toBe(VDOCK_ROOM.typewriterShelf.y);
    expect(VDOCK_POSE.floor.feetY).toBe(VDOCK_ROOM.floor.y);
    // mid-climb he is on the ladder (the inner side)
    expect(vChickAt("pad", 0.5).cx).toBe(LADDER_CX);
    expect(LADDER_CX + CHICK_EXTENT.right).toBeLessThanOrEqual(VDOCK_SCENE.w);
  });

  it("every_pose_and_every_climb_frame_keeps_his_whole_box_in_the_scene", () => {
    for (const prop of PROPS) for (let i = 0; i <= 30; i++) {
      const at = vChickAt(prop, i / 30);
      expect(Number.isInteger(at.cx) && Number.isInteger(at.feetY)).toBe(true);
      for (const dock of SIDES) {
        const b = mirrorBox(vChickBox(at.cx, at.feetY), dock);
        expect(roundRectContains(SCENE, VDOCK_RADIUS.scene, b), `${prop} t=${i / 30} ${dock} ${JSON.stringify(b)}`).toBe(true);
      }
    }
  });

  it("the_room_window_shelves_floor_and_ladder_sit_inside_the_scene_and_mirror_on_the_right", () => {
    for (const dock of SIDES) {
      const parts = roomParts(dock);
      for (const [name, b] of Object.entries(parts)) expect(inside(b, SCENE), `${dock} ${name}`).toBe(true);
    }
    const l = roomParts("left"), r = roomParts("right");
    // the ladder is on the inner side: right half on the left dock, left half on the right dock
    expect(l.ladder.x).toBeGreaterThan(VDOCK_SCENE.w / 2);
    expect(r.ladder.x + r.ladder.w).toBeLessThan(VDOCK_SCENE.w / 2);
    expect(r.ladder.x).toBe(VDOCK_SCENE.w - l.ladder.x - l.ladder.w);
    // top to bottom: window, notepad + jar shelf, typewriter shelf, floor
    expect(l.window.y + l.window.h).toBeLessThanOrEqual(l.notepad.y);
    expect(l.notepadShelf.y).toBeLessThan(l.typewriter.y);
    expect(l.typewriterShelf.y).toBeLessThan(l.floor.y);
  });
});

describe("FB-20: every state x left/right, at true size and at 2x, is never clipped", () => {
  it("walks_every_state_x_side_x_meeting_x_tier_x_display", () => {
    const failures: string[] = [];
    let cases = 0;
    for (const phase of ALL_LIVE_PHASES) for (const dock of SIDES) for (const meeting of MEETINGS) {
      const lay = pillLayout({ phase, meeting, dock });
      const rest = RESTING_PHASES.has(phase);
      const where = `${phase}/${dock}/${meeting}`;
      if (layoutProblems(lay).length) failures.push(`${where} layout: ${layoutProblems(lay).join(";")}`);
      // the shell box: flush to the edge, never wider than 64 px
      if (lay.char.w > VDOCK_MAX_W) failures.push(`${where} shell ${lay.char.w} px wide`);
      if (dock === "left" ? lay.char.x !== 0 : lay.char.x + lay.char.w !== lay.panel.w) failures.push(`${where} not flush`);
      // the bubble opens inward
      const inward = dock === "left" ? lay.bubble.x >= lay.char.x + lay.char.w : lay.bubble.x + lay.bubble.w <= lay.char.x;
      if (!inward || boxesOverlap(lay.bubble, lay.char)) failures.push(`${where} bubble not inward`);
      for (const prop of rest ? (["none"] as const) : PROPS) for (const dpr of DPRS) {
        const f = vdockFrame(dock, rest ? 0 : 1, prop, 1, dpr, lay.char);
        cases++;
        const tag = `${where} ${prop} dpr=${dpr}`;
        if (f.mode !== (rest ? "tab" : "room")) failures.push(`${tag} mode ${f.mode}`);
        if (f.shell.w > VDOCK_MAX_W) failures.push(`${tag} shell ${f.shell.w}`);
        if (!inside(f.shell, { x: 0, y: 0, w: lay.panel.w, h: lay.panel.h })) failures.push(`${tag} shell outside the panel`);
        if (!inside(f.inner, f.shell)) failures.push(`${tag} scene outside the shell`);
        if (rest ? f.shell.w !== 30 || f.shell.h !== 40 : f.inner.w !== 56 || f.inner.h !== 180) failures.push(`${tag} wrong size ${JSON.stringify(f.shell)}`);
        // Yappy's whole box inside the rounded window, on whole device px
        if (!f.fits) failures.push(`${tag} chick ${JSON.stringify(f.chick)} outside ${JSON.stringify(f.innerD)}`);
        if (![f.art.x, f.art.y, f.art.w, f.art.h].every(Number.isInteger)) failures.push(`${tag} art off the device grid`);
        // the whole room is drawn inside the window (room mode)
        if (f.mode === "room" && !inside(f.art, f.innerD)) failures.push(`${tag} room ${JSON.stringify(f.art)} outside ${JSON.stringify(f.innerD)}`);
      }
    }
    expect(failures, failures.slice(0, 8).join("\n")).toEqual([]);
    expect(cases).toBeGreaterThan(17 * 2 * 3);
  });

  it("at_true_size_and_2x_the_room_is_one_and_two_device_px_per_art_px", () => {
    const lay = pillLayout({ phase: "listening", meeting: "none", dock: "left" });
    expect(vdockFrame("left", 1, "none", 1, 1, lay.char).cell).toBe(1);
    expect(vdockFrame("left", 1, "none", 1, 2, lay.char).cell).toBe(2);
    const rest = pillLayout({ phase: "idle", meeting: "none", dock: "right" });
    expect(vdockFrame("right", 0, "none", 1, 2, rest.char).cell).toBe(1);
    expect(vdockFrame("right", 0, "none", 1, 1, rest.char).cell).toBe(0.5);
  });

  it("tab_cell_is_whole_when_it_can_be_and_never_zero", () => {
    expect(tabCell({ x: 0, y: 0, w: TAB_ART.w * 3, h: TAB_ART.h * 3 })).toBe(3);
    expect(tabCell({ x: 0, y: 0, w: TAB_ART.w / 2, h: TAB_ART.h })).toBe(0.5);
    expect(tabCell({ x: 0, y: 0, w: 0, h: 0 })).toBe(1);
  });

  it("a_cropped_frame_is_reported_and_the_rounded_check_is_not_blind", () => {
    const r: FitBox = { x: 0, y: 0, w: 20, h: 20 };
    expect(roundRectContains(r, 5, { x: 0, y: 0, w: 20, h: 20 })).toBe(false); // corners cut
    expect(roundRectContains(r, 0, { x: 0, y: 0, w: 20, h: 20 })).toBe(true);
    expect(roundRectContains(r, 5, { x: 5, y: 0, w: 10, h: 20 })).toBe(true);
    expect(roundRectContains(r, 5, { x: -1, y: 5, w: 10, h: 10 })).toBe(false);
    // a shell too small for him does not fit, and says so in one line
    const f = vdockFrame("left", 1, "none", 1, 1, { x: 0, y: 0, w: 64, h: 216 });
    expect(f.fits).toBe(true);
    const tiny = { ...f, fits: false };
    expect(vdockCropWarning(tiny, 1)).toBe("pill vdock_cropped dock=left mode=room shellW=64 shellH=216 dpr=1");
  });
});

describe("yap-1106-pill-vertical-dock-world: the room paints", () => {
  it("paints_the_window_with_the_real_sky_the_shelves_and_the_ladder", () => {
    for (const time of ["day", "night"] as const) for (const prop of PROPS) {
      const o = new Raster(VDOCK_SCENE.w, VDOCK_SCENE.h);
      const look = skyLook(time);
      drawTallRoom(o, look, POSE, { prop, propIn: 1, ink: 0.5, elapsed: 1.3 });
      // a pane pixel away from the sun / moon / mullions carries the sky
      const w = VDOCK_ROOM.window;
      const pane = o.px(w.x + 5, w.y + w.h - 5);
      expect(pane.a).toBe(255);
      expect(time === "night" ? pane.b > pane.r : pane.b > 200).toBe(true);
      // the ladder's outer rail and the floor are wood
      const rail = o.px(VDOCK_ROOM.ladder.x + VDOCK_ROOM.ladder.w - 1, VDOCK_ROOM.ladder.y + VDOCK_ROOM.ladder.h - 3);
      expect([rail.r, rail.g, rail.b]).toEqual([0x6b, 0x44, 0x28]);
      const floor = o.px(1, VDOCK_ROOM.floor.y + 15);
      expect([floor.r, floor.g, floor.b]).toEqual([0xa5, 0x67, 0x3f]);
      expect(o.globalAlpha).toBe(1);
    }
    // no sky yet: the fallback day sky
    const o = new Raster(VDOCK_SCENE.w, VDOCK_SCENE.h);
    drawTallRoom(o, null, POSE, { prop: "none", propIn: 0, ink: 0, elapsed: 0 });
    expect(o.px(VDOCK_ROOM.window.x + 5, VDOCK_ROOM.window.y + 5).b).toBeGreaterThan(200);
  });

  it("parity_dawn_and_dusk_put_the_sun_low_in_the_pane_day_puts_it_high", () => {
    // yap-1107-parity-pill (batch-3 bounce: vdock.ts branch coverage) — the
    // window's sun sits 7 art px under the pane top by day and 7 px above the
    // pane bottom at dawn and dusk; the core is #ffd23f.
    const w = VDOCK_ROOM.window;
    const gx = w.x + 3, gy = w.y + 3, gh = w.h - 6;
    const sunAt = (o: Raster, y: number) => { const p = o.px(gx + 7, y); return [p.r, p.g, p.b]; };
    for (const time of ["dawn", "dusk", "day"] as const) {
      const o = new Raster(VDOCK_SCENE.w, VDOCK_SCENE.h);
      drawTallRoom(o, skyLook(time), POSE, { prop: "none", propIn: 0, ink: 0, elapsed: 0 });
      const low = sunAt(o, gy + gh - 7), high = sunAt(o, gy + 7);
      if (time === "day") {
        expect(high).toEqual([0xff, 0xd2, 0x3f]);
        expect(low).not.toEqual([0xff, 0xd2, 0x3f]);
      } else {
        expect(low).toEqual([0xff, 0xd2, 0x3f]);
        expect(high).not.toEqual([0xff, 0xd2, 0x3f]);
      }
    }
  });

  it("paints_yappy_where_the_pose_says", () => {
    for (const prop of PROPS) {
      const o = new Raster(VDOCK_SCENE.w, VDOCK_SCENE.h);
      drawTallRoom(o, skyLook("day"), POSE, { prop, propIn: 1, ink: 1, elapsed: 0 });
      const at = vChickAt(prop, 1);
      // his yellow body is painted inside his box (the room has no yellow that big)
      const b = vChickBox(at.cx, at.feetY);
      let yellow = 0;
      for (let y = b.y; y < b.y + b.h; y++) for (let x = b.x; x < b.x + b.w; x++) {
        const c = o.px(x, y);
        if (c.r > 230 && c.g > 150 && c.g < 235 && c.b < 120) yellow++;
      }
      expect(yellow, `${prop}`).toBeGreaterThan(30);
    }
  });

  it("the_tab_chick_fills_its_buffer_and_nothing_spills", () => {
    const o = new Raster(TAB_ART.w, TAB_ART.h);
    drawTabChick(o, POSE);
    let painted = 0;
    for (let y = 0; y < TAB_ART.h; y++) for (let x = 0; x < TAB_ART.w; x++) if (o.px(x, y).a > 0) painted++;
    expect(painted).toBeGreaterThan(200);
    expect(o.globalAlpha).toBe(1);
  });
});
