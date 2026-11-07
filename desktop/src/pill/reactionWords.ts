/**
 * yap-1106-reaction-words — Yappy's reaction words, pure half.
 *
 * The approved table is the 2026-11-06 companions design page, section 5
 * ("Reaction words"): 48 triggers — body noises, mild cussing, exclamations,
 * phrases, the names you say most and dev words — each with a line for Best
 * friend, Badass, Badass + curse and Professional. Copy is verbatim; nothing
 * here is invented.
 *
 * The rules, all from that page:
 *
 * * **During the take.** Words come from the live stream
 *   (`transcript_partial`, yap-1106-partial-transcript-stream). Only the
 *   COMMITTED words count — the tentative tail is a guess, and "a word must be
 *   heard clearly".
 * * **At most one reaction per take.** Once a take has reacted (or a reaction
 *   was held back by the chatter setting or a meeting) it never reacts again.
 * * **Professional stays tame.** It answers only the few rows where a plain
 *   reply helps (the rows with a Professional line, e.g. "thank you" →
 *   "You're welcome."); it never reacts to body words or cussing, and its
 *   table has no entry for them. Rose has no approved column and never reacts.
 * * **Curse lines only with the curse setting on.** With it off, Badass uses
 *   its own column; with it on (the caller passes `curseAllowed(...)`), the
 *   curse column swaps in about 1 time in {@link CURSE_EVERY}. The strongest
 *   word is limited to once a day, like the take lines.
 *
 * Pure + deterministic: no I/O, no clock, no randomness of its own (callers
 * pass `rand`). Telemetry lines carry the table row and its kind, never the
 * transcript text.
 */
import { CURSE_EVERY, isStrongest, type VoiceId } from "./voices";

/** The six groups the design names. */
export type ReactionKind = "body" | "cuss" | "exclaim" | "phrase" | "name" | "dev";

export interface ReactionRow {
  /** The "You say" cell, as the design writes it. */
  word: string;
  kind: ReactionKind;
  /**
   * What counts as saying it: regex sources matched as whole words against the
   * normalized committed text (lower case, straight apostrophes, punctuation
   * dropped). The first entry is the word itself.
   */
  say: readonly string[];
  bestfriend: string;
  badass: string;
  /** Badass with the curse setting on, swapped in about 1 time in CURSE_EVERY. */
  badassCurse: string;
  /** null = the design's "—": Professional stays quiet. */
  professional: string | null;
}

const row = (
  word: string, kind: ReactionKind, say: readonly string[],
  bestfriend: string, badass: string, badassCurse: string, professional: string | null,
): ReactionRow => ({ word, kind, say, bestfriend, badass, badassCurse, professional });

