/**
 * yap-notetaker-onboard — the Set up Notetaker card: four rows, one dot per
 * working row, the X persists, Help reopens. Renders the SHIPPING component
 * with `renderToStaticMarkup` (no DOM), and pins the Rust side of the wire by
 * reading lib.rs, so the event name and the Help item cannot drift apart.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
// Vite `?raw`: lib.rs as a string, so the Rust half of the wire is pinned too.
import libRs from "../../src-tauri/src/lib.rs?raw";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import NotetakerSetupCard, { CARD_TITLE } from "./NotetakerSetupCard";
import NotetakerTour, { TOUR_STEPS } from "./NotetakerTour";
import {
  EMPTY_SETUP,
  NOTETAKER_SETUP_EVENT,
  dismiss,
  loadSetup,
  markDone,
  reopen,
  saveSetup,
  setupRows,
  shouldShow,
  importSummary,
  isImportable,
} from "./notetakerSetup";


beforeEach(() => invoke.mockReset());

describe("NotetakerSetupCard", () => {
  it("draws exactly four rows in Wilson's order, with the card title", () => {
    const html = renderToStaticMarkup(<NotetakerSetupCard initial={EMPTY_SETUP} />);
    expect(CARD_TITLE).toBe("Set up Notetaker");
    expect(html).toContain("Set up Notetaker");
    const rows = [...html.matchAll(/data-row="([a-z]+)"/g)].map((m) => m[1]);
    expect(rows).toEqual(["tour", "calendar", "import", "mcp"]);
  });

  it("gives each working row one progress dot (empty / done) and the MCP row none", () => {
    const html = renderToStaticMarkup(
      <NotetakerSetupCard initial={markDone(EMPTY_SETUP, "calendar")} />,
    );
    const dots = [...html.matchAll(/data-dot="([a-z]+)"/g)].map((m) => m[1]);
    expect(dots).toEqual(["empty", "done", "empty", "none"]);
    expect(html).toContain("1 of 3 done");
  });

  it("shows Set up MCP as Coming with no action", () => {
    const html = renderToStaticMarkup(<NotetakerSetupCard initial={EMPTY_SETUP} />);
    const mcp = html.slice(html.indexOf('data-row="mcp"'));
    expect(mcp).toContain("Coming");
    expect(mcp).not.toContain("<button");
    expect(setupRows(EMPTY_SETUP).find((r) => r.id === "mcp")?.verb).toBeNull();
  });

  it("uses a plain verb per working row and says what calendar access covers", () => {
    const verbs = setupRows(EMPTY_SETUP).map((r) => r.verb);
    expect(verbs).toEqual(["Start", "Connect", "Import", null]);
    const cal = setupRows(EMPTY_SETUP)[1].detail;
    expect(cal).toMatch(/read-only/i);
    expect(cal).toMatch(/Google or Exchange/);
    expect(setupRows(EMPTY_SETUP)[2].detail).toMatch(/Granola: Coming/);
  });

  it("renders nothing once dismissed, and nothing once every working step is done", () => {
    expect(renderToStaticMarkup(<NotetakerSetupCard initial={dismiss(EMPTY_SETUP)} />)).toBe("");
    const done = { ...EMPTY_SETUP, tour: true, calendar: true, import: true };
    expect(shouldShow(done)).toBe(false);
    expect(renderToStaticMarkup(<NotetakerSetupCard initial={done} />)).toBe("");
  });

  it("the X says where to reopen it, and dismiss PERSISTS through save_notetaker_setup", async () => {
    const html = renderToStaticMarkup(<NotetakerSetupCard initial={EMPTY_SETUP} />);
    expect(html).toContain("Reopen anytime from Help");
    invoke.mockResolvedValueOnce({ ...EMPTY_SETUP, dismissed: true });
    const stored = await saveSetup(dismiss(EMPTY_SETUP));
    expect(invoke).toHaveBeenCalledWith("save_notetaker_setup", {
      setup: { tour: false, calendar: false, import: false, dismissed: true },
    });
    expect(stored.dismissed).toBe(true);
    invoke.mockResolvedValueOnce(stored);
    expect(shouldShow(await loadSetup())).toBe(false);
    expect(invoke).toHaveBeenLastCalledWith("notetaker_setup_state");
  });

  it("Help reopens it: the menu item exists, Rust clears dismissed and emits the event the card listens to", () => {
    const back = reopen(markDone(dismiss(EMPTY_SETUP), "tour"));
    expect(shouldShow(back)).toBe(true);
    expect(back.tour).toBe(true); // progress is kept
    expect(libRs).toContain('"Set up Notetaker"');
    expect(libRs).toContain("HELP_SUBMENU_ID");
    expect(libRs).toContain(`app.emit("${NOTETAKER_SETUP_EVENT}", &setup)`);
    expect(libRs).toMatch(/setup\.dismissed = false;/);
  });

  it("import accepts Otter .txt/.srt only and summarises outcomes in one line", () => {
    expect(isImportable("Call.TXT")).toBe(true);
    expect(isImportable("call.srt")).toBe(true);
    expect(isImportable("call.docx")).toBe(false);
    expect(importSummary([{ name: "a.txt", meetingId: "m", segments: 3, error: null }])).toBe("Imported 1 meeting.");
    expect(
      importSummary([{ name: "b.srt", meetingId: null, segments: 0, error: "bad" }]),
    ).toBe("b.srt: bad");
  });
});

describe("NotetakerTour", () => {
  it("walks start, live meters, stop, transcript, notes, export — and ends on Finish", () => {
    expect(TOUR_STEPS.map((s) => s.id)).toEqual(["start", "meters", "stop", "transcript", "notes", "export"]);
    const first = renderToStaticMarkup(<NotetakerTour onFinish={() => {}} onClose={() => {}} />);
    expect(first).toContain("Next");
    const last = renderToStaticMarkup(
      <NotetakerTour onFinish={() => {}} onClose={() => {}} initialStep={TOUR_STEPS.length - 1} />,
    );
    expect(last).toContain("Finish");
    const transcript = renderToStaticMarkup(
      <NotetakerTour onFinish={() => {}} onClose={() => {}} initialStep={3} />,
    );
    expect(transcript).toContain("annual discount");
  });
});
