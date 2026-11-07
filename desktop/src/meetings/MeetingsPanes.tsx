/**
 * yap-1106-meetings-redesign — the presentational pieces of the two-pane
 * Meetings view (YAP-UI-SPEC-2026-11 §3.3). No `invoke` in here: every piece
 * takes its data and its callbacks, so `MeetingsPanes.test.tsx` renders them
 * with renderToStaticMarkup. The decisions live in `meetingsLayout.ts`.
 */
import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { KindChoice } from "../appTypes";
import {
  DETAIL_TABS,
  NO_FILTER,
  filterActive,
  fitChipCount,
  sunProgress,
  talkTimeLabel,
  undoSecondsLeft,
  type DetailTab,
  type MeetingFilter,
  type PendingDelete,
  type PersonRow,
} from "./meetingsLayout";

// ─── List pane: the title tooltip, only when the title is cut ────────────────

/**
 * yap-1107-meetings-row-no-stale-tooltip — Wilson (2026-11-07 12:47Z): a native
 * "Meeting 12:19 PM" tooltip floated over the NEXT row, from `title={m.title}`
 * on the whole row button. A tooltip that repeats text already on screen is
 * noise, and on a 60 px hit area it anchors wherever the pointer entered. So
 * the row carries no title attribute at all; the title SPAN gets one only
 * while the CSS ellipsis actually cuts it (scrollWidth > clientWidth). The
 * button's accessible name is its text content, which is always the full
 * title, so VoiceOver loses nothing either way.
 */
export const ROW_TITLE_SELECTOR = ".meeting-row-title";

/** The full title when the box cuts it, else null (no tooltip). Pure. */
export function truncatedTitle(text: string | null | undefined, scrollWidth: number, clientWidth: number): string | null {
  const full = (text ?? "").trim();
  if (!full) return null;
  if (!Number.isFinite(scrollWidth) || !Number.isFinite(clientWidth) || clientWidth <= 0) return null;
  return scrollWidth > clientWidth ? full : null;
}

/** Sets or clears ONE title span's tooltip from its measured box. Returns true when it is cut. */
export function syncTitleTooltip(el: HTMLElement): boolean {
  const full = truncatedTitle(el.textContent, el.scrollWidth, el.clientWidth);
  if (full) {
    if (el.getAttribute("title") !== full) el.setAttribute("title", full);
  } else if (el.hasAttribute("title")) {
    el.removeAttribute("title");
  }
  return full !== null;
}

/**
 * A callback ref for the list container: ONE ResizeObserver for the whole
 * list, observing every row's title span (no per-row observers, no polling).
 * It re-syncs on mount, on any title box resize (window / pane resize), when
 * the web font lands, and whenever `key` changes (rows or titles changed:
 * a longer title in an already-cut box does not resize it). `onCount` hears
 * (cut, total) only when the cut count changes — one telemetry line, never a
 * stream. React does not own the span's title attribute (the JSX never sets
 * one), so a re-render never fights this.
 */
export function useTruncatedTitles(
  key: string,
  onCount?: (truncated: number, total: number) => void,
): (root: HTMLElement | null) => void | (() => void) {
  const report = useRef(onCount);
  report.current = onCount;
  const last = useRef(-1);
  return useCallback(
    (root: HTMLElement | null) => {
      if (!root) return;
      const titles = () => Array.from(root.querySelectorAll<HTMLElement>(ROW_TITLE_SELECTOR));
      const tally = () => {
        const all = titles();
        const cut = all.filter((el) => el.hasAttribute("title")).length;
        if (cut !== last.current) {
          last.current = cut;
          report.current?.(cut, all.length);
        }
      };
      const syncAll = () => {
        for (const el of titles()) syncTitleTooltip(el);
        tally();
      };
      syncAll();
      let alive = true;
      void document.fonts?.ready.then(() => alive && syncAll()).catch(() => undefined);
      if (typeof ResizeObserver === "undefined") {
        return () => {
          alive = false;
        };
      }
      const ro = new ResizeObserver((entries) => {
        for (const e of entries) syncTitleTooltip(e.target as HTMLElement);
        tally();
      });
      for (const el of titles()) ro.observe(el);
      return () => {
        alive = false;
        ro.disconnect();
      };
    },
    [key],
  );
}

