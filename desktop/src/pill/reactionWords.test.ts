/**
 * yap-1106-reaction-words — pins the acceptance:
 *   1. A table keyed by voice matches words from the partial stream.
 *   2. One reaction per take at most.
 *   3. Professional never reacts to body words or cussing (it answers only the
 *      design's plain-reply rows); Rose never reacts.
 *   4. Curse lines appear only with the curse setting on.
 *   5. Every word in every voice (48 rows x 4 voices + the curse column).
 */
import { describe, expect, it } from "vitest";
// @ts-expect-error node:fs has no types in this repo (no @types/node); vitest provides it at run time.
import { readFileSync } from "node:fs";
import {
  findReaction, normalizeSpoken, REACTION_COUNT, REACTION_IDLE, REACTION_WORDS, REACTIONS_BY_VOICE,
  reactionLine, reactToPartial, type ReactionCtx, type ReactionState,
} from "./reactionWords";
import { CURSE_EVERY, isStrongest, seededRandom, type VoiceId } from "./voices";

const VOICES_ALL: readonly VoiceId[] = ["bestfriend", "badass", "professional", "rose"];

const ctx = (over: Partial<ReactionCtx> = {}): ReactionCtx => ({
  voice: "bestfriend",
  listening: true,
  chatterAllows: true,
  meeting: false,
  curse: false,
  rand: () => 0.99,
  strongUsedToday: false,
  ...over,
});

/** A sentence that says exactly `phrase` once, surrounded by words no row reacts to. */
const sentence = (phrase: string): string => `so the plan, ${phrase}, then the window.`;

describe("the approved table", () => {
  it("has the 48 rows of the design, each with a line for every column", () => {
    expect(REACTION_WORDS).toHaveLength(REACTION_COUNT);
    expect(new Set(REACTION_WORDS.map((r) => r.word)).size).toBe(REACTION_COUNT);
    for (const r of REACTION_WORDS) {
      expect(r.say.length).toBeGreaterThan(0);
      expect(r.bestfriend).not.toBe("");
      expect(r.badass).not.toBe("");
      expect(r.badassCurse).not.toBe("");
    }
  });

  it("is keyed by voice: Best friend and Badass react to all 48, Professional only to its plain-reply rows, Rose to none", () => {
    expect(Object.keys(REACTIONS_BY_VOICE.bestfriend)).toHaveLength(48);
    expect(Object.keys(REACTIONS_BY_VOICE.badass)).toHaveLength(48);
    expect(Object.keys(REACTIONS_BY_VOICE.badassCurse)).toHaveLength(48);
    expect(Object.keys(REACTIONS_BY_VOICE.rose)).toHaveLength(0);
    expect(REACTIONS_BY_VOICE.professional).toEqual({
      "achoo / sneeze": "Bless you.",
      "my bad": "No problem.",
      "hold on": "Still listening.",
      "scratch that": "Removed the last sentence.",
      "never mind": "Noted.",
      "thank you": "You're welcome.",
      Yappy: "Here.",
      deploy: "Good luck with the deploy.",
      "it works": "Nice.",
    });
  });

  it("Professional has no line for any body word or cuss word", () => {
    for (const r of REACTION_WORDS.filter((x) => x.kind === "body" || x.kind === "cuss")) {
      if (r.word === "achoo / sneeze") continue; // the design's one plain reply among body rows
      expect(r.professional, r.word).toBeNull();
    }
  });

  it("only the curse column swears with the strongest word", () => {
    for (const r of REACTION_WORDS) {
      expect(isStrongest(r.bestfriend), r.word).toBe(false);
      expect(isStrongest(r.badass), r.word).toBe(false);
      if (r.professional) expect(isStrongest(r.professional), r.word).toBe(false);
    }
    expect(REACTION_WORDS.filter((r) => isStrongest(r.badassCurse)).map((r) => r.word)).toEqual(["fuck", "let's go", "it works"]);
  });
});

