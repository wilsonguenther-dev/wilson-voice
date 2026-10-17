# Yap — landing-page deploy runbook

The Yap site is four hand-authored static files in `site/dist/` — plus the
Departure Mono woff2 the page sets its headings in, and its OFL licence text —
plus one very large binary: the notarized `.dmg` every visitor is there to download. Those two
halves live in different places on purpose — the HTML is in git, the DMG is a
GitHub release asset — so **deploying the site is a staging step, not a `git
push`**. This file is the whole procedure.

## Why Vercel

Forge (the self-hosted box) serves the site fine, but its edge caps a single
response at **25 MB**, and the DMG already crossed it (`Yap-0.9.1-arm64.dmg` is
31.9 MB) and keeps growing with every model and sidecar we bundle. The first release that crosses 25 MB would break the
Download button with no build failure and no test to catch it — the page would
still be 200, the button would just die. Vercel's static file limit is 100 MB, so
the download lives there now.

**Forge stays live as a mirror.** Nothing about the Forge deploy, its DNS, or its
config is touched by this runbook. Which host becomes canonical — and which
custom domain points at it — is Wilson's call, not the deploy script's.

- Vercel scope/team: **`wilson-guenthers-projects`** (account `wilsonguenther-9414`,
  wilsonguenther@gmail.com). The project moved here from team `drivia`
  (account `wilson-2398`) on 2026-09-27; nothing lives on `drivia` any more.
- Vercel project: **`yap`**, id `prj_uA0NyOkmbV3ezocAqCnEGLfLBV8G` (unchanged by
  the team move; also in `drivia-accounts.env` as `VERCEL_PROJECT_ID_YAP`)
- Production URLs: **https://yapvoice.app** (custom domain, `www` 308s to the
  apex) and **https://yap-lemon.vercel.app**
- Domain: `yapvoice.app` is Vercel-registered and its DNS zone moved to team
  `wilson-guenthers-projects` with the project (5 records, identical after the move)

The token and team id live in `~/.config/drivia/drivia-accounts.env` as
`VERCEL_TOKEN` and `VERCEL_TEAM_ID` (the old `VERCEL_NEW_TOKEN` /
`VERCEL_NEW_TEAM_ID` names are gone; the abandoned team's values are kept only as
`VERCEL_OLD_TEAM_DRIVIA_*` and must not be used for deploys). Source that file,
never echo it, never paste it into a repo. The CLI's default login on this machine
is now `wilsonguenther-9414`, but **every command below still passes
`--scope wilson-guenthers-projects` and `--token`** so a stale login or a second
team can never receive the deploy.

**Git link: connected, but git builds are IGNORED.** The project *is* linked to
`wilsonguenther-dev/wilson-voice`, production branch `main`, Root Directory
`site/dist`. A git build only ever sees what is committed under `site/dist/`,
which never includes the DMG (`*.dmg` is gitignored) or `updates/`. So every
merge that touched `site/dist/` used to auto-build a production deployment
*without* the download and the updater manifest, and silently replaced the last
manual deploy: from 0.9.0 (2026-10-16 14:14Z) until 2026-10-17 `yapvoice.app`
answered 404 for `/downloads/*.dmg` and `/updates/latest.json` with a perfect
200 on the page.

Since 2026-10-17 the project's Ignored Build Step
(`commandForIgnoringBuildStep`) is **`exit 0`**, which tells Vercel to skip
every git-triggered build. The git link is kept on purpose, for the Deployments
history and so builds can be re-enabled, but **every site release is a manual
staged deploy** (below). Check the setting, and confirm a merge was skipped:

```bash
curl -s -H "Authorization: Bearer $VERCEL_TOKEN" \
  "https://api.vercel.com/v9/projects/yap?teamId=$VERCEL_TEAM_ID" \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["commandForIgnoringBuildStep"])'   # exit 0
vercel ls yap --yes --scope wilson-guenthers-projects --token "$VERCEL_TOKEN"
# the git deployment for the merge commit must read "Canceled" (ignored), and
# the current Production row must still be the manual deploy
```

Do not change it back to a path-diff ignore step: any command that lets a
`main` build through puts a DMG-less deployment into production again.

## The staged deploy directory

Vercel uploads a directory. The directory we want does not exist anywhere on
disk, because the DMG is `.gitignore`d (`*.dmg`) and always will be — a 30 MB
binary is a release artifact, never a commit. So build it fresh each time,
**outside the repo**, and throw it away after.

