/**
 * voices — Yappy's three voices (yap-1106-voice-modes): Best friend, Badass and
 * Professional, plus Rose kept as a fourth. Copy is the approved table from the
 * 2026-11-06 companions design page, section 5 ("Three voices, picked by how
 * long you talk").
 *
 * Each voice has lines for a take of up to 5 words, about 21, about 61 and an
 * essay, plus lines it says while you are still talking. No take line repeats
 * within your last 20 takes (`pickTakeLine` + `rememberTakeLine`).
 *
 * The stored setting keeps its old ids so nobody's choice is lost on upgrade:
 * "friendly" = Best friend, "rude" = Badass, "rose" = Rose, and the new
 * "professional". The phase/nudge copy elsewhere is keyed by `ChatTone`
 * (live.ts) and reads Professional as friendly through `toChatTone`.
 *
 * Pure + deterministic: no I/O, no clock, no randomness. Callers pass the
 * nonce, the memory and the day.
 */

export type VoiceId = "bestfriend" | "badass" | "professional" | "rose";

/** The four take sizes the design names. */
export type TakeBand = "tiny" | "paragraph" | "memo" | "essay";
export const TAKE_BANDS: readonly TakeBand[] = ["tiny", "paragraph", "memo", "essay"];

/** Up to 5 words · about 21 · about 61 · an essay. */
export function takeBand(words: number): TakeBand {
  const w = Math.max(0, Math.floor(Number.isFinite(words) ? words : 0));
  if (w <= 5) return "tiny";
  if (w <= 40) return "paragraph";
  if (w <= 150) return "memo";
  return "essay";
}

export interface VoiceLines {
  /** User-facing name in Settings → Yappy's voice. */
  name: string;
  /** One-line description under the name. */
  blurb: string;
  /** Lines said when a take finishes, per take size. */
  take: Record<TakeBand, string[]>;
  /** Lines said while you are still talking, in escalating order. */
  during: string[];
  /** Badass only: swearing lines swapped in about 1 take in 4 when curse mode is on. */
  curse?: string[];
}

/**
 * Professional lines may carry `{w}` (the take's real word count) and `{saved}`
 * (typing time saved at 40 words a minute), filled by `fillLine`, so a line
 * that states a number states the true one.
 */
