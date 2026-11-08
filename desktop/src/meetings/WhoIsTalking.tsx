/**
 * yap-1106-who-is-this — the "Who was this?" cards on a finished meeting
 * (spec §9; approved design meetings.html §B, 2026-11-06).
 *
 * Approved sentences kept: "Who was this?", "Not sure", "Save", "Same person
 * as John?" (Yes / No), "N lines relabeled", "Left as Speaker 3. Rename any
 * time from the transcript.", "N voices to name" / "All set". At most five
 * cards, most speaking time first, a 3-second clip, never asked about you,
 * asked only AFTER the meeting (never during a recording).
 *
 * The card state machine is pure ([`cardReducer`]) so vitest pins it:
 * unknown → named / me / ignored / not-sure, with saving and error between.
 * yap-1106-fb-22b-who-was-this-cards (the approved after-meeting flow, which
 * SUPERSEDES the parts of yap-1106-who-is-this that differ): the card is
 * ▶ clip · “line” · "Who was this?" · name field · "Not sure" · "Save" —
 * never "That's me" (you are never asked about yourself). A voice under 70%
 * to a saved one reads "Same person as John? 62% sure" with "No, someone new"
 * / "Yes, John". States: unknown → named / not-sure / merged / new. A saved
 * card turns green: "John ✓ · 14 lines relabeled".
 */
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useMemo, useReducer, useState } from "react";
import PeopleStepper, { type People } from "./PeopleStepper";
import type { LabelRow } from "./transcript";

/** One card, as `meeting_voice_cards` sends it (`speaker_profiles::VoiceCard`). */
export interface VoiceCard {
  track: number;
  clusterIndex: number;
  seconds: number;
  lines: number;
  line: string;
  clipStart: number | null;
  clipEnd: number | null;
  suggestion: { profileId: number; displayName: string; similarity: number } | null;
}

/** `speaker_profiles::NameTarget`. */
export type NameTarget =
  | { kind: "profile"; profileId: number }
  | { kind: "newName"; name: string }
  | { kind: "me" };

export type CardPhase =
  | "unknown"
  | "saving"
  | "named"
  /** "Yes, John": the voice joined an existing voiceprint. */
  | "merged"
  /** "No, someone new": asks for a name, as a new speaker. */
  | "new"
  | "me"
  | "notSure"
  | "ignored"
  | "error";

export interface CardState {
  phase: CardPhase;
  /** What is typed in the name field. */
  name: string;
  /** "No" on "Same person as Alex?" — show the plain name card instead. */
  declinedSuggestion: boolean;
  /** The save in flight is a "Yes, John" merge. */
  merging?: boolean;
  /** Set once named: the name saved and how many lines moved. */
  savedName?: string;
  relabelled?: number;
  error?: string;
}

export type CardEvent =
  | { type: "type"; name: string }
  | { type: "save" }
  | { type: "me" }
  | { type: "yes" }
  | { type: "no" }
  | { type: "notSure" }
  | { type: "ignore" }
  | { type: "saved"; name: string; relabelled: number; me: boolean }
  | { type: "failed"; error: string };

export const INITIAL_CARD: CardState = { phase: "unknown", name: "", declinedSuggestion: false };

/** Phases the person can still act on. */
export function isOpen(state: CardState): boolean {
  return state.phase === "unknown" || state.phase === "new" || state.phase === "error";
}

/** Phases that end the card. */
export function isDone(state: CardState): boolean {
  return (
    state.phase === "named" ||
    state.phase === "merged" ||
    state.phase === "me" ||
    state.phase === "notSure" ||
    state.phase === "ignored"
  );
}

/**
 * The card state machine. Terminal phases ignore everything; "Save" with an
 * empty name does nothing (the button is disabled too); a failure keeps the
 * typed name and says so, and the card can be retried.
 */
