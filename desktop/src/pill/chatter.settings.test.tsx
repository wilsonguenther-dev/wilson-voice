/**
 * yap-1106-chatter-setting — the "Yappy talks" row in Settings › Companion and
 * the wiring that carries it to the pill. The policy itself is chatter.test.ts.
 */
import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../appShell";
import Companion, { readTakesSeen } from "../views/settings/Companion";
import { chatterSentence, TAKES_SEEN_KEY } from "./chatter";

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);
const esc = (s: string) => s.replace(/'/g, "&#x27;");
const src = (rel: string): string => readFileSync(new URL(rel, import.meta.url), "utf8") as string;
const stubTakes = (value: string | null) =>
  vi.stubGlobal("localStorage", { getItem: (k: string) => (k === TAKES_SEEN_KEY ? value : null) });

afterEach(() => { vi.unstubAllGlobals(); });

describe("Settings → Yappy talks", () => {
  it("shows always / less over time / only milestones, with less over time chosen by default", () => {
    stubTakes(null);
    const html = render(<Companion />, { settings: {}, saveSettings: () => undefined });
    expect(html).toContain("Yappy talks");
    const at = ["Always", "Less over time", "Only milestones"].map((n) => html.indexOf(`<strong>${n}</strong>`));
    at.forEach((i) => expect(i).toBeGreaterThan(-1));
    expect([...at].sort((a, b) => a - b)).toEqual(at);
    expect(html).toMatch(/aria-pressed="true"[^>]*data-chatter="less"/);
    expect(html).toMatch(/aria-pressed="false"[^>]*data-chatter="always"/);
  });

  it("shows the one plain sentence with the take count that drives it next to it", () => {
    stubTakes("37");
    const html = render(<Companion />, { settings: { yappyTalks: "less" }, saveSettings: () => undefined });
    const sentence = esc(chatterSentence("less"));
    expect(html).toContain(sentence);
    expect(html).toContain("Takes so far: 37");
    expect(html).toContain("12 takes until he talks less.");
    // next to it: the count follows the sentence inside the same line
    const p = html.slice(html.indexOf("data-chatter-sentence"));
    expect(p.indexOf("Takes so far: 37")).toBeGreaterThan(p.indexOf("From your 50th take"));
    expect(p.indexOf("Takes so far: 37")).toBeLessThan(p.indexOf("</p>"));
  });

  it("past the 50th take it says Yappy is talking less now", () => {
    stubTakes("120");
    const html = render(<Companion />, { settings: { yappyTalks: "less" }, saveSettings: () => undefined });
    expect(html).toContain("Takes so far: 120");
    expect(html).toContain("Yappy is talking less now.");
  });

  it("each mode shows its own sentence and highlights itself", () => {
    stubTakes("5");
    for (const mode of ["always", "milestones"] as const) {
      const html = render(<Companion />, { settings: { yappyTalks: mode }, saveSettings: () => undefined });
      expect(html).toContain(esc(chatterSentence(mode)));
      expect(html).toMatch(new RegExp(`aria-pressed="true"[^>]*data-chatter="${mode}"`));
      expect(html).toContain("Takes so far: 5");
      expect(html).not.toContain("until he talks less");
    }
  });

  it("an unknown stored mode reads as less over time", () => {
    stubTakes("0");
    const html = render(<Companion />, { settings: { yappyTalks: "sometimes" }, saveSettings: () => undefined });
    expect(html).toMatch(/aria-pressed="true"[^>]*data-chatter="less"/);
  });

  it("reads the take count safely: missing, junk and a throwing store are 0", () => {
    expect(readTakesSeen(undefined)).toBe(0);
    expect(readTakesSeen({ getItem: () => "abc" })).toBe(0);
    expect(readTakesSeen({ getItem: () => "64" })).toBe(64);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    expect(readTakesSeen({ getItem: () => { throw new Error("denied"); } })).toBe(0);
    expect(warn).toHaveBeenCalledWith("settings chatter takes_read_failed", expect.any(Error));
    warn.mockRestore();
  });
});

describe("Yappy talks — wiring", () => {
  it("the pill reads the mode and applies the policy to bubbles, hops, after-lines and tips", () => {
    const pill = src("./YappyPill.tsx");
    expect(pill).toContain('from "./chatter"');
    expect(pill).not.toMatch(/const TAKES_SEEN_KEY\s*=/);
    for (const s of [
      "chatterPolicy(chatterMode, takesSeen + 1)",
      "shortenBubble(raw, policy.maxChars)",
      "J.power = p * policy.hopScale",
      "allowDuringLine(policy, crossed",
      "allowDoneLine(policy, takesSeen)",
      "allowAfterLine(policy, afterLines)",
      "isNewUser(takesSeen) && policy.firstTimeTips",
      "chatterLog(chatterMode, takesSeen + 1, policy)",
    ]) expect(pill).toContain(s);
  });

  it("float-main carries yappyTalks to the pill stage and the tip row", () => {
    const main = src("../float-main.tsx");
    expect(main).toContain("setChatter(parseChatterMode(s?.yappyTalks))");
    expect(main).toMatch(/<Stage [^>]*chatter=\{chatter\}/);
    expect(main).toMatch(/<NudgeRow [^>]*chatter=\{chatter\}/);
  });

  it("first-time tips stop in the tip row once the policy says so; milestones still show", () => {
    const row = src("./NudgeRow.tsx");
    expect(row).toContain("chatterPolicy(chatterRef.current, context.takesSoFar + 1).firstTimeTips");
    expect(row).toContain("(tipsOn ? nextNudge(context, shown) : null) ?? nextMilestone(context, shown)");
  });
});
