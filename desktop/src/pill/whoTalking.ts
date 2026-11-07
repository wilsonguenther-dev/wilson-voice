/**
 * yap-1106-fb-22a — "Who's talking right now?", the live card's rules.
 *
 * OFF by default (Settings → Meetings → "Ask who's talking during meetings").
 * When on, `live_transcript.rs` raises one `meeting:live-ask` for a voice no
 * voiceprint claims once it has spoken about 5 s; this reducer decides what
 * the card under the pill shows. Pure and DOM-free so every rule is a test:
 *
 * - hidden when the setting is off, during a screen share, or after the meeting;
 * - a new voice with ≥ 5 s of speech → "Who's talking right now?";
 * - a voice under 70% like a known or just-named person → "Same person as
 *   John? 62% sure" with "No, someone new" / "Yes, John";
 * - Save → named, and a 2-second toast; Skip or 20 s with no input → that voice
 *   is deferred to the after-meeting cards (FB-22b);
 * - at most ONE question per voice per meeting;
 * - the card never takes focus (non-activating: you click it to type).
 *
 * Live names are provisional: the post-meeting pass is the source of truth
 * and keeps names given here.
 */

/** The event `live_transcript::LIVE_ASK_EVENT` names. */
export const WHO_ASK_EVENT = "meeting:live-ask";
/** The setting key on `AppSettings` (camelCase of `ask_whos_talking_live`). */
export const WHO_SETTING = "askWhosTalkingLive";
/** A voice is asked about after this much of its speech. */
export const WHO_MIN_SPEECH_S = 5;
/** An ignored card slides away after this long. */
export const WHO_IGNORE_MS = 20_000;
/** How long the "saved" toast stays. */
export const WHO_TOAST_MS = 2_000;
/** Under this similarity a known person is asked about, never assumed. */
export const WHO_SAME_BELOW = 0.7;
/** The card is non-activating: it never steals focus from the call. */
export const WHO_CARD_TAKES_FOCUS = false;

/** Mirrors `live_transcript::LiveAsk`. */
export type LiveAsk =
  | { kind: "new"; cluster: number; label: string; speechSeconds: number }
  | {
      kind: "samePerson";
      cluster: number;
      label: string;
      speechSeconds: number;
      name: string;
      similarity: number;
    };

/** What the card may show right now. */
export interface WhoContext {
  enabled: boolean;
  recording: boolean;
  screenSharing: boolean;
}

export type WhoCard =
  | { kind: "hidden" }
  | { kind: "ask"; cluster: number; label: string; shownAtMs: number }
  | { kind: "same"; cluster: number; label: string; name: string; percent: number; shownAtMs: number }
  | { kind: "toast"; cluster: number; text: string; untilMs: number };

export interface WhoState {
  card: WhoCard;
  /** Every voice asked about (or deferred) this meeting: never asked twice. */
  asked: number[];
  /** Voices deferred to the after-meeting cards. */
  deferred: number[];
  /** Names given live, by voice. */
  named: Record<number, string>;
  /** Questions that arrived while another card showed. */
  queue: LiveAsk[];
}

export const WHO_IDLE: WhoState = { card: { kind: "hidden" }, asked: [], deferred: [], named: {}, queue: [] };

export type WhoInput =
  | { type: "ask"; ask: LiveAsk; ctx: WhoContext; nowMs: number }
  | { type: "save"; name: string; nowMs: number }
  | { type: "skip"; nowMs: number }
  | { type: "same"; yes: boolean; nowMs: number }
  | { type: "tick"; ctx: WhoContext; nowMs: number }
  | { type: "reset" };

/** What the component must tell the backend. */
export type WhoEffect =
  | { kind: "name"; cluster: number; name: string }
  | { kind: "same"; cluster: number; yes: boolean }
  | { kind: "skip"; cluster: number; reason: "skip" | "timeout" | "screen_share" | "setting_off" };

export interface WhoStep {
  state: WhoState;
  effects: WhoEffect[];
}

/** Can a card show in this context at all? */
export function whoCanShow(ctx: WhoContext): boolean {
  return ctx.enabled && ctx.recording && !ctx.screenSharing;
}

/** "62% sure" — a whole percent, clamped. */
export function surePercent(similarity: number): number {
  if (!Number.isFinite(similarity)) return 0;
  return Math.max(0, Math.min(99, Math.round(similarity * 100)));
}

