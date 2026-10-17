/**
 * Y5-B review fixes: the SHIPPING views, rendered.
 *
 * `viewState.test.ts` pins the pure decision. The three defects the review
 * found (#192) were only visible in the views that call it, so these cases
 * render the real components inside AppContext and read the markup, the same
 * way `meetings/TranscriptList.test.tsx` does (`renderToStaticMarkup`, no DOM,
 * no new dependency).
 */
import { describe, expect, it, vi } from "vitest";
import type { ComponentProps, ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

// Record the EmptyState's action so a test can press the real button handler.
const pressed: { onAction?: () => void } = {};
vi.mock("../../ViewStates", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../ViewStates")>();
  return {
    ...actual,
    EmptyState: (props: ComponentProps<typeof actual.EmptyState>) => {
      pressed.onAction = props.onAction;
      return actual.EmptyState(props);
    },
  };
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));

import { AppContext, type AppCtx } from "../../appShell";
import Scratchpad from "../Scratchpad";
import Home from "../Home";
import Insights from "../Insights";

type State = Record<string, unknown>;

/** A ctx whose `setX` writes into `state.x`, so a pressed handler changes the next render. */
function makeCtx(state: State): AppCtx {
  return new Proxy(state, {
    get(target, key: string) {
      if (key in target) return target[key];
      if (/^set[A-Z]/.test(key)) {
        const field = key[3].toLowerCase() + key.slice(4);
        return (v: unknown) => {
          target[field] = v;
        };
      }
      return () => undefined;
    },
  }) as unknown as AppCtx;
}

function render(node: ReactElement, state: State): string {
  return renderToStaticMarkup(
    <AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>,
  );
}

const insightsRow = (over: State = {}) => ({
  totalWords: 0, totalSessions: 0, wordsToday: 0, sessionsToday: 0, avgWpm: 0,
  streakDays: 0, longestStreak: 0, wordsLast7: [], topApps: [], avgAsrSeconds: 0,
  ...over,
});

describe("Scratchpad: the first note on a fresh install", () => {
  const fresh = (): State => ({
    scratch: [], activeNoteId: null, noteTitle: "Scratchpad", noteBody: "",
    scratchComposing: false, booting: false, bootError: null,
  });

  it("shows the empty state before anything is asked for", () => {
    const html = render(<Scratchpad />, fresh());
    expect(html).toContain('data-empty-state="scratchpad"');
    expect(html).not.toContain("<textarea");
  });

  it("pressing the empty state's 'New note' mounts the editor", () => {
    const state = fresh();
    render(<Scratchpad />, state);
    expect(pressed.onAction).toBeTypeOf("function");
    pressed.onAction!();
    const html = render(<Scratchpad />, state);
    expect(html).toContain("<textarea");
    expect(html).toContain('class="note-title"');
    expect(html).not.toContain('data-empty-state="scratchpad"');
  });

  it("a draft sent from Home keeps its editor when its body is cleared", () => {
    const state = { ...fresh(), noteTitle: "From 9:00", noteBody: "", scratchComposing: true };
    expect(render(<Scratchpad />, state)).toContain("<textarea");
  });
});

describe("Home: no model gate over the history (YV54)", () => {
  const entry = (id: string, text: string) => ({
    id, text, backend: "native", asrSeconds: 0.4, wordCount: 3, createdAt: "2026-10-01T09:00:00Z",
  });
  it("renders existing history when the selected model is not on disk", () => {
    const html = render(<Home />, {
      booting: false, bootError: null, history: [entry("a", "first take here"), entry("b", "second take here")],
      failed: [], feedbackEdits: {}, insights: insightsRow({ totalSessions: 2 }), query: "",
      queryRef: { current: null }, settings: null, diffId: null, fixingId: null, fixDraft: "",
      retrying: null, clearing: false,
      status: { modelReady: false, recording: false, busy: false },
    });
    expect(html).not.toContain('data-empty-state="home:model"');
    expect(html).toContain("first take here");
    expect(html).toContain("second take here");
  });
});

describe("Insights: meetings without dictations is not empty", () => {
  const base = {
    booting: false, bootError: null, daily30: [], hasActivity: false,
    heat: { cols: 0, months: [], cells: [], max: 0 }, heatSummary: { words: 0, sessions: 0, best: null },
    monthlySeries: [], monthlyView: { quiet: 0, rows: [], max: 1 }, maxApp: 1, maxDaily30: 1, maxWeek: 1,
    settings: null, status: { hotkeys: { carbon: { state: "registered" }, fnTap: { state: "delivering" } }, accessibility: true },
  };
  const meetings = {
    totalMeetings: 2, meetingsLast7: 1, totalSeconds: 1200, completeMeetings: 2, partialMeetings: 0,
    failedMeetings: 0, segmentsIndexed: 40, meetingsWithAudio: 2, audioRetentionDays: 30,
  };
  it("shows the meeting strip for zero dictations and two meetings", () => {
    const html = render(<Insights />, { ...base, insights: insightsRow({ meetings }) });
    expect(html).not.toContain('data-empty-state="insights"');
    expect(html).toContain("meeting-strip");
  });
  it("is empty only when there are no dictations and no meetings", () => {
    const html = render(<Insights />, { ...base, insights: insightsRow({ meetings: { ...meetings, totalMeetings: 0 } }) });
    expect(html).toContain('data-empty-state="insights"');
  });
});
