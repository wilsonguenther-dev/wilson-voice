/**
 * yap-byom-0 — Settings → Model providers, rendered without a window
 * (`renderToStaticMarkup`, like meetingOffer.test.tsx).
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import {
  kindLine,
  ModelProvidersView,
  type ProviderView,
  type ProvidersView,
  testLine,
} from "./ModelProviders";

// The same pattern vendorStrings.test.ts bans from user-visible source text.
// That scan cannot see values that arrive from Rust at runtime, so the
// provider rows are checked here as rendered.
const VENDORS =
  /\b(anthropic|claude|openai|open ai|chatgpt|gpt-?\d|gpt|sonnet|opus|haiku|gemini|mistral|llama|ollama|openrouter|groq|deepseek|qwen)\b/i;

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

// The cloud kind as list_model_providers returns it: its model field is
// cloud_route::CLEANUP_MODEL. The leaves host is left out here — today it is
// the cloud API's own host, a separate disclosure question flagged to the
// loop, not this row's model id.
const cloudKey: ProviderView = {
  id: "cloud",
  kind: "cloud_key",
  label: "Cloud model (your key)",
  baseUrl: "",
  model: "claude-sonnet-5-5",
  contextTokens: 200_000,
  timeoutS: 60,
  leaves: true,
  leavesLabel: "Yes",
  leavesHost: null,
  keySaved: true,
  keyHint: "WXYZ",
  deletable: false,
};

// The sentence ProviderError::CloudModelNotFound.plain() returns (model_providers.rs).
const CLOUD_404 = "the cloud did not recognise the model this key is set to use (404)";

/** The visible text of one rendered provider row (tags stripped). */
function rowText(html: string, id: string): string {
  const start = html.indexOf(`data-provider="${id}"`);
  expect(start).toBeGreaterThan(-1);
  const end = html.indexOf("</li>", start);
  return html
    .slice(start, end)
    .replace(/^[^>]*>/, "")
    .replace(/<[^>]+>/g, " ");
}

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

  it("never shows the cloud model id on the cloud row or its Test connection error", () => {
    const html = render(
      {
        providers: [bundled, cloudKey],
        notesProvider: "",
        polishProvider: "",
        wiredKinds: ["bundled", "cloud_key"],
        activeLeaves: [],
      },
      {
        cloud: {
          running: false,
          result: {
            providerId: "cloud",
            ok: false,
            latencyMs: 120,
            reply: null,
            error: CLOUD_404,
            keyHint: "WXYZ",
          },
        },
      },
    );
    const row = rowText(html, "cloud");
    expect(row).toContain("Cloud model (your key)");
    expect(row).toContain("Failed after 120 ms");
    expect(row).not.toContain("claude-sonnet-5-5");
    expect(row).not.toMatch(VENDORS);
    expect(html).not.toMatch(VENDORS);
    expect(kindLine(cloudKey)).toBe("Cloud model (your key)");
  });

  it("keeps a custom endpoint's own model string on its row", () => {
    expect(kindLine(openrouter)).toBe("Custom endpoint · openai/gpt-4o-mini");
    expect(kindLine(bundled)).toBe("Bundled");
  });
});
