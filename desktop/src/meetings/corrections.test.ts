/**
 * YV130 — the correction rules, pinned.
 *
 * The interesting cases are all edges the prose gets wrong: N = 1, a cluster
 * offering itself as a merge target, a menu whose first item is the destructive
 * one, and an offer with nothing in it.
 */
import { describe, expect, it } from "vitest";
import {
  canSplit,
  mergeTargets,
  reassignMenu,
  relabelOffer,
  relabelResult,
  type CorrectableSegment,
} from "./corrections";

function seg(over: Partial<CorrectableSegment> = {}): CorrectableSegment {
  return {
    id: "s1",
    speakerId: null,
    speakerLocked: false,
    clusterIndex: 0,
    ...over,
  };
}

const speakers = [
  { id: "p-wilson", displayName: "Wilson" },
  { id: "p-jeisil", displayName: "Jeisil" },
];

describe("reassignMenu", () => {
  it("lists every enrolled voice and ticks the current one", () => {
    const items = reassignMenu(seg({ speakerId: "p-jeisil" }), speakers);
    expect(items.map((i) => i.label)).toEqual([
      "Wilson",
      "Jeisil",
      "Unknown speaker",
    ]);
    expect(items.filter((i) => i.current).map((i) => i.speakerId)).toEqual([
      "p-jeisil",
    ]);
  });

  it("puts the destructive item last and never first", () => {
    const items = reassignMenu(seg(), speakers);
    expect(items[0].speakerId).toBe("p-wilson");
    expect(items[items.length - 1].speakerId).toBeNull();
  });

  it("ticks 'Unknown speaker' when the segment is unlabelled", () => {
    const items = reassignMenu(seg({ speakerId: null }), speakers);
    expect(items[items.length - 1].current).toBe(true);
  });

  it("still offers the voices when nobody is enrolled yet", () => {
    expect(reassignMenu(seg(), []).map((i) => i.label)).toEqual([
      "Unknown speaker",
    ]);
  });
});

describe("mergeTargets", () => {
  it("never offers the cluster itself", () => {
    const items = mergeTargets(seg({ clusterIndex: 1 }), [0, 1, 2]);
    expect(items.map((i) => i.clusterIndex)).toEqual([0, 2]);
  });

  it("labels clusters one-based, because nobody is 'Speaker 0'", () => {
    expect(mergeTargets(seg({ clusterIndex: 0 }), [0, 1])[0].label).toBe(
      "Speaker 2",
    );
  });

  it("prefers a name once the cluster has one", () => {
    const items = mergeTargets(seg({ clusterIndex: 0 }), [0, 1], {
      1: "Jeisil",
    });
    expect(items[0].label).toBe("Jeisil");
  });

  it("offers nothing for a segment that was never clustered", () => {
    expect(mergeTargets(seg({ clusterIndex: null }), [0, 1])).toEqual([]);
  });

  it("offers nothing when the meeting has one cluster", () => {
    expect(mergeTargets(seg({ clusterIndex: 0 }), [0])).toEqual([]);
  });
});

describe("canSplit", () => {
  it("is offered only where split_cluster can actually answer", () => {
    expect(canSplit(0)).toBe(false);
    expect(canSplit(1)).toBe(false);
    expect(canSplit(2)).toBe(true);
  });
});

describe("relabelOffer", () => {
  it("asks rather than reports, and offers 'Not now'", () => {
    const copy = relabelOffer("Wilson", 3, 7)!;
    expect(copy.question).toBe(
      "Wilson's voice appears in 3 earlier meetings — label them too?",
    );
    expect(copy.apply).toBe("Apply");
    expect(copy.dismiss).toBe("Not now");
    // No past tense anywhere: this prompt precedes the write.
    expect(copy.question).not.toMatch(/labelled|applied|updated/i);
  });

  it("says '1 earlier meeting', not '1 earlier meetings'", () => {
    const copy = relabelOffer("Wilson", 1, 1)!;
    expect(copy.question).toBe(
      "Wilson's voice appears in 1 earlier meeting — label it too?",
    );
  });

  it("shows no prompt at all when there is nothing to offer", () => {
    expect(relabelOffer("Wilson", 0, 0)).toBeNull();
    expect(relabelOffer("Wilson", 2, 0)).toBeNull();
  });
});

describe("relabelResult", () => {
  it("reports what was done and offers one undo", () => {
    const r = relabelResult("Wilson", 7, 0);
    expect(r.message).toBe("Labelled 7 turns as Wilson.");
    expect(r.undo).toBe("Undo");
  });

  it("says so when the batch left a confirmed label alone", () => {
    expect(relabelResult("Wilson", 6, 1).message).toBe(
      "Labelled 6 turns as Wilson. 1 turn you had already named was left as it was.",
    );
    expect(relabelResult("Wilson", 5, 2).message).toBe(
      "Labelled 5 turns as Wilson. 2 turns you had already named were left as they were.",
    );
  });

  it("counts one turn as one turn", () => {
    expect(relabelResult("Jeisil", 1, 0).message).toBe(
      "Labelled 1 turn as Jeisil.",
    );
  });
});