/** The approved 48 rows, in the design's order. */
export const REACTION_WORDS: readonly ReactionRow[] = [
  // body noises
  row("butt", "body", ["butt", "butts"], "hehe. butt.", "did you just say butt.", "nice ass, I mean, sentence.", null),
  row("toot", "body", ["toot", "toots", "tooted", "tooting"], "toot toot! (honks)", "who tooted.", "who the hell tooted.", null),
  row("fart", "body", ["fart", "farts", "farted", "farting"], "pfft. Tidy did it.", "gross. noted.", "smells like a damn draft.", null),
  row("burp", "body", ["burp", "burps", "burped", "burping"], "bless you?", "classy.", "damn, excuse YOU.", null),
  row("poop", "body", ["poop", "poops", "pooped", "pooping"], "not on my notepad!", "charming.", "shit, literally.", null),
  row("pee", "body", ["pee", "pees", "peed", "peeing"], "hold it, almost done!", "TMI.", "go pee, damn.", null),
  row("achoo / sneeze", "body", ["achoo", "sneeze", "sneezes", "sneezed", "sneezing"], "bless you!", "gross.", "bless your damn heart.", "Bless you."),
  // mild cussing
  row("damn", "cuss", ["damn", "dammit", "damnit"], "ooh, spicy.", "damn right.", "damn right, damn it.", null),
  row("hell", "cuss", ["hell"], "yikes!", "hell yeah.", "hell yeah, hell yeah.", null),
  row("crap", "cuss", ["crap", "crappy"], "uh oh.", "crap indeed.", "holy crap.", null),
  row("shit", "cuss", ["shit", "shitty"], "whoa, language!", "same.", "same shit, different take.", null),
  row("fuck", "cuss", ["fuck", "fucking", "fucked", "fuckin"], "someone's fired up.", "big mood.", "fuck yeah, typed.", null),
  row("ass", "cuss", ["ass"], "(covers ears)", "rude.", "bad-ass sentence.", null),
  row("dang", "cuss", ["dang", "dangit"], "dang indeed!", "dang? say damn.", "say damn, damn it.", null),
  // exclamations
  row("bro", "exclaim", ["bro"], "bro!", "bro.", "bro, what the hell.", null),
  row("dude", "exclaim", ["dude", "du+de"], "dude!", "duuude.", "dude. damn.", null),
  row("yo", "exclaim", ["yo"], "yo yo!", "yo.", "yo, what's good, damn.", null),
  row("ayy", "exclaim", ["ayy", "ay{3,}"], "ayyy! (finger guns)", "ayy.", "ayy, hell yeah.", null),
  row("lol", "exclaim", ["lol", "lmao"], "hehe!", "lol. typed.", "lmao, damn.", null),
  row("nah", "exclaim", ["nah"], "nah? okay!", "nah.", "hell nah.", null),
  row("bruh", "exclaim", ["bruh"], "bruh moment.", "bruh.", "bruh, what the hell.", null),
  row("bet", "exclaim", ["bet"], "bet!", "bet.", "bet, damn.", null),
  row("oh my god / omg", "exclaim", ["oh my god", "oh my gosh", "omg"], "omg!", "OMG.", "oh my god, damn.", null),
  row("wow", "exclaim", ["wow"], "wow!", "wow. riveting.", "wow, damn.", null),
  row("sus", "exclaim", ["sus"], "that's sus!", "very sus.", "sus as hell.", null),
  // phrases
  row("my bad", "phrase", ["my bad"], "all good!", "yeah, your bad.", "your damn bad.", "No problem."),
  row("let's go", "phrase", ["let's go", "lets go"], "LET'S GO! (hops)", "let's go, I guess.", "let's fucking go.", null),
  row("no cap", "phrase", ["no cap"], "no cap!", "cap.", "no cap, damn.", null),
  row("I'm cooked", "phrase", ["i'm cooked", "im cooked", "i am cooked"], "you're not cooked!", "so cooked.", "cooked as hell.", null),
  row("hold on", "phrase", ["hold on"], "holding!", "holding. impatiently.", "hold on, damn it.", "Still listening."),
  row("scratch that", "phrase", ["scratch that"], "Tidy's on it! (sweeps)", "gone. like your idea.", "scratched that shit.", "Removed the last sentence."),
  row("never mind", "phrase", ["never mind", "nevermind"], "never minded!", "already forgot.", "never mind, damn.", "Noted."),
  row("wait", "phrase", ["wait"], "waiting!", "waiting.", "waiting, damn.", null),
  row("okay so", "phrase", ["okay so", "ok so"], "okay so!", "okay so what.", "okay so, damn.", null),
  row("to be honest / tbh", "phrase", ["to be honest", "tbh"], "honesty!", "tbh, sure.", "honestly, damn.", null),
  row("I don't know", "phrase", ["i don't know", "i dont know", "i do not know"], "that's okay!", "clearly.", "no shit.", null),
  row("thank you", "phrase", ["thank you"], "anytime, friend.", "yeah yeah.", "don't get sappy, damn.", "You're welcome."),
  // the names you say most
  row("Yappy", "name", ["yappy"], "that's me! (hops)", "what.", "what the hell do you want.", "Here."),
  row("Jeisil", "name", ["jeisil"], "Jeisil! say hi!", "the other boss.", "the other damn boss.", null),
  row("Drivia", "name", ["drivia"], "Drivia! (salutes)", "the empire.", "the damn empire.", null),
  row("Yap", "name", ["yap"], "that's our app!", "self-promo, huh.", "shameless as hell.", null),
  row("Hoot", "name", ["hoot"], "Hoot! (the owl waves)", "the owl's busy.", "the owl's busy, damn.", null),
  // dev words
  row("deploy", "dev", ["deploy", "deploys", "deployed", "deploying"], "ship it, good luck!", "pray for prod.", "pray for damn prod.", "Good luck with the deploy."),
  row("merge", "dev", ["merge", "merged", "merging"], "merge it!", "yolo merge.", "merge that shit.", null),
  row("bug", "dev", ["bug", "bugs"], "Tidy hates bugs!", "that's your bug.", "another damn bug.", null),
  row("prod", "dev", ["prod"], "careful in prod!", "prod. on a friday?", "prod. hell.", null),
  row("it works", "dev", ["it works"], "IT WORKS! (big hop)", "shocked.", "it fucking works.", "Nice."),
  row("rollback", "dev", ["rollback", "roll back", "rolled back"], "it happens!", "called it.", "roll that shit back.", null),
];

