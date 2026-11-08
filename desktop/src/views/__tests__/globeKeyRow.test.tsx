/**
 * yap-1107-globe-key-conflict-guard — the Permissions page's one Globe key
 * line: "Turn it off" while macOS would act on a lone Fn tap, "Put it back"
 * after Yap changed it, nothing at all otherwise.
 */
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => ({})) }));

import { GlobeKeyRowView, type GlobeStatus } from "../settings/parts/GlobeKeyRow";

const EMOJI = "Your Mac opens the emoji picker on a lone Fn tap; Yap can turn that off";
const base: GlobeStatus = {
  pref: null,
  conflict: { kind: "emoji_picker", pref: null, line: EMOJI },
  line: EMOJI,
  canTurnOff: true,
  canPutBack: false,
  turnOffLabel: "Turn it off",
  putBackLabel: "Put it back",
};
const html = (status: GlobeStatus | null, busy: "turn_off" | "put_back" | null = null) =>
  renderToStaticMarkup(
    <GlobeKeyRowView status={status} busy={busy} onTurnOff={() => {}} onPutBack={() => {}} />,
  );

describe("GlobeKeyRowView", () => {
  it("shows the one line and Turn it off on a conflict", () => {
    const out = html(base);
    expect(out).toContain(EMOJI);
    expect(out).toContain("Turn it off");
    expect(out).not.toContain("Put it back");
    expect(out).toContain('data-globe-key="conflict"');
  });

  it("shows the result and Put it back after turning it off", () => {
    const out = html({
      ...base,
      pref: 0,
      conflict: null,
      line: "A lone Fn tap now does nothing. macOS applies this fully after you restart your Mac.",
      canTurnOff: false,
      canPutBack: true,
    });
    expect(out).toContain("does nothing");
    expect(out).toContain("Put it back");
    expect(out).not.toContain("Turn it off");
    expect(out).toContain('data-globe-key="off"');
  });

  it("renders nothing without a line", () => {
    expect(html(null)).toBe("");
    expect(html({ ...base, conflict: null, line: null, canTurnOff: false })).toBe("");
  });

  it("disables the button while the change runs", () => {
    expect(html(base, "turn_off")).toContain("Turning it off…");
    expect(html(base, "turn_off")).toContain("disabled");
  });
});