export const VOICES: Record<VoiceId, VoiceLines> = {
  bestfriend: {
    name: "Best friend",
    blurb: "Warm, hyped, on your side",
    take: {
      tiny: ["got it, boss.", "short and sweet.", "say less, done.", "quick one, love it.", "boom. pasted.", "tiny but mighty.", "in and out."],
      paragraph: ["that's a whole thought, nice.", "clean little paragraph.", "you said it better than you'd type it.", "zero typos, all you.", "that one had rhythm.", "neat. on to the next.", "nailed the landing."],
      memo: ["okay storyteller, I see you.", "that's a full memo, friend.", "you just saved two minutes of typing.", "that read like you meant it.", "big brain paragraph, delivered.", "every bit of it, written.", "you've got momentum today."],
      essay: ["that's a chapter. proud of us.", "my wing's tired, my heart's full.", "you just dictated a blog post.", "somebody frame this one.", "an essay, and it slaps.", "I'd read that twice.", "okay, legend behavior."],
    },
    during: ["ooh, go on", "taking notes", "mhm, I'm with you", "ok ok ok", "pulling up my desk", "this is getting good", "keep cooking", "typing as fast as I can", "glasses on, let's go", "chapter two, baby", "you're on a tear", "don't stop now", "getting coffee for this", "okay this is a podcast", "my pencil's a stub", "still here, still typing"],
  },
  badass: {
    name: "Badass",
    blurb: "Roasts you, still does the work",
    take: {
      tiny: ["that's it?", "wow. three words.", "a grunt, transcribed.", "you could've typed that.", "riveting.", "noted. barely.", "short king energy."],
      paragraph: ["fine, that one was okay.", "look at you, using sentences.", "almost impressive.", "a paragraph. gold star.", "I've seen worse.", "acceptable. move along.", "not bad for a monday."],
      memo: ["you really like your own voice.", "a lot of words for one idea.", "my wing cramped on line four.", "you forgot how to type?", "okay, TED talk.", "somebody's caffeinated.", "I typed it. I'm not reading it."],
      essay: ["am I your only friend?", "this is a hostage situation.", "write a book already.", "I'm unionizing.", "that's a novella, chief.", "my typewriter filed a complaint.", "you okay? blink twice."],
    },
    during: ["here we go", "oh, you're serious", "mhm. sure.", "keep it moving", "a whole rant, huh", "you done yet?", "my wings hurt", "filing this mess", "an ESSAY, live?!", "chapter two??", "wrist cramp", "still going?!", "chapter FIVE", "I'm aging here", "the sun set", "I've had kids since you started"],
    curse: ["damn, that's a lot.", "hell of a monologue.", "shit, you can talk.", "what the hell, a podcast?", "goddamn, chapter three.", "yapping like hell today.", "holy shit, you finished.", "that was a damn speech.", "ass in chair, wings on keys.", "fine. fuck it. typed it all."],
  },
  professional: {
    name: "Professional",
    blurb: "Calm, short, tells you something useful",
    take: {
      tiny: ["Inserted, {w} words.", "Captured, ready to go.", "Done, no edits needed.", "Short take, pasted.", "Saved to history.", "Pasted at cursor.", "One line, clean."],
      paragraph: ["One paragraph, no fixes.", "Clean take, {w} words.", "Punctuation added.", "Paragraph ready.", "Two sentences, tidied.", "Pasted. Undo with ⌘Z.", "Captured and saved."],
      memo: ["Split into paragraphs.", "{w} words, captured.", "Long take, processed cleanly.", "Structured into parts.", "Saved you about {saved} of typing.", "Filler words removed.", "Ready for review in History."],
      essay: ["Essay captured; headings in Scratchpad.", "Long take, nothing dropped.", "Full draft inserted.", "About {saved} of typing saved.", "Summary available in History.", "Large take complete.", "Draft ready; worth a quick skim."],
    },
    during: ["Listening.", "Recording clearly.", "Good level.", "Capturing.", "Long take, still clear.", "Audio steady.", "Still capturing.", "On track.", "Extended take.", "Long take, all captured.", "Still recording.", "Saving as we go.", "Long session.", "Audio safe on disk.", "Still steady.", "Autosaved."],
  },
  rose: {
    name: "Rose",
    blurb: "Sweet and adoring",
    take: {
      tiny: ["mm, got it 🌹", "of course.", "anything for you.", "done, darling.", "little and lovely.", "for you, always.", "say no more, love."],
      paragraph: ["sweet, got that.", "you say it so well.", "every word, kept.", "that was lovely.", "written with care 🌹", "beautifully put.", "my favorite paragraph today."],
      memo: ["ooh, tell me more.", "I love hearing you talk.", "I hang on every word.", "you could talk to me all day 🌹", "what a story, love.", "I wrote every bit.", "you're glowing today."],
      essay: ["never stop talking to me.", "I'd listen to you forever 🌹", "a whole chapter, just for me?", "I'd frame this one.", "my heart's full, my wings are tired.", "the best essay yet.", "I saved every word, love."],
    },
    during: ["i'm here 🌹", "go on, love", "every word", "keep talking to me", "noting it, love", "so worth writing", "typing every word 🌹", "still listening", "don't stop, love", "your whole story", "i'm hanging on it", "every word matters", "forever, love 🌹", "i'd listen all day", "still every word", "never stop talking"],
  },
};

/** Settings → Yappy's voice, in display order: [stored setting id, voice]. */
export const VOICE_OPTIONS: ReadonlyArray<readonly [string, VoiceId]> = [
  ["friendly", "bestfriend"],
  ["rude", "badass"],
  ["professional", "professional"],
  ["rose", "rose"],
];

/** The stored `companionTone` → voice. Old ids map in place; unknown → Best friend. */
export function voiceFor(setting?: string | null): VoiceId {
  switch (setting) {
    case "rude":
    case "badass":
      return "badass";
    case "professional":
      return "professional";
    case "rose":
      return "rose";
    default:
      return "bestfriend";
  }
}

/** No take line repeats within this many takes. */
export const RECENT_TAKES = 20;
/** Curse mode swaps in a swearing line about 1 take in this many. */
export const CURSE_EVERY = 4;
/** The strongest word, limited to once a day. */
const STRONGEST = /\bfuck/i;
export const isStrongest = (line: string): boolean => STRONGEST.test(line);

