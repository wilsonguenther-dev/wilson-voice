// yap-1106-fb-14-news-quote-key — Settings → Yappy News → Advanced: the
// optional own quote-feed key. Pure helpers, the invoke contract, and the
// panel rendered statically (free tier and a saved key).
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));

import { invoke } from "@tauri-apps/api/core";
import { NewsQuoteKeyPanel } from "./NewsQuoteKey";
import { NewsSettingsView } from "../views/settings/News";
import {
  EMPTY_PREFS,
  QUOTE_KEY_LEAVES_LABEL,
  clearQuoteKey,
  initialQuoteProvider,
  loadQuoteKey,
  quoteTierLine,
  saveQuoteKey,
  type NewsView,
  type QuoteKeyView,
} from "./newsPrefs";

const PROVIDERS: QuoteKeyView["providers"] = [
  { id: "finnhub", label: "Finnhub", host: "finnhub.io", refreshLine: "Free plan: 60 calls a minute." },
  { id: "alpha_vantage", label: "Alpha Vantage", host: "www.alphavantage.co", refreshLine: "Free plan: 25 calls a day." },
];

const free: QuoteKeyView = {
  provider: null,
  saved: false,
  hint: "",
  tier: "free",
  leavesLabel: QUOTE_KEY_LEAVES_LABEL,
  providers: PROVIDERS,
  error: null,
};
const saved: QuoteKeyView = { ...free, provider: "alpha_vantage", saved: true, hint: "WXYZ", tier: "alpha_vantage" };
const asyncNoop = async () => true;
const noop = () => undefined;

describe("quote key helpers", () => {
  it("free tier needs no key", () => {
    expect(quoteTierLine(free)).toMatch(/No key needed/);
    expect(initialQuoteProvider(free)).toBe("finnhub");
  });

  it("a saved key names the provider, its limit and only the last four", () => {
    const line = quoteTierLine(saved);
    expect(line).toContain("Alpha Vantage");
    expect(line).toContain("ending WXYZ");
    expect(line).toContain("25 calls a day");
    expect(initialQuoteProvider(saved)).toBe("alpha_vantage");
  });

  it("uses the three backend commands", async () => {
    const mock = vi.mocked(invoke);
    mock.mockClear();
    await loadQuoteKey();
    await saveQuoteKey("finnhub", "abc12345");
    await clearQuoteKey();
    expect(mock.mock.calls).toEqual([
      ["get_news_quote_key"],
      ["set_news_quote_key", { provider: "finnhub", key: "abc12345" }],
      ["clear_news_quote_key"],
    ]);
  });
});

describe("Advanced panel", () => {
  it("free: both providers, the leaves-this-Mac label, no remove button", () => {
    const html = renderToStaticMarkup(
      <NewsQuoteKeyPanel view={free} busy={false} onSave={asyncNoop} onClear={noop} />,
    );
    expect(html).toContain('data-tier="free"');
    expect(html).toContain("Finnhub");
    expect(html).toContain("Alpha Vantage");
    expect(html).toContain("Leaves this Mac: only to that provider");
    expect(html).toContain("finnhub.io");
    expect(html).toContain('type="password"');
    expect(html).not.toContain("data-news-quote-clear");
  });

  it("saved: shows the hint and a way to remove the key", () => {
    const html = renderToStaticMarkup(
      <NewsQuoteKeyPanel view={saved} busy={false} onSave={asyncNoop} onClear={noop} />,
    );
    expect(html).toContain("Key saved, ending WXYZ");
    expect(html).toContain("data-news-quote-clear");
    expect(html).toContain("Replace key");
    expect(html).toContain("www.alphavantage.co");
  });

  it("a Keychain read error is one sentence on screen", () => {
    const html = renderToStaticMarkup(
      <NewsQuoteKeyPanel view={{ ...free, error: "Yap could not read the key." }} busy={false} onSave={asyncNoop} onClear={noop} />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain("Yap could not read the key.");
  });

  it("Settings → Yappy News renders the Advanced section before the footer", () => {
    const view: NewsView = {
      ...EMPTY_PREFS,
      cloudConnected: true,
      active: false,
      offer: "accepted",
      onboardingStepDue: false,
      homeCardDue: false,
      cloudConnectedAtMs: 1,
      lastRefreshMs: null,
    };
    const html = renderToStaticMarkup(
      <NewsSettingsView
        view={view}
        draft={EMPTY_PREFS}
        saving={false}
        onChange={noop}
        onSave={noop}
        onRevert={noop}
        onConnect={noop}
        advanced={<details data-news-advanced>Advanced</details>}
      />,
    );
    expect(html).toContain("data-news-advanced");
    expect(html.indexOf("data-news-advanced")).toBeLessThan(html.indexOf("settings-footer"));
  });
});