describe("every word in every voice", () => {
  for (const r of REACTION_WORDS) {
    for (const spoken of r.say.filter((s) => /^[a-z' ]+$/.test(s))) {
      for (const voice of VOICES_ALL) {
        it(`"${spoken}" (${r.word}) in ${voice}`, () => {
          const text = sentence(spoken);
          const want = reactionLine(r, voice);
          const hit = findReaction(text, voice);
          if (want === null) {
            expect(hit).toBeNull();
            const step = reactToPartial(REACTION_IDLE, 1, text, ctx({ voice }));
            expect(step.reaction).toBeNull();
            return;
          }
          expect(hit?.row.word).toBe(r.word);
          const step = reactToPartial(REACTION_IDLE, 1, text, ctx({ voice }));
          expect(step.reaction?.line).toBe(want);
          expect(step.reaction?.line).toBe(REACTIONS_BY_VOICE[voice][r.word]);
          expect(step.reaction?.kind).toBe(r.kind);
          expect(step.state.done).toBe(true);
        });
      }
      it(`"${spoken}" (${r.word}) in Badass + curse swaps in the curse column`, () => {
        const step = reactToPartial(REACTION_IDLE, 1, sentence(spoken), ctx({ voice: "badass", curse: true, rand: () => 0 }));
        expect(step.reaction?.line).toBe(r.badassCurse);
        expect(step.reaction?.line).toBe(REACTIONS_BY_VOICE.badassCurse[r.word]);
        expect(step.reaction?.column).toBe("badassCurse");
      });
    }
  }

  it("the regex spellings match their spoken forms", () => {
    expect(findReaction("duuude that was close", "bestfriend")?.row.word).toBe("dude");
    expect(findReaction("ayyy we did it", "bestfriend")?.row.word).toBe("ayy");
    expect(findReaction("ayyyyy", "badass")?.row.word).toBe("ayy");
  });

  it("capitals, curly apostrophes and punctuation do not hide a word", () => {
    expect(findReaction("Okay. LET’S GO!", "bestfriend")?.row.word).toBe("let's go");
    expect(findReaction("Thanks Yappy!", "professional")?.row.word).toBe("Yappy");
    expect(normalizeSpoken("I’m  COOKED...")).toBe(" i'm cooked ");
    expect(findReaction("he said 'yo' and left", "bestfriend")?.row.word).toBe("yo");
  });

  it("whole words only: no reaction inside a longer word", () => {
    for (const text of ["the button", "first class", "hello there", "a better idea", "jesus", "peer review", "the production schedule", "a passage", "brother", "yapping away", "shootout", "kindergarten"]) {
      for (const voice of VOICES_ALL) expect(findReaction(text, voice), `${text} / ${voice}`).toBeNull();
    }
  });

  it("the word said first wins; at the same spot the longer phrase wins", () => {
    expect(findReaction("wow the deploy worked", "bestfriend")?.row.word).toBe("wow");
    expect(findReaction("the deploy worked wow", "bestfriend")?.row.word).toBe("deploy");
    expect(findReaction("wait it works", "bestfriend")?.row.word).toBe("wait");
    expect(findReaction("oh my god", "badass")?.row.word).toBe("oh my god / omg");
  });

  it("an empty or wordless partial matches nothing", () => {
    expect(findReaction("", "bestfriend")).toBeNull();
    expect(findReaction(" ... ", "badass")).toBeNull();
  });
});

describe("one reaction per take", () => {
  it("a take reacts once, however many reaction words follow", () => {
    let s: ReactionState = REACTION_IDLE;
    const first = reactToPartial(s, 7, "butt", ctx());
    expect(first.reaction?.line).toBe("hehe. butt.");
    s = first.state;
    for (const more of ["butt toot", "butt toot fart", "butt toot fart thank you it works"]) {
      const step = reactToPartial(s, 7, more, ctx());
      expect(step.reaction).toBeNull();
      expect(step.log).toBeNull();
      s = step.state;
    }
  });

  it("a new take id can react again", () => {
    const a = reactToPartial(REACTION_IDLE, 3, "wow", ctx());
    const b = reactToPartial(a.state, 3, "wow bro", ctx());
    const c = reactToPartial(b.state, 4, "bro", ctx());
    expect(a.reaction?.line).toBe("wow!");
    expect(b.reaction).toBeNull();
    expect(c.reaction?.line).toBe("bro!");
  });

  it("partials without a reaction word keep the take open until one arrives", () => {
    const a = reactToPartial(REACTION_IDLE, 1, "so the plan for monday", ctx({ voice: "professional" }));
    expect(a.reaction).toBeNull();
    expect(a.state.done).toBe(false);
    const b = reactToPartial(a.state, 1, "so the plan for monday butt damn thank you", ctx({ voice: "professional" }));
    expect(b.reaction?.line).toBe("You're welcome.");
  });

  it("outside a take nothing reacts and nothing is spent", () => {
    const s = reactToPartial(REACTION_IDLE, 1, "butt", ctx({ listening: false }));
    expect(s.reaction).toBeNull();
    expect(s.state.done).toBe(false);
  });

  it("held back by the chatter setting or a meeting: logged once, and the take stays spent", () => {
    const quiet = reactToPartial(REACTION_IDLE, 2, "toot", ctx({ chatterAllows: false }));
    expect(quiet.reaction).toBeNull();
    expect(quiet.log).toBe("pill reaction held take=2 voice=bestfriend row=1 kind=body reason=chatter");
    expect(reactToPartial(quiet.state, 2, "toot wow", ctx()).reaction).toBeNull();
    const meet = reactToPartial(REACTION_IDLE, 2, "toot", ctx({ meeting: true }));
    expect(meet.reaction).toBeNull();
    expect(meet.log).toContain("reason=meeting");
  });

  it("the log names the row and kind, never the transcript", () => {
    const step = reactToPartial(REACTION_IDLE, 9, "the secret launch codes and then fart", ctx({ voice: "badass" }));
    expect(step.log).toBe("pill reaction fired take=9 voice=badass row=2 kind=body column=badass");
    expect(step.log).not.toContain("secret");
    expect(step.log).not.toContain("fart");
  });
});

describe("Professional and Rose", () => {
  it("Professional never reacts to any body or cuss word, in any spelling", () => {
    for (const r of REACTION_WORDS.filter((x) => (x.kind === "body" || x.kind === "cuss") && x.professional === null)) {
      for (const spoken of r.say.filter((s) => /^[a-z' ]+$/.test(s))) {
        expect(reactToPartial(REACTION_IDLE, 1, spoken, ctx({ voice: "professional", curse: true, rand: () => 0 })).reaction, spoken).toBeNull();
      }
    }
  });

  it("Rose never reacts", () => {
    for (const r of REACTION_WORDS) expect(reactToPartial(REACTION_IDLE, 1, r.say[0].replace(/[{}+,0-9]/g, ""), ctx({ voice: "rose" })).reaction).toBeNull();
  });

  it("Professional never swears, even when the caller wrongly passes curse", () => {
    const step = reactToPartial(REACTION_IDLE, 1, "thank you", ctx({ voice: "professional", curse: true, rand: () => 0 }));
    expect(step.reaction?.line).toBe("You're welcome.");
    expect(step.reaction?.column).toBe("professional");
  });
});

describe("curse lines only with the curse setting on", () => {
  it("curse off: 48 words x 200 takes never yield a curse-column line", () => {
    const rand = seededRandom(11);
    for (let take = 0; take < 200; take++) {
      for (const r of REACTION_WORDS) {
        const step = reactToPartial(REACTION_IDLE, take, sentence(r.say[0].replace(/[{}+,0-9]/g, "")), ctx({ voice: "badass", curse: false, rand }));
        if (step.reaction) {
          expect(step.reaction.column).toBe("badass");
          expect(step.reaction.line).toBe(r.badass);
        }
      }
    }
  });

  it("Best friend never takes the curse column even with curse passed", () => {
    const step = reactToPartial(REACTION_IDLE, 1, "fuck", ctx({ voice: "bestfriend", curse: true, rand: () => 0 }));
    expect(step.reaction?.line).toBe("someone's fired up.");
  });

  it("curse on: the curse column swaps in about 1 time in CURSE_EVERY", () => {
    const rand = seededRandom(42);
    let cursed = 0;
    const n = 2000;
    for (let take = 0; take < n; take++) {
      const step = reactToPartial(REACTION_IDLE, take, "bro", ctx({ voice: "badass", curse: true, rand }));
      if (step.reaction?.column === "badassCurse") cursed++;
    }
    expect(cursed / n).toBeGreaterThan(1 / CURSE_EVERY - 0.05);
    expect(cursed / n).toBeLessThan(1 / CURSE_EVERY + 0.05);
  });

  it("the strongest word is limited to once a day", () => {
    const first = reactToPartial(REACTION_IDLE, 1, "it works", ctx({ voice: "badass", curse: true, rand: () => 0 }));
    expect(first.reaction?.line).toBe("it fucking works.");
    expect(first.reaction?.strongest).toBe(true);
    const again = reactToPartial(REACTION_IDLE, 2, "it works", ctx({ voice: "badass", curse: true, rand: () => 0, strongUsedToday: true }));
    expect(again.reaction?.line).toBe("shocked.");
    expect(again.reaction?.column).toBe("badass");
    expect(again.reaction?.strongest).toBe(false);
    // A milder curse line is still allowed after the strongest word was used.
    const mild = reactToPartial(REACTION_IDLE, 3, "toot", ctx({ voice: "badass", curse: true, rand: () => 0, strongUsedToday: true }));
    expect(mild.reaction?.line).toBe("who the hell tooted.");
    expect(mild.reaction?.strongest).toBe(false);
  });
});

describe("the pill wiring (source)", () => {
  const src = readFileSync(new URL("./YappyPill.tsx", import.meta.url), "utf8") as string;
  it("YappyPill listens to the live word stream and folds only the committed words", () => {
    expect(src).toContain("listen<unknown>(TRANSCRIPT_PARTIAL_EVENT, (e) => onPartial(e.payload))");
    expect(src).toContain("reactToPartial(reaction, p.take, p.committed, {");
    expect(src).not.toMatch(/reactToPartial\([^)]*tentative/);
  });
  it("curse reaches the matcher only through curseAllowed, and chatter gates it like a tier line", () => {
    expect(src).toMatch(/curse: curseAllowed\(\{ enabled: curseRef\.current, voice: liveVoice, meeting: meetingRef\.current/);
    expect(src).toContain("chatterAllows: allowDuringLine(policy, true, 0)");
    expect(src).toContain('listening: phase === "listening"');
  });
});
