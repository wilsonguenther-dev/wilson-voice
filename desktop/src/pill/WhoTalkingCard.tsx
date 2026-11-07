/**
 * yap-1106-fb-22a — the "Who's talking right now?" card under the pill.
 *
 * Off by default (Settings → Meetings → "Ask who's talking during meetings").
 * The rules live in `whoTalking.ts`; this file only wires them: the setting,
 * the screen-share quiet state, the backend's `meeting:live-ask`, and the
 * answers back (`meeting_live_name` / `meeting_live_same` / `meeting_live_skip`).
 *
 * It never takes focus: no auto-focus attribute, no focus call, nothing that activates
 * the pill — you click the field to type. One `setTimeout` to the next
 * deadline (20 s ignore, 2 s toast), never an interval.
 */
import { useEffect, useReducer, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { holdPillHitbox } from "./drag";
import { PILL_QUIET_EVENT, PILL_QUIET_STATE, parseQuietState } from "./friendVisits";
import { PILL_LAYOUT_EVENT } from "./state";
import {
  WHO_ASK_EVENT,
  WHO_IDLE,
  WHO_IGNORE_MS,
  parseLiveAsk,
  reduceWho,
  whoCardView,
  type ForcedWho,
  type WhoContext,
  type WhoEffect,
  type WhoInput,
  type WhoState,
} from "./whoTalking";

interface Held {
  state: WhoState;
  effects: WhoEffect[];
}

function heldReducer(h: Held, input: WhoInput): Held {
  const step = reduceWho(h.state, input);
  if (step.state === h.state && step.effects.length === 0) return h;
  return { state: step.state, effects: step.effects };
}

const reason = (e: unknown) => (e instanceof Error ? e.name : "unknown");

function send(effect: WhoEffect): void {
  const done = invokeEffect(effect);
  done.catch((e: unknown) =>
    console.warn(`pill who_talking answer_failed kind=${effect.kind} cluster=${effect.cluster} reason=${reason(e)}`),
  );
}

function invokeEffect(effect: WhoEffect): Promise<unknown> {
  console.info(`pill who_talking answer kind=${effect.kind} cluster=${effect.cluster}${effect.kind === "skip" ? ` reason=${effect.reason}` : ""}`);
  switch (effect.kind) {
    case "name":
      return invoke("meeting_live_name", { cluster: effect.cluster, name: effect.name });
    case "same":
      return invoke("meeting_live_same", { cluster: effect.cluster, yes: effect.yes });
    case "skip":
      return invoke("meeting_live_skip", { cluster: effect.cluster, reason: effect.reason });
  }
}

/** The bars of "that voice, now": a quiet CSS wave (still under reduce-motion). */
function VoiceWave() {
  return (
    <span className="who-wave" aria-hidden data-who-wave>
      {[0, 1, 2, 3, 4, 5, 6].map((i) => (
        <span key={i} className="who-wave-bar" style={{ animationDelay: `${i * 90}ms` }} />
      ))}
    </span>
  );
}

export default function WhoTalkingCard({
  recording,
  forced = null,
}: {
  recording: boolean;
  /** A matrix scene (`forcedWhoCard`): drawn as given, no timers, no backend. */
  forced?: ForcedWho | null;
}) {
  const [enabled, setEnabled] = useState(false);
  const [screenSharing, setScreenSharing] = useState(false);
  const [held, dispatch] = useReducer(heldReducer, { state: forced?.state ?? WHO_IDLE, effects: [] });
  const [name, setName] = useState(forced?.name ?? "");
  const isForced = forced !== null;
  const wrapRef = useRef<HTMLDivElement>(null);
  const ctxRef = useRef<WhoContext>({ enabled, recording, screenSharing });
  ctxRef.current = { enabled, recording, screenSharing };

  // The setting, the quiet state and the questions. Same late-listener guard
  // as every other subscription in the pill.
  useEffect(() => {
    if (isForced) return;
    let dead = false;
    const unsubs: UnlistenFn[] = [];
    const push = (u: UnlistenFn) => (dead ? u() : unsubs.push(u));
    const fail = (what: string) => (e: unknown) =>
      console.warn(`pill who_talking listen_failed event=${what} reason=${reason(e)}`);
    const readSetting = (raw: unknown) => {
      const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
      if (!dead) setEnabled(o.askWhosTalkingLive === true);
    };
    invoke<unknown>("get_settings").then(readSetting, fail("get_settings"));
    listen<unknown>("settings", (e) => readSetting(e.payload)).then(push, fail("settings"));
    invoke<unknown>(PILL_QUIET_STATE).then(
      (v) => { if (!dead) setScreenSharing(parseQuietState(v).screenSharing); },
      fail(PILL_QUIET_STATE),
    );
    listen<unknown>(PILL_QUIET_EVENT, (e) => setScreenSharing(parseQuietState(e.payload).screenSharing)).then(
      push,
      fail(PILL_QUIET_EVENT),
    );
    listen<unknown>(WHO_ASK_EVENT, (e) => {
      const ask = parseLiveAsk(e.payload);
      if (!ask) {
        console.warn("pill who_talking ask_dropped reason=malformed");
        return;
      }
      console.info(`pill who_talking ask kind=${ask.kind} cluster=${ask.cluster} enabled=${ctxRef.current.enabled}`);
      dispatch({ type: "ask", ask, ctx: ctxRef.current, nowMs: Date.now() });
    }).then(push, fail(WHO_ASK_EVENT));
    return () => {
      dead = true;
      unsubs.forEach((u) => u());
    };
  }, [isForced]);

  // The meeting ended: forget this meeting's voices.
  useEffect(() => {
    if (!recording && !isForced) dispatch({ type: "reset" });
  }, [recording, isForced]);

  // A context change (screen share, setting off) is applied at once.
  useEffect(() => {
    if (isForced) return;
    dispatch({ type: "tick", ctx: { enabled, recording, screenSharing }, nowMs: Date.now() });
  }, [enabled, recording, screenSharing, isForced]);

  // One timer to the card's next deadline.
  const card = held.state.card;
  useEffect(() => {
    if (card.kind === "hidden" || isForced) return;
    const due = card.kind === "toast" ? card.untilMs : card.shownAtMs + WHO_IGNORE_MS;
    const t = setTimeout(
      () => dispatch({ type: "tick", ctx: ctxRef.current, nowMs: Date.now() }),
      Math.max(0, due - Date.now()),
    );
    return () => clearTimeout(t);
  }, [card, isForced]);

  // Effects go to the backend exactly once per reducer step.
  useEffect(() => {
    if (!isForced) held.effects.forEach(send);
  }, [held, isForced]);

  // The card is clickable where it is drawn (the pill window is otherwise
  // click-through), the same hitbox hold every pill card uses.
  const visible = card.kind !== "hidden";
  useEffect(() => {
    if (!visible) return;
    const hold = () => {
      const el = wrapRef.current;
      if (!el || !el.isConnected) return;
      const r = el.getBoundingClientRect();
      holdPillHitbox({ x: r.left, y: r.top, w: r.width, h: r.height }, "who-talking");
    };
    hold();
    const ro = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => hold());
    if (wrapRef.current && ro) ro.observe(wrapRef.current);
    window.addEventListener(PILL_LAYOUT_EVENT, hold);
    console.info(`pill who_talking shown kind=${card.kind}`);
    return () => {
      ro?.disconnect();
      window.removeEventListener(PILL_LAYOUT_EVENT, hold);
      holdPillHitbox(null, "who-talking");
    };
  }, [visible, card.kind]);

  // A new question starts with an empty field.
  const cluster = card.kind === "hidden" ? null : card.cluster;
  useEffect(() => {
    if (!isForced) setName("");
  }, [cluster, card.kind, isForced]);

  if (card.kind === "toast") {
    return (
      <div ref={wrapRef} className="who-card-wrap">
        <div className="who-card who-toast" data-who-card="toast" role="status" aria-live="polite">
          {card.text}
        </div>
      </div>
    );
  }
  const view = whoCardView(card);
  if (!view || card.kind === "hidden") return null;
  const save = () => dispatch({ type: "save", name, nowMs: Date.now() });
  return (
    <div ref={wrapRef} className="who-card-wrap">
    <div
      className="who-card"
      data-who-card={card.kind}
      data-takes-focus={String(view.takesFocus)}
      role="group"
      aria-label={view.title}
      onKeyDown={(e) => {
        if (e.key === "Escape") dispatch({ type: "skip", nowMs: Date.now() });
      }}
    >
      <span className="who-card-title">
        {card.kind === "same" ? (
          <>
            Same person as {card.name}? <span className="who-card-sure">{card.percent}% sure</span>
          </>
        ) : (
          view.title
        )}
      </span>
      <VoiceWave />
      {view.hasField && (
        <input
          className="who-card-field"
          type="text"
          aria-label={`Name for ${card.label}`}
          placeholder="Type a name"
          maxLength={40}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
          }}
        />
      )}
      <div className="who-card-actions">
        <button
          type="button"
          className="who-card-secondary"
          onClick={() =>
            card.kind === "same"
              ? dispatch({ type: "same", yes: false, nowMs: Date.now() })
              : dispatch({ type: "skip", nowMs: Date.now() })
          }
        >
          {view.secondary}
        </button>
        <button
          type="button"
          className="who-card-primary"
          disabled={card.kind === "ask" && name.trim().length === 0}
          onClick={() => (card.kind === "same" ? dispatch({ type: "same", yes: true, nowMs: Date.now() }) : save())}
        >
          {view.primary}
        </button>
      </div>
    </div>
    </div>
  );
}
