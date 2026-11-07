/**
 * yap-1106-friend-visits — the friend who is visiting the pill, painted crisp.
 *
 * Every decision is `friendVisits.ts`'s (pure, fake-clock tested). This file
 * only feeds it what the pill already knows and paints the answer:
 *
 *   events   `backtrack_caught` (Tidy), the meeting's notes turning ready
 *            (Hoot), Yappy's milestones — the 100th / 1000th `transcript`,
 *            the longest take of the week, a new streak record (`noteTake`;
 *            only when someone else sits in the pill) — and `friend_event { event }` for the
 *            owners whose backend signal lands separately (Gulp, Shelly,
 *            Cinder — Cinder's payload also says whether the cloud is on);
 *   quiet    the meeting recording (from the shell), a live call (the
 *            `call_offer` view — a call is where screens get shared) plus
 *            `pill_quiet { screenSharing, doNotDisturb }`; a visit on screen
 *            leaves the moment any of them turns on.
 *
 * The ledger (once a day each, the after-50 caps) is one localStorage row, so
 * a relaunch does not reset a friend's day. Storage failing costs only the
 * visit, never the pill. Decorative: `aria-hidden`, no pointer events — the
 * event's own card (Backtrack caught, Notes ready) is what is announced.
 * Reduce motion holds one frame. A visit animates for at most 4 s and then
 * nothing runs at all.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { CALL_OFFER_EVENT, CALL_OFFER_STATE } from "./callOfferModel";
import { BACKTRACK_CAUGHT_EVENT } from "./backtrackCaught";
import {
  EMPTY_LEDGER,
  FRIEND_EVENT,
  PILL_QUIET_EVENT,
  PILL_QUIET_STATE,
  VISIT_MAX_MS,
  EMPTY_TAKE_STATS,
  callLive,
  decideVisit,
  milestoneLine,
  noteTake,
  parseLedger,
  parseTakeStats,
  parseQuietState,
  quietReason,
  recordVisit,
  serializeLedger,
  serializeTakeStats,
  takeSpeechSeconds,
  visitLogLine,
  visitRemainingMs,
  type TakeStats,
  type Visit,
  type VisitContext,
  type VisitLedger,
  type VisitorId,
} from "./friendVisits";
import { prefersReducedMotion } from "./motion";
import { DEFAULT_CHATTER_MODE, type ChatterMode } from "./chatter";
import { drawFriendFrame, friendManifest, pickFrame } from "../sprites/friends";
import { FRIENDS } from "../sprites/art";
import { crispPlan, domBuffer, paintCrisp } from "../sprites/crisp";

const LEDGER_KEY = "yap.friends.visits";
const TAKES_KEY = "yap.friends.takes";
/** yap-1106-main-character-switch — Yappy's milestone counts (week, streak). */
const TAKE_STATS_KEY = "yap.friends.takeStats";
/** The visitor's CSS box (whole-number scale inside it, crisp.ts). */
const BOX = { w: 28, h: 28 };
/** Animation step while a visit is on screen (never runs between visits). */
const FRAME_MS = 125;

function loadLedger(): VisitLedger {
  try { return parseLedger(globalThis.localStorage?.getItem(LEDGER_KEY)); }
  catch (e) { console.warn("pill friend_visit ledger_load_failed", e instanceof Error ? e.name : "unknown"); return EMPTY_LEDGER; }
}
function saveLedger(l: VisitLedger): void {
  try { globalThis.localStorage?.setItem(LEDGER_KEY, serializeLedger(l)); }
  catch (e) { console.warn("pill friend_visit ledger_save_failed", e instanceof Error ? e.name : "unknown"); }
}
function loadTakes(): number {
  try {
    const n = Number(globalThis.localStorage?.getItem(TAKES_KEY));
    return Number.isFinite(n) && n > 0 ? Math.floor(n) : 0;
  } catch { return 0; }
}
function saveTakes(n: number): void {
  try { globalThis.localStorage?.setItem(TAKES_KEY, String(n)); }
  catch (e) { console.warn("pill friend_visit takes_save_failed", e instanceof Error ? e.name : "unknown"); }
}

function loadTakeStats(): TakeStats {
  try { return parseTakeStats(globalThis.localStorage?.getItem(TAKE_STATS_KEY)); }
  catch (e) { console.warn("pill friend_visit take_stats_load_failed", e instanceof Error ? e.name : "unknown"); return EMPTY_TAKE_STATS; }
}
function saveTakeStats(t: TakeStats): void {
  try { globalThis.localStorage?.setItem(TAKE_STATS_KEY, serializeTakeStats(t)); }
  catch (e) { console.warn("pill friend_visit take_stats_save_failed", e instanceof Error ? e.name : "unknown"); }
}

