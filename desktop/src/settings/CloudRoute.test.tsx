import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { CLOUD_FIRST_LINE, CloudRouteSettingsView, MODE_LABEL, spendLine, spendNote, type CloudRouteView } from "./CloudRoute";

const base: CloudRouteView = {
  mode: "cloud_when_online",
  spend: { capUsd: 6.5, todayUsd: 1.2345, monthToDateUsd: 23.4, todayFraction: 0.19, capReached: false, warn: false, pricesAsOf: "2026-11-06" },
  keyNeedsAttention: false,
  keySaved: true,
  keyHint: "WXYZ",
  notesBest: false,
  lastRoute: null,
  label: "Cloud model (your key)",
  leavesHost: "example.invalid",
};

const noop = () => {};
const render = (view: CloudRouteView, keyDraft = "") =>
  renderToStaticMarkup(
    <CloudRouteSettingsView
      view={view}
      keyDraft={keyDraft}
      capDraft={view.spend.capUsd.toFixed(2)}
      error={null}
      onKeyDraft={noop}
      onSaveKey={noop}
      onRemoveKey={noop}
      onMode={noop}
      onCapDraft={noop}
      onSaveCap={noop}
      onNotesBest={noop}
    />,
  );

describe("CloudRoute settings", () => {
  it("shows the three modes, the leaves label, and month-to-date spend", () => {
    const html = render(base);
    for (const label of Object.values(MODE_LABEL)) expect(html).toContain(label);
    expect(html).toContain("Leaves this Mac: Yes — example.invalid");
    expect(html).toContain("saved ••••WXYZ");
    expect(html).toContain("Today $1.23 of $6.50 · Month to date $23.40");
    expect(html).toContain("Remove key");
    expect(html).not.toMatch(/sk-[A-Za-z0-9]/);
  });

  it("says No when no key is saved and hides Remove", () => {
    const html = render({ ...base, keySaved: false, keyHint: "", leavesHost: null });
    expect(html).toContain("Leaves this Mac: No (no key saved)");
    expect(html).not.toContain("Remove key");
  });

  it("surfaces the key-attention line and the cap notes", () => {
    expect(render({ ...base, keyNeedsAttention: true })).toContain("Cloud key needs attention");
    expect(spendNote({ ...base.spend, warn: true })).toBe("Close to today's cloud limit.");
    expect(spendNote({ ...base.spend, warn: true, capReached: true })).toMatch(/limit reached today/);
    expect(spendNote(base.spend)).toBeNull();
    expect(spendLine(base.spend)).toBe("Today $1.23 of $6.50 · Month to date $23.40");
  });

  it("shows the cloud-first line under the mode switch, word for word, with no vendor name", () => {
    const exact =
      "With your key in, Yap uses the cloud first. It falls back to this Mac when you are offline, over your daily limit, or the cloud is slow.";
    expect(CLOUD_FIRST_LINE).toBe(exact);
    for (const view of [base, { ...base, mode: "always_local" as const }, { ...base, keySaved: false, keyHint: "", leavesHost: null }]) {
      const html = render(view);
      expect(html).toContain(`<p class="settings-help" data-line="cloud-first">${exact}</p>`);
      // Under the switch: after the mode fieldset, before the key field.
      const at = html.indexOf('data-line="cloud-first"');
      expect(at).toBeGreaterThan(html.indexOf('data-field="cloud-mode"'));
      expect(at).toBeLessThan(html.indexOf('data-field="cloud-key"'));
    }
    const lower = exact.toLowerCase();
    for (const bad of ["anthropic", "claude", "openai", "gpt", "sonnet", "opus", "gemini"]) expect(lower).not.toContain(bad);
  });

  it("names no vendor or model", () => {
    const html = render({ ...base, keyNeedsAttention: true, spend: { ...base.spend, warn: true } }).toLowerCase();
    for (const bad of ["anthropic", "claude", "openai", "gpt", "sonnet", "opus"]) expect(html).not.toContain(bad);
  });
});
