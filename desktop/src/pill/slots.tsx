/**
 * Y8-B — the Flow-Bar affordance slots. The pill becomes a BAR.
 *
 * Wispr's bar hosts eight named widgets; Yap's minimum viable set is five:
 * Dictate (centre) · Scratchpad · Cancel/Stop · Word-count + live WPM ·
 * Mic/Settings menu. YV95's `MeetingBadge` was the first thing that sat in the
 * capsule beside the mic, and it was a one-off child. This module generalises
 * it into ONE contract, so a sixth affordance is a row in `FLOW_BAR_SLOTS` and
 * not a fourth bespoke branch in `ClassicPill`:
 *
 *   { id, glyph, tooltip, visible(ctx), onPress }
 *
 * plus where it sits along the bar (`group`) and how much of the bar it takes.
 *
 * ONE LAYOUT FOR ALL THREE DOCKS. Slots are placed along the bar's LENGTH axis,
 * which Y5-I made orientation-neutral (`--flow-bar-length` along the bar,
 * `--flow-bar-thickness` across it; `pill/dock.ts` + `dock-geometry.json`). At
 * the bottom dock the length axis is a row; at a side dock, for a phase that
 * rotates into the 30 px strip, it is a column. The slots never learn which:
 * they are laid out by `layoutSlots` in length/across terms, and the container
 * carries `data-bar-axis` for float.css to turn into `flex-direction`.
 *
 * VISIBILITY IS A PURE FUNCTION OF CONTEXT, so it is testable without a
 * window, and an invisible slot occupies ZERO length: it is not rendered at
 * all — no element, no gap, no placeholder. A 30 px strip has no room for a
 * hole.
 *
 * NOT A SLOT: the trial numeral from Y2-B ("5d"). It rides the CAPSULE (the
 * license chip `pillLicense` decides in float-main), not the bar's slot row, so
 * the two systems never claim the same pixels. It is deliberately absent from
 * `FLOW_BAR_SLOTS` and must stay absent.
 *
 * NOT A DRAG HANDLE THIEF: a slot never handles `pointerdown`. The press bubbles
 * to the capsule, whose `usePillDrag` handlers (YV65, `pill/drag.ts`) own the
 * gesture, and a slot's click is swallowed whenever that press turned into a
 * drag (`pressSlot`), exactly as the capsule's own click is.
 */
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { Menu } from "@tauri-apps/api/menu";
import { Mic, MicOff, NotebookPen, Settings2, Square, X } from "lucide-react";
import { barAxis, barTokens, shellBarPhase, type BarDock } from "./dock";
import type { LivePhase, TranscribeProgress } from "./live";

export type SlotId = "dictate" | "scratchpad" | "cancel" | "wpm" | "menu";
/** Where along the bar's length a slot sits: before the centre, the centre, after it. */
export type SlotGroup = "lead" | "centre" | "trail";
/** Which way a tooltip opens. Always AWAY from the screen edge the bar is docked to. */
export type TooltipSide = "top" | "left" | "right";

/** The live estimate while listening: words off voiced seconds, and wall seconds listened. */
export interface ListenEstimate {
  /** `wordsFromVoiced` — an ESTIMATE, never a count (live.ts). */
  estWords: number;
  /** Wall-clock seconds the take has been (or was) listening. */
  sec: number;
}

/** Everything a slot may look at. Owned by the SHELL (float-main), never by a slot. */
export interface SlotCtx {
  /** The ONE phase the shell decided (`winningPhase(gate, take)`). */
  phase: LivePhase;
  dock: BarDock;
  /** A meeting on screen while no dictation is (Y5-I's `shellBarPhase`). */
  meetingOnScreen: boolean;
  /** Y3-C's real decode progress; `wordsSoFar` is REAL words from finished chunks. */
  progress: TranscribeProgress | null;
  listen: ListenEstimate;
  /** The user's "Add to Flow Bar" toggle for the Scratchpad (Wispr gates theirs the same way). */
  scratchpadOnBar: boolean;
}

/** What a press is allowed to do that the slot cannot do by itself. */
export interface SlotActions {
  /** The capsule's own start/stop (and its blocked / gated redirects). */
  toggleDictation(): void;
}

