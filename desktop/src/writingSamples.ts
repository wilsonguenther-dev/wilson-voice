// Y9-B — the pure half of Settings → Dictation → Writing style.
//
// The Rust side (db.rs `add_writing_sample`) is the real gate; this mirrors it
// so the button can say WHY it is disabled before the user presses it. The
// limits come from the backend's `UserContextView`, never from constants here,
// so the two can never disagree.

export type WritingSample = {
  id: string;
  body: string;
  words: number;
  createdAt: string;
  inUse: boolean;
};

export type UserContextView = {
  writingSamples: WritingSample[];
  customRules: string;
  maxSamples: number;
  minSampleWords: number;
  maxSampleWordsTotal: number;
  maxRulesWords: number;
};

export function countWords(text: string): number {
  const t = text.trim();
  return t === "" ? 0 : t.split(/\s+/).length;
}

/** Why a draft sample cannot be saved yet, or null when it can. */
export function sampleBlocker(draft: string, view: UserContextView): string | null {
  const words = countWords(draft);
  if (view.writingSamples.length >= view.maxSamples)
    return `You have ${view.maxSamples} samples — remove one to add another.`;
  if (words < view.minSampleWords)
    return `${words} of at least ${view.minSampleWords} words — paste a whole email or message you wrote.`;
  const used = view.writingSamples.reduce((n, s) => n + s.words, 0);
  const room = Math.max(0, view.maxSampleWordsTotal - used);
  if (words > room) return `${words} words — only ${room} more fit the ${view.maxSampleWordsTotal}-word budget.`;
  return null;
}

/** Why the rules draft cannot be saved, or null when it can. */
export function rulesBlocker(draft: string, view: UserContextView): string | null {
  const words = countWords(draft);
  return words > view.maxRulesWords
    ? `${words} words — keep custom rules under ${view.maxRulesWords}.`
    : null;
}
