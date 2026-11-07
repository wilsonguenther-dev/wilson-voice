/**
 * yap-1106-meetings-redesign — the presentational pieces of the two-pane
 * Meetings view (YAP-UI-SPEC-2026-11 §3.3). No `invoke` in here: every piece
 * takes its data and its callbacks, so `MeetingsPanes.test.tsx` renders them
 * with renderToStaticMarkup. The decisions live in `meetingsLayout.ts`.
 */
import type { ReactNode } from "react";
import type { KindChoice } from "../appTypes";
import {
  DETAIL_TABS,
  NO_FILTER,
  filterActive,
  sunProgress,
  talkTimeLabel,
  undoSecondsLeft,
  type DetailTab,
  type MeetingFilter,
  type PendingDelete,
  type PersonRow,
} from "./meetingsLayout";

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

/** One people chip. `you` gets the accent so a person finds themself first. */
export function PeopleChips({ people }: { people: readonly string[] }) {
  if (people.length === 0) return null;
  return (
    <span className="people-chips" aria-label="People">
      {people.map((p) => (
        <span key={p} className={p === "You" ? "person-chip is-you" : "person-chip"}>
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