export function cardReducer(state: CardState, event: CardEvent): CardState {
  if (isDone(state)) return state;
  if (state.phase === "saving") {
    if (event.type === "saved") {
      return {
        ...state,
        phase: event.me ? "me" : state.merging ? "merged" : "named",
        merging: false,
        savedName: event.name,
        relabelled: event.relabelled,
        error: undefined,
      };
    }
    if (event.type === "failed") return { ...state, phase: "error", merging: false, error: event.error };
    return state;
  }
  switch (event.type) {
    case "type":
      return { ...state, name: event.name };
    case "save":
      return state.name.trim() === "" ? state : { ...state, phase: "saving", merging: false, error: undefined };
    case "me":
      return { ...state, phase: "saving", merging: false, error: undefined };
    case "yes":
      return { ...state, phase: "saving", merging: true, error: undefined };
    case "no":
      // "No, someone new": a new speaker, asked for a name like any card.
      return { ...state, phase: "new", declinedSuggestion: true, error: undefined };
    case "notSure":
      return { ...state, phase: "notSure", error: undefined };
    case "ignore":
      return { ...state, phase: "ignored", error: undefined };
    case "failed":
      return { ...state, phase: "error", error: event.error };
    default:
      return state;
  }
}

/** "3 voices to name" / "1 voice to name" / "All set". */
export function headline(open: number): string {
  if (open <= 0) return "All set";
  return `${open} ${open === 1 ? "voice" : "voices"} to name`;
}

/** The once-per-meeting nudge: "2 new voices in Launch sync, name them". */
export function nudgeText(count: number, title: string): string {
  return `${count} new ${count === 1 ? "voice" : "voices"} in ${title}, name ${count === 1 ? "it" : "them"}`;
}

const NUDGE_KEY = "yap.whoIsThis.nudged.";

/** Nudge once per meeting, never while recording, only when there are cards. */
export function shouldNudge(
  meetingId: string,
  cards: number,
  recording: boolean,
  seen: (key: string) => boolean,
): boolean {
  return cards > 0 && !recording && !seen(NUDGE_KEY + meetingId);
}

/** "Probably Alex — voice match 0.82": a suggestion with its reason, never a fact. */
export function suggestionReason(s: { displayName: string; similarity: number }): string {
  return `Probably ${s.displayName} — voice match ${s.similarity.toFixed(2)}`;
}

/** "Same person as John? 62% sure" — the reason is the match, rounded. */
export function sameAsText(s: { displayName: string; similarity: number }): string {
  const pct = Math.max(0, Math.min(99, Math.round(s.similarity * 100)));
  return `Same person as ${s.displayName}? ${pct}% sure`;
}

/** "John ✓ · 14 lines relabeled" — what a saved card reads. */
export function doneText(state: Pick<CardState, "savedName" | "relabelled">): string {
  const n = state.relabelled ?? 0;
  return `${state.savedName ?? ""} ✓ · ${n} ${n === 1 ? "line" : "lines"} relabeled`;
}

/**
 * What an answer writes, decided in one pure place so vitest pins it:
 * Save / Yes write a name (and the voice) through `name_cluster`; "Not sure"
 * keeps NOTHING — no name, no voiceprint; it only tells the backend to drop
 * the stored voice and stop asking (`ignore_cluster`, ignore = false).
 */
export type Answer =
  | { command: "name_cluster"; target: NameTarget }
  | { command: "ignore_cluster"; ignore: boolean }
  | null;

export function answerFor(event: CardEvent, state: CardState, card: Pick<VoiceCard, "suggestion">): Answer {
  switch (event.type) {
    case "save": {
      const name = state.name.trim();
      return name ? { command: "name_cluster", target: { kind: "newName", name } } : null;
    }
    case "yes":
      return card.suggestion
        ? { command: "name_cluster", target: { kind: "profile", profileId: card.suggestion.profileId } }
        : null;
    case "notSure":
      return { command: "ignore_cluster", ignore: false };
    case "ignore":
      return { command: "ignore_cluster", ignore: true };
    default:
      return null;
  }
}

const RECENT_KEY = "yap.whoIsThis.recentNames";
const RECENT_MAX = 8;

/**
 * The name field's suggestions: calendar attendees (when the calendar is on),
 * then saved voices, then names typed recently. Case-insensitive dedupe;
 * "You" is never offered (you are never asked about yourself).
 */
export function nameSuggestions(attendees: string[], voices: string[], recent: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of [...attendees, ...voices, ...recent]) {
    const name = raw.trim();
    const key = name.toLowerCase();
    if (!name || key === "you" || seen.has(key)) continue;
    seen.add(key);
    out.push(name);
  }
  return out;
}