/** What the picker remembers across takes (persisted by the caller). */
export interface TakeLineMemory {
  /** The last `RECENT_TAKES` lines said, oldest first (unfilled templates). */
  recent: string[];
  /** The day (YYYY-MM-DD) the strongest word was last said, if any. */
  strongDay: string | null;
}

export const EMPTY_MEMORY: TakeLineMemory = { recent: [], strongDay: null };

export interface PickOpts {
  /** Rotates choices; pass something that changes per take (e.g. a take count). */
  nonce?: number;
  /**
   * Curse mode. Only Badass honours it; off by default. Callers pass the
   * result of `curseAllowed` (setting + voice + meeting / screen-share state),
   * never the raw setting.
   */
  curse?: boolean;
  /** Today as YYYY-MM-DD, for the once-a-day strongest word. */
  today?: string;
  /**
   * yap-1106-curse-setting — a random source in [0, 1). When given, curse
   * mode swaps a line in with probability 1 / CURSE_EVERY per take (and picks
   * the curse line with it); omitted, the nonce decides (every 4th take).
   * Tests pass `seededRandom(seed)`; the pill passes `Math.random`.
   */
  rand?: () => number;
}

/** Why curse mode is not active right now (`null` = it is). Logged, never shown. */
export type CurseBlock = "off" | "voice" | "meeting" | "screen_shared";

export interface CurseGate {
  /** Settings → Yappy's voice → the curse toggle (`companionCurse`, off by default). */
  enabled: boolean;
  voice: VoiceId;
  /** A meeting records, or its after-view (notes ready / failed) is on screen. */
  meeting: boolean;
  /** The screen is being shared or captured, when the shell knows it. */
  screenShared: boolean;
}

/**
 * yap-1106-curse-setting — the one rule for whether Yappy may swear: the toggle
 * is on, the voice is Badass (never Professional, Best friend or Rose), and
 * nobody else can be watching (no meeting state, no screen share).
 */
export function curseBlock(g: CurseGate): CurseBlock | null {
  if (!g.enabled) return "off";
  if (g.voice !== "badass") return "voice";
  if (g.meeting) return "meeting";
  if (g.screenShared) return "screen_shared";
  return null;
}
export const curseAllowed = (g: CurseGate): boolean => curseBlock(g) === null;

/**
 * A small seeded random source in [0, 1) (mulberry32), so a test can replay
 * the exact sequence of takes. Same seed, same sequence.
 */
