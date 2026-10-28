// yap24-PKG — SHIP IT WITHOUT HANDS. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §1.7 and §7). Runs after the Y-series packaging items
// (UPD-A/UPD-B in 30-y6). LIC-A (Stripe -> Supabase issuer) stays the licensing item; the audit
// only re-verified its premise: the revocation host (license.rs:117) no longer answers.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-PKG1', prompt: 'yap24-PKG', branch: 'loop/yap24-pkg1-one-command-local-release', gated: 'panel',
  title: 'One local release command, by EXTENDING scripts/sign-local.sh: --dry-run, --check-only, DMG, notarize, staple, verify and the updater manifest through the existing stage-updater-manifest.sh',
  preflight: `
    ( if ! ( test -x scripts/qa/release-smoke.sh ); then echo "DEPENDENCY-NOT-ON-$(printf MAIN): PR 242 (release smoke in sign-local.sh) has not landed; land it, then build PKG1"; exit 0; fi; grep -q -- '--check-only' scripts/sign-local.sh && grep -q -- '--dry-run' scripts/sign-local.sh && bash scripts/sign-local.sh --tauri-build --dry-run --check-only )
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26 (SECURITY-class: signing and notarization)
    DEPENDS: UPD-A (MERGED, 8782b4e: scripts/stage-updater-manifest.sh), UPD-B, and PR #242 (sign-local.sh --tauri-build plus the signed-app release smoke; the preflight holds this item until scripts/qa/release-smoke.sh exists)

    EVIDENCE
      - Releases are hand-run from docs/RELEASE.md; .github/workflows/release.yml is disabled
        (Actions is off account-wide — HARNESS.md "CI mode").
      - memory reference_yap_dmg_notarization: bundle_dmg.sh fails headless (Finder osascript),
        the manual hdiutil path is the reliable one; the notary keychain profile is yap-notary;
        releases must be built from a non-iCloud clone.
      - Tauri updater: signatures are mandatory and cannot be disabled; latest.json needs
        version, platforms.<target>.url and .signature (https://v2.tauri.app/plugin/updater/).

    DO: scripts/release-local.sh <version> [--dry-run] [--check-only]: fresh clone to a scratch
    dir -> npm ci -> stage both sidecars -> tauri build --bundles app -> codesign Developer ID
    with --options runtime --timestamp and Entitlements.plist -> hdiutil UDZO with /Applications
    link -> codesign the DMG -> xcrun notarytool submit --keychain-profile yap-notary --wait ->
    stapler staple -> spctl -a -t open -> .app.tar.gz + .sig + latest.json. --dry-run stops before
    notarize and prints each command; --check-only validates tools, identity presence and the
    keychain profile without building. Never prints a secret; never enables app-sandbox.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - REWRITTEN AS AN EXTENSION, NOT A SECOND SCRIPT: scripts/sign-local.sh already has --tauri-build (identity resolution, signing, verify) and #242 adds the
        signed-app release smoke. A parallel scripts/release-local.sh would be a second release path to keep honest; the acceptance fails if it exists.
        Extend sign-local.sh with: --dry-run (print each command, stop before notarize), --check-only (validate tools, identity presence and the
        yap-notary keychain profile without building), the DMG (hdiutil UDZO with an /Applications link), codesign of the DMG, notarytool submit
        --keychain-profile yap-notary --wait, stapler staple, spctl -a -t open, and the updater artifacts. latest.json comes from the EXISTING
        scripts/stage-updater-manifest.sh <tag> <stage-dir> (UPD-A): reuse it, do not re-implement it.
      - ONE ARGUMENT SHAPE: the preflight and the acceptance both run exactly bash scripts/sign-local.sh --tauri-build --dry-run --check-only (the old pair disagreed: no
        version in one, 0.0.0-ci in the other). The preflight guards with a grep for the new flags FIRST, because on today's main sign-local.sh
        forwards unknown arguments to npx tauri build, and an unguarded preflight would start a real build.
      - THE DOC is docs/RELEASING.md (the permission-identity runbook that exists), not docs/RELEASE.md; the acceptance greps it for the command.
      - Why it is top money: no notarized DMG on launch day means $0. Sign inside-out (sidecars first, no entitlements, then the app without --deep).
  `,
  acceptance: `
    test -x scripts/sign-local.sh
    bash -n scripts/sign-local.sh
    grep -q -- '--check-only' scripts/sign-local.sh
    grep -q -- '--dry-run' scripts/sign-local.sh
    bash scripts/sign-local.sh --tauri-build --dry-run --check-only
    grep -q "keychain-profile yap-notary" scripts/sign-local.sh
    grep -q 'stage-updater-manifest.sh' scripts/sign-local.sh
    grep -q "yap-polish" scripts/sign-local.sh
    grep -q "yap-diarize" scripts/sign-local.sh
    grep -q 'sign-local.sh --tauri-build' docs/RELEASING.md
    ! test -e scripts/release-local.sh
  `,
})

// Panel revisions 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, HIGH, GROUNDED): the DO text
// above already specifies --options runtime --timestamp and signing the app before the DMG,
// which is correct — but tauri.conf.json's signingIdentity is null, so a plain "tauri build"
// leaves the externalBin sidecars (yap-polish, yap-diarize) UNSIGNED, and notarization then
// rejects them (or, signed with --deep, they wrongly inherit the app's JIT entitlements). Sign
// INSIDE-OUT: each Contents/MacOS/yap-polish-* and yap-diarize-* binary first, with
// --options runtime --timestamp and NO entitlements, then the .app itself WITHOUT --deep. The
// acceptance above now runs the script's own --dry-run --check-only and greps for both sidecar
// names, so a plan that skips them fails the gate instead of only failing notarization later.
// Reword the audit's D27 to: keep manual notarized builds (product ledger, non-negotiable);
// PKG1 is a hand-invoked local script, never CI, never automatic.

