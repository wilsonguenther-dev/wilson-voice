import { describe, expect, it } from "vitest";
import {
  INITIAL_CARD,
  cardReducer,
  headline,
  isDone,
  isOpen,
  nudgeText,
  shouldNudge,
  suggestionReason,
  answerFor,
  doneText,
  nameSuggestions,
  pushRecent,
  sameAsText,
  type CardEvent,
  type CardState,
} from "./WhoIsTalking";

const run = (events: CardEvent[], from: CardState = INITIAL_CARD): CardState => events.reduce(cardReducer, from);

describe("who is this — the card state machine", () => {
  it("unknown → named: type a name, Save, the save lands", () => {
    const typed = run([{ type: "type", name: "John" }]);
    expect(typed.phase).toBe("unknown");
    const saving = cardReducer(typed, { type: "save" });
    expect(saving.phase).toBe("saving");
    const named = cardReducer(saving, { type: "saved", name: "John", relabelled: 14, me: false });
    expect(named).toMatchObject({ phase: "named", savedName: "John", relabelled: 14 });
    expect(isDone(named)).toBe(true);
    expect(isOpen(named)).toBe(false);
  });

  it("unknown → me: That's me", () => {
    const me = run([{ type: "me" }, { type: "saved", name: "You", relabelled: 3, me: true }]);
    expect(me.phase).toBe("me");
    expect(me.savedName).toBe("You");
  });

  it("unknown → ignored: Ignore this voice", () => {
    expect(run([{ type: "ignore" }]).phase).toBe("ignored");
  });

  it("unknown → not-sure: Not sure leaves Speaker N", () => {
    const s = run([{ type: "type", name: "Ma" }, { type: "notSure" }]);
    expect(s.phase).toBe("notSure");
    expect(isDone(s)).toBe(true);
  });

  it("an empty name never saves", () => {
    expect(run([{ type: "save" }]).phase).toBe("unknown");
    expect(run([{ type: "type", name: "   " }, { type: "save" }]).phase).toBe("unknown");
  });

  it("a failed save keeps the typed name, says so, and can be retried", () => {
    const failed = run([{ type: "type", name: "Maya" }, { type: "save" }, { type: "failed", error: "Couldn't save that name. Try again." }]);
    expect(failed).toMatchObject({ phase: "error", name: "Maya" });
    expect(isOpen(failed)).toBe(true);
    const retried = run([{ type: "save" }, { type: "saved", name: "Maya", relabelled: 9, me: false }], failed);
    expect(retried.phase).toBe("named");
    expect(retried.error).toBeUndefined();
  });

  it("while saving, other actions are ignored", () => {
    const saving = run([{ type: "type", name: "Dev" }, { type: "save" }]);
    expect(cardReducer(saving, { type: "ignore" })).toBe(saving);
    expect(cardReducer(saving, { type: "type", name: "x" })).toBe(saving);
  });

  it("terminal phases ignore every later event", () => {
    for (const end of [
      run([{ type: "ignore" }]),
      run([{ type: "notSure" }]),
      run([{ type: "me" }, { type: "saved", name: "You", relabelled: 1, me: true }]),
      run([{ type: "type", name: "J" }, { type: "save" }, { type: "saved", name: "J", relabelled: 1, me: false }]),
    ]) {
      expect(cardReducer(end, { type: "save" })).toBe(end);
      expect(cardReducer(end, { type: "failed", error: "x" })).toBe(end);
      expect(cardReducer(end, { type: "type", name: "y" })).toBe(end);
    }
  });

  it("'Same person as Alex?': Yes saves, No starts someone new", () => {
    expect(run([{ type: "yes" }]).phase).toBe("saving");
    const no = run([{ type: "no" }]);
    expect(no).toMatchObject({ phase: "new", declinedSuggestion: true });
    expect(isOpen(no)).toBe(true);
  });
});

