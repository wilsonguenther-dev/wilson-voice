# Pill — Dictate island

Runtime pieces for the on-screen Dictate pill (the small always-on capsule that
reacts while you talk). Product vision: `docs/OPEN-SOURCE-ROADMAP.md` §3b–3d.

## Pieces
- `ClassicPill.tsx` — the original obsidian waveform capsule, kept as a
  selectable pill style. Bars driven by a `--level` CSS var each rAF frame.
- `YappyPill.tsx` — the pixel-chick companion pill. A pull-back camera fills the
  capsule with the pixel world while dictating; longer prompts add props.
- `mouth.ts` — `MouthDriver`: turns the live mic level (`audio_level` events)
  into a smoothed 0..1 jaw-open so the companion's mouth mimics real speech.
- `tone.ts` — pure, deterministic `reactiveLine(words, {tone, curseFilter},
  nonce)`: the pill's "yapping" one-liners, bucketed by word count.

> The origami fold engine (a baked crease-pattern player) was prototyped and
> retired — the switchable Classic/Yappy pills replaced it. See the roadmap for
> the history.
