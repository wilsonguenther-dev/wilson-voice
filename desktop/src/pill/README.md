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

## Accessibility floor (Y5-J)

**Names.** Every phase's accessible name is `phaseAccessibleName(phase, progress)` in
`live.ts` — the phase table's own `phaseVisual(phase).label`, refined to the measured
chunk count while a busy phase has one. Both pills use it (`ClassicPill`'s capsule
`aria-label`, `YappyPill`'s `.kami-stage` group label); neither spells a label by hand.
Guarded by `every_phase_has_an_accessible_name` in `live.test.ts`.

**One live region.** `PhaseAnnouncer.tsx` is the first child of each pill root:
visually hidden, `aria-live="polite"`, `aria-atomic`, text = `phaseAnnouncement(phase)`
— a function of the phase alone, so a waveform frame, a mic level or a chunk tick
cannot re-announce. The shell's `.phase-strip` is plain copy (it used to be a second
region), Yappy's progress is a `progressbar` (it used to be an implicit-polite
`status`), and nothing that moves per frame is inside a live region.

**Focus.** `src/a11y/focus.css` is the one `:focus-visible` rule for both windows;
float.css points `--focus-ring` at `--focus-ring-pill`.

**The panel is non-activating** (`float_pill.rs` builds it `.focused(false)` as a
non-activating NSPanel), so the keyboard never lands in the pill. Every affordance the
pill offers is therefore ALSO reachable without it:

| Pill affordance | Reachable from |
| --- | --- |
| Capsule click — start / stop a take | **Binding:** the dictation hotkey (fn or the chord set in Settings). **Tray:** "Start Dictation" / "Stop Dictation". |
| Blocked / waiting capsule — open Permissions | **Main window:** sidebar → Permissions, and Home's "Setup incomplete" banner (a `<button>` since Y5-J). **Tray:** "Open Yap". |
| Gated capsule — open Settings → License | **Tray:** the upgrade item (`license::UPGRADE_TRAY_LABEL`). **Main window:** Settings → License. |
| Meeting badge — stop recording (`.rec-stop`) | **Tray:** the meeting toggle ("Record a meeting" / its stop label). |
| Meeting card — "Notes ready — Open" | **Tray:** the notes-ready item (`MEETING_NOTES_TRAY_LABEL`). **Main window:** Meetings. |
| Copy card — Copy | **Binding / tray:** "Paste Last Transcript" (⌃⌘V). **Main window:** Home → history. |
| Copy card — Dismiss | No equivalent needed: dismissing loses nothing, the take stays in history. |

A new pill affordance adds a row here, or it is mouse-only.
