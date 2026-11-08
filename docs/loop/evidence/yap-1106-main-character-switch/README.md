# yap-1106-main-character-switch — integrator pill matrix

Measured 2026-11-08T07:46:37Z on loop3/integration-2 at 55119fce (batch 2, on top of batch 1 / main 215f838c):
`node scripts/pill-matrix.mjs --out <scratch>` (the script refuses a path inside the repo; PNGs are copied here by hand).

Result: 1665 captures = 37 phases x 3 docks x 5 characters (yappy, hoot, tidy, gulp, shelly) x 3 meeting states (none, recording, saved), @2x, headless Chrome. 1661 clean; 4 carried one finding each, all the same one: panel-not-union, window 248x128 vs layout 246x128, side dock, meeting state "saved" (trial/hoot/left, ended/hoot/right, drag-blob/hoot/left, drag-blob/gulp/left).

Those 4 are the known intermittent 2 px settle race on the saved beat (logged by batch 3 on 2026-11-07 16:53Z, before this item existed: "pillmatrix-intermittent=urgent/side/saved panel-not-union 2px"). Re-running the three phases (--only trial / drag-blob / ended, 45 captures each) moved the finding to different cells: trial/yappy/right/saved and drag-blob/gulp/right/saved once each, ended clean. It also hits Yappy, so it is not caused by this item. No empty-text, clipped-text, overflow, one-pill-overlap, slot-off-layout, card or phase-not-applied finding in any of the 1665.

Files here (from the run): idle__<character>__bottom__none.png (each friend whole, in the one pill) and idle__<character>__bottom__recording.png (meeting clock inside the same pill; with Hoot as main he sits at the tape himself).
