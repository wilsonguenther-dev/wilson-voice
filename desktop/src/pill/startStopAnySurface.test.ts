// yap-1107-start-stop-any-surface — the frontend surfaces only SEND an intent
// and RENDER the state the backend tells them. No surface keeps an "I started
// it" flag that could refuse to stop a take another surface started.
// @ts-expect-error -- node builtin, untyped here (no @types/node); the test runs under Node.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8") as string;

/** The body of the first `{ ... }` block that follows `marker`. */
function blockAfter(src: string, marker: string): string {
  const at = src.indexOf(marker);
  expect(at, `marker not found: ${marker}`).toBeGreaterThanOrEqual(0);
  const open = src.indexOf("{", at);
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}" && --depth === 0) return src.slice(open, i + 1);
  }
  throw new Error(`unbalanced block after ${marker}`);
}

describe("pill click sends the toggle intent and nothing else", () => {
  const yappy = read("./YappyPill.tsx");
  it("the canvas tap invokes pill_click_toggle and never decides start vs stop itself", () => {
    const tap = blockAfter(yappy, "tapRef.current = () =>");
    expect(tap).toContain('invoke("pill_click_toggle")');
    expect(tap).not.toMatch(/recording|listening|stop_recording|start_recording/);
  });
  it("renders the phase from the backend status and recording events", () => {
    expect(yappy).toContain('listen<AppStatus>("status"');
    expect(yappy).toContain('listen<boolean>("recording"');
  });
});

describe("the capsule and the Dictate button send manual_toggle", () => {
  const shell = read("./PillShell.tsx");
  const app = read("../appShell.ts");
  it("a live take is never blocked by the busy guard (a click mid-take stops it)", () => {
    expect(shell).toMatch(/const busy = status\.busy && !live/);
    const toggle = blockAfter(shell, "const onToggle = useCallback(() =>");
    expect(toggle).toContain('invoke("manual_toggle")');
    expect(toggle).not.toMatch(/status\.recording/);
  });
  it("the in-app Dictate button only invokes manual_toggle", () => {
    const fn = blockAfter(app, "async function toggleRecord()");
    expect(fn).toContain('invoke("manual_toggle")');
    expect(fn).not.toMatch(/recording/);
  });
});

describe("the backend has ONE take machine every surface goes through", () => {
  const lib = read("../../src-tauri/src/lib.rs");
  const machine = read("../../src-tauri/src/take_control.rs");
  it("one state enum, one decision", () => {
    expect(machine.match(/pub enum TakePhase/g)?.length).toBe(1);
    expect(machine).toContain("pub fn intent(&mut self, surface: Surface, intent: Intent");
  });
  it("pill, hotkey, menu bar and Dictate button each send a surface-tagged intent", () => {
    for (const s of ["Pill", "Hotkey", "MenuBar", "DictateButton"]) {
      expect(lib).toContain(`take_control::Surface::${s},`);
    }
    expect(lib).toContain("take_intent(app, state, take_control::Surface::Hotkey, intent");
  });
  it("no surface reads the recording flag to pick start vs stop", () => {
    for (const fn of ["fn pill_click_toggle(", "fn manual_toggle("]) {
      const body = blockAfter(lib, fn);
      // Everything before the intent is sent: no peek at the recording flag.
      // (The pill's start half reads it AFTER start_recording, to latch
      // hands-free on a take that really started — that is not a decision.)
      const beforeIntent = body.slice(0, body.indexOf("take_intent"));
      expect(body.indexOf("take_intent")).toBeGreaterThan(0);
      expect(beforeIntent).not.toContain("state.recording.lock()");
    }
    const tray = blockAfter(lib, "TrayCommand::ToggleDictation =>");
    expect(tray).not.toContain("state.recording.lock()");
    expect(tray).toContain("take_intent(");
  });
});