/** The approved count (design page: "48 triggers"). */
export const REACTION_COUNT = 48;

/** Which column a voice reads. "badassCurse" is Badass with the curse setting on. */
export type ReactionColumn = "bestfriend" | "badass" | "badassCurse" | "professional";

/** The line `voice` says for row `r` (null = this voice never reacts to it). */
export function reactionLine(r: ReactionRow, voice: VoiceId, curseSwap = false): string | null {
  switch (voice) {
    case "bestfriend":
      return r.bestfriend;
    case "badass":
      return curseSwap ? r.badassCurse : r.badass;
    case "professional":
      return r.professional;
    default:
      // Rose has no approved reaction column.
      return null;
  }
}

/**
 * The table keyed by voice: word → line, for every word that voice reacts to.
 * Professional holds only its plain-reply rows; Rose is empty; "badassCurse" is
 * the column curse mode swaps in.
 */
export const REACTIONS_BY_VOICE: Readonly<Record<VoiceId | "badassCurse", Readonly<Record<string, string>>>> = (() => {
  const of = (voice: VoiceId, curse = false): Record<string, string> => {
    const out: Record<string, string> = {};
    for (const r of REACTION_WORDS) {
      const line = reactionLine(r, voice, curse);
      if (line !== null) out[r.word] = line;
    }
    return out;
  };
  return {
    bestfriend: of("bestfriend"),
    badass: of("badass"),
    badassCurse: of("badass", true),
    professional: of("professional"),
    rose: of("rose"),
  };
})();