/** Paint one frame of `id` into `canvas` at a whole-number scale. */
function paintVisitor(canvas: HTMLCanvasElement, id: VisitorId, tMs: number, reduced: boolean): void {
  const dpr = window.devicePixelRatio;
  if (id === "yappy") {
    paintCrisp<HTMLCanvasElement>(canvas, FRIENDS.yappy, 0, undefined, BOX, dpr, domBuffer, "yappy");
    return;
  }
  const m = friendManifest(id);
  if (!m) return;
  try {
    const plan = crispPlan(BOX.w, BOX.h, m.w, m.h, dpr);
    if (canvas.width !== plan.backingW) canvas.width = plan.backingW;
    if (canvas.height !== plan.backingH) canvas.height = plan.backingH;
    canvas.style.width = `${plan.cssW}px`;
    canvas.style.height = `${plan.cssH}px`;
    canvas.style.imageRendering = "pixelated";
    const ctx = canvas.getContext("2d");
    if (!ctx) { console.warn(`pill friend_visit paint_failed friend=${id} reason=no-2d-context`); return; }
    const buf = domBuffer(m.w, m.h);
    const pick = pickFrame(m, tMs, { acting: true, reducedMotion: reduced });
    drawFriendFrame(buf.ctx, id, pick.kind, pick.index);
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, plan.backingW, plan.backingH);
    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(buf.image, 0, 0, m.w, m.h, plan.dx, plan.dy, m.w * plan.scale, m.h * plan.scale);
  } catch (e) {
    console.warn(`pill friend_visit paint_failed friend=${id} reason=${e instanceof Error ? e.name : "unknown"}`);
  }
}

export interface FriendVisitProps {
  /** A meeting is recording (the shell's `meeting.status.recording`). */
  meetingRecording: boolean;
  /** The meeting's notes just turned ready (Hoot's event). */
  meetingNotesReady: boolean;
  /** Who sits in the pill (`pill_style`); they never visit themselves. */
  occupant: string;
  /** DEV / matrix only: a visitor pinned on screen (`?friend=`). */
  forced?: VisitorId | null;
  /** yap-1106-chatter-setting — "Yappy talks"; omitted, it is "less over time". */
  chatter?: ChatterMode;
}

