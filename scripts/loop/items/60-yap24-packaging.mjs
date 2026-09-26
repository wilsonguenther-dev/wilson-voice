// yap24-PKG — SHIP IT WITHOUT HANDS. Written 2026-09-26 by the architecture audit
// (docs/ARCHITECTURE-AUDIT-2026-09-26.md §1.7 and §7). Runs after the Y-series packaging items
// (UPD-A/UPD-B in 30-y6). LIC-A (Stripe -> Supabase issuer) stays the licensing item; the audit
// only re-verified its premise: the revocation host (license.rs:117) no longer answers.
// SHARED PREAMBLE + STANDARD GATE: 00-y0-harness-and-gates.mjs.

ITEMS.push({
  id: 'yap24-PKG1', prompt: 'yap24-PKG', branch: 'loop/yap24-pkg1-one-command-local-release', gated: 'panel',
  title: 'One local release command: build, Developer ID sign, hdiutil DMG, notarize, staple, verify, updater artifacts — with a dry run',
  preflight: `
    test -x scripts/release-local.sh
    bash scripts/release-local.sh --dry-run --check-only
  `,
  spec: `
    Panel: pending (SECURITY-class: signing and notarization)
    DEPENDS: UPD-A (#197, open), UPD-B

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
  `,
  acceptance: `
    test -x scripts/release-local.sh
    bash -n scripts/release-local.sh
    bash scripts/release-local.sh 0.0.0-ci --dry-run --check-only
    grep -q "keychain-profile yap-notary" scripts/release-local.sh
    grep -q "release-local.sh" docs/RELEASE.md
    grep -q "yap-polish" scripts/release-local.sh
    grep -q "yap-diarize" scripts/release-local.sh
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
  title: 'A fresh Mac with no network at first launch is told exactly why dictation is waiting — or can dictate with a bundled tiny model',
  preflight: `
    grep -q "first_run_offline" desktop/src-tauri/src/models.rs
  `,
  spec: `
    Panel: pending (product decision: bundle whisper-tiny in the DMG, +~40 MB, or not)
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
  `,
  acceptance: `
    cd desktop && npm ci && cd src-tauri
    test "$(cargo test --features custom-protocol --lib models::tests::first_run_offline_ 2>&1 | tee /dev/stderr | grep -c '0 passed')" -eq 0
    cargo clippy --all-targets --features custom-protocol
  `,
})

// Panel revision 2026-09-26T17:35:00Z (Senior Panel synthesis — applied, LOW, GROUNDED): the
// pre-flight/acceptance named no test, so cargo test --lib models passed on a branch containing
// only the first_run_offline IDENTIFIER with zero matching tests (an unmatched cargo filter
// exits 0 and prints "0 passed"). The acceptance above now filters on the concrete test-name
// prefix and fails when nothing matched.
