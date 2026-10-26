# Pill — Dictate island

Runtime pieces for the on-screen Dictate pill (the small always-on capsule that
reacts while you talk). Product vision: `docs/OPEN-SOURCE-ROADMAP.md` §3b–3d.

## Pieces
- `ClassicPill.tsx` — the original obsidian waveform capsule, kept as a
  selectable pill style. Bars driven by a `--level` CSS var each rAF frame.
- `YappyPill.tsx` — the pixel-chick companion pill. A pull-back camera fills the
  capsule with the pixel world while dictating; longer prompts add props.
- `live.ts` — the PURE live-commentary state machine: what Yappy reacts to WHILE
  you talk (tier ladder, props, repeating chatter, speech measured against the
  take's own room tone) plus the loop's frame budget. Unit-tested in
  `live.test.ts` (`npm test`) against synthetic 10s/60s/300s takes.
- `mouth.ts` — `MouthDriver`: turns the live mic level (`audio_level` events)
  into a smoothed 0..1 jaw-open so the companion's mouth mimics real speech.
- `tone.ts` — pure, deterministic `reactiveLine(words, {tone, curseFilter},
  nonce)`: the pill's "yapping" one-liners, bucketed by word count.

> The origami fold engine (a baked crease-pattern player) was prototyped and
> retired — the switchable Classic/Yappy pills replaced it. See the roadmap for
> the history.

## Visual QA: the pill matrix (yap24-UI3)

A green gate cannot see a pill. `node scripts/pill-matrix.mjs --out <scratch dir>`
photographs float.html in every phase x dock {bottom,left,right} x character
{classic,yappy} in headless Chrome (never the app, never a visible window) and
fails on empty text, clipped text and overflow. A PILL item runs
`--only <its phase>` and attaches the PNGs (plus `contact-sheet.png`) to its PR —
never commits them. Phases come from `matrixPhases()` in `forced.ts`, so a new
phase is photographed with no edit to the script. `forced.ts` is the DEV-ONLY
`?phase=&pill=&dock=` handle; a release build compiles it out.
