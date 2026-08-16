# yap23 — the anti-alias EER, and what it gates

**This file is a verbatim mirror of one block, and it exists so a test can read it.**

OS-8's ordering requirement is that the anti-alias EER be **measured before the
enrollment thresholds are tuned**, or those thresholds permanently encode the
aliasing they were measured through. YV124 (PR #139) instrumented that
measurement and could not take it: `yap-diarize` answers `no_backend` until
YV122 lands, so there are no CAM++ embeddings on any machine, CI included.

YV129's gate — `desktop/src-tauri/tests/enrollment_thresholds_refuse_an_unmeasured_eer.rs`
— reads the block below. While it still contains the literal string
`EER: UNMEASURED`, no tuned enrollment band may ship in the crate: not a
`const`, not a `Default`, not a literal in a shipping call site. The moment the
EER is measured, the block below is replaced with the numbers and the bands may
be tuned against them.

**Why the block is copied here rather than read from the backlog note.** The
SSOT is `~/Obsidian/Wilson-Brain/Notes/Yap-23-Diarization-Backlog-2026-08-15.md`,
which is not in this repository and is on no CI runner. A gate that can only run
on one laptop is not a gate. So the block is mirrored into the repo, and the
same test — on a machine that *does* have the note, and only there — asserts the
mirror is byte-identical to the SSOT, so the copy cannot drift. Point
`YAP23_BACKLOG_PATH` at the note to run that half elsewhere.

**Nothing here may be edited by hand except by re-copying the SSOT block.**

## Flipping this file to MEASURED is three things, not one

A review finding closed the obvious hole in the first version of this gate: the
file it reads lives in the same repository as the diff that tunes the band, so a
tuning PR could edit its own input — flip `EER: UNMEASURED` to
`EER: 0.031 (measured)`, add a band, and go green on CI, where the SSOT does not
exist and the drift check skipped instead of failing. A word is not a
measurement. So a MEASURED state now has to satisfy all three of these, and the
gate fails — never skips — on any one of them:

1. **The mirror must match the SSOT, and the SSOT must be REACHABLE.** Once this
   file stops saying `EER: UNMEASURED`, an absent backlog note is a hard failure.
   That means the flip has to be made and verified on a machine that has the
   vault (or with `YAP23_BACKLOG_PATH` pointed at the note), and the SSOT has to
   be edited first — this file is a copy, in that order.
2. **The block must carry the harness's machine-generated provenance record**
   (below), and the gate verifies it for internal consistency rather than
   reading it: the EER must be the mean of its own FAR/FRR, every rate must be a
   multiple of its sample's resolution (`1/genuine`, `1/impostor`), the
   new-voice floor must BE the equal-error threshold (which is where
   `bands_from_distribution` places it), the auto-confirm edge must sit above it
   and inside the FAR budget, and both operating points must appear in the
   printed ROC sweep, which must be monotone. Forging that is fabricating a
   coherent run, not editing an adjective.
3. **`MIRROR_SHA256` in `tests/support/bands.rs` must be updated in the same
   commit** to the digest the failing test prints, so an edit to this file is a
   second deliberate line in the diff rather than prose that quietly changed.

The provenance record goes INSIDE the YV124 block — in the SSOT first, then
copied here — in exactly this shape (values are illustrative; the harness prints
them):

```
<!-- BEGIN YV129 MEASUREMENT PROVENANCE -->
harness: meeting_eval::tune_enrollment_band
run_id: <iso8601>-<short hash of the run>
corpus_digest: sha256:<64 hex over the fixture WAV + RTTM>
fixture: room-3-near-field
genuine: 18
impostor: 48
eer: 0.0208
far_at_eer: 0.0208
frr_at_eer: 0.0208
eer_threshold: 0.5900
target_far: 0.0208
auto_confirm: 0.6100
new_voice_floor: 0.5900
far_at_auto_confirm: 0.0208
frr_at_new_voice_floor: 0.0208
sweep:
  0.3900 far=0.0625 frr=0.0000
  0.5900 far=0.0208 frr=0.0208
  0.6100 far=0.0208 frr=0.0556
  0.6300 far=0.0000 frr=0.1111
<!-- END YV129 MEASUREMENT PROVENANCE -->
```

`target_far` is a **policy** input, not a measurement — how often the app may
name a stranger with nobody in the loop — and it may not be finer than the
sample's resolution (`1/impostor`). That is why it is a `TargetFar` parameter
supplied by the caller rather than a constant anywhere in the crate.

<!-- BEGIN YV124 MEASURED BLOCK — verbatim mirror of the SSOT, do not reflow -->
**MEASURED (YV124, PR #139, branch `feat/yv124`, macOS 26.5.2 / arm64, corpus `~/yap-eval-corpus/meetings`).**

- **Fold rejection, real audio, gated, on BOTH multi-speaker fixtures:** fixture (e) `room-3-near-field` in-band fold `0.067392` (pre-fix `resample_linear`) vs `0.002596` (shipped 8th-order Butterworth `resample_decimate`) = **28.3 dB removed**; fixture (f) `classroom-6-far-field` `0.040976` vs `0.001406` = **29.3 dB removed**. Each arm is scored against ITS OWN decimation of the same utterance without the added >8 kHz noise, so the number is the folded energy and never the two filters' phase difference. `meeting_eval_anti_alias_eer_regression` asserts ≥20 dB per fixture — the same bar YV92's WER arm holds (which measures 27.8 dB on fixture (c)).
- **EER: UNMEASURED, deliberately not estimated, and the skip now EXPIRES.** YV122 (PR #137) had not merged when this arm landed, so `yap-diarize` on `main` still answers `load_models` with `no_backend` and there are no CAM++ embeddings on any machine, CI included. The arm asks the SHIPPED `DiarizePool` for an embedder, prints `EER=UNMEASURED reason=no inference backend in yap-diarize (load_models answered 'no_backend')`, and then **panics unless the machine has declared its reason in `YAP_EER_UNMEASURED_OK`**. The first shipped version returned quietly here, which made the arm a permanently self-skipping gate — it would have stayed green past YV122 without ever computing an EER. Every other failure of the embedder request (spawn, deadline, protocol, an unknown refusal tag) still panics rather than skipping — `tests/support/diarize.rs::embedder()` is where that is enforced.
- **WHERE the expiry binds, stated plainly, because the first revision of this fix got it wrong.** That revision set `YAP_EER_UNMEASURED_OK: "1"` in `.github/workflows/ci.yml` and claimed CI was therefore held to the expiry. **It was not.** `meeting_eval_anti_alias_eer_regression` opens on `corpus()`, and the corpus is `say`-generated audio under `~/yap-eval-corpus/meetings` that no CI runner has and no checkout carries — so on CI the arm returns on its first line, before the fold assert, before `embedder()` and before the declaration is ever read. Reproduced rather than reasoned: `YAP_EVAL_CORPUS=/nonexistent env -u YAP_EER_UNMEASURED_OK cargo test --features custom-protocol --test meeting_eval anti_alias` → `ok. 8 passed; 0 failed`, green with no declaration. The two catalog models were never the precondition the deleted ci.yml comment named; **the corpus is**, so following that comment (install the models, delete the line) would have left the hole open. The variable is gone from `ci.yml` and `meeting_eval_anti_alias_eer_ci_does_not_declare_what_it_never_reaches` keeps it from drifting back. **Today the declaration half of the expiry binds only on a corpus-equipped developer machine — one machine, the one these numbers were measured on.** The half that binds everywhere, CI included, is the next bullet.
- **The automated half: a corpus-free, model-free backend probe.** `meeting_eval_anti_alias_eer_unmeasured_expires_when_the_sidecar_gains_a_backend` asks the shipped `yap-diarize` directly whether this build has an inference backend — two ordinary non-model files get past `load_backend`'s existence check, so the answer separates `model_not_found` from `no_backend` — and requires the answer to still be `no_backend`. That is the one condition under which `EER: UNMEASURED` is honest, it is checkable with no corpus and no model in 0.15 s, and **no environment variable turns it off**. The day YV122 merges to `main`, CI goes red and stays red until the EER is measured on a corpus machine and written into this block. Proved non-vacuous both ways (M11/M12 in `docs/pr-screenshots/YV124/non-vacuous-mutations.txt`): `load_backend` returning `Ok(192)` or a different refusal tag each turn it red. Deleting that test is the LAST step of closing this item, not the first.
- **The local declaration names its REASON, so it cannot be exported once and forgotten.** `YAP_EER_UNMEASURED_OK` must equal this machine's current `Embedder::skip_tag()` — `no_backend` or `models_missing` — and nothing else counts, the old bare `1` included. An `export YAP_EER_UNMEASURED_OK=no_backend` left in a shell profile therefore stops counting the moment YV122 lands and the machine's reason becomes `models_missing`, which is exactly the moment the EER becomes measurable. Nothing in the repo sets the variable.
- **The EER's resolution is recorded, and it is why the arm no longer asserts on the EER alone.** Fixture (e) yields 18 genuine / 48 impostor pairs, so FRR moves in steps of `1/18 = 0.0556`, FAR in `1/48 = 0.0208`, and the smallest non-zero EER the distribution can express is `0.0104`. Fixture (f), after the overlap trim, yields 19 genuine / 72 impostor pairs (`frr_step 0.0526`, `far_step 0.0139`). Three clearly distinct `say` voices will very likely score `0.0000` on BOTH arms, at which point `eer_shipped <= eer_pre_fix` is `0 <= 0` and measures nothing. So `arm_is_not_worse` gates on three statistics, not one: the EER, the genuine/impostor **margin** (`mean(genuine) - mean(impostor)` in `CosineSimilarity`) and **d′** (that margin in pooled standard deviations). Both of the latter are continuous and cannot bottom out; ROC-AUC is printed for context and never asserted on alone. When both arms sit at the floor the arm prints `EER=SATURATED` naming the floor, so a saturated result can never be read as a measured null.
- **Fixture (f) is a second SCORED fixture, not just a second fold measurement.** OS-8 names far-field classroom room tone and the corpus already carries it: 6 voices, `direct_gain` 0.22–0.55, 21 RTTM turns. Its overlap is what rules it out elsewhere — but this arm slices embeddings from the ground-truth RTTM rather than from a segmentation pass, so YV126's mechanism ceiling does not apply. `unoverlapped_spans` trims each turn to its longest stretch with nobody else talking and drops what is left under 2.0 s (a fixture-construction floor, below every untrimmed turn in either fixture, not an accuracy threshold): **21 turns → 14 spans, all 6 speakers still present**. Correction to the review finding that asked for this: it estimated “only 6 of 21 turns overlap” and “roughly 3× the pairs” — measured, 10 of 21 turns touch a neighbour, and (f) yields 91 pairs against (e)'s 66, i.e. **1.38×**, not 3×. The reason to score it is the acoustics and the sixth identity, not the pair count.
- **The gate itself executes on every machine, even with no model.** `arm_is_not_worse` is a function rather than three inline `assert!`s precisely because the `pool.embed()` loop above it cannot run until YV122 lands. `meeting_eval_anti_alias_eer_the_gate_rejects_a_worse_shipped_arm` runs it in both directions on real `ArmScore` values and regresses the EER, the margin and d′ **each in isolation**, so no one of the three can be dropped and covered for by the others. What remains unexecuted on this base is the I/O loop — write WAV, `pool.embed`, dimension check — and nothing else.
- **The scoring is proved able to fail without a model:** `meeting_eval_anti_alias_eer_arm_scores_the_degradation_os8_predicts` runs the SAME `genuine_and_impostor` + `enrollment_eer` over synthetic vectors and measures clean EER `0.0000` vs common-mode-corrupted EER `0.4410` (18 genuine / 48 impostor pairs), then runs the gate with the arms swapped and requires it to reject. These vectors are NOT CAM++ output and no number here is a claim about the model.
- **Fill in on the next pass, and CI will insist:** when YV122 is on `main`, `meeting_eval_anti_alias_eer_unmeasured_expires_when_the_sidecar_gains_a_backend` turns CI red — on the merge commit, with no corpus and no model needed. Closing it means going to a corpus-equipped machine, installing the two catalog models, **unsetting `YAP_EER_UNMEASURED_OK`**, re-running `cargo test --test meeting_eval anti_alias_eer_regression -- --nocapture`, pasting `eer_pre_fix` / `eer_shipped`, `margin_pre_fix` / `margin_shipped` and `dprime_pre_fix` / `dprime_shipped` for BOTH fixtures here, and only then deleting that test. Until those numbers exist, OS-8's EER half is instrumented, not closed. Acceptance criterion 1 above (`prints EER for both … asserts eer_shipped <= eer_pre_fix`) is therefore **unmet on this base by construction** — no machine can meet it without an inference backend — and it is what the expiry exists to force rather than something this item claims to have delivered. **YV129 must not tune a threshold while the `EER: UNMEASURED` line above is still present** (see YV129's acceptance criteria, where that is now a runnable gate).
- **Spec-name note:** acceptance criterion 2 names the existing arm `anti_alias_wer_regression`; no test carries that substring (it is `meeting_eval_antialias_decimation_does_not_regress_wer_on_broadband_noise`). A `cargo test` name filter that matches nothing exits 0, so that criterion was evidenced against the real name. Unmodified and green: `wer_linear=0.0417 wer_antialiased=0.0417`, `removed_db=27.8`.
<!-- END YV124 MEASURED BLOCK -->
