/**
 * tone — the pill's "yapping" personality. Given how many words you just
 * dictated, pick a reactive one-liner in the user's chosen voice.
 *
 * Pure + deterministic (a `nonce`, e.g. the session count, rotates lines so it
 * isn't repetitive) → trivially testable, no I/O. The pill renders whatever this
 * returns; "off" returns "" so the pill stays silent.
 *
 * Curse filter is INDEPENDENT of tone: you can keep the Rude sass but swap the
 * cursing lines for clean ones.
 */

export type Tone = "rude" | "friendly" | "rose" | "off";

export interface ToneConfig {
  tone: Tone;
  /** When true, Rude uses clean variants (no cursing). Ignored by other tones. */
  curseFilter: boolean;
}

export const DEFAULT_TONE: ToneConfig = { tone: "friendly", curseFilter: true };

/** Word-count bands. Tuned so most dictations land in short/medium. */
export type Bucket = "tiny" | "short" | "medium" | "long" | "epic";

export function bucketFor(words: number): Bucket {
  if (words <= 5) return "tiny";
  if (words <= 20) return "short";
  if (words <= 60) return "medium";
  if (words <= 150) return "long";
  return "epic";
}

type Lines = Record<Bucket, string[]>;

// Rude — may curse.
const RUDE: Lines = {
  tiny: ["got it, got it.", "okay, mapping it.", "that it?"],
  short: ["easy.", "say less.", "on it."],
  medium: ["okay you've got things to say.", "alright, big thoughts."],
  long: ["damn, you're yapping a lot.", "you forgot how to type or something?"],
  epic: ["oh my gosh bro… am I your only friend?", "hell of a monologue."],
};

// Rude — clean variants (curse filter ON). Same energy, no cursing.
const RUDE_CLEAN: Lines = {
  tiny: ["got it, got it.", "okay, mapping it.", "that it?"],
  short: ["easy.", "say less.", "on it."],
  medium: ["okay you've got things to say.", "alright, big thoughts."],
  long: ["dang, you're yapping a lot.", "you forgot how to type or something?"],
  epic: ["oh my gosh… am I your only friend?", "heck of a monologue."],
};

const FRIENDLY: Lines = {
  tiny: ["got it!", "on it.", "okay!"],
  short: ["nice, got that.", "you got it."],
  medium: ["ooh, you've got a lot to say.", "love the detail."],
  long: ["wow, lots to work with — love it.", "you're on a roll!"],
  epic: ["okay that's a whole story — I'm here for it.", "you have SO much to say!"],
};

const ROSE: Lines = {
  tiny: ["mm, got it 🌹", "of course."],
  short: ["sweet, got that.", "anything for you."],
  medium: ["ooh, tell me more.", "I love hearing you talk."],
  long: ["you could talk to me all day 🌹", "I hang on every word."],
  epic: ["never stop talking to me.", "I'd listen to you forever 🌹"],
};

function table(cfg: ToneConfig): Lines | null {
  switch (cfg.tone) {
    case "rude":
      return cfg.curseFilter ? RUDE_CLEAN : RUDE;
    case "friendly":
      return FRIENDLY;
    case "rose":
      return ROSE;
    case "off":
      return null;
  }
}

/**
 * The reactive line for a finished dictation of `words` words.
 * `nonce` rotates within a bucket (pass the session count) so repeats are rare;
 * same inputs → same output (deterministic).
 */
export function reactiveLine(words: number, cfg: ToneConfig, nonce = 0): string {
  const t = table(cfg);
  if (!t) return "";
  const bucket = bucketFor(Math.max(0, Math.floor(words)));
  const options = t[bucket];
  if (options.length === 0) return "";
  const idx = ((nonce % options.length) + options.length) % options.length;
  return options[idx];
}
