/**
 * yap-1106-fb-22a — the live "Who's talking right now?" card state machine.
 * Every acceptance rule is one `it`.
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error the repo carries no @types/node; vitest runs under node
import { readFileSync } from "node:fs";
import {
  WHO_CARD_TAKES_FOCUS,
  WHO_IDLE,
  WHO_IGNORE_MS,
  WHO_MIN_SPEECH_S,
  WHO_TOAST_MS,
  forcedWhoCard,
  parseLiveAsk,
  surePercent,
  reduceWho,
  savedToast,
  whoCardView,
  type LiveAsk,
  type WhoContext,
  type WhoInput,
  type WhoState,
} from "./whoTalking";

const ON: WhoContext = { enabled: true, recording: true, screenSharing: false };
const OFF: WhoContext = { enabled: false, recording: true, screenSharing: false };
const SHARING: WhoContext = { enabled: true, recording: true, screenSharing: true };

const fresh = (cluster: number, speechSeconds = 5.2): LiveAsk => ({
  kind: "new",
  cluster,
  label: `Speaker ${cluster + 1}`,
  speechSeconds,
});
const same = (cluster: number, similarity: number): LiveAsk => ({
  kind: "samePerson",
  cluster,
  label: `Speaker ${cluster + 1}`,
  speechSeconds: 6,
  name: "John",
  similarity,
});

function run(inputs: WhoInput[], from: WhoState = WHO_IDLE) {
  let state = from;
  const effects = [] as ReturnType<typeof reduceWho>["effects"];
  for (const i of inputs) {
    const step = reduceWho(state, i);
    state = step.state;
    effects.push(...step.effects);
  }
  return { state, effects };
}

describe("who's talking — live card", () => {
  it("is hidden when the setting is off (the default)", () => {
    const { state, effects } = run([{ type: "ask", ask: fresh(1), ctx: OFF, nowMs: 0 }]);
    expect(state.card.kind).toBe("hidden");
    expect(effects).toEqual([]);
  });

  it("a full fixture meeting with the setting off shows zero live cards", () => {
    let shown = 0;
    let state = WHO_IDLE;
    for (let t = 0; t < 60; t++) {
      const inputs: WhoInput[] = [
        { type: "ask", ask: fresh(t % 5, 5 + t), ctx: OFF, nowMs: t * 10_000 },
        { type: "ask", ask: same(t % 5, 0.62), ctx: OFF, nowMs: t * 10_000 + 1 },
        { type: "tick", ctx: OFF, nowMs: t * 10_000 + 5_000 },
      ];
      for (const i of inputs) {
        state = reduceWho(state, i).state;
        if (whoCardView(state.card) || state.card.kind === "toast") shown++;
      }
    }
    expect(shown).toBe(0);
  });

  it("asks about a new voice once it has spoken 5 s, not before", () => {
    const early = run([{ type: "ask", ask: fresh(1, WHO_MIN_SPEECH_S - 0.5), ctx: ON, nowMs: 0 }]);
    expect(early.state.card.kind).toBe("hidden");
    const { state } = run([{ type: "ask", ask: fresh(1, WHO_MIN_SPEECH_S), ctx: ON, nowMs: 0 }]);
    expect(state.card.kind).toBe("ask");
    expect(whoCardView(state.card)?.title).toBe("Who's talking right now?");
    expect(whoCardView(state.card)?.hasField).toBe(true);
  });

  it("Save names the voice, tells the backend, and shows a 2-second toast", () => {
    const { state, effects } = run([
      { type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 },
      { type: "save", name: "  John ", nowMs: 1_000 },
    ]);
    expect(state.named[1]).toBe("John");
    expect(effects).toEqual([{ kind: "name", cluster: 1, name: "John" }]);
    expect(state.card).toEqual({ kind: "toast", cluster: 1, text: savedToast("John"), untilMs: 1_000 + WHO_TOAST_MS });
    expect(savedToast("John")).toBe("John · every line they said is now theirs");
    const later = reduceWho(state, { type: "tick", ctx: ON, nowMs: 1_000 + WHO_TOAST_MS });
    expect(later.state.card.kind).toBe("hidden");
  });

  it("an empty name saves nothing", () => {
    const { state, effects } = run([
      { type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 },
      { type: "save", name: "   ", nowMs: 10 },
    ]);
    expect(state.card.kind).toBe("ask");
    expect(effects).toEqual([]);
  });

  it("Skip defers the voice to the after-meeting cards", () => {
    const { state, effects } = run([
      { type: "ask", ask: fresh(2), ctx: ON, nowMs: 0 },
      { type: "skip", nowMs: 500 },
    ]);
    expect(state.card.kind).toBe("hidden");
    expect(state.deferred).toEqual([2]);
    expect(effects).toEqual([{ kind: "skip", cluster: 2, reason: "skip" }]);
  });

  it("20 s with no input slides the card away and defers the voice", () => {
    const before = run([
      { type: "ask", ask: fresh(2), ctx: ON, nowMs: 0 },
      { type: "tick", ctx: ON, nowMs: WHO_IGNORE_MS - 1 },
    ]);
    expect(before.state.card.kind).toBe("ask");
    const { state, effects } = run([{ type: "tick", ctx: ON, nowMs: WHO_IGNORE_MS }], before.state);
    expect(state.card.kind).toBe("hidden");
    expect(state.deferred).toEqual([2]);
    expect(effects).toEqual([{ kind: "skip", cluster: 2, reason: "timeout" }]);
  });

  it("under 70% like a known person asks 'Same person as John?'", () => {
    const { state } = run([{ type: "ask", ask: same(3, 0.62), ctx: ON, nowMs: 0 }]);
    const view = whoCardView(state.card);
    expect(view?.title).toBe("Same person as John? 62% sure");
    expect(view?.primary).toBe("Yes, John");
    expect(view?.secondary).toBe("No, someone new");
    expect(view?.hasField).toBe(false);
  });

  it("'Yes, John' merges; 'No, someone new' asks who it is", () => {
    const yes = run([
      { type: "ask", ask: same(3, 0.62), ctx: ON, nowMs: 0 },
      { type: "same", yes: true, nowMs: 100 },
    ]);
    expect(yes.effects).toEqual([{ kind: "same", cluster: 3, yes: true }]);
    expect(yes.state.named[3]).toBe("John");
    expect(yes.state.card.kind).toBe("toast");
    const no = run([
      { type: "ask", ask: same(3, 0.62), ctx: ON, nowMs: 0 },
      { type: "same", yes: false, nowMs: 100 },
    ]);
    expect(no.effects).toEqual([{ kind: "same", cluster: 3, yes: false }]);
    expect(no.state.card.kind).toBe("ask");
    expect(no.state.named[3]).toBeUndefined();
  });

  it("asks at most once per voice per meeting", () => {
    const { state } = run([
      { type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 },
      { type: "skip", nowMs: 100 },
      { type: "ask", ask: fresh(1, 30), ctx: ON, nowMs: 60_000 },
      { type: "ask", ask: same(1, 0.6), ctx: ON, nowMs: 70_000 },
    ]);
    expect(state.card.kind).toBe("hidden");
    const named = run([
      { type: "ask", ask: fresh(4), ctx: ON, nowMs: 0 },
      { type: "save", name: "Ana", nowMs: 10 },
      { type: "tick", ctx: ON, nowMs: 10 + WHO_TOAST_MS },
      { type: "ask", ask: fresh(4, 40), ctx: ON, nowMs: 90_000 },
    ]);
    expect(named.state.card.kind).toBe("hidden");
    // A new meeting starts over.
    const again = run([{ type: "reset" }, { type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 }], state);
    expect(again.state.card.kind).toBe("ask");
  });

  it("a second voice waits its turn behind the first card", () => {
    const { state } = run([
      { type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 },
      { type: "ask", ask: fresh(2), ctx: ON, nowMs: 10 },
      { type: "skip", nowMs: 20 },
      { type: "tick", ctx: ON, nowMs: 30 },
    ]);
    expect(state.card.kind).toBe("ask");
    expect(state.card.kind === "ask" && state.card.cluster).toBe(2);
  });

  it("never shows during a screen share; one that starts mid-question hides it", () => {
    const sharing = run([{ type: "ask", ask: fresh(1), ctx: SHARING, nowMs: 0 }]);
    expect(sharing.state.card.kind).toBe("hidden");
    expect(sharing.state.deferred).toEqual([1]);
    expect(sharing.effects).toEqual([{ kind: "skip", cluster: 1, reason: "screen_share" }]);
    const mid = run([
      { type: "ask", ask: fresh(2), ctx: ON, nowMs: 0 },
      { type: "tick", ctx: SHARING, nowMs: 1_000 },
    ]);
    expect(mid.state.card.kind).toBe("hidden");
    expect(mid.effects).toEqual([{ kind: "skip", cluster: 2, reason: "screen_share" }]);
  });

  it("never takes focus: no autofocus in the card, and the view says so", () => {
    expect(WHO_CARD_TAKES_FOCUS).toBe(false);
    const { state } = run([{ type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 }]);
    expect(whoCardView(state.card)?.takesFocus).toBe(false);
    const src = readFileSync(new URL("./WhoTalkingCard.tsx", import.meta.url), "utf8") as string;
    expect(src).not.toMatch(/autoFocus/);
    expect(src).not.toMatch(/\.focus\(/);
  });

  it("forces each of the six design frames for headless captures, and nothing else", () => {
    const frames = ["new", "typed", "toast", "another", "same", "skip"].map((f) => forcedWhoCard(`?whoCard=${f}`));
    expect(frames.map((f) => f?.state.card.kind)).toEqual(["ask", "ask", "toast", "ask", "same", "ask"]);
    expect(frames[1]?.name).toBe("John");
    expect(frames[3]?.name).toBe("Maya");
    expect(whoCardView(frames[4]!.state.card)?.title).toBe("Same person as John? 62% sure");
    expect(forcedWhoCard("")).toBeNull();
    expect(forcedWhoCard("?whoCard=nope")).toBeNull();
  });

  it("rounds the sure percent and never shows 100% or a broken number", () => {
    expect(surePercent(0.624)).toBe(62);
    expect(surePercent(1.2)).toBe(99);
    expect(surePercent(-0.3)).toBe(0);
    expect(surePercent(Number.NaN)).toBe(0);
  });

  it("answers only the card that is showing", () => {
    expect(reduceWho(WHO_IDLE, { type: "save", name: "John", nowMs: 0 }).effects).toEqual([]);
    expect(reduceWho(WHO_IDLE, { type: "same", yes: true, nowMs: 0 }).effects).toEqual([]);
    expect(reduceWho(WHO_IDLE, { type: "skip", nowMs: 0 }).effects).toEqual([]);
    expect(reduceWho(WHO_IDLE, { type: "tick", ctx: ON, nowMs: 0 }).state).toBe(WHO_IDLE);
    const asking = reduceWho(WHO_IDLE, { type: "ask", ask: fresh(1), ctx: ON, nowMs: 0 }).state;
    expect(reduceWho(asking, { type: "same", yes: true, nowMs: 1 }).effects).toEqual([]);
    expect(reduceWho(asking, { type: "tick", ctx: OFF, nowMs: 1 }).effects).toEqual([{ kind: "skip", cluster: 1, reason: "setting_off" }]);
    const notRecording = { ...ON, recording: false };
    expect(reduceWho(WHO_IDLE, { type: "ask", ask: fresh(1), ctx: notRecording, nowMs: 0 }).state).toBe(WHO_IDLE);
    const toast = reduceWho(asking, { type: "save", name: "Jo", nowMs: 0 }).state;
    expect(reduceWho(toast, { type: "tick", ctx: ON, nowMs: 1 }).state).toBe(toast);
  });

  it("parses the backend's ask and drops malformed payloads", () => {
    expect(parseLiveAsk({ kind: "new", cluster: 1, label: "Speaker 2", speechSeconds: 5.1 })).toEqual(fresh(1, 5.1));
    expect(
      parseLiveAsk({ kind: "samePerson", cluster: 3, label: "Speaker 4", speechSeconds: 6, name: "John", similarity: 0.62 }),
    ).toEqual(same(3, 0.62));
    expect(parseLiveAsk({ kind: "new", cluster: -1, label: "x", speechSeconds: 5 })).toBeNull();
    expect(parseLiveAsk(null)).toBeNull();
    expect(parseLiveAsk({ kind: "other", cluster: 1, label: "x", speechSeconds: 5 })).toBeNull();
  });
});
