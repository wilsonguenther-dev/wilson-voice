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
import { Menu, type MenuItemOptions, type PredefinedMenuItemOptions, type SubmenuOptions } from "@tauri-apps/api/menu";
import { Mic, MicOff, NotebookPen, Settings2, Square, StickyNote, Users, X } from "lucide-react";
import { barAxis, barTokens, shellBarPhase, type BarDock } from "./dock";
import { isListening, type LivePhase, type TranscribeProgress } from "./live";
import { parseMeetingPopover, popoverEntries, type KindChoice, type PopoverEntry } from "./meetingPopover";
import { MEETING_STOP_FAILED_EVENT } from "./meetingDictation";

export type SlotId =
  | "dictate"
  | "scratchpad"
  | "cancel"
  | "wpm"
  | "menu"
  // yap-1106-pill-notes-and-meeting / WF-03 — the hover rail and the hands-free stop.
  | "meeting-start"
  | "note"
  | "meeting-stop"
  | "hands-free-stop";
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
  /**
   * yap-1106-pill-notes-and-meeting / WF-03 — the resting pill is hovered or
   * holds keyboard focus, so its action rail is out. Decided by the CAPSULE
   * (PillShell owns the hover machine); absent = closed.
   */
  railOpen?: boolean;
  /** A meeting is RECORDING right now (`MeetingStatus.recording`). Absent = no. */
  meetingRecording?: boolean;
  /** The running meeting's clock as the backend rendered it ("12:34"), for a Note's timestamp. */
  meetingClock?: string | null;
  /** The running meeting's title, if it has one. */
  meetingTitle?: string | null;
  /** The live keys the rail's keycaps show, read from `hotkey_bindings`. Absent = no keycaps. */
  keys?: RailKeys;
  /** The key that ends a hands-free take (`AppStatus.chord_label`). */
  stopKey?: string;
}

/**
 * WF-03 — the keys the rail names, each the user's REAL current binding off
 * the backend's live hotkey table (`hotkey_bindings`), never a default typed
 * here. `null` = that action has no working key right now, and its tooltip
 * shows no keycap rather than a key that does nothing.
 */
export interface RailKeys {
  dictate: string | null;
  scratchpad: string | null;
  meeting: string | null;
}
export const NO_RAIL_KEYS: RailKeys = { dictate: null, scratchpad: null, meeting: null };

/** The slice of `hotkey_bindings` (hotkeyRecorder.ts `HotkeyBindingsReport`) the rail reads. */
export interface RailBindingRow {
  action: string;
  key: string;
  liveKey: string | null;
}

/**
 * The rail's keycaps from the live hotkey table. Prefers the key that performs
 * the action RIGHT NOW (`liveKey`, which differs from `key` when a fallback is
 * held) and falls back to the stored key; an empty or missing row is `null`.
 * Pure, so "a rebind changes the tooltip" is a unit test, not a restart.
 */