/** The toast after Save. Neutral wording: a typed first name says nothing about pronouns. */
export function savedToast(name: string): string {
  return `${name} · every line they said is now theirs`;
}

function cardFor(ask: LiveAsk, nowMs: number): WhoCard {
  if (ask.kind === "samePerson" && ask.similarity < WHO_SAME_BELOW) {
    return {
      kind: "same",
      cluster: ask.cluster,
      label: ask.label,
      name: ask.name,
      percent: surePercent(ask.similarity),
      shownAtMs: nowMs,
    };
  }
  return { kind: "ask", cluster: ask.cluster, label: ask.label, shownAtMs: nowMs };
}

function defer(s: WhoState, cluster: number): WhoState {
  return s.deferred.includes(cluster) ? s : { ...s, deferred: [...s.deferred, cluster] };
}

/** Show the next queued question, if the context allows it. */
function next(s: WhoState, ctx: WhoContext | null, nowMs: number): WhoState {
  if (!ctx || !whoCanShow(ctx) || s.queue.length === 0) return { ...s, card: { kind: "hidden" } };
  const [ask, ...rest] = s.queue;
  return { ...s, queue: rest, card: cardFor(ask, nowMs) };
}

export function reduceWho(s: WhoState, input: WhoInput): WhoStep {
  switch (input.type) {
    case "reset":
      return { state: WHO_IDLE, effects: [] };
    case "ask": {
      const { ask, ctx, nowMs } = input;
      // Never shown while off, during a screen share, or outside a meeting,
      // and never twice for one voice. A voice we could not ask about is
      // deferred to the after-meeting cards; it is not asked again later.
      if (s.asked.includes(ask.cluster) || s.named[ask.cluster] !== undefined) return { state: s, effects: [] };
      if (ask.speechSeconds < WHO_MIN_SPEECH_S) return { state: s, effects: [] };
      if (!whoCanShow(ctx)) {
        const reason = ctx.enabled ? "screen_share" : "setting_off";
        if (!ctx.enabled || !ctx.recording) return { state: s, effects: [] };
        return {
          state: defer({ ...s, asked: [...s.asked, ask.cluster] }, ask.cluster),
          effects: [{ kind: "skip", cluster: ask.cluster, reason }],
        };
      }
      const asked = [...s.asked, ask.cluster];
      if (s.card.kind === "ask" || s.card.kind === "same") {
        return { state: { ...s, asked, queue: [...s.queue, ask] }, effects: [] };
      }
      return { state: { ...s, asked, card: cardFor(ask, nowMs) }, effects: [] };
    }
    case "save": {
      if (s.card.kind !== "ask") return { state: s, effects: [] };
      const name = input.name.trim();
      if (name.length === 0) return { state: s, effects: [] };
      const cluster = s.card.cluster;
      return {
        state: {
          ...s,
          named: { ...s.named, [cluster]: name },
          card: { kind: "toast", cluster, text: savedToast(name), untilMs: input.nowMs + WHO_TOAST_MS },
        },
        effects: [{ kind: "name", cluster, name }],
      };
    }
    case "same": {
      if (s.card.kind !== "same") return { state: s, effects: [] };
      const { cluster, name } = s.card;
      if (input.yes) {
        return {
          state: {
            ...s,
            named: { ...s.named, [cluster]: name },
            card: { kind: "toast", cluster, text: savedToast(name), untilMs: input.nowMs + WHO_TOAST_MS },
          },
          effects: [{ kind: "same", cluster, yes: true }],
        };
      }
      // "No, someone new": a speaker of its own; ask who it is right away.
      return {
        state: { ...s, card: { kind: "ask", cluster, label: s.card.label, shownAtMs: input.nowMs } },
        effects: [{ kind: "same", cluster, yes: false }],
      };
    }
    case "skip": {
      if (s.card.kind !== "ask" && s.card.kind !== "same") return { state: s, effects: [] };
      const cluster = s.card.cluster;
      return {
        state: next(defer(s, cluster), null, input.nowMs),
        effects: [{ kind: "skip", cluster, reason: "skip" }],
      };
    }
    case "tick": {
      const { ctx, nowMs } = input;
      const c = s.card;
      if (c.kind === "toast") {
        return { state: nowMs >= c.untilMs ? next(s, ctx, nowMs) : s, effects: [] };
      }
      if (c.kind === "hidden") {
        return { state: s.queue.length > 0 ? next(s, ctx, nowMs) : s, effects: [] };
      }
      if (!whoCanShow(ctx)) {
        // A screen share started (or the setting went off) mid-question: the
        // card leaves at once and that voice waits for the after-meeting card.
        const reason = !ctx.enabled ? "setting_off" : "screen_share";
        return {
          state: { ...defer(s, c.cluster), card: { kind: "hidden" } },
          effects: [{ kind: "skip", cluster: c.cluster, reason }],
        };
      }
      if (nowMs - c.shownAtMs >= WHO_IGNORE_MS) {
        return {
          state: next(defer(s, c.cluster), ctx, nowMs),
          effects: [{ kind: "skip", cluster: c.cluster, reason: "timeout" }],
        };
      }
      return { state: s, effects: [] };
    }
  }
}

