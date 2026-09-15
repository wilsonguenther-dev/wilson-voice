# Yap — release runbook

Everything needed to cut a release that ships **working auto-update**. Read it
end to end before the first tag: the updater is the one part of the pipeline
where a silent misconfiguration (wrong key, missing `latest.json`, a URL that
404s) looks exactly like "no update available" to every user forever.

## What a release is made of

| Asset | Produced by | Purpose |
|---|---|---|
| `Yap_<version>_aarch64.dmg` | `--bundles dmg` | The human download. Notarized + stapled. |
| `Yap.app.tar.gz` | `--bundles updater` | The payload the installed app downloads and swaps in. |
| `Yap.app.tar.gz.sig` | `--bundles updater` | minisign signature over the tarball, verified against `plugins.updater.pubkey`. |
| `latest.json` | `includeUpdaterJson: true` (or written by hand) | The manifest the app polls. **Without it the endpoint 404s and auto-update is silently dead.** |

The updater artifacts only appear when `bundle.createUpdaterArtifacts` is true
(it is) **and** the signing key is in the environment. With no key the bundler
prints `A public key has been found, but no private key` and skips the tarball —
a release built that way looks fine and updates nothing.

## Code signing — which identity signs what

macOS keys every TCC grant (Microphone, Accessibility, Input Monitoring) to the
**code signature's designated requirement**, not to the path or the bundle name.
So the rule that matters is short: *if a grant disappeared after a rebuild, the
signature changed.* Nothing about the permission itself broke — macOS is looking
at a program it has never seen before and correctly refusing it.

That is why `bundle.macOS.signingIdentity` in `tauri.conf.json` is `null` and
must stay `null`. It used to be `"-"` — ad-hoc — which has no stable designated
requirement at all: the cdhash changes on every build, so every
`npm run tauri build` handed the user a stranger and every grant had to be
re-given. **Unsigned is not the safer fallback; it is worse.** An unsigned
bundle has no designated requirement either, and it passes any "no ad-hoc"
check. Neither is ever produced on purpose here.

The identity is resolved at sign time from `APPLE_SIGNING_IDENTITY`, or from the
keychain by `scripts/sign-local.sh`. No certificate name, team id or SHA is
committed to this repo.

| | RELEASE | LOCAL |
|---|---|---|
| identity | `Developer ID Application` | `Apple Development` |
| who signs | `.github/workflows/release.yml` → `tauri-action`, via the `APPLE_SIGNING_IDENTITY` repo secret | `scripts/sign-local.sh`, from the keychain |
| notarized + stapled | yes (`APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`) | no |
| `spctl --assess` says | "Notarized Developer ID" | *not* notarized — do not assert that string on a local build |
| designated requirement | stable | stable |
| TCC grants survive a rebuild | yes | yes |

Both profiles give a **stable** designated requirement, which is the only
property TCC persistence needs. Notarization is about Gatekeeper on someone
else's Mac, not about whether your own grants stick. So a locally signed build
is a fully legitimate daily driver; it just cannot pass a notarization assertion,
and any smoke check that asserts "Notarized Developer ID" is asserting a RELEASE
property and must say so.

```bash
# What is on this machine (this is the command every failure prints):
security find-identity -v -p codesigning

# Build + sign in one step. Refuses to run if no identity is present — it never
# falls back to ad-hoc and never leaves the bundle unsigned.
cd desktop && npm run desktop:build

# Sign (or re-sign) a bundle that already exists:
scripts/sign-local.sh "/Applications/Yap.app"

# Just resolve the identity, sign nothing — the preflight:
scripts/sign-local.sh --identity
```

`scripts/sign-local.sh` prefers `Developer ID Application`, falls back to
`Apple Development`, and honours an explicit `APPLE_SIGNING_IDENTITY` (verifying
it is actually in the keychain, so a typo fails loudly instead of producing an
unsigned app). After signing it verifies with `codesign -dv --verbose=4` and
`codesign -d --entitlements :-`, and fails if the result reports `Signature=adhoc`,
carries no `Authority=` chain, or shows `com.apple.security.app-sandbox` as true.
`scripts/rebuild_app.sh` calls it rather than repeating the incantation.

**Never enable the App Sandbox.** It silently kills the CGEvent tap, the
synthesized Cmd-V and `AXIsProcessTrusted`, and the failure looks exactly like a
permissions bug even when every grant is present. `tests/entitlements.rs` fails
the build if `com.apple.security.app-sandbox` is missing or true, if the
microphone entitlement is dropped, or if either dylib-injection entitlement
(`cs.disable-library-validation`, `cs.allow-dyld-environment-variables`) is
granted.

## Signing keys

