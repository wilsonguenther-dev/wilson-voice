/**
 * yap-1106-settings-redesign (bounce fix, batch 2) — Settings › Meetings holds
 * call audio, kinds, retention, Speakers / voiceprints and destinations. The
 * section shell is pinned in settings.redesign.test.tsx; this renders the
 * REAL new blocks (static markup, no window).
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => []) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../../appShell";
import MeetingPrefs, { KIND_EXPLAINS } from "../settings/MeetingPrefs";
import Privacy from "../settings/Privacy";
import Meetings from "../settings/Meetings";
import privacySrc from "../settings/Privacy.tsx?raw";

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State = {}) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);

const KINDS = [
  { kind: "in_person", label: "In person", menuId: "meeting_start_in_person" },
  { kind: "virtual", label: "Virtual", menuId: "meeting_start_virtual" },
  { kind: "unknown", label: "Not sure", menuId: "meeting_start_unknown" },
];
const idle = { recording: false, elapsedSeconds: 0 };

describe("Settings › Meetings blocks", () => {
  it("Meeting kinds: a radio per published kind, the current one checked, with what it changes", () => {
    const html = render(<MeetingPrefs part="kinds" />, {
      meetingKind: "virtual",
      meetingKinds: KINDS,
      meetingStatus: idle,
    });
    expect(html).toContain("<h3>Meeting kinds</h3>");
    expect(html).toContain('role="radiogroup" aria-label="New meetings start as"');
    expect([...html.matchAll(/role="radio"/g)].length).toBe(3);
    expect(html).toMatch(/aria-checked="true" class="chip selected"[^>]*>Virtual</);
    expect(html).toContain(KIND_EXPLAINS.virtual.replace(/"/g, "&quot;"));
    expect(html).not.toContain("applies to the next one");
  });

  it("Meeting kinds: empty state before the kinds load, and a recording note while one records", () => {
    const empty = render(<MeetingPrefs part="kinds" />, {
      meetingKind: "unknown",
      meetingKinds: [],
      meetingStatus: { recording: true, elapsedSeconds: 12 },
    });
    expect(empty).toContain("data-meeting-kinds-empty");
    expect(empty).not.toContain('role="radiogroup"');
    expect(empty).toContain("applies to the next one");
  });

  it("Speakers / voiceprints: how speakers are labelled, and that no voiceprint is kept", () => {
    const html = render(<MeetingPrefs part="speakers" />);
    expect(html).toContain("<h3>Speakers</h3>");
    expect(html).toContain("Speaker 1");
    expect(html).toContain("<h3>Voiceprints</h3>");
    expect(html).toContain("data-voiceprints-empty");
  });

  it("Retention renders in Meetings; Models & privacy no longer repeats it", () => {
    const html = render(<Privacy part="retention" />);
    expect(html).toContain("<h3>Meeting audio retention</h3>");
    expect(html).toContain("deleted 7 days after the meeting started");
    // the sentence the Rust sweep test reads stays in Privacy.tsx, once
    const flat = privacySrc.split(/\s+/).join(" ");
    expect(flat.split("is deleted 7 days after the meeting started.").length).toBe(2);
    expect(privacySrc).toContain("{withRetention && <MeetingRetention />}");
  });

  it("destinations render on their own, without the call-audio panels", () => {
    const html = render(<Meetings part="exports" />, { settings: {} });
    expect(html).toContain("<h3>Meeting exports</h3>");
    expect(html).toContain('data-settings-pane="meeting-exports"');
    expect(html).not.toContain("Call audio");
    expect(html).not.toContain("Meeting detection");
  });

  it("never names an AI vendor or model", () => {
    const text = [
      render(<MeetingPrefs part="kinds" />, { meetingKind: "in_person", meetingKinds: KINDS, meetingStatus: idle }),
      render(<MeetingPrefs part="speakers" />),
      render(<Privacy part="retention" />),
    ].join(" ");
    expect(text).not.toMatch(/openai|gemini|whisper|parakeet|anthropic|claude|gpt|llama|qwen|mistral|nvidia|pyannote/i);
  });
});