// ─── List pane: filters ──────────────────────────────────────────────────────

export function MeetingFilters({
  filter,
  kinds,
  people,
  onChange,
}: {
  filter: MeetingFilter;
  kinds: readonly KindChoice[];
  people: readonly string[];
  onChange: (next: MeetingFilter) => void;
}) {
  return (
    <div className="meetings-filters" role="group" aria-label="Filter meetings">
      <select
        aria-label="Kind of meeting"
        value={filter.kind}
        onChange={(e) => onChange({ ...filter, kind: e.target.value })}
      >
        <option value="all">Any kind</option>
        {kinds.map((k) => (
          <option key={k.kind} value={k.kind}>
            {k.label}
          </option>
        ))}
      </select>
      <select
        aria-label="Notes"
        value={filter.notes}
        onChange={(e) => onChange({ ...filter, notes: e.target.value as MeetingFilter["notes"] })}
      >
        <option value="all">With or without notes</option>
        <option value="with">Has notes</option>
        <option value="without">No notes yet</option>
      </select>
      {people.length > 0 && (
        <select
          aria-label="Person"
          value={filter.person ?? ""}
          onChange={(e) => onChange({ ...filter, person: e.target.value || null })}
        >
          <option value="">Anyone</option>
          {people.map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
      )}
      {filterActive(filter) && (
        <button type="button" className="ghost" onClick={() => onChange(NO_FILTER)}>
          Clear filters
        </button>
      )}
    </div>
  );
}

/**
 * yap-1107-meetings-row-whole-click-target — the row's open button fills the
 * whole card, and a row's status column (Retry / Cancel notes) sits in the same
 * grid cell on its right edge. This ref callback measures that column and puts
 * its width on the row as --row-status-w, which the button reserves as right
 * padding, so the text never runs under a status button and the card stays one
 * target edge to edge. React 19 calls the returned cleanup on detach.
 */
export function reserveStatusWidth(el: HTMLElement | null): (() => void) | undefined {
  const row = el?.parentElement;
  if (!el || !row) return undefined;
  const apply = () => row.style.setProperty("--row-status-w", `${Math.ceil(el.getBoundingClientRect().width)}px`);
  apply();
  if (typeof ResizeObserver === "undefined") return undefined;
  const watch = new ResizeObserver(apply);
  watch.observe(el);
  return () => {
    watch.disconnect();
    row.style.removeProperty("--row-status-w");
  };
}

function chipClass(name: string): string {
  return name === "You" ? "person-chip is-you" : "person-chip";
}

/** The "+N" chip's accessible label: how many, and who. */
export function moreChipLabel(hidden: readonly string[]): string {
  return `${hidden.length} more: ${hidden.join(", ")}`;
}

/**
 * yap-1107-meetings-row-chips-baseline — a list row's ONE chip line: the first
 * `shown` people as chips, the rest folded into a "+N" chip whose accessible
 * label names them (no title tooltip: yap-1107-meetings-row-no-stale-tooltip
 * keeps every title attribute off the row except an ellipsised title). Chips past `shown` stay in the DOM out of
 * flow and invisible (`is-overflow`) so `FitPeopleChips` can measure them on
 * the next resize; the "+N" chip is always rendered for the same reason.
 * Pure markup: `MeetingsPanes.test.tsx` renders it with renderToStaticMarkup.
 */
export function PeopleChipLine({ people, shown }: { people: readonly string[]; shown: number }) {
  const k = Math.max(0, Math.min(people.length, Math.floor(shown)));
  const hidden = people.slice(k);
  return (
    <>
      {people.map((p, i) => (
        <span
          key={p}
          className={i < k ? chipClass(p) : `${chipClass(p)} is-overflow`}
          data-person-chip={i}
          aria-hidden={i < k ? undefined : true}
        >
          {p}
        </span>
      ))}
      <span
        className={hidden.length > 0 ? "person-chip people-more" : "person-chip people-more is-overflow"}
        data-people-more={hidden.length}
        role={hidden.length > 0 ? "img" : undefined}
        aria-label={hidden.length > 0 ? moreChipLabel(hidden) : undefined}
        aria-hidden={hidden.length > 0 ? undefined : true}
      >
        {`+${Math.max(1, hidden.length)}`}
      </span>
    </>
  );
}

/**
 * The measured chip line: every chip gets its real width, and whatever does
 * not fit beside the state chip collapses into "+N" (never a second line).
 * Re-measures when the line's width or a chip's width changes (window or
 * pane resize, the web font arriving).
 * Without ResizeObserver (jsdom, SSR) it shows every chip — the CSS still
 * clips, so it can never wrap.
 */
function FitPeopleChips({ people }: { people: readonly string[] }) {
  const ref = useRef<HTMLSpanElement | null>(null);
  const [shown, setShown] = useState(people.length);
  const key = people.join("\u0000");
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const fit = () => {
      const chips = Array.from(el.querySelectorAll<HTMLElement>("[data-person-chip]"));
      const more = el.querySelector<HTMLElement>("[data-people-more]");
      const cs = getComputedStyle(el);
      const gap = parseFloat(cs.columnGap || cs.gap || "0");
      const next = fitChipCount(
        chips.map((c) => c.offsetWidth),
        gap,
        el.clientWidth,
        more ? more.offsetWidth : 0,
      );
      setShown((cur) => (cur === next ? cur : next));
    };
    fit();
    if (typeof ResizeObserver === "undefined") return;
    // The line AND every chip: a chip's width changes when the web font
    // lands, without the line's width changing.
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    for (const c of Array.from(el.querySelectorAll<HTMLElement>("[data-person-chip], [data-people-more]"))) ro.observe(c);
    return () => ro.disconnect();
  }, [key]);
  return (
    <span ref={ref} className="people-chips is-fit" aria-label="People" data-people-chips={people.length}>
      <PeopleChipLine people={people} shown={shown} />
    </span>
  );
}