export interface FlowBarSlot {
  id: SlotId;
  group: SlotGroup;
  glyph(ctx: SlotCtx): ReactNode;
  tooltip(ctx: SlotCtx): string;
  visible(ctx: SlotCtx): boolean;
  /** `null` = a readout, not a control (the WPM slot). */
  onPress: ((ctx: SlotCtx, act: SlotActions) => void) | null;
  /** Length the slot takes ALONG the bar when visible, in CSS px. */
  lengthPx: number;
  /** Extent ACROSS the bar when visible, in CSS px. Must fit the bar's thickness. */
  acrossPx(ctx: SlotCtx): number;
}

/**
 * A slot's box: 20 px across leaves a 5 px inset each side of the 30 px strip,
 * and four slots plus the waveform still fit the float window's 140 pt height
 * when the bar is a side-dock column (float_pill.rs `PILL_H`).
 */
export const SLOT_PX = 20;
/** Space between two VISIBLE slots along the bar. Never paid for an invisible one. */
export const SLOT_GAP_PX = 3;

/**
 * DB-D (the Scratchpad as a real second window, PR #299) has NOT landed on main.
 * Until it does the Scratchpad slot is never shown, whatever the toggle says:
 * a slot whose feature has not landed is not shown (Y8-B's own rule). DB-D
 * flips this to true in the same PR that ships the window and the toggle.
 */
export const SCRATCHPAD_WINDOW_LANDED = false;

/** A take is live: recording, or decoding in chunks (Y3-D's cancel reaches both). */
const takeIsLive = (p: LivePhase): boolean => p === "listening" || p === "transcribing";
const permissionStopped = (p: LivePhase): boolean => p === "blocked" || p === "waiting" || p === "gated";

/** The bar's thickness for this context, straight off Y5-I's geometry table. */
export function barThickness(ctx: SlotCtx): number {
  return barTokens(shellBarPhase(ctx.phase, ctx.meetingOnScreen), ctx.dock).thickness;
}

/** The bar's length axis for this context: a column only for a strip phase at a side dock. */
export function slotAxis(ctx: SlotCtx): "row" | "column" {
  return barAxis(shellBarPhase(ctx.phase, ctx.meetingOnScreen), ctx.dock);
}

/** The tooltip opens away from the docked edge: up from the bottom, inward from a side. */
export function tooltipSide(dock: BarDock): TooltipSide {
  return dock === "left" ? "right" : dock === "right" ? "left" : "top";
}

/** Words and words-per-minute, and whether the numbers are an estimate. */
export interface WordReadout {
  words: number;
  wpm: number | null;
  estimate: boolean;
}
/** Below this much listening a rate is noise, so none is stated. */
export const MIN_WPM_SEC = 3;

/**
 * The WPM slot's numbers — the same HONESTY RULE Y3-C applies to the pill's
 * commentary. While TRANSCRIBING the count is `progress.wordsSoFar`: real words
 * in text that real chunks decoded, over the seconds the take actually listened.
 * While LISTENING there is no text yet, so it may only be the estimate off voiced
 * seconds, and it SAYS it is one (`estimate: true`, drawn with a "~"). Any other
 * phase has no number at all.
 */
export function wordReadout(ctx: SlotCtx): WordReadout | null {
  const rate = (words: number) =>
    ctx.listen.sec >= MIN_WPM_SEC ? Math.round(words / (ctx.listen.sec / 60)) : null;
  if (ctx.phase === "transcribing" && ctx.progress) {
    const words = ctx.progress.wordsSoFar;
    return { words, wpm: rate(words), estimate: false };
  }
  if (ctx.phase === "listening") {
    return { words: Math.round(ctx.listen.estWords), wpm: rate(ctx.listen.estWords), estimate: true };
  }
  return null;
}

const openMain = (nav: string, tab?: string) => {
  invoke("show_main").catch(() => {});
  emit("navigate", nav).catch(() => {});
  if (tab) emit("settings-tab", tab).catch(() => {});
};

