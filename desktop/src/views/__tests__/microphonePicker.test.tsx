/**
 * WF-09 — Settings › Audio › Microphone: "System Default" first, every input
 * device after it, the saved pick selected, and one plain sentence saying what
 * the next take records from (including the unplugged-pick fallback).
 */
import { describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined), emit: vi.fn(async () => undefined) }));

import { AppContext, type AppCtx } from "../../appShell";
import Audio from "../settings/Audio";
import MicrophonePicker, {
  MICROPHONES_CHANGED,
  micHint,
  systemDefaultLabel,
  type MicList,
} from "../settings/MicrophonePicker";

type State = Record<string, unknown>;
const makeCtx = (state: State): AppCtx =>
  new Proxy(state, {
    get: (t, k: string) => (k in t ? t[k] : () => undefined),
  }) as unknown as AppCtx;
const render = (node: ReactElement, state: State) =>
  renderToStaticMarkup(<AppContext.Provider value={makeCtx(state)}>{node}</AppContext.Provider>);

const builtIn = { id: "coreaudio:Built-in", name: "MacBook Pro Microphone", isSystemDefault: true };
const mv7 = { id: "coreaudio:MV7", name: "Shure MV7", isSystemDefault: false };
const list = (over: Partial<MicList>): MicList => ({
  devices: [builtIn, mv7],
  chosenId: null,
  chosenName: null,
  chosenMissing: false,
  inUseId: builtIn.id,
  inUseName: builtIn.name,
  ...over,
});

describe("WF-09 microphone picker", () => {
  it("system_default_is_first_and_names_the_device_it_means", () => {
    expect(systemDefaultLabel(list({}))).toBe("System Default (MacBook Pro Microphone)");
    expect(systemDefaultLabel(list({ devices: [mv7] }))).toBe("System Default");
  });

  it("says_which_device_the_next_take_uses", () => {
    expect(micHint(list({ chosenId: mv7.id, chosenName: mv7.name, inUseId: mv7.id, inUseName: mv7.name }))).toBe(
      "Your next take records from Shure MV7.",
    );
  });

  it("an_unplugged_pick_falls_back_to_the_default_and_says_so", () => {
    const hint = micHint(list({ devices: [builtIn], chosenId: mv7.id, chosenName: mv7.name, chosenMissing: true }));
    expect(hint).toContain("Shure MV7 isn't connected");
    expect(hint).toContain("system default");
  });

  it("no_device_is_an_empty_state_not_an_error", () => {
    expect(micHint(list({ devices: [], inUseId: null, inUseName: null }))).toContain("No microphone is connected");
  });

  it("listens_for_the_backend_device_watcher_event", () => {
    expect(MICROPHONES_CHANGED).toBe("microphones-changed");
  });

  it("renders_a_loading_state_first_and_sits_in_the_audio_tab", () => {
    const html = render(<MicrophonePicker />, { toast: () => undefined, toastError: () => undefined });
    expect(html).toContain("data-mic-picker");
    expect(html).toContain("Looking for microphones");
    const audio = render(<Audio />, { settings: {}, saveSettings: () => undefined });
    expect(audio).toContain("data-mic-picker");
  });
});