/**
 * People chips. `You` gets the accent so a person finds themself first. The
 * detail header lets them wrap; a list row passes `fit` and gets one line
 * that overflows into "+N" (yap-1107-meetings-row-chips-baseline).
 */
export function PeopleChips({ people, fit = false }: { people: readonly string[]; fit?: boolean }) {
  if (people.length === 0) return null;
  if (fit) return <FitPeopleChips people={people} />;
  return (
    <span className="people-chips" aria-label="People">
      {people.map((p) => (
        <span key={p} className={chipClass(p)}>
          {p}
        </span>
      ))}
    </span>
  );
}

/** The list pane's loading state: six quiet rows at their final height (§3.3 Loading). */
export function ListSkeleton() {
  return (
    <ul className="meetings-list is-skeleton" aria-hidden data-loading-state="meetings:list">
      {Array.from({ length: 6 }, (_, i) => (
        <li key={i} className="meeting-row skeleton-row">
          <span className="skeleton-line wide" />
          <span className="skeleton-line" />
        </li>
      ))}
    </ul>
  );
}

// ─── Detail pane ─────────────────────────────────────────────────────────────

/** Two-pane width with nothing selected yet (§3.3 Empty, detail column). */
export function PickAMeeting({ count }: { count: number }) {
  return (
    <div className="meeting-pick" data-empty-state="meetings:detail">
      <h3>Pick a meeting</h3>
      <p className="tiny muted">
        {count === 1 ? "Your meeting is" : `Your ${count} meetings are`} on the left. ↑ and ↓ move, Return opens.
      </p>
    </div>
  );
}

export function DetailTabs({
  tab,
  counts,
  onTab,
}: {
  tab: DetailTab;
  counts: Partial<Record<DetailTab, number>>;
  onTab: (tab: DetailTab) => void;
}) {
  return (
    <div className="meeting-tabs" role="tablist" aria-label="Meeting sections">
      {DETAIL_TABS.map((t) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          id={`meeting-tab-${t.id}`}
          aria-selected={tab === t.id}
          aria-controls={`meeting-panel-${t.id}`}
          tabIndex={tab === t.id ? 0 : -1}
          title={t.keys}
          className={tab === t.id ? "meeting-tab selected" : "meeting-tab"}
          onClick={() => onTab(t.id)}
        >
          {t.label}
          {typeof counts[t.id] === "number" && <span className="meeting-tab-count">{counts[t.id]}</span>}
        </button>
      ))}
    </div>
  );
}

/**
 * The "Who's talking?" strip (§9) on top of the detail, shown only when a
 * voice still needs a name. Until voiceprints land it says what is true —
 * these lines are one unnamed label — and sends the person to People.
 */