/**
 * The Mic/Settings menu is a NATIVE popup (`Menu.popup`), not a DOM popover: a
 * 30 px strip in a click-through panel has nowhere to draw a list, and an
 * NSMenu can never open off-screen. Allowed by `core:default` (which carries
 * `core:menu:default`, `allow-popup` included) in capabilities/default.json.
 */
async function openBarMenu(): Promise<void> {
  const menu = await Menu.new({
    items: [
      { text: "Microphone…", action: () => openMain("settings", "audio") },
      { text: "Shortcut…", action: () => openMain("settings", "shortcut") },
      { text: "Settings…", action: () => openMain("settings") },
    ],
  });
  await menu.popup();
}

/** The fixed-size slots: one SLOT_PX box across the bar. */
const fixedAcross = () => SLOT_PX;

/**
 * THE FIVE. Order within a group is order along the bar. Exactly five, per the
 * spec, and no empty ones: every row here draws something real when visible.
 */
export const FLOW_BAR_SLOTS: readonly FlowBarSlot[] = [
  {
    id: "scratchpad",
    group: "lead",
    glyph: () => <NotebookPen strokeWidth={2.2} />,
    tooltip: () => "Open Scratchpad",
    visible: (ctx) => SCRATCHPAD_WINDOW_LANDED && ctx.scratchpadOnBar && ctx.phase === "listening",
    onPress: () => openMain("scratchpad"),
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
  },
  {
    id: "cancel",
    group: "lead",
    glyph: () => <X strokeWidth={2.4} />,
    tooltip: (ctx) => (ctx.phase === "transcribing" ? "Cancel — keep the audio" : "Cancel this take"),
    visible: (ctx) => takeIsLive(ctx.phase),
    // Y3-D's ONE cancel: the same command the Escape hotkey and Home's button reach.
    onPress: () => { invoke("cancel_transcription").catch(() => {}); },
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
  },
  {
    id: "dictate",
    group: "centre",
    glyph: (ctx) =>
      permissionStopped(ctx.phase) ? <MicOff strokeWidth={2.2} />
        : ctx.phase === "listening" ? <Square fill="currentColor" strokeWidth={0} />
          : <Mic strokeWidth={2.2} />,
    tooltip: (ctx) =>
      ctx.phase === "listening" ? "Stop dictation"
        : permissionStopped(ctx.phase) ? "Dictation is unavailable — open to fix"
          : "Start dictation",
    // Always: the centre IS the bar. At rest it is the seed, so it is exactly as
    // thick as the bar and never thicker.
    visible: () => true,
    onPress: (_ctx, act) => act.toggleDictation(),
    lengthPx: SLOT_PX,
    acrossPx: (ctx) => Math.min(SLOT_PX, barThickness(ctx)),
  },
  {
    id: "wpm",
    group: "trail",
    glyph: (ctx) => {
      const r = wordReadout(ctx);
      if (!r) return null;
      return (
        <>
          <b className="slot-words">{r.words}</b>
          <small className="slot-rate">{r.wpm === null ? "wpm" : `${r.estimate ? "~" : ""}${r.wpm}`}</small>
        </>
      );
    },
    tooltip: (ctx) => {
      const r = wordReadout(ctx);
      if (!r) return "";
      const rate = r.wpm === null ? "" : ` · ${r.estimate ? "about " : ""}${r.wpm} words per minute`;
      return r.estimate
        ? `About ${r.words} words so far${rate} (estimate until the text is decoded)`
        : `${r.words} words decoded${rate}`;
    },
    visible: (ctx) => wordReadout(ctx) !== null,
    onPress: null,
    lengthPx: SLOT_PX + 4,
    acrossPx: fixedAcross,
  },
  {
    id: "menu",
    group: "trail",
    glyph: () => <Settings2 strokeWidth={2.2} />,
    tooltip: () => "Microphone and settings",
    visible: (ctx) => ctx.phase === "listening",
    onPress: () => { openBarMenu().catch(() => openMain("settings", "audio")); },
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
  },
];

export const slotById = (id: SlotId): FlowBarSlot => {
  const s = FLOW_BAR_SLOTS.find((x) => x.id === id);
  if (!s) throw new Error(`no flow-bar slot ${id}`);
  return s;
};