export function seededRandom(seed: number): () => number {
  let a = Math.floor(Number.isFinite(seed) ? seed : 0) >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export interface PickedLine {
  /** The line as shown, templates filled. */
  text: string;
  /** The line as stored in memory (template form). */
  key: string;
  band: TakeBand;
  /** "band" = the take's own size, "neighbor" = a nearby size, "any" = any line of the voice. */
  source: "band" | "neighbor" | "any" | "curse";
}

const mod = (n: number, m: number) => ((Math.floor(n) % m) + m) % m;

/** Bands ordered by distance from `band` (itself first), used when its lines are all recent. */
function bandsByDistance(band: TakeBand): TakeBand[] {
  const i = TAKE_BANDS.indexOf(band);
  return [...TAKE_BANDS].sort((a, b) => Math.abs(TAKE_BANDS.indexOf(a) - i) - Math.abs(TAKE_BANDS.indexOf(b) - i));
}

/** Typing time saved at 40 words a minute, as "45 s" or "5 min". */
export function typingSaved(words: number): string {
  const sec = Math.round((Math.max(0, words) / 40) * 60);
  return sec < 120 ? `${sec} s` : `${Math.max(1, Math.round(sec / 60))} min`;
}

/** Fill `{w}` and `{saved}` with the take's real numbers. */
export function fillLine(line: string, words: number): string {
  const w = Math.max(0, Math.floor(words));
  return line.replace(/\{w\}/g, String(w)).replace(/\{saved\}/g, typingSaved(w));
}

/**
 * The line Yappy says when a take of `words` words finishes, in `voice`,
 * never one of the lines in `memory.recent` (the last 20 takes). Prefers the
 * take's own size, then the nearest size, then any line of the voice. Every
 * voice has at least 28 take lines, so 20 recent ones can never exhaust it.
 */
export function pickTakeLine(words: number, voice: VoiceId, memory: TakeLineMemory, opts: PickOpts = {}): PickedLine {
  const v = VOICES[voice] ?? VOICES.bestfriend;
  const nonce = opts.nonce ?? 0;
  const band = takeBand(words);
  const recent = new Set(memory.recent.slice(-RECENT_TAKES));
  const fresh = (l: string) => !recent.has(l);

  // Curse mode: Badass only, about 1 take in 4, strongest word once a day.
  // With `rand`, the draw happens on every take (so the sequence is replayable
  // from the seed); without it, every CURSE_EVERY-th nonce.
  const roll = opts.rand ? opts.rand() : -1;
  const swap = opts.rand ? roll < 1 / CURSE_EVERY : mod(nonce, CURSE_EVERY) === 0;
  if (voice === "badass" && opts.curse && v.curse && v.curse.length && swap) {
    const strongUsed = opts.today != null && memory.strongDay === opts.today;
    const pool = v.curse.filter((l) => fresh(l) && !(strongUsed && isStrongest(l)));
    if (pool.length) {
      const idx = opts.rand
        ? Math.min(pool.length - 1, Math.floor((roll * CURSE_EVERY) * pool.length))
        : mod(Math.floor(nonce / CURSE_EVERY), pool.length);
      const key = pool[Math.max(0, idx)];
      return { text: fillLine(key, words), key, band, source: "curse" };
    }
  }

  const order = bandsByDistance(band);
  for (let i = 0; i < order.length; i++) {
    const pool = v.take[order[i]].filter(fresh);
    if (pool.length) {
      const key = pool[mod(nonce, pool.length)];
      return { text: fillLine(key, words), key, band, source: i === 0 ? "band" : "neighbor" };
    }
  }
  // Unreachable while every voice has more take lines than RECENT_TAKES; kept
  // so a future trimmed voice degrades to a repeat instead of silence.
  const all = TAKE_BANDS.flatMap((b) => v.take[b]);
  const key = all[mod(nonce, all.length)] ?? "";
  return { text: fillLine(key, words), key, band, source: "any" };
}

/** Memory after saying `picked` (keeps the last `RECENT_TAKES`). */
export function rememberTakeLine(memory: TakeLineMemory, picked: Pick<PickedLine, "key">, today?: string): TakeLineMemory {
  const recent = [...memory.recent, picked.key].slice(-RECENT_TAKES);
  const strongDay = today != null && isStrongest(picked.key) ? today : memory.strongDay;
  return { recent, strongDay };
}

/** Parse persisted memory; anything malformed reads as empty. */
export function parseMemory(raw: string | null | undefined): TakeLineMemory {
  if (!raw) return { ...EMPTY_MEMORY, recent: [] };
  try {
    const v = JSON.parse(raw) as Partial<TakeLineMemory>;
    const recent = Array.isArray(v.recent) ? v.recent.filter((x): x is string => typeof x === "string").slice(-RECENT_TAKES) : [];
    const strongDay = typeof v.strongDay === "string" ? v.strongDay : null;
    return { recent, strongDay };
  } catch {
    return { ...EMPTY_MEMORY, recent: [] };
  }
}

/**
 * The during-take lines for live tier number `tierIndex` of `tierCount`
 * (live.ts `LIVE_TIERS`; passed in so this file stays a leaf with no cycle):
 * the voice's escalating list cut into one slice per tier, so a long take
 * climbs through all of them.
 */
export function duringLines(voice: VoiceId, tierIndex: number, tierCount = 5): string[] {
  const all = (VOICES[voice] ?? VOICES.bestfriend).during;
  const n = Math.max(1, Math.floor(tierCount));
  const i = Math.min(n - 1, Math.max(0, Math.floor(tierIndex)));
  const from = Math.floor((i * all.length) / n);
  const to = Math.floor(((i + 1) * all.length) / n);
  return all.slice(from, Math.max(to, from + 1));
}

/** Every distinct line a voice can say (take + during + curse). */
export function distinctLines(voice: VoiceId): Set<string> {
  const v = VOICES[voice];
  return new Set([...TAKE_BANDS.flatMap((b) => v.take[b]), ...v.during, ...(v.curse ?? [])]);
}
