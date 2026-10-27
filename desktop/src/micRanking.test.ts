import { describe, expect, it } from "vitest";
import { builtInAlternative, moveInRanking, rankedOrder, type MicDeviceRow } from "./micRanking";

const row = (name: string, rank: number | null, extra: Partial<MicDeviceRow> = {}): MicDeviceRow => ({
  name,
  present: true,
  rank,
  class: "other",
  systemDefault: false,
  wins: false,
  ...extra,
});

describe("PERM-H mic ranking (frontend half)", () => {
  it("reads the ranked order from the rows, unranked rows excluded", () => {
    expect(rankedOrder([row("B", 1), row("Unranked", null), row("A", 0)])).toEqual(["A", "B"]);
  });

  it("drag-to-reorder moves a ranked row and ranks an unranked one", () => {
    expect(moveInRanking(["A", "B", "C"], "C", 0)).toEqual(["C", "A", "B"]);
    expect(moveInRanking(["A", "B", "C"], "A", 2)).toEqual(["B", "C", "A"]);
    expect(moveInRanking(["A", "B"], "New", 1)).toEqual(["A", "New", "B"]);
    expect(moveInRanking(["A", "B"], "A", 99)).toEqual(["B", "A"]);
    expect(moveInRanking(["A", "B"], "B", -3)).toEqual(["B", "A"]);
  });

  it("offers an ATTACHED built-in mic instead of AirPods", () => {
    const rows = [
      row("AirPods", 0, { class: "bluetooth" }),
      row("MacBook Pro Microphone", null, { class: "built_in", present: false }),
    ];
    expect(builtInAlternative(rows)).toBeUndefined();
    rows[1].present = true;
    expect(builtInAlternative(rows)?.name).toBe("MacBook Pro Microphone");
  });
});
