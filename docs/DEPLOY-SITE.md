# Yap — landing-page deploy runbook

The Yap site is four hand-authored static files in `site/dist/` plus one very
large binary: the notarized `.dmg` every visitor is there to download. Those two
halves live in different places on purpose — the HTML is in git, the DMG is a
GitHub release asset — so **deploying the site is a staging step, not a `git
push`**. This file is the whole procedure.

## Why Vercel

Forge (the self-hosted box) serves the site fine, but its edge caps a single
response at **25 MB** and `Yap-0.7.0-arm64.dmg` is 22.4 MB and growing with every
model and sidecar we bundle. The first release that crosses 25 MB would break the
Download button with no build failure and no test to catch it — the page would
still be 200, the button would just die. Vercel's static file limit is 100 MB, so
the download lives there now.

**Forge stays live as a mirror.** Nothing about the Forge deploy, its DNS, or its
config is touched by this runbook. Which host becomes canonical — and which
custom domain points at it — is Wilson's call, not the deploy script's.

- Vercel scope/team: **`drivia`** (account `wilson-2398`, wilson@drivia.consulting)
- Vercel project: **`yap`**
- Production URL: **https://yap-lemon.vercel.app**

The management token lives in `~/.config/drivia/drivia-accounts.env` as
`VERCEL_NEW_TOKEN`. Source that file, never echo it, never paste it into a repo.
The CLI's *default* login on this machine is still the old, suspended
`wilsonguenther-9414` account, so **every command below must pass `--scope drivia`
and `--token`** — omit them and you deploy into a dead account.

## The staged deploy directory

Vercel uploads a directory. The directory we want does not exist anywhere on
disk, because the DMG is `.gitignore`d (`*.dmg`) and always will be — a 22 MB
binary is a release artifact, never a commit. So build it fresh each time,
**outside the repo**, and throw it away after:

```bash
STAGE=$(mktemp -d)/yap-site
mkdir -p "$STAGE/downloads"
cp site/dist/*.html site/dist/*.css site/dist/vercel.json "$STAGE/"
gh release download v0.7.0 --repo wilsonguenther-dev/wilson-voice \
  --pattern 'Yap-0.7.0-arm64.dmg' --dir "$STAGE/downloads"
```

Verify the asset you just staged is the asset that was notarized, before it goes
anywhere public:

```bash
shasum -a 256 "$STAGE/downloads/Yap-0.7.0-arm64.dmg"
gh release view v0.7.0 --repo wilsonguenther-dev/wilson-voice \
  --json assets -q '.assets[].size'      # must read 22410130 for v0.7.0
```

The three Download buttons in `site/dist/index.html` point at the **relative**
path `/downloads/Yap-0.7.0-arm64.dmg`, which resolves against whatever host is
serving the page. That is why moving hosts needed no HTML change at all, and why
the same `site/dist/` still works on Forge unmodified. Keep them relative.

`site/dist/vercel.json` declares `framework: null` / `buildCommand: null` so
Vercel's autodetect cannot decide this is an npm project and try to build it, and
`cleanUrls: false` because the footer links are written as `privacy.html` /
`terms.html` — turning clean URLs on would 308-redirect every one of them.

## Deploy

```bash
set -a; . ~/.config/drivia/drivia-accounts.env; set +a
cd "$STAGE"
vercel link --yes --project yap --scope drivia --token "$VERCEL_NEW_TOKEN"
vercel deploy --prod --yes --archive=tgz --scope drivia --token "$VERCEL_NEW_TOKEN"
```

`--archive=tgz` matters: without it the CLI uploads file-by-file and a 22 MB DMG
is a slow, flaky single request. `vercel link` is idempotent — it creates the
project the first time and re-links after that.

Then bump the version everywhere it is written down: the three `href`s and the
`v0.7.0` label in `site/dist/index.html`, and the `--pattern` above.

## Verify (do not skip — a broken download is invisible from the dashboard)

```bash
U=https://yap-lemon.vercel.app
curl -s -o /dev/null -w '%{http_code}\n' $U          # 200
curl -s -o /dev/null -w '%{http_code}\n' $U          # 200 again — see below
curl -sI $U/downloads/Yap-0.7.0-arm64.dmg | grep -i content-length   # 22410130
curl -sL -o /tmp/yap.dmg $U/downloads/Yap-0.7.0-arm64.dmg
shasum -a 256 /tmp/yap.dmg        # must equal the release asset's hash
```

The page is loaded **twice** deliberately. A compressed response that is cached
wrong fails only on the *repeat* visit — the first load looks perfect — and no
unit test, CI job, or local preview can see it. Only hitting the deployed URL a
second time can.

Sizes and hashes are the acceptance criteria, not "the page looked right": a
truncated or proxied DMG still renders a working page and hands the user a
disk image macOS refuses to mount.

## What this runbook does not do

- Touch Forge, its Caddy config, or any DNS record.
- Buy or attach a custom domain. `yap-lemon.vercel.app` is the production URL
  until Wilson picks one.
- Publish the release. That is `docs/RELEASE.md`; this runbook assumes the
  notarized DMG is already a release asset and only ever *reads* it.
