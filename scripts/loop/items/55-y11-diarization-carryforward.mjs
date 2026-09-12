// Y11 — THE PARKED DIARIZATION QUEUE. The yap23 loop was stopped mid-finisher on
// a fast-track order; six items were PARKED with real blocking defects, each with
// a tracking issue on the repo carrying the `audit-carryforward` label, and six
// PRs left open. Verified live on 2026-09-12 with
// `gh issue list --repo wilsonguenther-dev/wilson-voice --state open` (6 open,
// #150-#155) and the refs `origin/feat/yv127`…`origin/feat/yv134`.
// docs/loop/HARNESS.md records the same six PRs and says Drain settles them.
//
// The resume note is explicit that PARKED = the FIRST work of the next Yap
// session, and that ALL SIX also need a rebase picking up `min_embed`.
//
// Also carried forward, and the reason Y11-G exists: the eval numbers standing
// in the repo are REGRESSION FLOORS measured on a `say`-generated synthetic
// corpus (DER 0.34-0.45, EER 0.272 — the embedder hears the synthesizer), and a
// real-voice corpus is a known, named next need.
//
// This file runs LAST. Diarization is the notetaker's accuracy layer; nothing in
// Wilson's 2026-09-12 list depends on it, and it must not delay the dictation
// work. It is in the plan because leaving six blocking defects untracked in a
// shipped subsystem is how they become folklore.
//
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'Y11-A', prompt: 'Y11', branch: 'loop/y11-a-rebase-the-six-parked-branches-on-min-embed', gated: null,
  title: 'Rebase the six parked branches onto main so each is evaluated against the shipped min_embed, not against what main was',
  preflight: `
    test 0 -eq "$(git branch -r --list 'origin/feat/yv1*' | wc -l)"
    gh pr list --repo wilsonguenther-dev/wilson-voice --state open --json number --jq 'length' | grep -qx 0
  `,
  spec: `
    MEASURED on 2026-09-12:
      git for-each-ref  ->  origin/feat/yv127 (d6667ac), yv128 (6e386e1),
                            yv129 (3d78900), yv130 (77a0c95), yv131 (5353261),
                            yv134 (de0c4d6), all dated 2026-08-15/16.
      origin/main       ->  4e8c9adf (YV126).
      gh issue list     ->  #150-#155, all labelled audit-carryforward.

    The resume note's instruction, which is the whole of this item: all six
    "need a rebase picking up min_embed". Until they do, every review of them is
    a review of a tree that no longer exists — and the sibling lesson here is
    exact: main moves under long loops, so rebase before evaluating checks.

    Do, per branch, in issue order (#150 first):
      1. Rebase onto current main. Rerun the full gate
         (docs/loop/HARNESS.md "The gate", all eight commands, exit codes read
         bare).
      2. Re-read its tracking issue and decide ONE of three outcomes, recorded
         in the PR:
            - the defect is fixed by the rebase (main moved past it) -> close the
              issue with the evidence and merge or close the PR accordingly;
            - the defect survives -> the PR stays open, the issue stays open, and
              the specific item below (Y11-B..F) owns the fix;
            - the branch is superseded -> close the PR naming the commit that
              supersedes it, and keep the issue if the defect is still live.
      3. Never merge a rebased branch whose tracking issue is still open and
         unaddressed. That is the exact mistake that created six carry-forwards.
      * Also settle the one open watch item: docs/loop/HARNESS.md and the resume
        note both flag a transient red main run on c3fd5d89 (cargo test exit
        101, logs expired, two green runs since). Run
        \`cargo test --features custom-protocol\` ten times in a row on current
        main and record the pass count in the PR body. If it recurs, capture the
        log immediately — "CI is flaky" must not be allowed to take root.

    What NOT to do:
      - Do NOT merge a parked branch to clear the queue. Six open PRs is not the
        problem; six unaddressed defects is.
      - Do NOT close a tracking issue without the evidence in the comment.
      - Do NOT squash the six into one branch. Each has its own defect and its
        own issue.
  `,
  acceptance: `
    test 0 -eq "$(git branch -r --list 'origin/feat/yv12*' | wc -l)"
    test 0 -eq "$(git branch -r --list 'origin/feat/yv13*' | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol ; test $? -eq 0
    cargo clippy --all-targets --features custom-protocol ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-B', prompt: 'Y11', branch: 'loop/y11-b-overlap-honesty-in-the-shipped-string', gated: null,
  title: 'Issue #150: the impossibility framing survives in transcript.ts where the guard cannot see it',
  preflight: `
    gh issue view 150 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #150, title verbatim: "YV127 overlap honesty (#142): the
    impossibility framing survives in transcript.ts and the guard cannot see it".
    Labelled \`audit-carryforward\` = "Verified finding carried forward from a
    reflection/cleanup pass".

    Read #150 and PR #142 in full before writing code. The shape of the defect,
    which is the shape this whole plan keeps finding: a guard was added, a
    string it was supposed to police lives in a file the guard's scope does not
    cover, and the guard therefore passes while the wrong sentence ships. A grep
    proving absence with the wrong SCOPE proves nothing.

    Do:
      * Fix the surviving string in \`desktop/src/meetings/transcript.ts\` so the
        user-facing framing is accurate.
      * WIDEN THE GUARD so it could not have missed it: the guard must cover
        every file that can produce user-facing transcript copy — the TS
        rendering modules and the Rust export path both — and the test must name
        its scope explicitly rather than relying on a default search root.
      * Prove it non-vacuously: reintroduce the offending sentence in a scratch
        copy and assert the guard now fails. Record that in the PR body (the
        repo's own convention, docs/pr-screenshots/YV105/…/non-vacuous-mutations.txt),
        and add the row to Y7-C's docs/loop/MUTATIONS.md table.
      * Keep \`tests/meeting_transcript_render_two_track.rs\`,
        \`meeting_transcript_render_single_track_unchanged.rs\` and the vitest
        \`src/meetings/transcript.test.ts\` green.
      * Close #150 with the evidence.

    Depends on Y11-A.

    What NOT to do:
      - Do NOT widen the guard by searching the whole repo without a scope. A
        repo-wide grep that matches a doc or a test fixture is a guard that will
        be disabled the first time it is inconvenient.
      - Do NOT fix the string without fixing the guard. The guard is the defect.
  `,
  acceptance: `
    gh issue view 150 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -q 'transcript.ts' docs/loop/MUTATIONS.md
    cd ${APP} && npm ci
    npm test ; test $? -eq 0
    cd src-tauri
    cargo test --features custom-protocol --test meeting_transcript_render_two_track ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_transcript_render_single_track_unchanged ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'DB-E', prompt: 'Y11', branch: 'loop/db-e-speaker-profiles-store-the-weights-digest', gated: null,
  title: 'Issue #151: speaker_profiles stores a catalog id where it must store the pinned weights digest',
  preflight: `
    gh issue view 151 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #151, verbatim: "YV128 speaker_profiles (#143): the model
    skip stores a catalog id, not the pinned weights digest".

    Why it is blocking and not cosmetic: a speaker embedding is only comparable
    to another embedding produced by the SAME WEIGHTS. A catalog id is a
    mutable label — the same id can point at re-quantized or updated weights —
    so a profile enrolled under one set of weights will be silently scored
    against another, and the failure looks like a bad match rather than a
    schema bug. The repo already pins digests elsewhere: YV123 vendored the
    models via a mirror and \`tests/supply_chain.rs\` asserts the sidecar pins
    sherpa-onnx exactly. This is that same discipline, one layer up.

    Do:
      * A migration adding the weights DIGEST to \`speaker_profiles\` (and to the
        skip record the issue names), idempotent
        (tests/db_migration_idempotent.rs must stay green).
      * Enrollment records the digest of the weights that produced the
        embedding. Scoring REFUSES to compare across digests — refuses, not
        silently re-embeds and not silently scores anyway. A refusal is a
        visible state the UI can explain; a cross-weights score is a wrong
        answer.
      * A re-enrollment path so a user whose model changed can re-enroll rather
        than lose their roster.
      * There are no users of this feature to migrate (it shipped days before
        the loop stopped), so prefer the correct schema over a compatibility
        shim — but do not destroy existing rows: mark them as
        unknown-digest and refuse to score them until re-enrolled.

    Tests \`tests/speaker_profile_digest.rs\`: enrollment stores the digest;
    scoring across digests refuses; an unknown-digest row is not scored and not
    deleted; the migration is idempotent; re-enrollment clears the refusal.

    Close #151 with the evidence. Depends on Y11-A.

    What NOT to do:
      - Do NOT store the catalog id as a proxy for the digest.
      - Do NOT silently re-embed on a digest mismatch.
      - Do NOT delete existing profile rows.
  `,
  acceptance: `
    gh issue view 151 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -qE 'weights_digest|weights_sha' desktop/src-tauri/src/db.rs
    test -f desktop/src-tauri/tests/speaker_profile_digest.rs
    grep -q 'scoring_across_digests_refuses' desktop/src-tauri/tests/speaker_profile_digest.rs
    grep -q 'an_unknown_digest_row_is_not_scored_and_not_deleted' desktop/src-tauri/tests/speaker_profile_digest.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test speaker_profile_digest  ; test $? -eq 0
    cargo test --features custom-protocol --test db_migration_idempotent ; test $? -eq 0
    cargo test --features custom-protocol --test supply_chain            ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-C', prompt: 'Y11', branch: 'loop/y11-c-enrollment-bands-retuned-on-the-scored-population', gated: null,
  title: 'Issue #152: bands tuned on utterance pairs, applied to roster-max centroid scoring — FAR 1.000 on the shipped path',
  preflight: `
    gh issue view 152 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #152, verbatim: "YV129 enrollment matching (#144): bands
    tuned on utterance pairs, applied to roster-max centroid scoring
    (FAR 1.000)". The resume note flags it with an exclamation mark for a
    reason: "FAR 0.034 tuned vs 1.000 shipped path".

    This is the single most serious defect in the parked queue. A false-accept
    rate of 1.000 on the shipped scoring path means the matcher accepts
    EVERY candidate — the feature reports confident speaker identities that are
    not identities at all. And the tuned number that was recorded, 0.034, was
    measured on a DIFFERENT scoring population (utterance-to-utterance pairs)
    than the one that ships (max over a roster of centroids). A threshold tuned
    on one population and applied to another is not a threshold.

    Do:
      * Retune the bands on the POPULATION THAT SHIPS: roster-max over
        centroids, at the roster sizes the product actually produces. Report FAR
        and FRR at the chosen operating point, and report them per roster size —
        roster-max FAR grows with roster size and a single number hides that.
      * Where the honest answer is "this mechanism cannot reach a usable FAR at
        roster size N", SAY SO and gate the feature at N rather than shipping a
        threshold that cannot hold. A refused identification is a product state;
        a wrong one is a defect. YV126's own resolution was "a floor instead of
        a reject", so the precedent for an honest floor already exists in this
        subsystem.
      * Every number written into the repo must name the population, the corpus,
        the roster size and the date. The standing failure mode here is a
        measurement outliving the thing that produced it, and ci.yml already
        carries a guard built for exactly that
        (\`meeting_eval_anti_alias_eer_measurement_stays_backed_by_a_real_backend\`)
        — keep it green and follow its example.
      * The corpus caveat is not optional: the numbers standing in the repo are
        floors on a \`say\`-generated synthetic corpus where the embedder hears
        the synthesizer. Any number produced here inherits that caveat until
        Y11-G lands a real-voice corpus, and the PR body must say so in one
        sentence.

    Tests: extend \`tests/diarization_metrics.rs\` and \`tests/meeting_eval.rs\`
    with the roster-max arm, and add
    \`bands_are_tuned_on_the_population_that_ships\` asserting the tuning
    harness and the shipped scorer call the SAME function.

    Close #152 with the numbers. Depends on Y11-A, DB-E.

    What NOT to do:
      - Do NOT re-report the 0.034 figure. It describes a path that does not ship.
      - Do NOT ship a threshold you could not measure.
      - Do NOT present a synthetic-corpus number as a real-voice number.
  `,
  acceptance: `
    gh issue view 152 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -q 'bands_are_tuned_on_the_population_that_ships' desktop/src-tauri/tests/diarization_metrics.rs
    grep -qE 'roster' desktop/src-tauri/tests/meeting_eval.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test diarization_metrics ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_eval        ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_cluster_attribution ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-D', prompt: 'Y11', branch: 'loop/y11-d-split-partition-seeding-and-the-outlier', gated: null,
  title: 'Issue #153: split_partition\'s farthest-pair seeding does not separate speakers when an outlier is the far point',
  preflight: `
    gh issue view 153 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Tracking issue #153, verbatim: "YV130 correction UX (#145):
    split_partition's farthest-pair seeding does not separate speakers when an
    outlier is the far point".

    A farthest-pair seed is the textbook 2-means initialisation and it has a
    textbook failure: if the two most distant embeddings are one real speaker
    and one outlier (a cough, a door, a clipped segment), the split separates
    the outlier from everything else and the two speakers stay merged. The
    user-visible symptom is a correction action that appears to do nothing,
    which is worse than an absent feature — this lives under "correction UX"
    for that reason.

    Do:
      * Replace or guard the seeding. Options, in the order worth trying:
        k-means++ style probabilistic seeding; a trimmed farthest pair that
        excludes points beyond a robust distance quantile; or a seed pair chosen
        to maximise the resulting partition's separation rather than the pair
        distance. Implement ONE, measured against the others on the eval corpus,
        and say in the PR body which you tried and what each scored.
      * The outlier itself must go somewhere defensible: assign it to its
        nearest cluster or to a named noise bucket, and NEVER let one outlier
        become a "speaker" in the roster. A phantom speaker in the transcript is
        the worst outcome of this bug.
      * Reuse \`tests/diarize_cluster_distance_threshold.rs\` and
        \`diarize_cluster_rank_and_floor.rs\` — YV126 established the distance
        threshold and the floor, and this fix must not move either.
      * Regression case, committed: a synthetic partition with one extreme
        outlier and two genuinely distinct speakers, where the old seeding
        provably fails and the new one provably separates. That fixture IS the
        proof; without it this is an unverifiable refactor.

    Tests \`tests/diarize_split_seeding.rs\`: the outlier fixture separates the
    two speakers; one outlier never becomes a roster speaker; the seeding is
    deterministic for a given input (no unseeded randomness — the loop's
    generated scripts forbid nondeterminism and an eval that changes per run is
    not an eval); the YV126 threshold and floor are unchanged.

    Close #153 with the numbers. Depends on Y11-A, Y11-C.

    What NOT to do:
      - Do NOT introduce unseeded randomness. If the seeding is probabilistic,
        the seed is an explicit parameter.
      - Do NOT let an outlier become a speaker.
      - Do NOT change the YV126 distance threshold in this item.
  `,
  acceptance: `
    gh issue view 153 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    test -f desktop/src-tauri/tests/diarize_split_seeding.rs
    grep -q 'the_outlier_fixture_separates_the_two_speakers' desktop/src-tauri/tests/diarize_split_seeding.rs
    grep -q 'one_outlier_never_becomes_a_roster_speaker' desktop/src-tauri/tests/diarize_split_seeding.rs
    grep -q 'seeding_is_deterministic_for_a_given_input' desktop/src-tauri/tests/diarize_split_seeding.rs
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test diarize_split_seeding              ; test $? -eq 0
    cargo test --features custom-protocol --test diarize_cluster_distance_threshold ; test $? -eq 0
    cargo test --features custom-protocol --test diarize_cluster_rank_and_floor     ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-E', prompt: 'Y11', branch: 'loop/y11-e-false-mechanism-sentence-in-the-shipped-asset', gated: null,
  title: 'Issues #154 and #155: a false mechanism claim in a shipped asset, and a comment naming call sites that do not exist',
  preflight: `
    gh issue view 154 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    gh issue view 155 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
  `,
  spec: `
    Two issues, one class of defect, so one item.

    #154, verbatim: "YV131 cross-device drift (#146): the false-mechanism
    sentence survives in the shipped cohort asset and its generator".
    #155, verbatim: "YV134 phase-closing E2E (#149): the 0.35 comment claims
    shipped call sites that do not exist".

    Both are FALSE CAPABILITY CLAIMS in the tree: prose asserting a mechanism or
    a call site that is not there. That class is a BLOCKING finding by the
    reviewer contract, and it is corrosive beyond its size — the next engineer
    reads the comment, believes the mechanism, and builds on it. The repo has
    already been bitten: ci.yml carries a long note about an earlier revision of
    that very file claiming a precondition it never reached
    ("It was not, and could not be ... The arm returns on its first line here").

    Do:
      * #154: fix the sentence in the shipped cohort asset AND in its GENERATOR,
        which is the part that matters — fixing only the asset means the next
        regeneration restores the false claim. Then add the guard that would
        have caught it, scoped to include generated assets, and prove it
        non-vacuously.
      * #155: delete or correct the 0.35 comment, and — the real fix — add the
        assertion that a comment naming a call site must name one that exists.
        The repo has the precedent:
        \`meeting_eval_anti_alias_eer_ci_does_not_declare_what_it_never_reaches\`
        is exactly this kind of guard. Follow it, and keep
        \`tests/matrix_coverage.rs\` green.
      * Both rows go into Y7-C's docs/loop/MUTATIONS.md.
      * Then sweep once for the same class across the diarization modules: any
        doc comment naming a constant, a call site or a mechanism must name one
        that exists. Report the count found in the PR body, fix them, and say
        plainly if the sweep is not exhaustive and why — an honest partial sweep
        beats a claimed complete one.

    Close #154 and #155 with the evidence. Depends on Y11-A.

    What NOT to do:
      - Do NOT fix the asset without fixing the generator.
      - Do NOT delete a comment to pass a guard. Either the mechanism exists and
        the comment is right, or the comment goes and the guard proves it went.
      - Do NOT claim an exhaustive sweep you did not run.
  `,
  acceptance: `
    gh issue view 154 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    gh issue view 155 --repo wilsonguenther-dev/wilson-voice --json state --jq .state | grep -qx CLOSED
    grep -q 'cohort' docs/loop/MUTATIONS.md
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test matrix_coverage ; test $? -eq 0
    cargo test --features custom-protocol --test meeting_eval    ; test $? -eq 0
    cargo test --features custom-protocol ; test $? -eq 0
  `,
})

