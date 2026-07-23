# Pill — origami fold engine

Runtime playback for the "yapping pill" origami mascot. Full rationale, license
audit, and bake plan: [`docs/research/origami-yapping-pill.md`](../../../docs/research/origami-yapping-pill.md).
Product vision: `docs/OPEN-SOURCE-ROADMAP.md` §3b–3d.

## Pieces
- `foldPlayer.ts` — dependency-free Canvas2D player. Interpolates one eased
  scalar `progress` (0 = folded, 1 = open) across baked vertex frames. Unfold =
  `animateTo("open")`, fold-back = `animateTo("fold")` — reverse traversal of the
  same frames, so the refold is **bit-exact**. Ships with `demoFlapModel()` so it
  runs before the real bake.

## `crane.json` schema (what the offline bake must emit)
A `FoldModel`:
```jsonc
{
  "name": "crane",
  "faces":  [[0,1,2], ...],        // vertex-index polygons, shared by all frames
  "edges":  [[0,1], ...],          // optional crease/boundary lines
  "frames": [                       // ordered folded → open
    { "vertices": [[x,y], ...] },   // x,y normalized to [0,1]; index 0 = folded
    ...
    { "vertices": [[x,y], ...] }    // last = fully open mascot
  ]
}
```

## Baking `crane.json` (offline, one-time)
1. Load Amanda Ghassaei's **Origami Simulator** (MIT) with a traditional crane
   crease pattern (its MIT `assets/traditionalCrane.svg`; NOT the named Lang CPs).
2. Drive `globals.setCreasePercent(p)` for `p` in `0..1` (N≈32 steps); after each
   solve settles, read the 3D vertex buffer.
3. Project to 2D (orthographic, front view), normalize into the unit square,
   write each step as a frame. Emit the `FoldModel` JSON (~30KB gzip).
4. Drop it in `desktop/src/pill/models/crane.json`; `foldPlayer.setModel(...)`.

Do the bake in a scratch tool — Origami Simulator is GPL-adjacent as a live dep,
so it stays a build-time tool, never bundled. Its `assets` crease patterns are MIT.

## Wiring (next step, not yet done)
Drive the player from the existing Tauri events in `float-main.tsx`:
`recording=true` → `animateTo("open")`; transcription done → `animateTo("fold")`.
The current CSS pill stays until the player is proven at parity.