export function railKeysFrom(rows: readonly RailBindingRow[] | null | undefined): RailKeys {
  const keyOf = (...actions: string[]): string | null => {
    for (const a of actions) {
      const r = rows?.find((x) => x.action === a);
      const k = (r?.liveKey ?? r?.key ?? "").trim();
      if (k) return k;
    }
    return null;
  };
  return {
    dictate: keyOf("dictate_hold", "dictate_toggle"),
    scratchpad: keyOf("open_scratchpad"),
    meeting: keyOf("record_meeting"),
  };
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
  /** The live key that performs this action, drawn as a keycap in the tooltip. `null`/absent = none. */
  keycap?(ctx: SlotCtx): string | null;
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
const takeIsLive = (p: LivePhase): boolean => isListening(p) || p === "transcribing";
const permissionStopped = (p: LivePhase): boolean => p === "blocked" || p === "waiting" || p === "gated";

/** The bar's thickness for this context, straight off Y5-I's geometry table. */
export function barThickness(ctx: SlotCtx): number {
  return barTokens(shellBarPhase(ctx.phase, ctx.meetingOnScreen), ctx.dock).thickness;
}

/**
 * The bar's length axis for this context: a column only for a strip phase at a
 * side dock — and for the resting rail at a side dock, which rotates with the
 * strip it grew out of (WF-03: Dictate, Scratchpad, Meeting stacked).
 */
export function slotAxis(ctx: SlotCtx): "row" | "column" {
  // A meeting's banner stays a row at every dock (dock-geometry.json), rail or not.
  if (railShown(ctx) && !ctx.meetingOnScreen) return ctx.dock === "bottom" ? "row" : "column";
  return barAxis(shellBarPhase(ctx.phase, ctx.meetingOnScreen), ctx.dock);
}

/**
 * WF-03 — the resting rail is out: the pill is hovered or focused, and nothing
 * else owns the bar (no take, no permission/license stop). A meeting that is
 * recording does not close it: the rail is where its Note and Stop live.
 */
export function railShown(ctx: SlotCtx): boolean {
  return !!ctx.railOpen && ctx.phase === "idle";
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
  if (isListening(ctx.phase)) {
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
 * Telemetry: ONE structured line per rail action and per failure, in the repo's
 * "<module> <event> key=value" shape. Never the note text, never a key, never a
 * path — only which action, the dock, and the outcome.
 */
export function railLogLine(event: string, fields: Record<string, string | number | boolean> = {}): string {
  const kv = Object.entries(fields).map(([k, v]) => `${k}=${String(v).replace(/\s+/g, "_")}`);
  return ["pill_rail", event, ...kv].join(" ");
}
function railLog(event: string, fields: Record<string, string | number | boolean> = {}): void {
  console.info(railLogLine(event, fields));
}
const railFailureListeners = new Set<(sentence: string) => void>();
/**
 * The shell (float-main) shows a failed rail action's sentence in its phase
 * strip — the pill's one line of text — because the main window's toast host
 * (toast.ts) lives in another window. Returns the unsubscribe.
 */
export function subscribeRailFailure(cb: (sentence: string) => void): () => void {
  railFailureListeners.add(cb);
  return () => { railFailureListeners.delete(cb); };
}
/** A rail action failed: one log line, and the one sentence (with its way forward) a person can act on. */
export function railFailed(action: string, err: unknown, sentence: string): void {
  console.warn(railLogLine("failed", { action, error: typeof err === "string" ? err.slice(0, 80) : "error" }));
  railFailureListeners.forEach((cb) => cb(sentence));
}

/** The Scratchpad window (DB-D) — the rail's Scratchpad, and Note when no meeting runs. */
async function openScratchpad(dock: BarDock, noteId?: string): Promise<void> {
  railLog("scratchpad_open", { dock, note: noteId != null });
  // fix-b5-1 — with a note id the window lands ON that note (its own tab,
  // active, caret after the seed); without one it keeps the last active note.
  const opened = noteId != null ? invoke("open_scratchpad", { noteId }) : invoke("open_scratchpad");
  await opened.catch((e) => {
    railFailed("scratchpad", e, "Scratchpad didn't open here, so it opened in Yap.");
    openMain("scratchpad");
  });
}

type PopoverMenuItem = MenuItemOptions | PredefinedMenuItemOptions | SubmenuOptions;

/**
 * WF-08 — the Meeting button's popover: today's next three meetings (each a
 * Record row), "Record now" WITH ITS KIND (the backend's own picker,
 * `meeting_kind_choices`, the list the tray and the Meetings tab render), and,
 * when no calendar is in use, "No upcoming meetings" + "Connect calendar",
 * which opens the existing setup card. What it lists comes from the backend
 * (`meeting_popover` → `calendar::meeting_popover`, the SAME snapshot the menu
 * bar's "Today's Meetings" reads); `popoverEntries` (meetingPopover.ts) orders
 * the rows. It is a native popup, so it escapes the small pill window and
 * arrow keys / Return / Escape work. Nothing records until the person picks a
 * row — a click is the consent (product rule).
 */
async function startMeetingMenu(dock: BarDock): Promise<void> {
  const [rawPopover, got] = await Promise.all([
    invoke<unknown>("meeting_popover").catch(() => null),
    invoke<KindChoice[] | null>("meeting_kind_choices").catch(() => null),
  ]);
  const model = parseMeetingPopover(rawPopover);
  const choices = Array.isArray(got) ? got.filter((c) => c && c.kind && c.label) : [];
  railLog("meeting_menu", { dock, state: model?.state ?? "unavailable", rows: model?.meetings.length ?? 0 });
  const start = (kind: string | null) => {
    const t0 = Date.now();
    invoke("start_meeting", { title: null, kind })
      .then(() => railLog("meeting_start", { kind: kind ?? "unknown", dock, ms: Date.now() - t0 }))
      .catch((e) => railFailed("meeting_start", e, typeof e === "string" && e ? e : "Meeting didn't start — try Record in the menu bar."));
  };
  const record = (key: string, row: number) => {
    const t0 = Date.now();
    invoke("meeting_popover_record", { key })
      .then(() => railLog("meeting_popover_record", { row, dock, ms: Date.now() - t0 }))
      .catch((e) => railFailed("meeting_popover_record", e, typeof e === "string" && e ? e : "That meeting didn't start — try Record now."));
  };
  const connect = (nav: string, tab: string | null) => {
    railLog("meeting_connect_calendar", { dock, nav, tab: tab ?? "none" });
    openMain(nav, tab ?? undefined);
  };
  const entries = popoverEntries(model, choices);
  // No backend answer and no kinds: the button does what it always did.
  if (entries.length === 1 && entries[0].type === "record-now" && entries[0].kinds.length === 0) {
    start(null);
    return;
  }
  let recordRow = 0;
  const items = entries.map((e: PopoverEntry): PopoverMenuItem => {
    switch (e.type) {
      case "record": {
        const row = recordRow++;
        return { text: e.text, action: () => record(e.key, row) };
      }
      case "note":
        return { text: e.text, enabled: false };
      case "connect":
        return { text: e.text, action: () => connect(e.nav, e.tab) };
      case "separator":
        return { item: "Separator" };
      case "record-now":
        return e.kinds.length === 0
          ? { text: e.text, action: () => start(null) }
          : { text: e.text, items: e.kinds.map((c) => ({ text: c.label, action: () => start(c.kind) })) };
    }
  });
  const menu = await Menu.new({ items });
  await menu.popup();
}

async function stopMeeting(dock: BarDock): Promise<void> {
  const t0 = Date.now();
  await invoke("stop_meeting")
    .then(() => railLog("meeting_stop", { dock, ms: Date.now() - t0 }))
    .catch((e) => {
      railFailed("meeting_stop", e, "Meeting didn't stop — use Stop in the menu bar.");
      // yap-fx-06 — the rail's Stop failing opens the same stop-failed card as
      // the badge's (Try again / Quit Yap), so force quit is never the only way out.
      window.dispatchEvent(new CustomEvent(MEETING_STOP_FAILED_EVENT));
    });
}

/** "Meeting note · 12:34" — the note's timestamp is the meeting's own clock. */
export function meetingNoteTitle(clock: string | null | undefined, title?: string | null): string {
  const at = (clock ?? "").trim() || "0:00";
  const of = (title ?? "").trim();
  return of ? `Meeting note · ${of} · ${at}` : `Meeting note · ${at}`;
}

/**
 * DECISION (item 18): "take notes there". While a meeting records, Note makes a
 * TIMESTAMPED note — titled off the meeting's own clock and seeded with
 * "[12:34] " — and opens it in the Scratchpad, the one window that takes typing
 * while the meeting app stays in front. With no meeting running, Note is the
 * Scratchpad itself.
 */
async function takeNote(ctx: SlotCtx): Promise<void> {
  if (!ctx.meetingRecording) { await openScratchpad(ctx.dock); return; }
  const at = (ctx.meetingClock ?? "").trim() || "0:00";
  const t0 = Date.now();
  let noteId: string;
  try {
    const made = await invoke<{ id?: unknown } | null>("save_note_draft", {
      id: null,
      title: meetingNoteTitle(at, ctx.meetingTitle),
      content: `[${at}] `,
    });
    if (!made || typeof made.id !== "string" || made.id === "") throw new Error("save_note_draft returned no id");
    noteId = made.id;
  } catch (e) {
    railFailed("meeting_note", e, "Note didn't save — open the Scratchpad and type it there.");
    return;
  }
  // One line, no note text: the dock and how long the save took.
  railLog("meeting_note", { dock: ctx.dock, ms: Date.now() - t0 });
  // fix-b5-1 — open the Scratchpad ON the note just made, not the last one used.
  await openScratchpad(ctx.dock, noteId);
}

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
 * THE FIVE take-time slots (Y8-B), plus item 18 / WF-03's rail: Meeting, Note,
 * Stop meeting at rest, and the hands-free Stop. Order within a group is order
 * along the bar; no empty ones: every row here draws something real when visible.
 */
export const FLOW_BAR_SLOTS: readonly FlowBarSlot[] = [
  {
    id: "scratchpad",
    group: "lead",
    glyph: () => <NotebookPen strokeWidth={2.2} />,
    tooltip: () => "Scratchpad",
    keycap: (ctx) => ctx.keys?.scratchpad ?? null,
    // WF-03: on the resting rail always; mid-take only behind the toggle (DB-D).
    visible: (ctx) => railShown(ctx) || (SCRATCHPAD_WINDOW_LANDED && ctx.scratchpadOnBar && isListening(ctx.phase)),
    onPress: (ctx) => { void openScratchpad(ctx.dock); },
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
        : isListening(ctx.phase) ? <Square fill="currentColor" strokeWidth={0} />
          : <Mic strokeWidth={2.2} />,
    tooltip: (ctx) =>
      isListening(ctx.phase) ? "Stop dictation"
        : permissionStopped(ctx.phase) ? "Dictation is unavailable — open to fix"
          : railShown(ctx) ? "Dictate"
            : "Start dictation",
    // WF-03: "Dictate fn" — the live hold key, at rest only (mid-take the key is the stop key).
    keycap: (ctx) => (ctx.phase === "idle" ? ctx.keys?.dictate ?? null : null),
    // Always: the centre IS the bar. At rest it is the seed, so it is exactly as
    // thick as the bar and never thicker.
    visible: () => true,
    onPress: (_ctx, act) => act.toggleDictation(),
    lengthPx: SLOT_PX,
    // At rest it is the seed, exactly as thick as the bar; with the rail out the
    // pill is hover-expanded (float.css), so it takes a full slot box.
    acrossPx: (ctx) => (railShown(ctx) ? SLOT_PX : Math.min(SLOT_PX, barThickness(ctx))),
  },
  {
    // Item 18 / WF-08 — the Meeting popover (today's meetings, Record now, Connect calendar). Hidden once one records.
    id: "meeting-start",
    group: "trail",
    glyph: () => <Users strokeWidth={2.2} />,
    tooltip: () => "Record a meeting",
    keycap: (ctx) => ctx.keys?.meeting ?? null,
    visible: (ctx) => railShown(ctx) && !ctx.meetingRecording,
    onPress: (ctx) => { startMeetingMenu(ctx.dock).catch((e) => railFailed("meeting_menu", e, "Meetings didn't open — try Record in the menu bar.")); },
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
  },
  {
    // Item 18 — a timestamped Note into the running meeting (see `takeNote`).
    id: "note",
    group: "trail",
    glyph: () => <StickyNote strokeWidth={2.2} />,
    tooltip: (ctx) => `Note at ${(ctx.meetingClock ?? "").trim() || "0:00"}`,
    visible: (ctx) => railShown(ctx) && !!ctx.meetingRecording,
    onPress: (ctx) => { void takeNote(ctx); },
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
  },
  {
    // Item 18 — Stop meeting, beside its Note. The meeting key toggles it too.
    id: "meeting-stop",
    group: "trail",
    glyph: () => <Square fill="currentColor" strokeWidth={0} />,
    tooltip: () => "Stop meeting",
    keycap: (ctx) => ctx.keys?.meeting ?? null,
    visible: (ctx) => railShown(ctx) && !!ctx.meetingRecording,
    onPress: (ctx) => { void stopMeeting(ctx.dock); },
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
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
    visible: (ctx) => isListening(ctx.phase),
    onPress: () => { openBarMenu().catch(() => openMain("settings", "audio")); },
    lengthPx: SLOT_PX,
    acrossPx: fixedAcross,
  },
  {
    // Item 18 — hands-free polish: a stop you can CLICK (and Tab to), its key
    // as a keycap. The capsule's own stop glyph is aria-hidden and untabbable.
    id: "hands-free-stop",
    group: "trail",
    glyph: () => <Square fill="currentColor" strokeWidth={0} />,
    tooltip: () => "Stop hands-free",
    keycap: (ctx) => (ctx.stopKey ?? "").trim() || null,
    // A row only: on a side dock's column the Classic window (300x140) has no
    // room for a third trail slot — pill-matrix found the column pushed past
    // both window edges. There the key (and the capsule) still stop it.
    visible: (ctx) => ctx.phase === "listening-locked" && slotAxis(ctx) === "row",
    onPress: (ctx, act) => { railLog("hands_free_stop", { dock: ctx.dock }); act.toggleDictation(); },
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
 * A slot's tooltip: the words, then the live key as a keycap ("Dictate [fn]").
 * Re-rendered from `ctx.keys` on every frame, so a rebind shows up at once.
 */
export function SlotTip({ text, keycap, side }: { text: string; keycap: string | null; side: TooltipSide }) {
  return (
    <span className="slot-tip" role="tooltip" data-tip-side={side}>
      {text}
      {keycap ? <kbd className="keycap">{keycap}</kbd> : null}
    </span>
  );
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
    <span className="slots" data-slot-group={group} data-bar-axis={slotAxis(ctx)} data-rail={railShown(ctx) ? "open" : undefined}>
      {shown.map((p) => {
        const slot = slotById(p.id);
        const tip = slot.tooltip(ctx);
        const key = slot.keycap?.(ctx) ?? null;
        const tipEl = <SlotTip text={tip} keycap={key} side={side} />;
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
              // A readout is not an action (Y5-J): it only keeps a click on it
              // from reaching the capsule, which would start a dictation.
              onClickCapture={(e) => e.stopPropagation()}
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
            data-pill-control={`slot-${p.id}`}
            data-tip-side={side}
            aria-label={tip}
            aria-keyshortcuts={key ?? undefined}
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