**Root-directory gotcha:** the project's Root Directory is `site/dist`, and the
CLI applies it to the uploaded directory too. Files must therefore sit at
`$STAGE/site/dist/...` and you deploy from `$STAGE`. Stage them at the top of
`$STAGE` and the build fails (or serves nothing) because `site/dist` does not
exist in the upload.

```bash
TAG=v0.9.1; VER=${TAG#v}                       # the tag you are shipping
STAGE=$(mktemp -d)/yap-site
OUT="$STAGE/site/dist"
mkdir -p "$OUT/downloads"
cp site/dist/*.html site/dist/*.css site/dist/*.woff2 \
   site/dist/DepartureMono-LICENSE.txt site/dist/vercel.json "$OUT/"
gh release download "$TAG" --repo wilsonguenther-dev/wilson-voice \
  --pattern "Yap-$VER-arm64.dmg" --dir "$OUT/downloads"
scripts/stage-updater-manifest.sh "$TAG" "$OUT"   # writes $OUT/updates/
```

Verify the asset you just staged is the asset that was notarized, before it goes
anywhere public:

```bash
shasum -a 256 "$OUT/downloads/Yap-$VER-arm64.dmg"
gh release view "$TAG" --repo wilsonguenther-dev/wilson-voice \
  --json assets -q '.assets[] | "\(.name) \(.size)"'   # 0.9.1 DMG: 31914068
```

The three Download buttons in `site/dist/index.html` point at the **relative**
path `/downloads/Yap-<version>-arm64.dmg`, which resolves against whatever host is
serving the page. That is why moving hosts needed no HTML change at all, and why
the same `site/dist/` still works on Forge unmodified. Keep them relative.

`site/dist/vercel.json` declares `framework: null` / `buildCommand: null` so
Vercel's autodetect cannot decide this is an npm project and try to build it, and
`cleanUrls: false` because the footer links are written as `privacy.html` /
`terms.html` — turning clean URLs on would 308-redirect every one of them.

## Deploy

```bash
set -a; . ~/.config/drivia/drivia-accounts.env; set +a
unset VERCEL_PROJECT_ID          # the env file exports one for another project
cd "$STAGE"
vercel link --yes --project yap --scope wilson-guenthers-projects --token "$VERCEL_TOKEN"
rm -f .env.local site/dist/.env.local   # vercel link may pull env vars; never upload them
vercel deploy --prod --yes --archive=tgz --scope wilson-guenthers-projects --token "$VERCEL_TOKEN"
```

`drivia-accounts.env` exports `VERCEL_PROJECT_ID` for a different project, and
the CLI refuses (or links the wrong project) when `VERCEL_PROJECT_ID` is set
without a matching `VERCEL_ORG_ID`. Unset it before `vercel link`, or export
`VERCEL_ORG_ID="$VERCEL_TEAM_ID"` together with
`VERCEL_PROJECT_ID="$VERCEL_PROJECT_ID_YAP"`. `VERCEL_TOKEN` is the durable
`drivia-harness` token.

**Previews sit behind the Vercel login wall** (deployment protection), so an
unauthenticated `curl` of a `yap-xxxx-wilson-guenthers-projects.vercel.app`
preview URL answers 302 to the Vercel login page, not your files. Verify on production
(`https://yapvoice.app`, `https://yap-lemon.vercel.app`), or deploy a preview
first and `vercel promote <preview-url>` once it checks out with an
authenticated look.

`--archive=tgz` matters: without it the CLI uploads file-by-file and a 22 MB DMG
is a slow, flaky single request. `vercel link` is idempotent — it creates the
project the first time and re-links after that.

## The updater manifest ships with the DMG (UPD-A)

`plugins.updater.endpoints` in `desktop/src-tauri/tauri.conf.json` now reads,
in order:

1. `https://yap-lemon.vercel.app/updates/latest.json` — **primary**, the same
   host that serves the DMG.
2. the GitHub `releases/latest/download/latest.json` URL — **fallback**. It
   stops resolving the instant this repo is flipped private, and flipping it is
   a routine, recurring procedure (`docs/loop/HARNESS.md`).

The app asks **both** endpoints on every check and offers the **newest**
version either one announces (`check_every_endpoint` in `src-tauri/src/lib.rs`).
It does not stop at the first manifest that parses, which is what the plugin's
own endpoint loop does. So a primary left on an older release can never hide a
newer release on GitHub, and a primary that answers junk (an SPA catch-all
returning 200 text/html, a 5xx) can never switch off the fallback. "Install now"
uses that same check, so the tarball it downloads is the one named by the
manifest that won.