ITEMS.push({
  id: 'yap24-PKG2', prompt: 'yap24-PKG', branch: 'loop/yap24-pkg2-first-run-without-a-network', gated: 'panel',
  title: 'A first launch on ANY network gets an honest wait: one download with size, progress and an ETA, a plain offline sentence, and the first dictation the moment the model lands. No bundled second model.',
  preflight: `
    grep -q "fn first_run_offline_" desktop/src-tauri/src/models.rs
    grep -q 'etaLabel' desktop/src/ModelSetup.tsx
  `,
  spec: `
    Panel: APPROVED-WITH-CHANGES 2026-10-26. DECIDED: option B (no bundle). Option A (bundle whisper-tiny) is DEFERRED to after the first 100 users, see the last paragraph for the evidence.
    DEPENDS: Y6-A (onboarding ends in one pasted dictation)

    EVIDENCE
      - models.rs:134 "NOTHING SHIPS IN THE DMG": every model (ASR, polish, diarize, Silero VAD
        at vad.rs:491) is fetched on first use from huggingface.co (HEAD 200 on 2026-09-26 for the
        pinned parakeet, Qwen polish and yap-diarize-models revisions) or github.com, each
        sha256-verified against catalog.json / vad.rs constants.
      - A first launch on a plane (Wispr "cannot work on a plane" is Yap's own positioning line)
        cannot dictate at all until a download succeeds.

    DO: option A (default if the panel says yes) — bundle whisper-tiny-Q8_0 as a resource,
    sha256-verified on first launch, used only until the recommended model finishes downloading;
    option B — no bundle, and the onboarding model step detects offline (one HEAD with a 5 s
    timeout) and says "Yap needs one download (N MB) before it can transcribe; connect once."
    Either way a test covers the offline branch.

    Panel revisions 2026-10-26 (Senior Panel plan-audit, two seats + synthesis; applied as preflight/acceptance GATES, not only prose):
      - DUEL, seat B (bundle whisper-tiny, option A) vs seat A (detect offline, one sentence, option B). WINNER: B, with seat B's reframing kept. Evidence:
        (1) catalog.json: parakeet Q8_0 is 731,357,568 bytes, whisper-tiny Q8_0 is 45,981,088 bytes. True, but (2) the 731 MB download is ALREADY
        deferred as far as it can go: Onboarding.tsx:157 mounts useModelSetup({ autoDownload: true }), which starts the recommended download the
        moment the overlay appears, in the background, while the user walks the permission steps; nothing blocks on it until the final dictation
        step (YV54 makes the button wait with its reason). At 100 Mbps that is about a minute, hidden behind the permission steps; at 25 Mbps
        about 4 minutes. So option A buys seconds on a typical network, not minutes. (3) The pre-mortem risk is real: the FIRST dictation of
        every new user would come from whisper-tiny (multilingual, much worse than parakeet), and the first review reads "worse than Apple
        dictation". That is the highest-stakes sample Yap ever gets. (4) The 8 GB budget does NOT decide it (tiny adds about 60 MB, DMG +46 MB). (5) A
        bundled model needs a second engine path, sha verification, switch-over, and an asr_model guard to keep tiny out of meetings; the
        guards would be gates, but the cost buys nothing on the common path. Seat B's real insight stays: the problem is ANY slow network, not only "no
        network". So:
      - CHANGE: the model step shows size, percent and a plain ETA (pure function etaLabel in ModelSetup.tsx, vitest over it), says "Yap needs one
        download (N MB) before it can transcribe; it has started" while waiting, and ONE HEAD with a 5 s timeout detects offline and says "Connect once,
        Yap finishes the download and starts working" (never an error, the download resumes on reconnect). Rust tests first_run_offline_* cover the
        offline branch; the acceptance requires at least one of them to RUN.
      - GATES: the whisper-tiny resource must NOT appear in tauri.conf.json (option A cannot be smuggled in). Meetings stay pinned to the recommended
        model via NT2 R4 (asr_model); a meeting recorded before the download lands waits in waiting_for_model (NT2 R3).
      - REGEX BUG FIXED: the old acceptance used grep -c '0 passed', which matches "10 passed" and "20 passed" and so FAILED a green run, and it
        PASSED a run with failures. It is now: ... 2>&1 | grep -qE 'test result: ok\. [1-9][0-9]* passed'.
      - DEFERRED (after the first 100 users): option A, bundling whisper-tiny for dictation-only first use, with the guards written as gates (dictation
        only, never meetings, asr_model stored, pill says quick model now). Revisit if the install funnel shows drop-off at the model step.
  `,
  acceptance: `
    grep -q "fn first_run_offline_" desktop/src-tauri/src/models.rs
    grep -q 'etaLabel' desktop/src/ModelSetup.tsx
    ! grep -rn 'whisper-tiny' desktop/src-tauri/tauri.conf.json
    cd desktop && npm ci
    npx vitest run src/ModelSetup
    cd src-tauri
    YAP_DATA_DIR="$(mktemp -d)/yap-state" cargo test --features custom-protocol --lib models::tests::first_run_offline_ 2>&1 | tee /dev/stderr | grep -qE 'test result: ok\\. [1-9][0-9]* passed'
    cargo clippy --all-targets --features custom-protocol
  `,
})

// Panel revision 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, LOW, GROUNDED): the
// pre-flight/acceptance named no test, so cargo test --lib models passed on a branch containing
// only the first_run_offline IDENTIFIER with zero matching tests (an unmatched cargo filter
// exits 0 and prints "0 passed"). The acceptance above now filters on the concrete test-name
// prefix and fails when nothing matched.
