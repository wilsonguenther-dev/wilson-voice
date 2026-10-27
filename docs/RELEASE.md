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
carries no `Authority=` chain, shows `com.apple.security.app-sandbox` as true, or
is missing `flags=0x10000(runtime)` on the app or either sidecar (see
[Entitlement audit](#entitlement-audit)).
`scripts/rebuild_app.sh` calls it rather than repeating the incantation, and
copies both staged sidecars (`desktop/src-tauri/binaries/<name>-<triple>`) into
the bundle first; it checks they are staged before it touches the installed app.

**Never enable the App Sandbox.** It silently kills the CGEvent tap, the
synthesized Cmd-V and `AXIsProcessTrusted`, and the failure looks exactly like a
permissions bug even when every grant is present. `tests/entitlements.rs` fails
the build if `com.apple.security.app-sandbox` is missing or true, if the
microphone entitlement is dropped, or if either dylib-injection entitlement
(`cs.disable-library-validation`, `cs.allow-dyld-environment-variables`) is
granted.

## Entitlement audit

*yap24-OS4, measured 2026-10-26 on Apple M4 Pro, macOS 26.6.2, Developer ID Application identity.*

Every signature `scripts/sign-local.sh` makes carries the **hardened runtime**
(`codesign --options runtime --timestamp`), and its verify step fails unless
`codesign -dv` reads back `flags=0x10000(runtime)` on the app AND on each
sidecar. That matters for more than notarization: `com.apple.security.cs.*`
entitlements are inert when the runtime is off, so an audit run on a
runtime-off signature passes every removal vacuously — which is exactly what
the first draft of this item would have measured.

**Which file signs which binary depends on the path.** `scripts/sign-local.sh`'s
default sign mode (re-signing an existing bundle, and what `scripts/rebuild_app.sh`
calls) is **inside-out, never deep**: deep signing stamps the APP's entitlements
(including the microphone grant) onto every nested binary, so each sidecar is
signed first with its own file, then the bundle seals them. Every `tauri build`
path (`npm run desktop:build` = `sign-local.sh --tauri-build`, `release.yml` →
`tauri-action`, `scripts/test-build.sh`) is different: tauri-bundler signs the
app **and every `externalBin`** with `Entitlements.plist` and never reads
`entitlements/*.plist`. **So the shipped DMG's sidecars carry the app's
entitlements, `device.audio-input` included** (read back from an installed
Tauri-built `/Applications/Yap.app` in the PR #262 review). This item does not
change that; `verify_bundle` prints, per sidecar, whether it carries
`device.audio-input`, and fails in sign mode if it does.

| binary | `sign-local.sh` sign mode: file → grants | `tauri build` / DMG: file → grants |
|---|---|---|
| `wilson-voice` (the app) | `Entitlements.plist` → `device.audio-input`, `app-sandbox` = false | `Entitlements.plist` → same |
| `yap-polish` (llama.cpp + Metal) | `entitlements/yap-polish.plist` → nothing | `Entitlements.plist` → `device.audio-input`, `app-sandbox` = false |
| `yap-diarize` (sherpa-onnx, CPU) | `entitlements/yap-diarize.plist` → nothing | `Entitlements.plist` → `device.audio-input`, `app-sandbox` = false |

Because the Tauri path stamps `Entitlements.plist` onto the sidecars, removing
the two JIT keys from it removes them from the DMG sidecars too; the sidecar
rows of the experiment below (signed with empty files, a strict subset) cover
that.

`app-sandbox` stays present and false (Wilson's rule: utility apps are never
sandboxed). The dylib-injection pair (`cs.disable-library-validation`,
`cs.allow-dyld-environment-variables`) stays absent (YV23).

### The experiment — `scripts/qa/entitlement-diet.sh`

```bash
YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_MODEL_DIR="$HOME/code/wilson-voice-loop/cache/models" \
  bash scripts/qa/entitlement-diet.sh
```

It builds the release binaries (`--features custom-protocol`), then signs one
copy of the app binary per variant with the real identity and the runtime on.
Before a variant's run counts, three things are read back from the signature
and the process: `flags=0x10000(runtime)` (or the script exits 3), the granted
`cs.*` keys match the variant, and a `DYLD_PRINT_LIBRARIES=1` probe prints
**no** dyld lines (the runtime strips `DYLD_*`). A **control** copy signed with
the same identity but WITHOUT the runtime must print dyld lines — proof the
probe can tell runtime-on from runtime-off, i.e. that this experiment can fail.
Then the signed binary does its real work: `--transcribe-file` on the committed
fixture (`tests/fixtures/quick-brown-fox-16k.wav`) — the in-process
transcribe-cpp decode on ggml-Metal — against a scratch `YAP_DATA_DIR` with the
cached Parakeet Q8_0 model linked in and selected (an empty scratch root never
reaches Metal; it starts a model download instead). A pass needs exit 0, the
fixture's words on stdout and `parakeet: using metal backend` on stderr. The
sidecars are signed with their own files and must complete their protocol
handshake (`yap-polish` loads its GGUF with every layer offloaded to Metal and
creates a context before it says ready).

### Result

```
control (same identity, runtime OFF): flags=0x0(none) · DYLD_PRINT_LIBRARIES lines=712 (expected > 0)

| variant | cs.* granted | CodeDirectory flags | DYLD probe | run | engine | output | verdict |
|---|---|---|---|---|---|---|---|
| full | allow-jit allow-unsigned-executable-memory  | flags=0x10000(runtime) | dyld=0 | exit=0 | metal=yes | "The quick brown fox jumps over the lazy dog." | PASS |
| -cs.allow-jit | allow-unsigned-executable-memory  | flags=0x10000(runtime) | dyld=0 | exit=0 | metal=yes | "The quick brown fox jumps over the lazy dog." | PASS |
| -cs.allow-unsigned-executable-memory | allow-jit  | flags=0x10000(runtime) | dyld=0 | exit=0 | metal=yes | "The quick brown fox jumps over the lazy dog." | PASS |
| shipped | none | flags=0x10000(runtime) | dyld=0 | exit=0 | metal=yes | "The quick brown fox jumps over the lazy dog." | PASS |
| yap-polish (own plist) | none | flags=0x10000(runtime) | - | exit=0 | handshake=yes | {"type":"ready","version":"0.9.2","model_loaded":true} | PASS |
| yap-diarize (own plist) | none | flags=0x10000(runtime) | - | exit=0 | handshake=yes | {"type":"ready","version":"0.1.0"} | PASS |
note: --transcribe-meeting is not wired on this tree (NT2) — not run
entitlement-diet: PASS — every variant signed with flags=0x10000(runtime) ran its real work
```

| entitlement | removal passes? | decision |
|---|---|---|
| `cs.allow-jit` | yes — signed-runtime Metal decode passes without it | **removed** |
| `cs.allow-unsigned-executable-memory` | yes — signed-runtime Metal decode passes without it | **removed** |
| `device.audio-input` | not a candidate (the runtime denies the capture device without it) | kept |
| `app-sandbox` = false | not a candidate (rule) | kept |

Why it holds: ggml-Metal and llama.cpp compile their shaders through
`MTLDevice` / `MTLCompilerService` (the 7 s `ggml_metal_library_init` on a
cold run), not into writable-executable pages of this process, and WKWebView's
JavaScript JIT runs in the separate `com.apple.WebKit.WebContent` XPC process
under WebKit's own signature.

**Not covered by this run** — re-run the script after any of these lands:
the GUI path was not launched from a runtime-signed copy (a `--smoke` launch
opens the window and tray on the operator's display), so the WKWebView claim
rests on the WebKit process model, not on a measurement; `--transcribe-meeting`
(NT2) is not wired yet; and the notarized `tauri build` bundle itself is the
final smoke item's job (`scripts/sign-local.sh --tauri-build` now fails its
verify if Tauri's signature lacks the runtime on the app or a sidecar).
If a future engine genuinely needs one of these entitlements, add it back with
a failing signed-runtime run in this section — never on a hunch.

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
   creates a **draft** GitHub Release with every asset plus `latest.json`. Its
   last step is the CI share of the release smoke,
   `scripts/qa/release-smoke.sh --signature-only --app <built Yap.app> --dmg <built .dmg>`:
   a red run means the draft is NOT publishable. A CI runner holds no TCC grants
   and no installed Yap, so the `dr_change` and launch steps cannot run there —
   **the tag path is unsmoked until step 5 has run.**
5. **Release smoke on the draft's own DMG (mandatory — a draft that has not
   passed it is not publishable).** On the release host (every grant held for
   the release identity, any running Yap quit), download the DMG asset from the
   draft and run the full smoke against exactly that file:

   ```bash
   gh release download v0.7.1 --repo wilsonguenther-dev/wilson-voice \
     --pattern '*.dmg' --dir "$SMOKE_DIR"
   DMG="$(ls "$SMOKE_DIR"/*.dmg)"
   MNT="$(mktemp -d)" && hdiutil attach -readonly -nobrowse -mountpoint "$MNT" "$DMG"
   ditto "$MNT/Yap.app" "$SMOKE_DIR/Yap.app" && hdiutil detach "$MNT"
   bash scripts/qa/release-smoke.sh --app "$SMOKE_DIR/Yap.app" --dmg "$DMG" \
     --compare /Applications/Yap.app
   ```

   Exit 0 (`release-smoke: OK`) or the draft stays a draft. A deliberate
   requirement change follows [RELEASING.md](RELEASING.md) (both overrides, the
   `release_note=` line in the notes).
6. Verify the draft before publishing (see *Verify* below), then publish it.
   Publishing is what makes
   `https://github.com/wilsonguenther-dev/wilson-voice/releases/latest/download/latest.json`
   resolve — the `latest` alias only ever points at a *published, non-prerelease*
   release, so a release left as a draft leaves every client on 404.
7. Stage the same release on the site host, the **primary** updater endpoint
   (UPD-A), in the same sitting as publishing it:

   ```bash
   scripts/stage-updater-manifest.sh v0.7.1 "$STAGE"
   ```

   then deploy `$STAGE` per `docs/DEPLOY-SITE.md`. The script stages the
   tarball, its `.sig` and a `latest.json` whose `url` points at the site host,
   so an update offered through the primary also *installs* through it while the
   repo is private. The app takes the newest version across both endpoints, so a
   late re-stage only delays private-window updates; it never hides a GitHub
   release from anyone.

## Manual release (local, when CI can't sign)

Used when notarizing from the Mac that holds the Developer ID cert. Build
through `npm run desktop:build` — never a bare `npx tauri build` — because that
entry point (`scripts/sign-local.sh --tauri-build`) ends in the mandatory
release smoke and exits with its status ([RELEASING.md](RELEASING.md)).
`desktop:build` builds the sidecars itself, and `sign-local.sh` resolves the
Developer ID identity from the keychain.

```bash
cd desktop
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/wilson-voice-v2.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/wilson-voice-v2.key.password)"
npm run desktop:build -- --bundles dmg updater   # exit != 0 = not a release; stop here
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

Stapling rewrites the DMG, so run the full release smoke again on the exact
file you are about to upload (mandatory — `gh release create` below never runs
on a DMG that has not passed it):

```bash
bash ../scripts/qa/release-smoke.sh --app "src-tauri/target/release/bundle/macos/Yap.app" \
  --dmg "src-tauri/target/release/bundle/dmg/Yap_0.7.1_aarch64.dmg" \
  --compare /Applications/Yap.app
```

**No other DMG path is a release path.** A DMG made any other way — e.g.
`npx tauri build --bundles app` + `hdiutil create` + `codesign` + `notarytool`,
which is how 0.9.0 and 0.9.2 were built — MUST pass
`release-smoke.sh --app <that .app> --dmg <that DMG> --compare /Applications/Yap.app`
before it is uploaded; a DMG that has not passed it is not a release.

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

## Verify before publishing

First, the permission-identity check in [RELEASING.md](RELEASING.md): `scripts/qa/perm-probe.sh --compare` against the installed build; `dr_change=yes` goes into the release notes.

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

`plugins.updater.endpoints` lists the site host first
(`https://yap-lemon.vercel.app/updates/latest.json`, staged by step 6) and the
GitHub releases URL second. Every check asks both and offers the newest version
either one announces, so neither endpoint can shadow the other. The GitHub URL
needs no auth only while the repo is public. When paid distribution lands, the endpoint
moves to a licence-checking server that returns the same `latest.json` shape —
the manifest contract above does not change, only who serves it.

## Test channel

To run an unreviewed head next to the real Yap without touching its TCC grants, see [TEST-CHANNEL.md](TEST-CHANNEL.md) (`npm run test-build -- --pr N` → `/Applications/Yap Test.app`).

## LIFECYCLE MANUAL CHECKLIST

Y6-D. These are the lifecycle events a `cargo test` process cannot reach (no
window-server session: it cannot sleep the Mac, unplug a monitor, enter a
fullscreen Space, launch a second app copy or change a TCC grant). The decision
behind each one is unit-tested in `desktop/src-tauri/tests/lifecycle_e2e.rs`;
the EVENT is proven only by a person running the row on a real Mac and writing
the date, the machine and what they saw. A blank result is an open gap, not a
pass. Written 2026-10-27; authoring machine Mac16,8, macOS 26.6.2 — no row has
been run yet.

| Row | Steps | Expected | Date | Machine | Result |
|---|---|---|---|---|---|
| Cold launch with nothing installed | `YAP_DATA_DIR=$(mktemp -d)` launch of the build; no model, no grants | Main window on onboarding; no pill until onboarding is saved | — | — | not yet run |
| Sleep the Mac mid-take | Hold to dictate, close the lid (or  → Sleep) mid-sentence, wake | No paste after wake; the take is in History as recoverable; Escape works in other apps | — | — | not yet run |
| Unplug the monitor the pill is on | Make an external display primary, show the pill, unplug it | Pill reappears on the built-in within ~1.5 s, fully on screen | — | — | not yet run |
| Pill floats over a FULLSCREEN app | Put Safari/Keynote in fullscreen, hold to dictate | Record whether the pill is visible over the fullscreen Space (ARCHITECTURE.md FULLSCREEN limitation) | — | — | not yet run |
| Swap the output device mid-take | Mute-while-dictating ON; start a take on speakers, connect AirPods, stop | Speakers unmuted at their prior volume; AirPods volume untouched; then unplug-headphones-mid-take variant: re-plugged headphones are not muted | — | — | not yet run |
| Launch Yap a second time | With Yap running, `open -n /Applications/Yap.app` | First copy's window comes forward; second process exits; `lsof <data dir>/history.db` shows one process | — | — | not yet run |
| TCC grants survive a relaunch | Quit and relaunch with Microphone, Accessibility, Input Monitoring, Screen Recording granted | No permission prompt on launch; health rows all green | — | — | not yet run |

