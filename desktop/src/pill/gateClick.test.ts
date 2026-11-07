/**
 * yap-1106-pill-trial-clickable — the gated pill is the Buy button.
 * Pure policy (route, nub lock, 6 s hover-held settle, hover bus) plus a
 * server render of YappyPill proving the copy and the lock are in the markup.
 */
import { describe, expect, it, vi } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve(null)),
}));
vi.mock("@tauri-apps/api/event", () => ({
  emit: vi.fn(() => Promise.resolve()),
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import YappyPill from "./YappyPill";
import FLOAT_CSS from "../float.css?raw";
import TOKENS_CSS from "../tokens.css?raw";
import YAPPY_PILL_SRC from "./YappyPill.tsx?raw";
import { pillLicense, type PillLicense } from "./license";
import { sceneLicense } from "./forced";
import { DURATION } from "./motion";
import { GATED_SETTLE_MS, PHASE_HOLD_MS } from "./live";
import {
  GATE_BUY_LABEL, GATE_BUY_TOOLTIP, GATE_CLICK_COMMAND, GATE_HOLD_IDLE, GATE_HOLD_MS, GATE_HOVER_GRACE_MS,
  GATE_CTA_LABEL, GATE_OPENING_LABEL, GATE_PRESS_MS, gateChipCopy,
  GATE_LOCK_LABEL, YAPPY_HOP_EVENT, gateClickArgs, gateClickRoute, gateHovered, gateSettleDueIn, nubLockVisible,
  onGateHover, reduceGateHold, setGateHover, GATE_HITBOX_OWNER_CHIP, GATE_HITBOX_OWNER_LOCK,
} from "./gateClick";

const ended = (): PillLicense => pillLicense(sceneLicense("ended"));
const trial = (): PillLicense => pillLicense(sceneLicense("trial"));
const problem: PillLicense = { show: true, tone: "problem", glyph: "⚿", value: null, title: "Key problem", action: "license" };

describe("yap-1106-pill-trial-clickable — copy and route", () => {
  it("gate_copy_says_buy_and_names_the_destination", () => {
    expect(GATE_BUY_LABEL).toBe("Buy a license");
    expect(GATE_BUY_TOOLTIP).toBe("Opens drivia.consulting in your browser");
    expect(GATE_LOCK_LABEL).toContain(GATE_BUY_LABEL);
    expect(GATE_LOCK_LABEL).toContain(GATE_BUY_TOOLTIP);
    // The pill names a command, never a URL or the old checkout host.
    expect(GATE_CLICK_COMMAND).toBe("pill_gate_click");
    for (const s of [GATE_BUY_LABEL, GATE_BUY_TOOLTIP, GATE_LOCK_LABEL, GATE_CLICK_COMMAND]) {
      expect(s).not.toContain("://");
      expect(s).not.toContain(["buy", "stripe", "com"].join("."));
    }
  });

  it("gate_click_routes_an_ended_trial_to_the_purchase_page", () => {
    expect(ended().tone).toBe("ended");
    expect(gateClickRoute(ended())).toBe("purchase_page");
    expect(gateClickRoute(null)).toBe("purchase_page");
    expect(gateClickArgs(ended(), "pill")).toEqual({ surface: "pill", target: "purchase_page" });
    expect(gateClickArgs(ended(), "nub")).toEqual({ surface: "nub", target: "purchase_page" });
  });

  it("gate_click_never_sells_to_a_paying_customer", () => {
    expect(gateClickRoute(problem)).toBe("license_tab");
    expect(gateClickArgs(problem, "chip").target).toBe("license_tab");
  });

  it("nub_lock_only_after_the_refusal_on_an_ended_trial", () => {
    expect(nubLockVisible(ended(), false, false)).toBe(true);
    expect(nubLockVisible(ended(), true, false)).toBe(false); // the full refusal is up
    expect(nubLockVisible(ended(), false, true)).toBe(false); // the mic reason wins
    expect(nubLockVisible(trial(), false, false)).toBe(false);
    expect(nubLockVisible(problem, false, false)).toBe(false);
    expect(nubLockVisible(null, false, false)).toBe(false);
  });
});

describe("yap-1106-pill-trial-clickable — the pill stays 6 s, longer while hovered", () => {
  it("gate_hold_is_six_seconds_from_the_one_duration_table", () => {
    expect(GATE_HOLD_MS).toBe(6000);
    expect(GATE_HOLD_MS).toBe(DURATION.gateHold);
    expect(GATED_SETTLE_MS).toBe(GATE_HOLD_MS);
    expect(PHASE_HOLD_MS.gated).toBe(GATE_HOLD_MS);
  });

  it("gate_settles_six_seconds_after_it_went_up", () => {
    const s = reduceGateHold(GATE_HOLD_IDLE, { type: "shown", at: 1000 });
    expect(gateSettleDueIn(GATE_HOLD_IDLE, 1000)).toBeNull();
    expect(gateSettleDueIn(s, 1000)).toBe(6000);
    expect(gateSettleDueIn(s, 4000)).toBe(3000);
    expect(gateSettleDueIn(s, 9000)).toBe(0);
  });

  it("gate_hover_holds_it_up_past_six_seconds", () => {
    let s = reduceGateHold(GATE_HOLD_IDLE, { type: "shown", at: 0 });
    s = reduceGateHold(s, { type: "hover", on: true, at: 2000 });
    expect(gateSettleDueIn(s, 2000)).toBeNull();
    expect(gateSettleDueIn(s, 60_000)).toBeNull(); // a minute on it: still up
    s = reduceGateHold(s, { type: "hover", on: false, at: 60_000 });
    expect(gateSettleDueIn(s, 60_000)).toBe(GATE_HOVER_GRACE_MS);
    // Leaving early (before 6 s) never shortens the hold.
    let early = reduceGateHold(GATE_HOLD_IDLE, { type: "shown", at: 0 });
    early = reduceGateHold(early, { type: "hover", on: true, at: 500 });
    early = reduceGateHold(early, { type: "hover", on: false, at: 1000 });
    expect(gateSettleDueIn(early, 1000)).toBe(5000);
  });

  it("gate_shown_mid_hover_keeps_holding_and_settled_clears", () => {
    let s = reduceGateHold(GATE_HOLD_IDLE, { type: "hover", on: true, at: 0 });
    s = reduceGateHold(s, { type: "shown", at: 10 });
    expect(gateSettleDueIn(s, 10)).toBeNull();
    s = reduceGateHold(s, { type: "settled" });
    expect(s.shownAt).toBeNull();
    expect(s.hovered).toBe(true);
    expect(reduceGateHold(s, { type: "hover", on: true, at: 20 })).toBe(s);
  });

  it("gate_hover_bus_reaches_float_main_once_per_change", () => {
    const seen: boolean[] = [];
    const off = onGateHover((on) => seen.push(on));
    setGateHover(true);
    setGateHover(true);
    expect(gateHovered()).toBe(true);
    setGateHover(false);
    off();
    setGateHover(true);
    setGateHover(false);
    expect(seen).toEqual([true, false]);
  });
});

describe("yap-1106-pill-trial-clickable — YappyPill markup", () => {
  it("gated_pill_chip_is_a_button_with_the_browser_tooltip", () => {
    const html = renderToStaticMarkup(createElement(YappyPill, { gate: "gated", license: ended() }));
    expect(html).toMatch(/<button[^>]*class="kami-blocked gated"/);
    expect(html).toContain(`title="${GATE_BUY_TOOLTIP}"`);
    expect(html).toContain(GATE_BUY_LABEL); // in the accessible name
    expect(html).not.toContain("kami-gate-lock"); // the refusal is up, not the nub
  });

  it("resting_nub_shows_the_lock_after_the_trial_ended", () => {
    const html = renderToStaticMarkup(createElement(YappyPill, { gate: "idle", license: ended() }));
    expect(html).toMatch(/<button[^>]*class="kami-license kami-license-ended kami-gate-lock"/);
    expect(html).toContain(`aria-label="${GATE_LOCK_LABEL}"`);
    const running = renderToStaticMarkup(createElement(YappyPill, { gate: "idle", license: trial() }));
    expect(running).not.toContain("kami-gate-lock");
  });

  it("hop_event_name_matches_the_backend", () => {
    expect(YAPPY_HOP_EVENT).toBe("yappy-hop");
  });
});

describe("yap-1106-pill-trial-clickable — pill.html §13 frames", () => {
  it("gate_chip_copy_follows_the_approved_frames", () => {
    // Press refused: the headline plus a "Get Yap" button, no tooltip.
    expect(gateChipCopy("Trial ended", false, false)).toEqual({
      lead: "Trial ended", cta: GATE_CTA_LABEL, ctaFilled: false, tooltip: null,
    });
    expect(GATE_CTA_LABEL).toBe("Get Yap");
    // Hover: the button fills in as "Buy a license"; the tooltip says where.
    expect(gateChipCopy("Trial ended", true, false)).toEqual({
      lead: "Trial ended", cta: GATE_BUY_LABEL, ctaFilled: true, tooltip: GATE_BUY_TOOLTIP,
    });
    // Click: "Opening drivia.consulting…" alone, hovered or not.
    for (const hovered of [false, true]) {
      expect(gateChipCopy("Trial ended", hovered, true)).toEqual({
        lead: GATE_OPENING_LABEL, cta: null, ctaFilled: false, tooltip: null,
      });
    }
    expect(GATE_OPENING_LABEL).toBe("Opening drivia.consulting\u2026");
  });

  it("gate_press_is_120_ms_from_the_duration_table", () => {
    expect(GATE_PRESS_MS).toBe(120);
    expect(GATE_PRESS_MS).toBe(DURATION.gatePress);
  });

  it("gated_chip_renders_the_get_yap_button_and_no_tooltip_at_rest", () => {
    const html = renderToStaticMarkup(createElement(YappyPill, { gate: "gated", license: ended() }));
    expect(html).toMatch(/<span class="kami-gate-cta"[^>]*>Get Yap<\/span>/);
    expect(html).not.toContain("kami-gate-tip");
    expect(html).not.toContain("is-pressed");
    expect(html).not.toContain(GATE_OPENING_LABEL);
  });

  it("gate_chip_colours_are_tokens_measured_from_the_artifact", () => {
    expect(TOKENS_CSS).toContain("--pill-gate-cta-surface: #efece4;");
    expect(TOKENS_CSS).toContain("--pill-gate-cta-surface-hover: #ffd23f;");
    expect(TOKENS_CSS).toContain("--pill-gate-cta-text: #1d1c1a;");
    expect(TOKENS_CSS).toContain("--pill-gate-tooltip-surface: #1d1c1a;");
    for (const v of ["--pill-gate-cta-surface)", "--pill-gate-cta-surface-hover)", "--pill-gate-tooltip-surface)"]) {
      expect(FLOAT_CSS).toContain(`var(${v}`);
    }
    expect(FLOAT_CSS).toMatch(/\.kami-blocked\.gated\.is-pressed\s*\{\s*transform:\s*scale\(0\.97\)/);
  });
});

describe("yap-1106-pill-trial-clickable — the click reaches the webview", () => {
  it("gate_chip_and_nub_lock_are_unioned_into_the_click_through_hitbox", () => {
    // The panel is click-through outside the published hitbox, and the chip
    // (with its "Get Yap" button) and the nub lock can sit outside the canvas
    // capsule. Each is unioned in under its own owner while mounted.
    expect(GATE_HITBOX_OWNER_CHIP).not.toBe(GATE_HITBOX_OWNER_LOCK);
    expect(GATE_HITBOX_OWNER_CHIP).not.toBe("capsule");
    expect(GATE_HITBOX_OWNER_LOCK).not.toBe("capsule");
    expect(YAPPY_PILL_SRC).toContain('[GATE_HITBOX_OWNER_CHIP, ".kami-blocked.gated"]');
    expect(YAPPY_PILL_SRC).toContain('[GATE_HITBOX_OWNER_LOCK, ".kami-gate-lock"]');
    expect(YAPPY_PILL_SRC).toContain("extendPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, owner)");
    expect(YAPPY_PILL_SRC).toContain("for (const [owner] of owners) extendPillHitbox(null, owner)");
  });
});