`latest.json` is **not** produced by `npm run desktop:build`. It comes from
tauri-action's `includeUpdaterJson: true` in `release.yml` (or is written by
hand, see `docs/RELEASE.md`), and its `url` names a **GitHub release asset**.
Copied here unchanged, it would move the lookup and leave the download behind:
in a private-repo window the check would succeed through Vercel, and "Install
now" would then 404 on GitHub. So the staging step above has a third part,
the manifest **and** the bytes it points at, through one script:

```bash
scripts/stage-updater-manifest.sh "$TAG" "$STAGE/site/dist"   # already in the staging block above
```

It downloads `latest.json`, `Yap.app.tar.gz` and `Yap.app.tar.gz.sig` from that
release (`gh` is authenticated, so this works while the repo is private) into
`$STAGE/site/dist/updates/` and rewrites every `platforms.*.url` to
`https://yap-lemon.vercel.app/updates/<tarball>`. It refuses to finish if the
manifest's version is not the tag, if any referenced file is missing or over
Vercel's 100 MB static limit, or if any manifest signature differs from its
`.sig`. The signature covers the tarball's bytes, not its url, so the rewrite
does not invalidate it, and the app still verifies against
`plugins.updater.pubkey`. Verify after the deploy, twice:

```bash
curl -s https://yap-lemon.vercel.app/updates/latest.json | python3 -m json.tool
curl -sI "$(curl -s https://yap-lemon.vercel.app/updates/latest.json \
  | python3 -c 'import json,sys;print(json.load(sys.stdin)["platforms"]["darwin-aarch64"]["url"])')" | head -1
```

Run this for **every** release (`docs/RELEASE.md` step 6). Newest-wins means a
skipped re-stage costs nothing while the repo is public, but in a private window
the primary is the only endpoint that answers.

`plugins.updater.pubkey` is **never** regenerated as part of a deploy. A new
keypair makes every installed copy unable to verify any update.

Then bump the version everywhere it is written down: the three `href`s and the
version label in `site/dist/index.html`, and `TAG` in the staging block above.

## Verify (do not skip — a broken download is invisible from the dashboard)

```bash
U=https://yapvoice.app        # then repeat with https://yap-lemon.vercel.app
# Content identity, not reachability: every page must come back byte-for-byte
# equal to the file in the tree you deployed. Empty diff = pass.
for p in index.html terms.html privacy.html style.css; do
  for i in 1 2; do
    curl -s --compressed "$U/$p" | diff - site/dist/$p > /dev/null \
      && echo "$p load$i OK" || echo "$p load$i MISMATCH"
  done
done
curl -sI $U/downloads/Yap-$VER-arm64.dmg | grep -i content-length   # 31914068 for 0.9.1
curl -sL -o /tmp/yap.dmg $U/downloads/Yap-$VER-arm64.dmg
shasum -a 256 /tmp/yap.dmg        # must equal the release asset's hash
curl -s $U/updates/latest.json | python3 -c 'import json,sys;print(json.load(sys.stdin)["version"])'   # $VER
```

Each page is fetched **twice** deliberately. A compressed response that is cached
wrong fails only on the *repeat* visit — the first load looks perfect — and no
unit test, CI job, or local preview can see it. Only hitting the deployed URL a
second time can.

**A status code is not evidence.** `200` proves the host answered, not that it
answered with the files you just staged — a project that was deployed once and
then never redeployed serves stale HTML with a perfect `200` forever. This
already bit a review preview: the deploy went out before a second round of
edits, both legal pages kept serving the superseded copy, and the `200`-only
check reported the site healthy while the reviewer was reading text that was not
on the branch. Diff the bytes; the diff is the acceptance criterion.

The same rule applies to any throwaway review preview: after the last commit
that touches `site/dist/`, redeploy and re-diff, and say in the PR body which
commit the preview serves so an approver knows the link and the branch agree.

Sizes and hashes are the acceptance criteria, not "the page looked right": a
truncated or proxied DMG still renders a working page and hands the user a
disk image macOS refuses to mount.

## What this runbook does not do

- Touch Forge, its Caddy config, or any DNS record.
- Buy, attach or move a custom domain. `yapvoice.app` is already attached to the
  `yap` project on team `wilson-guenthers-projects`; domain and DNS changes are
  Wilson's call.
- Re-enable git builds. The ignore step stays `exit 0` until git builds can
  ship the DMG and `updates/` (they cannot today); changing it is Wilson's call.
- Publish the release. That is `docs/RELEASE.md`; this runbook assumes the
  notarized DMG is already a release asset and only ever *reads* it.
