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
    grep -q "keychain-profile yap-notary" scripts/release-local.sh
    grep -q "release-local.sh" docs/RELEASE.md
  `,
})

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
    cargo test --features custom-protocol --lib models
    cargo clippy --all-targets --features custom-protocol
  `,
})