The updater keypair is minisign; the app trusts exactly one public key, the one
committed at `desktop/src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.

Local truth lives in two files, both `chmod 600`:

```
~/.tauri/wilson-voice-v2.key            # private key (v2 — YV82)
~/.tauri/wilson-voice-v2.key.password   # its password, no trailing newline
~/.tauri/wilson-voice-v2.key.pub        # public half, mirrors tauri.conf.json
```

The same two values are the repo secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` that `.github/workflows/release.yml` feeds
to `tauri-action`. GitHub secrets are write-only — you can never read a password
back out of them, which is exactly how the v1 key became unusable. **If the
files above are ever lost, the key is gone: the only recovery is to generate a
new pair, ship a new `pubkey`, and accept that already-installed builds trusting
the old key can never auto-update again.** Back them up with the rest of `~/.tauri`.

To rotate deliberately:

```bash
cd desktop
PW="$(openssl rand -base64 36 | tr -d '\n=')"
printf '%s' "$PW" > ~/.tauri/wilson-voice-v3.key.password
chmod 600 ~/.tauri/wilson-voice-v3.key.password
npx tauri signer generate -w ~/.tauri/wilson-voice-v3.key --password "$PW"
chmod 600 ~/.tauri/wilson-voice-v3.key
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo wilsonguenther-dev/wilson-voice < ~/.tauri/wilson-voice-v3.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo wilsonguenther-dev/wilson-voice < ~/.tauri/wilson-voice-v3.key.password
# then paste the contents of ~/.tauri/wilson-voice-v3.key.pub into
# tauri.conf.json plugins.updater.pubkey and ship that change BEFORE the release
# that is signed with it.
```

Order matters: the *installed* build verifies the download, so the new public
key has to reach users in a release signed by the **old** key before the new key
signs anything they are expected to accept.

## Tagged release (the normal path)