/** The recent-names list after saving `name` (newest first, capped). */
export function pushRecent(recent: string[], name: string): string[] {
  const clean = name.trim();
  if (!clean) return recent;
  return [clean, ...recent.filter((r) => r.toLowerCase() !== clean.toLowerCase())].slice(0, RECENT_MAX);
}

function readRecent(): string[] {
  try {
    const v = JSON.parse(window.localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function cardKey(c: Pick<VoiceCard, "track" | "clusterIndex">): string {
  return `${c.track}:${c.clusterIndex}`;
}

function logWho(event: string, fields: Record<string, string | number | boolean> = {}): void {
  const kv = Object.entries(fields)
    .map(([k, v]) => `${k}=${v}`)
    .join(" ");
  console.info(`meetings who_is_this ${event}${kv ? " " + kv : ""}`);
}

interface Props {
  meetingId: string;
  title: string;
  /** "Speaker N" for a card, as the transcript labels it. */
  labelFor: (track: number, clusterIndex: number) => string;
  audioOnDisk: boolean;
  retentionDays: number;
  /** A recording is running: never ask now. */
  recording: boolean;
  /** Saved names to merge into the transcript (`[segment id, name]`). */
  onNames: (names: [string, string][]) => void;
  toast: (message: string) => void;
  /** Calendar attendees of this meeting, when the calendar is on. */
  attendees?: string[];
  /** yap-1106-fb-23 — distinct voices in the transcript ("4 voices"). */
  voiceCount?: number | null;
  /** yap-1106-fb-23 — the lines after a People change regrouped the voices. */
  onLabelRows?: (rows: LabelRow[]) => void;
}

type RecapReply = {
  outcome: { kind: "applied"; voices: number; merged: number } | { kind: "needsConfirm"; from: string; into: string };
  rows: LabelRow[];
};

/** Each card's own state lives in the parent so the headline can count. */
type Cards = Record<string, CardState>;

function cardsReducer(state: Cards, action: { key: string; event: CardEvent } | { reset: string[] }): Cards {
  if ("reset" in action) return Object.fromEntries(action.reset.map((k) => [k, INITIAL_CARD]));
  return { ...state, [action.key]: cardReducer(state[action.key] ?? INITIAL_CARD, action.event) };
}

export default function WhoIsTalking({
  meetingId,
  title,
  labelFor,
  audioOnDisk,
  retentionDays,
  recording,
  onNames,
  toast,
  attendees = [],
  voiceCount = null,
  onLabelRows,
}: Props) {
  // yap-1106-fb-23 — "4 voices · People − 4 +": changing the count after the
  // meeting re-runs only the merge on the stored voices (no transcription, no
  // embedding) and relabels the lines; two named people merge only after
  // "Merge Maya into John?".
  const [people, setPeople] = useState<People>(null);
  const [mergeAsk, setMergeAsk] = useState<{ from: string; into: string; count: People } | null>(null);
  const [regrouping, setRegrouping] = useState(false);
  useEffect(() => {
    let live = true;
    invoke<number | null>("meeting_expected_speakers", { meetingId })
      .then((n) => live && setPeople(n ?? null))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [meetingId]);
  async function regroup(count: People, confirmMerge: boolean) {
    setRegrouping(true);
    try {
      const reply = await invoke<RecapReply>("meeting_recap_speakers", { meetingId, count, confirmMerge });
      if (reply.outcome.kind === "needsConfirm") {
        setMergeAsk({ from: reply.outcome.from, into: reply.outcome.into, count });
        logWho("people_merge_asked");
        return;
      }
      setMergeAsk(null);
      setPeople(count);
      if (reply.rows.length) onLabelRows?.(reply.rows);
      logWho("people_regrouped");
    } catch {
      toast("Couldn't regroup the voices. Try again.");
      logWho("people_regroup_failed");
    } finally {
      setRegrouping(false);
    }
  }
  const [cards, setCards] = useState<VoiceCard[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [states, dispatch] = useReducer(cardsReducer, {});
  const [voices, setVoices] = useState<string[]>([]);
  const [purged, setPurged] = useState(!audioOnDisk);
  const [recent, setRecent] = useState<string[]>(() => readRecent());

  useEffect(() => {
    let live = true;
    setCards(null);
    setLoadError(null);
    setPurged(!audioOnDisk);
    if (recording) return;
    invoke<VoiceCard[]>("meeting_voice_cards", { meetingId })
      .then((c) => {
        if (!live) return;
        setCards(c);
        dispatch({ reset: c.map(cardKey) });
        logWho("cards", { n: c.length });
        const seen = (k: string) => {
          try {
            return window.localStorage.getItem(k) === "1";
          } catch {
            return false;
          }
        };
        if (shouldNudge(meetingId, c.length, recording, seen)) {
          toast(nudgeText(c.length, title));
          try {
            window.localStorage.setItem(NUDGE_KEY + meetingId, "1");
          } catch {
            /* the nudge may repeat; nothing else depends on it */
          }
        }
      })
      .catch((e) => {
        if (!live) return;
        setLoadError(String(e));
        logWho("cards_failed");
      });
    invoke<[string, string][]>("meeting_speaker_names", { meetingId })
      .then((n) => live && n.length > 0 && onNames(n))
      .catch(() => logWho("names_failed"));
    invoke<{ displayName: string; isMe: boolean }[]>("list_voices")
      .then((v) => live && setVoices(v.filter((p) => !p.isMe).map((p) => p.displayName)))
      .catch(() => logWho("voices_failed"));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [meetingId, recording, audioOnDisk]);

  const open = useMemo(
    () => (cards ?? []).filter((c) => isOpen(states[cardKey(c)] ?? INITIAL_CARD) || states[cardKey(c)]?.phase === "saving").length,
    [cards, states],
  );

  if (recording || loadError || !cards || cards.length === 0) {
    // Errors stay quiet here: the strip is a suggestion, the transcript still reads.
    return null;
  }

  async function name(card: VoiceCard, target: NameTarget, event: CardEvent) {
    const key = cardKey(card);
    dispatch({ key, event });
    try {
      const r = await invoke<{ displayName: string; relabelled: number; sampleAdded: boolean }>("name_cluster", {
        meetingId,
        track: card.track,
        clusterIndex: card.clusterIndex,
        target,
      });
      dispatch({ key, event: { type: "saved", name: r.displayName, relabelled: r.relabelled, me: target.kind === "me" } });
      if (target.kind === "newName") {
        const next = pushRecent(recent, r.displayName);
        setRecent(next);
        try {
          window.localStorage.setItem(RECENT_KEY, JSON.stringify(next));
        } catch {
          /* suggestions only; nothing depends on it */
        }
      }
      logWho("named", { target: target.kind, relabelled: r.relabelled, sample: r.sampleAdded });
      const names = await invoke<[string, string][]>("meeting_speaker_names", { meetingId });
      onNames(names);
    } catch (e) {
      dispatch({ key, event: { type: "failed", error: "Couldn't save that name. Try again." } });
      logWho("name_failed", { target: target.kind });
      toast(`Couldn't save that name: ${String(e)}`);
    }
  }

  async function dismiss(card: VoiceCard, ignore: boolean) {
    const key = cardKey(card);
    dispatch({ key, event: { type: ignore ? "ignore" : "notSure" } });
    try {
      await invoke("ignore_cluster", { meetingId, track: card.track, clusterIndex: card.clusterIndex, ignore });
      logWho(ignore ? "ignored" : "not_sure");
    } catch {
      logWho("dismiss_failed", { ignore });
      toast("Couldn't save that answer. It will ask again next time you open this meeting.");
    }
  }

  async function play(card: VoiceCard) {
    if (card.clipStart === null || card.clipEnd === null) return;
    try {
      const b64 = await invoke<string>("meeting_cluster_clip", {
        meetingId,
        track: card.track,
        start: card.clipStart,
        end: card.clipEnd,
      });
      await new Audio(`data:audio/wav;base64,${b64}`).play();
      logWho("clip_played");
    } catch (e) {
      if (String(e).includes("audio_purged")) setPurged(true);
      else toast("Couldn't play that clip.");
      logWho("clip_failed");
    }
  }

  const listId = `who-names-${meetingId}`;
  return (
    <section className="who-cards" role="region" aria-label="Who was this?">
      <header className="who-cards-head">
        <strong>{headline(open)}</strong>
        {!recording && (
          <PeopleStepper
            value={people}
            voices={voiceCount}
            attendees={attendees.length ? attendees.length : null}
            disabled={regrouping}
            onChange={(next) => void regroup(next, false)}
          />
        )}
        {mergeAsk && (
          <span className="people-merge-ask" role="alertdialog" aria-label={`Merge ${mergeAsk.from} into ${mergeAsk.into}?`}>
            Merge {mergeAsk.from} into {mergeAsk.into}?
            <button type="button" onClick={() => void regroup(mergeAsk.count, true)} disabled={regrouping}>
              Merge
            </button>
            <button type="button" onClick={() => setMergeAsk(null)} disabled={regrouping}>
              Keep both
            </button>
          </span>
        )}
        {purged && (
          <span className="tiny muted">
            The audio was deleted after {retentionDays} days, so there are no clips. You can still name each voice from its
            line.
          </span>
        )}
      </header>
      <datalist id={listId}>
        {nameSuggestions(attendees, voices, recent).map((v) => (
          <option key={v} value={v} />
        ))}
      </datalist>
      <ul className="who-card-list">
        {cards.map((card) => {
          const key = cardKey(card);
          const st = states[key] ?? INITIAL_CARD;
          const speaker = labelFor(card.track, card.clusterIndex);
          if (st.phase === "named" || st.phase === "merged" || st.phase === "me") {
            return (
              <li key={key} className="who-card is-done is-saved" data-phase={st.phase} role="status">
                <strong>{doneText(st)}</strong>
              </li>
            );
          }
          if (st.phase === "notSure" || st.phase === "ignored") {
            return (
              <li key={key} className="who-card is-done" data-phase={st.phase}>
                <strong>
                  {speaker} · {st.phase === "notSure" ? "not sure" : "ignored"}
                </strong>
                <span className="tiny muted">Left as {speaker}. Rename any time from the transcript.</span>
              </li>
            );
          }
          const saving = st.phase === "saving";
          const ask = card.suggestion && !st.declinedSuggestion ? card.suggestion : null;
          return (
            <li key={key} className="who-card" data-phase={st.phase} aria-busy={saving}>
              <div className="who-card-clip">
                {!purged && card.clipStart !== null && card.clipEnd !== null ? (
                  <button
                    type="button"
                    className="ghost"
                    onClick={() => void play(card)}
                    aria-label={`Play a ${Math.round(card.clipEnd - card.clipStart)}-second clip of ${speaker}`}
                  >
                    ▶ 0:0{Math.max(1, Math.round(card.clipEnd - card.clipStart))}
                  </button>
                ) : null}
                <span className="who-card-line">“{card.line}”</span>
              </div>
              {ask ? (
                <div className="who-card-ask">
                  <strong>{sameAsText(ask)}</strong>
                  <div className="who-card-actions">
                    <button
                      type="button"
                      className="ghost"
                      disabled={saving}
                      onClick={() => {
                        dispatch({ key, event: { type: "no" } });
                        logWho("someone_new");
                      }}
                    >
                      No, someone new
                    </button>
                    <button
                      type="button"
                      disabled={saving}
                      onClick={() => void name(card, { kind: "profile", profileId: ask.profileId }, { type: "yes" })}
                    >
                      Yes, {ask.displayName}
                    </button>
                  </div>
                </div>
              ) : (
                <form
                  className="who-card-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (st.name.trim()) void name(card, { kind: "newName", name: st.name.trim() }, { type: "save" });
                  }}
                >
                  <label className="who-card-label" htmlFor={`who-${meetingId}-${key}`}>
                    Who was this?
                  </label>
                  <input
                    id={`who-${meetingId}-${key}`}
                    list={listId}
                    value={st.name}
                    placeholder="Name"
                    autoComplete="off"
                    disabled={saving}
                    onChange={(e) => dispatch({ key, event: { type: "type", name: e.target.value } })}
                  />
                  <div className="who-card-actions">
                    <button type="button" className="ghost" disabled={saving} onClick={() => void dismiss(card, false)}>
                      Not sure
                    </button>
                    <button type="submit" disabled={saving || st.name.trim() === ""}>
                      Save
                    </button>
                  </div>
                </form>
              )}
              {st.phase === "error" && st.error ? (
                <p className="tiny who-card-error" role="alert">
                  {st.error}
                </p>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
