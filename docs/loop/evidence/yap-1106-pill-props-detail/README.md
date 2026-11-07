# yap-1106-pill-props-detail: before / after next to the approved art

Approved page: `~/Obsidian/Wilson-Brain/Projects/Yap/design-2026-11-06/pill.html`, section 11
"The props, with more detail". Art source: `assets/yappy.js` (the `A.hd` branches of drawScene),
copied byte for byte to `desktop/src/sprites/approved/yappy.js`.

| prop | before (shipped) | after (this item) |
| --- | --- | --- |
| notepad, true size | notepad-before-1x.png | notepad-after-1x.png |
| notepad, 4x | notepad-before-4x.png | notepad-after-4x.png |
| desk + typewriter + mug + pencils, true size | desk-before-1x.png | desk-after-1x.png |
| desk + typewriter + mug + pencils, 4x | desk-before-4x.png | desk-after-4x.png |

The "after" frames ARE the artifact crop: `props.snapshot.test.ts` renders the app's props and the
approved script's props through the same raster and requires zero mismatched pixels (zero colour and
zero position tolerance) for the notepad at three slide-in amounts and the desk at seven typing beats.
The live pill matrix (`scripts/pill-matrix.mjs`) shows the notepad in its transcribing captures; it has
no desk-take phase, so the desk is proven by these frames and the snapshot test.

## Approved sentences kept (pill.html section 11, verbatim)

- "Same pixel size, same place in the scene." Kept: the props draw into the same 176 x 44 scene buffer
  at the shipped anchors (notepad at cx + 12 on the grass line, desk x 4..PW-4 with its top on GY - 2).
- "The notepad gets a clip, a margin line and handwriting that grows." Kept: props.ts drawNotepadHD; the
  handwriting fills over 6 s of noting during a take and clears once the pad has gone.
- "the desk gets wood grain and an edge highlight" Kept: props.ts drawDeskHD.
- "the typewriter gets a carriage, return levers, round keys that light as he types, and a page that
  bounces" Kept: six round keys light left to right (typewriterBeat), the page lifts one px every third beat.
- "a mug and pencils sit on the left." Kept: mug with steam at cx - 30, a yellow and a blue pencil beside it.
- "Shown at true size, then twice the size." The evidence frames are at true size (1x) and 4x.