1. Bump `version` in `desktop/package.json` **and** `desktop/src-tauri/tauri.conf.json`
   (they must match — the updater compares the manifest version against the
   running app's config version).
2. Add the release's line to `docs/CHANGELOG-YAP.md`, merge to `main`.
3. Tag and push:

   ```bash
   git tag v0.7.1 && git push --tags
   ```

4. `.github/workflows/release.yml` builds the sidecar, compiles release, bundles
   `dmg` + `updater`, signs + notarizes when the Apple secrets are present, and
   creates a **draft** GitHub Release with every asset plus `latest.json`.
5. Verify the draft before publishing (see *Verify* below), then publish it.
   Publishing is what makes
   `https://github.com/wilsonguenther-dev/wilson-voice/releases/latest/download/latest.json`
   resolve — the `latest` alias only ever points at a *published, non-prerelease*
   release, so a release left as a draft leaves every client on 404.

## Manual release (local, when CI can't sign)

Used when notarizing from the Mac that holds the Developer ID cert.

```bash
cd desktop
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/wilson-voice-v2.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/wilson-voice-v2.key.password)"
export APPLE_SIGNING_IDENTITY="Developer ID Application: … (TEAMID)"
npm run sidecar
npx tauri build --bundles dmg updater
```

Artifacts land in `src-tauri/target/release/bundle/` (`macos/Yap.app`,
`macos/Yap.app.tar.gz`, `macos/Yap.app.tar.gz.sig`, `dmg/Yap_<version>_aarch64.dmg`).

Notarize + staple the DMG (Apple only staples the container, so staple the DMG,
not the tarball — the tarball is protected by the minisign signature instead):

```bash
xcrun notarytool submit "src-tauri/target/release/bundle/dmg/Yap_0.7.1_aarch64.dmg" \
  --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_PASSWORD" --wait
xcrun stapler staple "src-tauri/target/release/bundle/dmg/Yap_0.7.1_aarch64.dmg"
xcrun stapler validate "src-tauri/target/release/bundle/dmg/Yap_0.7.1_aarch64.dmg"
spctl -a -vvv -t install "src-tauri/target/release/bundle/dmg/Yap_0.7.1_aarch64.dmg"
```

`APPLE_PASSWORD` is an **app-specific password**, not the Apple ID password, and
`notarytool` will not read one out of iCloud Keychain in a non-interactive shell —
pass it explicitly or store it with `xcrun notarytool store-credentials`.

Then upload and write the manifest by hand:

```bash
gh release create v0.7.1 --repo wilsonguenther-dev/wilson-voice --draft \
  --title "Yap v0.7.1" --notes-file /tmp/notes.md \
  "src-tauri/target/release/bundle/dmg/Yap_0.7.1_aarch64.dmg" \
  "src-tauri/target/release/bundle/macos/Yap.app.tar.gz" \
  latest.json
```

## `latest.json` shape

Tauri v2's default (`--format json`) manifest. This is the exact shape the app
parses; every field below is required.

```json
{
  "version": "0.7.1",
  "notes": "Fixes the stuck-recording latch and ships polish presets.",
  "pub_date": "2026-08-10T15:04:05Z",
  "platforms": {
    "darwin-aarch64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlds…",
      "url": "https://github.com/wilsonguenther-dev/wilson-voice/releases/download/v0.7.1/Yap.app.tar.gz"
    }
  }
}
```

- `version` — plain semver, **no leading `v`**. The app updates only when this is
  strictly greater than its own `tauri.conf.json` version.
- `pub_date` — RFC 3339 / ISO 8601 with a timezone. Malformed dates fail the parse
  and the whole check errors out.
- `platforms` — key is `<os>-<arch>`; Apple Silicon is `darwin-aarch64`. Add
  `darwin-x86_64` only when an Intel build is actually published, and point it at
  its own tarball — a client on a missing key sees "no update", not an error.
- `signature` — the **entire contents** of `Yap.app.tar.gz.sig`, inlined as a
  one-line string (it is already base64; do not re-encode, do not add a newline).
- `url` — must point at the `.app.tar.gz` asset **on the release**, not at the
  `latest/download/` alias, so a client that starts a download mid-publish cannot
  get a different build than the one it verified the signature for.

Generate it from the real artifacts rather than by hand:

```bash
jq -n --arg v "0.7.1" --arg sig "$(cat src-tauri/target/release/bundle/macos/Yap.app.tar.gz.sig)" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg notes "…" '
  {version:$v, notes:$notes, pub_date:$date,
   platforms:{"darwin-aarch64":{signature:$sig,
     url:("https://github.com/wilsonguenther-dev/wilson-voice/releases/download/v" + $v + "/Yap.app.tar.gz")}}}' \
  > latest.json
```

## ISSUANCE — the licensing path (LIC-A)

A release that ships a Buy button and a dead issuer is worse than one with no
Buy button. Do this **before** tagging, and re-run the liveness check after the
build that carries a new `YAP_ISSUER_BASE_URL`. Full detail, including the
Runtime Dependencies table, is in `docs/YAP-LICENSING.md`.

```bash
# 1. Deploy the issuer (dedicated Yap project — NOT the Drivia project).
supabase functions deploy yap-license --no-verify-jwt --project-ref "$YAP_SUPABASE_PROJECT_REF"

# 2. Set the secrets. They live here and nowhere else — never in the repo.
supabase secrets set --project-ref "$YAP_SUPABASE_PROJECT_REF" \
  YAP_SIGNING_KEY_PEM="$(cat /path/to/license-signing-ed25519.pem)" \
  YAP_PUBLIC_KEY_SPKI_B64=... \
  STRIPE_WEBHOOK_SECRET=whsec_... \
  RESEND_API_KEY=re_...

# 3. Register the Stripe webhook endpoint (checkout.session.completed only) at
#    https://<ref>.functions.supabase.co/yap-license/stripe-webhook
#    and copy its signing secret into step 2.

# 4. Build the app against that issuer. The URL is compile-time; a build made
#    without it points at an unresolvable .invalid host by design.
export YAP_ISSUER_BASE_URL="https://$YAP_SUPABASE_PROJECT_REF.functions.supabase.co/yap-license"
npm --prefix desktop run desktop:build

# 5. LIVENESS — one command, and it belongs HERE, in the checklist, never at
#    runtime. A 200 with a kids[] array is the only acceptable answer.
curl -fsS "$YAP_ISSUER_BASE_URL/revoked.json" | python3 -c 'import json,sys; d=json.load(sys.stdin); print("kids:", len(d["kids"]))'

# 6. Re-activate the Stripe Payment Link (it ships `active: false` until the
#    delivery path has been walked). Then walk it once, for real:
#    buy → the email arrives → paste the key into a clean YAP_DATA_DIR →
#    the entitlement flips → a dictation completes.
YAP_DATA_DIR="$(mktemp -d)" npm --prefix desktop run desktop:dev
```

Only after that walk is the Forge issuer (`drivia-forge`, `server/src/routes/yap.ts`)
a candidate for decommissioning. Two live issuers signing with one key is fine.
One dead customer path is not.

## Verify before publishing

```bash
# 1. The manifest is reachable and well-formed once published.
curl -sfL https://github.com/wilsonguenther-dev/wilson-voice/releases/latest/download/latest.json | jq .

# 2. Its url actually resolves to the tarball (302 → 200, not 404).
curl -sIL "$(curl -sfL …/latest.json | jq -r '.platforms."darwin-aarch64".url')" | tail -1

# 3. The signature in the manifest matches the .sig asset byte for byte.
diff <(curl -sfL …/latest.json | jq -r '.platforms."darwin-aarch64".signature') \
     src-tauri/target/release/bundle/macos/Yap.app.tar.gz.sig
```

Then install the **previous** version from its DMG, launch it, and let the
launch-time check run (Settings → *Check for updates* forces it). Seeing the
prompt and completing an install is the only proof the whole chain works; a green
release job proves only that files were uploaded.

## Endpoint

`plugins.updater.endpoints` stays on the public GitHub releases URL. The repo is
public, so the asset needs no auth. When paid distribution lands, the endpoint
moves to a licence-checking server that returns the same `latest.json` shape —
the manifest contract above does not change, only who serves it.
