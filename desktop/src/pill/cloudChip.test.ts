import { describe, expect, it } from "vitest";
import { CAP_REACHED_LINE, cloudChip, notesRouteLine } from "./cloudChip";
import type { CloudRouteChip } from "../appTypes";

const chip = (route: CloudRouteChip["route"], reason: CloudRouteChip["reason"], tooltip = "why"): CloudRouteChip => ({
  route,
  reason,
  label: route === "cloud" ? "Cloud" : "On this Mac",
  tooltip,
  job: "polish",
});

describe("cloudChip", () => {
  it("is hidden before the first take and for a default install", () => {
    expect(cloudChip(null)).toBeNull();
    expect(cloudChip({ cloudRoute: null })).toBeNull();
    expect(cloudChip({ cloudRoute: chip("local", "user") })).toBeNull();
    expect(cloudChip({ cloudRoute: chip("local", "no_key") })).toBeNull();
  });

  it("says Cloud with the reason when the cloud wrote the take", () => {
    const m = cloudChip({ cloudRoute: chip("cloud", "online", "Written in the cloud with your key") });
    expect(m).toEqual({
      route: "cloud",
      reason: "online",
      label: "Cloud",
      title: "Written in the cloud with your key",
      tone: "cloud",
    });
  });

  it("says On this Mac with the fallback reason in the tooltip", () => {
    for (const reason of ["offline", "breaker", "timeout", "error", "constrained"] as const) {
      const m = cloudChip({ cloudRoute: chip("local", reason, `because ${reason}`) });
      expect(m?.label).toBe("On this Mac");
      expect(m?.title).toBe(`because ${reason}`);
      expect(m?.tone).toBe("local");
    }
  });

  it("carries the cap line when the daily limit is reached", () => {
    const m = cloudChip({ cloudRoute: chip("local", "cap", "ignored") });
    expect(m?.title).toBe(CAP_REACHED_LINE);
    expect(m?.title).toBe("Cloud limit reached today, using this Mac");
    expect(m?.tone).toBe("limit");
  });

  it("names no vendor or model anywhere", () => {
    const all = [
      cloudChip({ cloudRoute: chip("cloud", "online") }),
      cloudChip({ cloudRoute: chip("local", "cap") }),
      cloudChip({ cloudRoute: chip("local", "timeout") }),
    ]
      .map((m) => `${m?.label} ${m?.title}`)
      .join(" ")
      .toLowerCase();
    for (const bad of ["anthropic", "claude", "openai", "gpt", "sonnet", "opus"]) {
      expect(all).not.toContain(bad);
    }
  });

  it("writes the notes header line from the stored route", () => {
    expect(notesRouteLine("cloud")).toBe("Notes written in the cloud");
    expect(notesRouteLine("local")).toBe("Notes written on this Mac");
    expect(notesRouteLine(null)).toBeNull();
    expect(notesRouteLine(undefined)).toBeNull();
  });
});