ITEMS.push({
  id: 'Y11-F', prompt: 'Y11', branch: 'loop/y11-f-real-voice-eval-corpus', gated: 'panel',
  title: 'A real-voice eval corpus to replace the synthetic one — the numbers are only floors until it exists',
  preflight: `
    test -f desktop/src-tauri/tests/fixtures/real_voice_manifest.json
    cd ${APP} && npm ci && cd src-tauri && cargo test --features custom-protocol --test meeting_eval
  `,
  spec: `
    GATED: 'panel'. The mechanism is clear; what a corpus of real human voices
    may contain and where it may live are Wilson's calls, not an agent's.

    THE PROBLEM, from the yap23 close-out verbatim: "Eval numbers so far are
    REGRESSION FLOORS on a synthetic say-voice corpus (DER 0.34-0.45,
    EER 0.272 — CAM++ hears the synthesizer; real-voice corpus is a known next
    need)." A speaker embedder scored on text-to-speech output is being asked to
    tell apart one synthesizer from itself; the numbers bound regressions and
    say nothing about accuracy on people. Every number Y11-C produces inherits
    that ceiling.

    WHAT THE PANEL AND WILSON MUST DECIDE before this item is built:
      1. WHOSE VOICES. A public research corpus under a licence that permits
         this use, voices recorded with explicit consent, or both. Each has a
         different consent and licence posture.
      2. WHERE IT LIVES. It cannot be committed to a public repo. The existing
         scan corpus precedent is an external drive plus a manifest with
         checksums in the repo — the eval already works this way
         (tests/fixtures/meeting_eval_manifest.json plus its .sha256, and
         meeting_eval.rs opening on a corpus path no runner has).
      3. CONSENT AND RETENTION. Recorded voices are biometric data. How long is
         it kept, who can access it, and what does a contributor's withdrawal
         require? Yap's whole position is local-only privacy; an eval corpus of
         real voices sitting on a build box is the one place that promise could
         be undone by its own tooling.
      4. WHETHER CI EVER SEES IT. The answer is almost certainly no, and the
         existing pattern already handles that honestly: the corpus arm returns
         on its first line in CI, and a separate guard asserts the measurement
         stays backed by a real backend. Confirm rather than assume.

    BUILD REGARDLESS, because it is useful the moment a corpus exists:
      * A manifest schema + checksum file for a real-voice corpus, mirroring the
        existing meeting_eval manifest exactly, with an EMPTY manifest committed.
      * The eval arm that consumes it, skipping with a NAMED reason when the
        corpus is absent — never a silent skip, which is how an unmeasured
        number gets reported as measured.
      * A report that prints DER/EER per corpus and refuses to print a single
        blended number across a synthetic and a real corpus. Blending them is
        how the synthetic ceiling would disappear from the record.
      * Every published number carries its corpus name. Update the existing
        recorded numbers to say "synthetic" explicitly, which is a one-line
        honesty fix that does not need the panel.

    Depends on Y11-C.

    What NOT to do:
      - Do NOT commit voice audio to the repo.
      - Do NOT record anyone without explicit consent.
      - Do NOT report a blended number.
      - Do NOT skip the arm silently.
  `,
  acceptance: `
    test -f desktop/src-tauri/tests/fixtures/real_voice_manifest.json
    test -f desktop/src-tauri/tests/fixtures/real_voice_manifest.sha256
    grep -q 'synthetic' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'refuses_to_blend_corpora_into_one_number' desktop/src-tauri/tests/meeting_eval.rs
    grep -q 'absent_corpus_skips_with_a_named_reason' desktop/src-tauri/tests/meeting_eval.rs
    test 0 -eq "$(find desktop/src-tauri/tests/fixtures -name '*.wav' -newer desktop/src-tauri/tests/fixtures/README.md | wc -l)"
    cd ${APP} && npm ci && cd src-tauri
    cargo test --features custom-protocol --test meeting_eval        ; test $? -eq 0
    cargo test --features custom-protocol --test diarization_metrics ; test $? -eq 0
  `,
})
