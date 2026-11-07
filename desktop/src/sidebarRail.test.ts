import { describe, expect, it } from "vitest";
import {
  NARROW_QUERY,
  NAV_ORDER,
  RAIL_AUTO_BELOW_PX,
  SIDEBAR_COLLAPSED_KEY,
  SIDEBAR_EXPANDED_PX,
  SIDEBAR_RAIL_PX,
  isNarrowWindow,
  loadCollapsed,
  navShortcutHint,
  railDelta,
  railReason,
  railShown,
  saveCollapsed,
  sidebarKeyAction,
  type SidebarKey,
} from "./sidebarRail";

function memStore(seed: Record<string, string> = {}) {
  const m = new Map(Object.entries(seed));
  return {
    getItem: (k: string) => (m.has(k) ? (m.get(k) as string) : null),
    setItem: (k: string, v: string) => {
      m.set(k, v);
    },
    map: m,
  };
}

const key = (over: Partial<SidebarKey>): SidebarKey => ({
  code: "KeyA",
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...over,
});

describe("sidebar rail persistence", () => {
  it("defaults to expanded with no storage, an empty store, or a throwing store", () => {
    expect(loadCollapsed(null)).toBe(false);
    expect(loadCollapsed(undefined)).toBe(false);
    expect(loadCollapsed(memStore())).toBe(false);
    expect(
      loadCollapsed({
        getItem: () => {
          throw new Error("denied");
        },
      }),
    ).toBe(false);
  });

  it("round-trips the choice across a relaunch (a fresh read of the same store)", () => {
    const s = memStore();
    expect(saveCollapsed(s, true)).toBe(true);
    expect(s.map.get(SIDEBAR_COLLAPSED_KEY)).toBe("1");
    expect(loadCollapsed(s)).toBe(true);
    expect(saveCollapsed(s, false)).toBe(true);
    expect(s.map.get(SIDEBAR_COLLAPSED_KEY)).toBe("0");
    expect(loadCollapsed(s)).toBe(false);
  });

  it("treats anything but \"1\" as expanded", () => {
    expect(loadCollapsed(memStore({ [SIDEBAR_COLLAPSED_KEY]: "true" }))).toBe(false);
    expect(loadCollapsed(memStore({ [SIDEBAR_COLLAPSED_KEY]: "1" }))).toBe(true);
  });

  it("reports a refused write instead of throwing", () => {
    expect(saveCollapsed(null, true)).toBe(false);
    expect(
      saveCollapsed(
        {
          setItem: () => {
            throw new Error("quota");
          },
        },
        true,
      ),
    ).toBe(false);
  });
});

describe("automatic rail under 900 px", () => {
  it("is narrow strictly under the spec line", () => {
    expect(RAIL_AUTO_BELOW_PX).toBe(900);
    expect(isNarrowWindow(899)).toBe(true);
    expect(isNarrowWindow(720)).toBe(true);
    expect(isNarrowWindow(900)).toBe(false);
    expect(isNarrowWindow(1190)).toBe(false);
    expect(isNarrowWindow(Number.NaN)).toBe(false);
    expect(NARROW_QUERY).toBe("(max-width: 899px)");
  });

  it("shows the rail when chosen or narrow, and says why", () => {
    expect(railShown(false, false)).toBe(false);
    expect(railShown(true, false)).toBe(true);
    expect(railShown(false, true)).toBe(true);
    expect(railShown(true, true)).toBe(true);
    expect(railReason(false, false)).toBe("expanded");
    expect(railReason(true, false)).toBe("user");
    expect(railReason(false, true)).toBe("narrow");
    expect(railReason(true, true)).toBe("narrow");
  });

  it("widens the main column by exactly the rail delta", () => {
    expect(SIDEBAR_EXPANDED_PX).toBe(220);
    expect(SIDEBAR_RAIL_PX).toBe(56);
    expect(railDelta()).toBe(164);
  });
});

describe("sidebar keys (both states use the same handler)", () => {
  it("Ctrl-Cmd-S toggles", () => {
    expect(sidebarKeyAction(key({ code: "KeyS", metaKey: true, ctrlKey: true }))).toEqual({ kind: "toggle" });
  });

  it("Cmd-1..Cmd-8 map to the nav in sidebar order", () => {
    NAV_ORDER.forEach((nav, i) => {
      expect(sidebarKeyAction(key({ code: `Digit${i + 1}`, metaKey: true }))).toEqual({ kind: "nav", nav });
    });
    expect(sidebarKeyAction(key({ code: "Digit9", metaKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "Digit0", metaKey: true }))).toBeNull();
  });

  it("ignores plain keys, other chords, repeats, IME and consumed events", () => {
    expect(sidebarKeyAction(key({ code: "KeyS" }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "KeyS", metaKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "KeyS", ctrlKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "Digit1" }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "Digit1", ctrlKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "KeyM", metaKey: true, ctrlKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "KeyS", metaKey: true, ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "Digit1", metaKey: true, altKey: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "KeyS", metaKey: true, ctrlKey: true, repeat: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "Digit2", metaKey: true, isComposing: true }))).toBeNull();
    expect(sidebarKeyAction(key({ code: "Digit2", metaKey: true, defaultPrevented: true }))).toBeNull();
  });

  it("names each item's shortcut for the tooltip", () => {
    expect(navShortcutHint("home")).toBe("⌘1");
    expect(navShortcutHint("meetings")).toBe("⌘4");
    expect(navShortcutHint("settings")).toBe("⌘8");
    expect(navShortcutHint("nope" as never)).toBe("");
  });
});