export function FriendVisit({ meetingRecording, meetingNotesReady, occupant, forced = null, chatter = DEFAULT_CHATTER_MODE }: FriendVisitProps) {
  const [visit, setVisit] = useState<Visit | null>(null);
  const [quiet, setQuiet] = useState({ screenSharing: false, doNotDisturb: false });
  /** A call app is live (offering / chip / recording) — as quiet as a meeting. */
  const [inCall, setInCall] = useState(false);
  const meetingQuiet = meetingRecording || inCall;
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const ledgerRef = useRef<VisitLedger>(EMPTY_LEDGER);
  const takesRef = useRef(0);
  const takeStatsRef = useRef<TakeStats>(EMPTY_TAKE_STATS);
  const visitRef = useRef<Visit | null>(null);
  const ctxRef = useRef({ meetingQuiet, occupant, quiet, chatter });
  // Synced first, so every effect below (and every listener) reads this render.
  useEffect(() => {
    ctxRef.current = { meetingQuiet, occupant, quiet, chatter };
  }, [meetingQuiet, occupant, quiet, chatter]);

  useEffect(() => {
    ledgerRef.current = loadLedger();
    takesRef.current = loadTakes();
    takeStatsRef.current = loadTakeStats();
  }, []);

  const raise = useCallback((event: string, cloudConnected = false) => {
    const c = ctxRef.current;
    const ctx: VisitContext = {
      meetingRecording: c.meetingQuiet,
      screenSharing: c.quiet.screenSharing,
      doNotDisturb: c.quiet.doNotDisturb,
      takesSoFar: takesRef.current,
      occupant: c.occupant,
      cloudConnected,
      chatter: c.chatter,
    };
    const d = decideVisit(event, ctx, ledgerRef.current, Date.now(), visitRef.current);
    console.info(visitLogLine(event, d));
    if (!d.show) return;
    ledgerRef.current = recordVisit(ledgerRef.current, d.visit);
    saveLedger(ledgerRef.current);
    visitRef.current = d.visit;
    setVisit(d.visit);
  }, []);

  // The events friends own, plus the quiet state.
  useEffect(() => {
    if (forced) return undefined;
    let dead = false;
    const unsubs: UnlistenFn[] = [];
    const push = (u: UnlistenFn) => (dead ? u() : unsubs.push(u));
    const fail = (name: string) => (e: unknown) =>
      console.warn(`pill friend_visit listen_failed event=${name} reason=${e instanceof Error ? e.name : "unknown"}`);
    listen<unknown>(BACKTRACK_CAUGHT_EVENT, () => raise("backtrack-caught")).then(push, fail(BACKTRACK_CAUGHT_EVENT));
    listen<unknown>(FRIEND_EVENT, (e) => {
      const p = e.payload;
      const o = p && typeof p === "object" ? (p as Record<string, unknown>) : null;
      const event = typeof p === "string" ? p : typeof o?.event === "string" ? o.event : "";
      raise(event, o?.cloudConnected === true);
    }).then(push, fail(FRIEND_EVENT));
    listen<unknown>(PILL_QUIET_EVENT, (e) => {
      const q = parseQuietState(e.payload);
      console.info(`pill friend_visit quiet screen_sharing=${q.screenSharing} dnd=${q.doNotDisturb}`);
      setQuiet(q);
    }).then(push, fail(PILL_QUIET_EVENT));
    // A pill that mounted after the last change reads the quiet state once.
    invoke<unknown>(PILL_QUIET_STATE)
      .then((v) => { if (!dead) setQuiet(parseQuietState(v)); })
      .catch((e: unknown) => console.warn(`pill friend_visit quiet_state_failed reason=${e instanceof Error ? e.name : "unknown"}`));
    const call = (view: unknown) => {
      const live = callLive(view);
      setInCall((was) => {
        if (was !== live) console.info(`pill friend_visit call live=${live}`);
        return live;
      });
    };
    listen<unknown>(CALL_OFFER_EVENT, (e) => call(e.payload)).then(push, fail(CALL_OFFER_EVENT));
    // A pill that mounted mid-call reads the live view once.
    invoke<unknown>(CALL_OFFER_STATE)
      .then((v) => { if (!dead) call(v); })
      .catch((e: unknown) => console.warn(`pill friend_visit call_state_failed reason=${e instanceof Error ? e.name : "unknown"}`));
    listen<unknown>("transcript", (e) => {
      takesRef.current += 1;
      saveTakes(takesRef.current);
      // yap-1106-main-character-switch — Yappy's milestones: the 100th take,
      // the longest take of the week, a new streak record. decideVisit keeps
      // him in the studio when he is the one sitting in the pill.
      const note = noteTake(takeStatsRef.current, takesRef.current, takeSpeechSeconds(e.payload), Date.now());
      takeStatsRef.current = note.stats;
      saveTakeStats(note.stats);
      if (note.milestone) {
        console.info(`pill yappy_milestone kind=${note.milestone} takes=${takesRef.current} streak=${note.stats.streak}`);
        raise(note.milestone);
      }
    }).then(push, fail("transcript"));
    return () => {
      dead = true;
      unsubs.splice(0).forEach((u) => u());
    };
  }, [forced, raise]);

  // Hoot's event: the notes turning ready (an edge, not a level).
  const notesWas = useRef(meetingNotesReady);
  useEffect(() => {
    const was = notesWas.current;
    notesWas.current = meetingNotesReady;
    if (meetingNotesReady && !was && !forced) raise("meeting-notes-ready");
  }, [meetingNotesReady, forced, raise]);

  // A meeting, a share or Do Not Disturb ends a visit at once; otherwise the
  // visit ends on its own clock (at most 4 s).
  const silent = quietReason({ meetingRecording: meetingQuiet, ...quiet }) !== null;
  useEffect(() => {
    if (!visit) return undefined;
    const end = () => { visitRef.current = null; setVisit(null); };
    if (silent) {
      console.info(`pill friend_visit ended friend=${visit.friend} reason=quiet`);
      end();
      return undefined;
    }
    const left = Math.min(visitRemainingMs(visit, Date.now()), VISIT_MAX_MS);
    const t = window.setTimeout(end, left);
    return () => clearTimeout(t);
  }, [visit, silent]);

  // Paint while on screen, and only then.
  // A pinned matrix visitor obeys the quiet rule too, so the meeting photo proves it.
  const who: VisitorId | null = silent ? null : forced ?? (visit ? visit.friend : null);
  useEffect(() => {
    const cv = canvasRef.current;
    if (!who || !cv) return undefined;
    const reduced = prefersReducedMotion();
    const t0 = performance.now();
    paintVisitor(cv, who, 0, reduced);
    if (reduced || forced) return undefined;
    const id = window.setInterval(() => paintVisitor(cv, who, performance.now() - t0, false), FRAME_MS);
    return () => clearInterval(id);
  }, [who, forced]);

  if (!who) return null;
  return (
    <div
      className="friend-visit"
      data-friend-visit={who}
      data-visit-event={visit && !forced ? visit.event : undefined}
      data-visit-line={visit && !forced ? milestoneLine(visit.event, takesRef.current) ?? undefined : undefined}
      aria-hidden="true"
    >
      <canvas ref={canvasRef} className="friend-visit-art" />
    </div>
  );
}
