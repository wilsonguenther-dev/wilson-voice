# Yap — landing-page deploy runbook

The Yap site is four hand-authored static files in `site/dist/` — plus the
Departure Mono woff2 the page sets its headings in, and its OFL licence text —
plus one very large binary: the notarized `.dmg` every visitor is there to download. Those two
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

**Git link: dropped by the team transfer.** The project used to be linked to
`wilsonguenther-dev/wilson-voice` @ `main` (root `site/dist`, path-diff ignore
step), and the transfer removed that link because the gmail Vercel account has
no GitHub login connection yet. Until Wilson connects GitHub on that account and
re-links the project (`~/.config/drivia/vercel-migration-2026-09-27/relink-git.sh`),
**a push to `main` builds nothing on Vercel.** The CLI deploy below does not
depend on the git link and keeps working.

## The staged deploy directory

Vercel uploads a directory. The directory we want does not exist anywhere on
disk, because the DMG is `.gitignore`d (`*.dmg`) and always will be — a 22 MB
binary is a release artifact, never a commit. So build it fresh each time,
**outside the repo**, and throw it away after:

```bash
STAGE=$(mktemp -d)/yap-site
mkdir -p "$STAGE/downloads"
cp site/dist/*.html site/dist/*.css site/dist/*.woff2 \
   site/dist/DepartureMono-LICENSE.txt site/dist/vercel.json "$STAGE/"
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
vercel link --yes --project yap --scope wilson-guenthers-projects --token "$VERCEL_TOKEN"
vercel deploy --prod --yes --archive=tgz --scope wilson-guenthers-projects --token "$VERCEL_TOKEN"
```

`VERCEL_TOKEN` is, as of 2026-09-27, a CLI login token that expires
2026-09-28T02:24Z. If `vercel deploy` answers 401/403, the durable replacement
token has not been written into `drivia-accounts.env` yet; that is Wilson's
dashboard step, not something to work around with the old team's token.

`--archive=tgz` matters: without it the CLI uploads file-by-file and a 22 MB DMG
is a slow, flaky single request. `vercel link` is idempotent — it creates the
project the first time and re-links after that.

Then bump the version everywhere it is written down: the three `href`s and the
`v0.7.0` label in `site/dist/index.html`, and the `--pattern` above.

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
curl -sI $U/downloads/Yap-0.7.0-arm64.dmg | grep -i content-length   # 22410130
curl -sL -o /tmp/yap.dmg $U/downloads/Yap-0.7.0-arm64.dmg
shasum -a 256 /tmp/yap.dmg        # must equal the release asset's hash
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
- Re-link the Vercel project to GitHub. That needs Wilson's GitHub login
  connection on the gmail Vercel account (see above).
- Publish the release. That is `docs/RELEASE.md`; this runbook assumes the
  notarized DMG is already a release asset and only ever *reads* it.