export function WhoIsTalkingStrip({
  unnamed,
  onOpenPeople,
}: {
  unnamed: readonly PersonRow[];
  onOpenPeople: () => void;
}) {
  if (unnamed.length === 0) return null;
  const lines = unnamed.reduce((n, p) => n + p.lines, 0);
  return (
    <div className="who-strip" role="region" aria-label="Who's talking?">
      <strong>Who's talking?</strong>
      <span className="tiny">
        {lines} {lines === 1 ? "line is" : "lines are"} labelled “{unnamed[0].name}”: {unnamed[0].reason.toLowerCase()}.
      </span>
      <button type="button" className="ghost" onClick={onOpenPeople}>
        See people
      </button>
    </div>
  );
}

export function PeopleTab({ people }: { people: readonly PersonRow[] }) {
  if (people.length === 0) {
    return (
      <div className="empty" data-empty-state="meetings:people">
        <h3>No one yet</h3>
        <p>People show up here once there is a transcript to read them from.</p>
      </div>
    );
  }
  return (
    <ul className="people-list">
      {people.map((p) => (
        <li key={p.name} className="person-row">
          <span className={p.name === "You" ? "person-chip is-you" : "person-chip"}>{p.name}</span>
          <span className="person-stats tiny">
            {p.lines} {p.lines === 1 ? "line" : "lines"} · {talkTimeLabel(p.seconds)} · first at {p.firstOffset}
          </span>
          {/* Every guessed label carries its reason (APPROVED design rule). */}
          <span className="person-why tiny muted">Why this label: {p.reason}</span>
        </li>
      ))}
    </ul>
  );
}

// ─── Recording hero ──────────────────────────────────────────────────────────

/**
 * The recording state's own card (§3.3 Recording hero): red dot, title, the
 * sun-clock time, the call-audio line, the live transcript, Stop. `live` and
 * `callAudio` are passed in so this stays renderable without a window.
 */
export function RecordingHero({
  title,
  clock,
  elapsedSeconds,
  note,
  callAudio,
  live,
  busy,
  stopLabel = "Stop meeting",
  onStop,
}: {
  title: string;
  clock: string;
  elapsedSeconds: number;
  note: string | null;
  callAudio: ReactNode;
  live: ReactNode;
  busy: boolean;
  /** "Stop meeting", or the second-press wording when Stop asks first. */
  stopLabel?: string;
  onStop: () => void;
}) {
  const sun = sunProgress(elapsedSeconds);
  return (
    <section className="recording-hero" role="status" aria-label="Recording a meeting" data-recording-hero>
      <div className="recording-hero-head">
        <span className="rec-dot" aria-hidden />
        <div className="recording-hero-title">
          <strong>{title || "Recording a meeting"}</strong>
          <span className="tiny">Stop from here, the menu bar, ⌃⌘M, or the pill.</span>
        </div>
        <div className="sun-clock" aria-label={`Recording for ${clock}`}>
          <span className="sun-track" aria-hidden>
            <span className="sun" style={{ left: `${Math.round(sun * 100)}%` }} />
          </span>
          <span className="rec-clock">{clock}</span>
        </div>
        <button type="button" className="primary danger" disabled={busy} onClick={onStop}>
          {stopLabel}
        </button>
      </div>
      {note && <p className="tiny rec-sysaudio-note">{note}</p>}
      {callAudio}
      <div className="recording-hero-live">{live}</div>
    </section>
  );
}

// ─── Undo bar ────────────────────────────────────────────────────────────────

/** "Meeting deleted." + Undo for 10 s (§3.3 Delete); the files go only after it. */
export function UndoDeleteBar({
  pending,
  now,
  onUndo,
}: {
  pending: readonly PendingDelete[];
  now: number;
  onUndo: (id: string) => void;
}) {
  if (pending.length === 0) return null;
  return (
    <div className="undo-bar" role="status" aria-live="polite">
      {pending.map((p) => (
        <div key={p.id} className="undo-row" data-undo-delete={p.id}>
          <span>
            “{p.title}” deleted. <span className="tiny muted">{undoSecondsLeft(p, now)} s to undo</span>
          </span>
          <button type="button" className="ghost" onClick={() => onUndo(p.id)}>
            Undo
          </button>
        </div>
      ))}
    </div>
  );
}