// yap-1106-fb-22b-who-was-this-cards — unknown → named / not-sure / merged / new.
describe("who was this — the after-meeting card state machine", () => {
  const card = { suggestion: { profileId: 7, displayName: "John", similarity: 0.62 } };

  it("unknown → named: Save writes the typed name through name_cluster", () => {
    const typed = run([{ type: "type", name: " John " }]);
    expect(answerFor({ type: "save" }, typed, card)).toEqual({
      command: "name_cluster",
      target: { kind: "newName", name: "John" },
    });
    const named = run([{ type: "save" }, { type: "saved", name: "John", relabelled: 14, me: false }], typed);
    expect(named.phase).toBe("named");
    expect(doneText(named)).toBe("John ✓ · 14 lines relabeled");
  });

  it("unknown → merged: Yes, John adds this voice to John's voiceprint", () => {
    expect(answerFor({ type: "yes" }, INITIAL_CARD, card)).toEqual({
      command: "name_cluster",
      target: { kind: "profile", profileId: 7 },
    });
    const merged = run([{ type: "yes" }, { type: "saved", name: "John", relabelled: 9, me: false }]);
    expect(merged.phase).toBe("merged");
    expect(isDone(merged)).toBe(true);
    expect(doneText(merged)).toBe("John ✓ · 9 lines relabeled");
  });

  it("unknown → new: No, someone new asks for a name, then saves a NEW speaker", () => {
    const fresh = run([{ type: "no" }]);
    expect(fresh.phase).toBe("new");
    expect(answerFor({ type: "yes" }, fresh, { suggestion: null })).toBeNull();
    const typed = cardReducer(fresh, { type: "type", name: "Priya" });
    expect(answerFor({ type: "save" }, typed, card)).toEqual({
      command: "name_cluster",
      target: { kind: "newName", name: "Priya" },
    });
    const named = run([{ type: "save" }, { type: "saved", name: "Priya", relabelled: 4, me: false }], typed);
    expect(named.phase).toBe("named");
  });

  it("unknown → not-sure: leaves Speaker N and persists nothing", () => {
    const typed = run([{ type: "type", name: "Maybe Sam" }]);
    const answer = answerFor({ type: "notSure" }, typed, card);
    // No name and no voiceprint ever leave the card: the only call drops the
    // stored voice and stops the asking.
    expect(answer).toEqual({ command: "ignore_cluster", ignore: false });
    expect(JSON.stringify(answer)).not.toContain("Sam");
    expect(JSON.stringify(answer)).not.toContain("name_cluster");
    const notSure = cardReducer(typed, { type: "notSure" });
    expect(notSure.phase).toBe("notSure");
    expect(notSure.savedName).toBeUndefined();
    expect(notSure.relabelled).toBeUndefined();
    expect(isDone(notSure)).toBe(true);
  });

  it("a failed merge can be retried and is not counted as merged", () => {
    const failed = run([{ type: "yes" }, { type: "failed", error: "Couldn't save that name. Try again." }]);
    expect(failed).toMatchObject({ phase: "error", merging: false });
    const named = run([{ type: "type", name: "Jo" }, { type: "save" }, { type: "saved", name: "Jo", relabelled: 1, me: false }], failed);
    expect(named.phase).toBe("named");
    expect(doneText(named)).toBe("Jo ✓ · 1 line relabeled");
  });

  it("copy: 'Same person as John? 62% sure'", () => {
    expect(sameAsText({ displayName: "John", similarity: 0.62 })).toBe("Same person as John? 62% sure");
    expect(sameAsText({ displayName: "John", similarity: 0.999 })).toBe("Same person as John? 99% sure");
  });

  it("name suggestions: attendees, then saved voices, then recent names, never You", () => {
    expect(nameSuggestions(["John Park", "Ana"], ["ana", "You", "Ben"], ["Cy", "john park"])).toEqual([
      "John Park",
      "Ana",
      "Ben",
      "Cy",
    ]);
    expect(pushRecent(["Ana", "Ben"], " ben ")).toEqual(["ben", "Ana"]);
  });
});

describe("who is this — copy and nudge", () => {
  it("headline counts the voices left, then reads All set", () => {
    expect(headline(3)).toBe("3 voices to name");
    expect(headline(1)).toBe("1 voice to name");
    expect(headline(0)).toBe("All set");
  });

  it("a suggestion carries its reason, never a fact", () => {
    expect(suggestionReason({ displayName: "Alex", similarity: 0.8234 })).toBe("Probably Alex — voice match 0.82");
  });

  it("nudges once per meeting, never during a recording", () => {
    expect(nudgeText(2, "Launch sync")).toBe("2 new voices in Launch sync, name them");
    const never = () => false;
    expect(shouldNudge("m1", 2, false, never)).toBe(true);
    expect(shouldNudge("m1", 2, true, never)).toBe(false);
    expect(shouldNudge("m1", 0, false, never)).toBe(false);
    expect(shouldNudge("m1", 2, false, (k) => k.endsWith("m1"))).toBe(false);
  });
});
