#!/usr/bin/env bash
# UPD-A — stage the updater manifest AND the bytes it points at on the site host.
#
#   scripts/stage-updater-manifest.sh <tag> <stage-dir> [base-url]
#
# Writes into <stage-dir>/updates/:
#   Yap.app.tar.gz        the signed updater tarball, unchanged
#   Yap.app.tar.gz.sig    its minisign signature, unchanged
#   latest.json           the release's manifest with EVERY platforms.*.url
#                         rewritten to <base-url>/updates/<tarball>
#
# The GitHub-generated latest.json names a GitHub release asset as its url. Served
# from the site host unchanged, it would move the lookup and leave the download
# behind: in a private-repo window the check would succeed and "Install now"
# would then 404 on GitHub. So this script refuses to finish unless every url
# points at <base-url>, every referenced file is staged next to the manifest,
# and every manifest signature equals its staged .sig byte for byte. The
# signature covers the tarball's bytes, not its url, so rewriting the url does
# not invalidate it (the app still verifies against plugins.updater.pubkey).
#
# <base-url> defaults to the primary updater endpoint's host in tauri.conf.json.
# Needs: gh (authenticated — works while the repo is private), python3.
set -euo pipefail

TAG="${1:?usage: stage-updater-manifest.sh <tag> <stage-dir> [base-url]}"
STAGE="${2:?usage: stage-updater-manifest.sh <tag> <stage-dir> [base-url]}"
BASE="${3:-https://yap-lemon.vercel.app}"
BASE="${BASE%/}"
REPO="wilsonguenther-dev/wilson-voice"
OUT="$STAGE/updates"
VERCEL_FILE_LIMIT=$((100 * 1024 * 1024))

mkdir -p "$OUT"
gh release download "$TAG" --repo "$REPO" --clobber --dir "$OUT" \
  --pattern 'latest.json' --pattern 'Yap.app.tar.gz' --pattern 'Yap.app.tar.gz.sig'

python3 -I - "$OUT" "$BASE" "$TAG" "$VERCEL_FILE_LIMIT" <<'PY'
import json, os, sys
from urllib.parse import urlparse

out, base, tag, limit = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
path = os.path.join(out, "latest.json")
with open(path) as f:
    manifest = json.load(f)

want = tag.lstrip("v")
if str(manifest.get("version", "")).lstrip("v") != want:
    sys.exit(f"latest.json announces {manifest.get('version')!r}, tag is {tag}")

platforms = manifest.get("platforms") or {}
if not platforms:
    sys.exit("latest.json has no platforms")

for name, entry in platforms.items():
    asset = os.path.basename(urlparse(entry["url"]).path)
    local = os.path.join(out, asset)
    if not os.path.isfile(local):
        sys.exit(f"{name}: {asset} is referenced by the manifest but was not staged")
    size = os.path.getsize(local)
    if size >= limit:
        sys.exit(f"{name}: {asset} is {size} bytes, over the host's static file limit")
    sig_path = local + ".sig"
    if not os.path.isfile(sig_path):
        sys.exit(f"{name}: {asset}.sig was not staged")
    with open(sig_path) as f:
        sig = f.read().strip()
    if entry.get("signature", "").strip() != sig:
        sys.exit(f"{name}: manifest signature does not equal {asset}.sig")
    entry["url"] = f"{base}/updates/{asset}"
    print(f"{name}: url -> {entry['url']} ({size} bytes, signature == .sig)")

with open(path, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
print(f"staged {path} for {tag}")
PY
