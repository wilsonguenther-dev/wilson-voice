/**
 * yap-1106-main-character-switch — who sits in the pill.
 *
 * companions.html section 04: Yappy, Hoot, Tidy, Gulp or Shelly; each needs a
 * pose for all 17 pill states; meetings stay Hoot's (in the pill he records
 * himself, no visit); Yappy out of the pill visits only for milestones.
 */
import { describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import { PILL_CHARACTER_IDS, characters, resolveCharacter } from "../registry";
import { CHARACTER_PHASES, CHARACTER_TONES, fitsBox, type CharacterFrame } from "../types";
import { FRIEND_LINES, FRIEND_POSES, PILL_FRIENDS, friendLine, friendPhaseArt, gulp, hoot, shelly, tidy } from "./index";
import { phaseCopy, type LivePhase } from "../../live";
import PillShell from "../../PillShell";

const frame = (phase: LivePhase, tone: CharacterFrame["tone"] = "friendly", box = { w: 240, h: 48 }): CharacterFrame => ({
  phase, tone, level: 0.4, box, dock: "x", reducedMotion: false,
});

describe("who sits in the pill (yap-1106-main-character-switch)", () => {
  it("settings_offer_exactly_yappy_hoot_tidy_gulp_shelly_and_each_is_registered", () => {
    expect(PILL_CHARACTER_IDS).toEqual(["yappy", "hoot", "tidy", "gulp", "shelly"]);
    // Settings → Companion offers this same list (settings.contained.test.tsx).
    for (const id of PILL_CHARACTER_IDS) expect(resolveCharacter(id).id).toBe(id);
    expect(resolveCharacter("hoot")).toBe(hoot);
    expect(resolveCharacter("tidy")).toBe(tidy);
    expect(resolveCharacter("gulp")).toBe(gulp);
    expect(resolveCharacter("shelly")).toBe(shelly);
    // Room-only friends (and Cinder, "later") are never registered: they read as Yappy.
    for (const id of ["ember", "sal", "mochi", "cinder"]) expect(resolveCharacter(id).id).toBe("yappy");
    expect(characters()[0].id).toBe("yappy");
  });

  it("each_choice_has_all_17_pill_states_in_every_voice", () => {
    expect(CHARACTER_PHASES.length).toBe(17);
    for (const id of PILL_CHARACTER_IDS) {
      const c = resolveCharacter(id);
      const sprites = new Set<string>();
      for (const phase of CHARACTER_PHASES) {
        for (const tone of CHARACTER_TONES) {
          const paint = c.render(frame(phase, tone));
          expect(paint, `${id}/${phase}/${tone}`).not.toBeNull();
          expect(paint?.art.sprite.length ?? 0, `${id}/${phase} art`).toBeGreaterThan(0);
          expect(paint?.copy.trim().length ?? 0, `${id}/${phase}/${tone} copy`).toBeGreaterThan(0);
        }
        sprites.add(c.render(frame(phase))?.art.sprite ?? "");
      }
      // Real poses, not one frame for everything.
      expect(sprites.size, `${id} poses`).toBeGreaterThan(5);
    }
  });

  it("friend_pose_table_covers_every_phase_and_settled_states_never_work", () => {
    expect(Object.keys(FRIEND_POSES).sort()).toEqual([...CHARACTER_PHASES].sort());
    for (const p of ["done", "empty", "cancelled", "blocked", "gated", "sleepy", "idle"] as LivePhase[]) {
      expect(FRIEND_POSES[p].motion, p).not.toBe("work");
    }
    for (const id of PILL_FRIENDS) {
      const art = friendPhaseArt(id);
      for (const p of CHARACTER_PHASES) expect(art[p].sprite.startsWith(`${id}-`), `${id}/${p}`).toBe(true);
      // The latched take wears the listening art (the lock is the shell's chip).
      expect(art["listening-locked"]).toEqual(art.listening);
    }
  });

  it("friends_add_their_own_lines_on_top_of_the_shared_voice", () => {
    expect(friendLine("hoot", "listening", "friendly")).toBe("go on. I'm listening.");
    expect(friendLine("gulp", "done", "friendly")).toBe("delicious words.");
    // No own line: the shared voice line, so every voice still applies.
    expect(friendLine("tidy", "error", "rude")).toBe(phaseCopy("error", "rude"));
    for (const id of PILL_FRIENDS) expect(Object.keys(FRIEND_LINES[id]).length).toBeGreaterThan(2);
  });

  it("a_friend_never_paints_past_the_box_and_paints_crisp", () => {
    for (const c of [hoot, tidy, gulp, shelly]) {
      for (const box of [{ w: 0, h: 0 }, { w: 3, h: 17 }, { w: 120, h: 30 }, { w: 240, h: 48 }]) {
        const paint = c.render(frame("listening", "friendly", box));
        expect(paint && fitsBox(paint.size, box), `${c.id} ${JSON.stringify(box)}`).toBe(true);
        expect(paint?.canvas).toEqual({ imageSmoothingEnabled: false });
      }
      expect(c.stage).toBeUndefined();
    }
  });

  it("the_real_shell_mounts_a_friend_in_its_body_slot", () => {
    for (const c of [hoot, tidy, gulp, shelly]) {
      const markup = renderToStaticMarkup(createElement(PillShell, { character: c, gate: "listening", tone: "friendly" }));
      expect(markup).toContain(`data-character="${c.id}"`);
      expect(markup).toContain(`data-friend="${c.id}"`);
      expect(markup).toContain('class="char-friend"');
      expect(markup).toContain(`data-sprite="${c.id}-listen"`);
    }
  });
});
