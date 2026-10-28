/**
 * yap-byom-0 — Settings → Model providers, rendered without a window
 * (`renderToStaticMarkup`, like meetingOffer.test.tsx).
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import {
  ModelProvidersView,
  type ProviderView,
  type ProvidersView,
  testLine,
} from "./ModelProviders";

const bundled: ProviderView = {
  id: "bundled",
  kind: "bundled",
  label: "Bundled model (on this Mac)",
  baseUrl: "",
  model: "",
  contextTokens: 4096,
  timeoutS: 60,
  leaves: false,
  leavesLabel: "No",
  leavesHost: null,
  keySaved: false,
  keyHint: "",
  deletable: false,
};

const openrouter: ProviderView = {
  id: "openrouter",
  kind: "open_ai_compatible",
  label: "OpenRouter",
  baseUrl: "https://openrouter.ai/api/v1",
  model: "openai/gpt-4o-mini",
  contextTokens: 4096,
  timeoutS: 60,
  leaves: true,
  leavesLabel: "Yes — openrouter.ai",
  leavesHost: "openrouter.ai",
  keySaved: true,
  keyHint: "WXYZ",
  deletable: true,
};

const noop = () => {};
const render = (view: ProvidersView, tests = {}) =>
  renderToStaticMarkup(
    <ModelProvidersView view={view} tests={tests} onTest={noop} onPick={noop} onDelete={noop} />,
  );

describe("ModelProviders", () => {
  it("labels every row with Leaves this Mac and shows both pickers", () => {
    const html = render({
      providers: [bundled],
      notesProvider: "",
      polishProvider: "",
      wiredKinds: ["bundled"],
      activeLeaves: [],
    });
    expect(html).toContain("Leaves this Mac: No");
    expect(html).toContain("Notes use");
    expect(html).toContain("Polish uses");
    expect(html).toContain("sends the meeting transcript");
    expect(html).toContain("sends every dictation");
    expect(html).toContain("Test connection");
    expect(html).not.toContain(">Delete<"); // Bundled cannot be deleted
  });

  it("hides kinds this build has not wired — no placeholder", () => {
    const html = render({
      providers: [bundled, openrouter],
      notesProvider: "",
      polishProvider: "",
      wiredKinds: ["bundled"],
      activeLeaves: [],
    });
    expect(html).not.toContain("OpenRouter");
    expect(html).not.toContain("OpenAI-compatible");
    expect(html).not.toMatch(/coming/i);
  });

  it("never renders more of a key than its last four, and names where text goes", () => {
    const html = render({
      providers: [bundled, openrouter],
      notesProvider: "openrouter",
      polishProvider: "",
      wiredKinds: ["bundled", "open_ai_compatible"],
      activeLeaves: [{ task: "notes", host: "openrouter.ai" }],
    });
    expect(html).toContain("Key saved ••••WXYZ");
    expect(html).not.toMatch(/sk-[A-Za-z0-9]/);
    expect(html).toContain("Leaves this Mac: Yes — openrouter.ai");
    expect(html).toContain("Notes go to openrouter.ai");
  });

  it("shows a finished test's latency and reply, or its error sentence", () => {
    expect(
      testLine({ providerId: "b", ok: true, latencyMs: 812, reply: "OK", error: null, keyHint: "" }),
    ).toBe("Connected in 812 ms — replied “OK”");
    expect(
      testLine({
        providerId: "b",
        ok: false,
        latencyMs: 40,
        reply: null,
        error: "redirected to evil.example — not followed",
        keyHint: "",
      }),
    ).toContain("redirected to evil.example — not followed");
  });
});