/** The length a slot takes along the bar: ZERO when it is not visible. */
export function slotLength(slot: FlowBarSlot, ctx: SlotCtx): number {
  return slot.visible(ctx) ? slot.lengthPx : 0;
}

export interface SlotPlacement {
  id: SlotId;
  group: SlotGroup;
  /** Offset along the bar's length from its leading end, in px. */
  offset: number;
  length: number;
  across: number;
}
export interface SlotLayout {
  axis: "row" | "column";
  tooltip: TooltipSide;
  placements: SlotPlacement[];
  /** Total length the slots take, gaps included — only between VISIBLE slots. */
  length: number;
}

const GROUP_ORDER: Record<SlotGroup, number> = { lead: 0, centre: 1, trail: 2 };

/** Place every VISIBLE slot along the bar's length axis. Invisible ones are absent. */
export function layoutSlots(ctx: SlotCtx): SlotLayout {
  const shown = FLOW_BAR_SLOTS.filter((s) => s.visible(ctx))
    .map((s, i) => ({ s, i }))
    .sort((a, b) => GROUP_ORDER[a.s.group] - GROUP_ORDER[b.s.group] || a.i - b.i)
    .map(({ s }) => s);
  let at = 0;
  const placements = shown.map((s, i) => {
    if (i > 0) at += SLOT_GAP_PX;
    const p = { id: s.id, group: s.group, offset: at, length: s.lengthPx, across: s.acrossPx(ctx) };
    at += s.lengthPx;
    return p;
  });
  return { axis: slotAxis(ctx), tooltip: tooltipSide(ctx.dock), placements, length: at };
}

/**
 * A slot's press. Returns whether it acted. A press that became a DRAG is the
 * drag's (YV65), so it never also presses the slot under it.
 */
export function pressSlot(slot: FlowBarSlot, ctx: SlotCtx, act: SlotActions, dragged: boolean): boolean {
  if (dragged || !slot.onPress || !slot.visible(ctx)) return false;
  slot.onPress(ctx, act);
  return true;
}

/**
 * One group of slots (lead or trail) as DOM, inside the capsule. The centre
 * (Dictate) is the capsule's own body, drawn by the character, which reads
 * `slotById("dictate")` for its glyph and tooltip.
 */
export function FlowBarSlots(
  { ctx, group, actions, dragged }: {
    ctx: SlotCtx;
    group: Exclude<SlotGroup, "centre">;
    actions: SlotActions;
    dragged: () => boolean;
  },
) {
  const side = tooltipSide(ctx.dock);
  const shown = layoutSlots(ctx).placements.filter((p) => p.group === group);
  if (shown.length === 0) return null;
  return (
    <span className="slots" data-slot-group={group} data-bar-axis={slotAxis(ctx)}>
      {shown.map((p) => {
        const slot = slotById(p.id);
        const tip = slot.tooltip(ctx);
        const tipEl = (
          <span className="slot-tip" role="tooltip" data-tip-side={side}>{tip}</span>
        );
        const style = { ["--slot-length" as string]: `${p.length}px`, ["--slot-across" as string]: `${p.across}px` };
        if (!slot.onPress) {
          return (
            <span
              key={p.id}
              className="slot readout"
              data-slot={p.id}
              data-tip-side={side}
              role="img"
              aria-label={tip}
              style={style}
              onClick={(e) => e.stopPropagation()}
            >
              {slot.glyph(ctx)}
              {tipEl}
            </span>
          );
        }
        return (
          <button
            key={p.id}
            type="button"
            className="slot"
            data-slot={p.id}
            data-tip-side={side}
            aria-label={tip}
            style={style}
            onClick={(e) => {
              e.stopPropagation();
              pressSlot(slot, ctx, actions, dragged());
            }}
            onKeyDown={(e) => {
              // The capsule's own Enter/Space toggles dictation; a focused slot
              // keeps its key to itself.
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                e.stopPropagation();
                pressSlot(slot, ctx, actions, false);
              }
            }}
          >
            {slot.glyph(ctx)}
            {tipEl}
          </button>
        );
      })}
    </span>
  );
}