/** The card's words, one place (the component and the tests read these). */
export interface WhoCardView {
  title: string;
  primary: string;
  secondary: string;
  hasField: boolean;
  takesFocus: boolean;
}

export function whoCardView(card: WhoCard): WhoCardView | null {
  switch (card.kind) {
    case "hidden":
    case "toast":
      return null;
    case "ask":
      return { title: "Who's talking right now?", primary: "Save", secondary: "Skip", hasField: true, takesFocus: WHO_CARD_TAKES_FOCUS };
    case "same":
      return {
        title: `Same person as ${card.name}? ${card.percent}% sure`,
        primary: `Yes, ${card.name}`,
        secondary: "No, someone new",
        hasField: false,
        takesFocus: WHO_CARD_TAKES_FOCUS,
      };
  }
}

/** Parse one `meeting:live-ask` payload; anything malformed is dropped. */
export function parseLiveAsk(raw: unknown): LiveAsk | null {
  if (!raw || typeof raw !== "object") return null;
  const o = raw as Record<string, unknown>;
  const cluster = typeof o.cluster === "number" && Number.isInteger(o.cluster) && o.cluster >= 0 ? o.cluster : null;
  const speechSeconds = typeof o.speechSeconds === "number" ? o.speechSeconds : null;
  const label = typeof o.label === "string" ? o.label : null;
  if (cluster === null || speechSeconds === null || label === null) return null;
  if (o.kind === "new") return { kind: "new", cluster, label, speechSeconds };
  if (o.kind === "samePerson" && typeof o.name === "string" && typeof o.similarity === "number") {
    return { kind: "samePerson", cluster, label, speechSeconds, name: o.name, similarity: o.similarity };
  }
  return null;
}

/** A forced matrix scene: the card state and what the field shows. */
export interface ForcedWho {
  state: WhoState;
  name: string;
}

/**
 * The six frames of the approved Meetings page §A, for headless captures
 * (`?whoCard=new|typed|toast|another|same|skip`), dev/matrix builds only.
 */
export function forcedWhoCard(search: string): ForcedWho | null {
  const ask = (cluster: number, label: string): WhoState => ({
    ...WHO_IDLE,
    asked: [cluster],
    card: { kind: "ask", cluster, label, shownAtMs: Number.MAX_SAFE_INTEGER / 2 },
  });
  switch (new URLSearchParams(search).get("whoCard")) {
    case "new":
      return { state: ask(0, "Speaker 1"), name: "" };
    case "typed":
      return { state: ask(0, "Speaker 1"), name: "John" };
    case "toast":
      return {
        state: { ...WHO_IDLE, asked: [0], named: { 0: "John" }, card: { kind: "toast", cluster: 0, text: savedToast("John"), untilMs: Number.MAX_SAFE_INTEGER } },
        name: "",
      };
    case "another":
      return { state: ask(1, "Speaker 2"), name: "Maya" };
    case "same":
      return {
        state: {
          ...WHO_IDLE,
          asked: [2],
          card: { kind: "same", cluster: 2, label: "Speaker 3", name: "John", percent: 62, shownAtMs: Number.MAX_SAFE_INTEGER / 2 },
        },
        name: "",
      };
    case "skip":
      return { state: ask(3, "Speaker 4"), name: "" };
    default:
      return null;
  }
}