/** Lower case, straight apostrophes, everything but letters/digits/apostrophes → one space, quote marks dropped. */
export function normalizeSpoken(text: string): string {
  const t = text
    .toLowerCase()
    .replace(/[\u2018\u2019\u02bc`]/g, "'")
    .replace(/[^a-z0-9']+/g, " ")
    // A quote mark around a word ('yo') is not part of it.
    .replace(/(^| )'+|'+(?= |$)/g, "$1")
    .trim();
  return ` ${t} `;
}

const PATTERNS: ReadonlyArray<RegExp> = REACTION_WORDS.map(
  (r) => new RegExp(` (?:${r.say.join("|")})(?= )`),
);

export interface ReactionMatch {
  /** Index into REACTION_WORDS. */
  index: number;
  row: ReactionRow;
}

/**
 * The reaction word said FIRST in `text` that `voice` reacts to (ties → the
 * longer phrase). Null when none — always null for Rose, and for Professional
 * on every body / cuss row.
 */
export function findReaction(text: string, voice: VoiceId): ReactionMatch | null {
  const t = normalizeSpoken(text);
  if (t.trim() === "") return null;
  let best: { index: number; at: number; len: number } | null = null;
  for (let i = 0; i < REACTION_WORDS.length; i++) {
    if (reactionLine(REACTION_WORDS[i], voice) === null) continue;
    const m = PATTERNS[i].exec(t);
    if (!m) continue;
    if (!best || m.index < best.at || (m.index === best.at && m[0].length > best.len)) {
      best = { index: i, at: m.index, len: m[0].length };
    }
  }
  return best ? { index: best.index, row: REACTION_WORDS[best.index] } : null;
}

/** One take's reaction state. Keyed by the partial's take id. */
export interface ReactionState {
  /** The take the state belongs to; null = none seen yet. */
  take: number | null;
  /** This take already reacted (or a reaction was held back): no more. */
  done: boolean;
}

export const REACTION_IDLE: ReactionState = { take: null, done: false };

export interface ReactionCtx {
  /** The capsule's live voice. */
  voice: VoiceId;
  /** The pill is in a take (listening). Partials outside a take never react. */
  listening: boolean;
  /** The chatter setting lets a during-take line through (`allowDuringLine(policy, true, 0)`). */
  chatterAllows: boolean;
  /** A meeting records: takes' reactions stay quiet. */
  meeting: boolean;
  /** `curseAllowed(...)` — the toggle, Badass, no meeting, no screen share. Never the raw setting. */
  curse: boolean;
  /** A random source in [0, 1) for the 1-in-CURSE_EVERY swap. */
  rand: () => number;
  /** The strongest word was already said today (TakeLineMemory.strongDay === today). */
  strongUsedToday: boolean;
}

export interface Reaction {
  line: string;
  index: number;
  kind: ReactionKind;
  column: ReactionColumn;
  /** The line holds the strongest word: the caller records today in the memory. */
  strongest: boolean;
}

export interface ReactionStep {
  state: ReactionState;
  reaction: Reaction | null;
  /** One telemetry line when something happened (fired / held back); never transcript text. */
  log: string | null;
}

/**
 * Fold one partial (`take`, its committed words) into the take's state. Returns
 * the reaction to say, at most once per take.
 */
export function reactToPartial(s: ReactionState, take: number, committed: string, ctx: ReactionCtx): ReactionStep {
  const state: ReactionState = s.take === take ? s : { take, done: false };
  if (state.done || !ctx.listening) return { state, reaction: null, log: null };
  const m = findReaction(committed, ctx.voice);
  if (!m) return { state, reaction: null, log: null };
  const done: ReactionState = { take, done: true };
  const held = !ctx.chatterAllows ? "chatter" : ctx.meeting ? "meeting" : null;
  if (held) {
    return { state: done, reaction: null, log: `pill reaction held take=${take} voice=${ctx.voice} row=${m.index} kind=${m.row.kind} reason=${held}` };
  }
  let column: ReactionColumn = ctx.voice === "badass" ? "badass" : ctx.voice === "professional" ? "professional" : "bestfriend";
  let line = reactionLine(m.row, ctx.voice) as string;
  if (ctx.voice === "badass" && ctx.curse && ctx.rand() < 1 / CURSE_EVERY) {
    const cursed = m.row.badassCurse;
    if (!(isStrongest(cursed) && ctx.strongUsedToday)) {
      column = "badassCurse";
      line = cursed;
    }
  }
  const strongest = column === "badassCurse" && isStrongest(line);
  return {
    state: done,
    reaction: { line, index: m.index, kind: m.row.kind, column, strongest },
    log: `pill reaction fired take=${take} voice=${ctx.voice} row=${m.index} kind=${m.row.kind} column=${column}`,
  };
}
