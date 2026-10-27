#!/usr/bin/env bash
# UPD-B — the local release: build, sign, notarize, staple, and stage the four
# files the updater and the site need, next to each other, plus the previous
# build as the rollback. GitHub Actions is off account-wide, so this is the only
# producer of Yap.app.tar.gz + .sig + latest.json that does not depend on CI.
#
#   scripts/release-local.sh <tag> [--stage DIR] [--notes-file FILE] [--deploy]
#       Full release of desktop/src-tauri/tauri.conf.json's version (tag v<version>).
#       1. preflight: clean tree, tag == tauri.conf.json == package.json version,
#          site/dist/index.html already links the new DMG, the updater key is in
#          the environment, notarization credentials are in the environment, and
#          scripts/sign-local.sh resolves the RELEASE profile (Developer ID, SEC-A)
#       2. npm run desktop:build -- --bundles app dmg   (app carries the updater
#          triple: tauri-bundler makes Yap.app.tar.gz + .sig only when the app
#          bundle is a target; 'updater' alone drops the .app; sign-local.sh: signs
#          with that identity, tauri notarizes + staples the .app, mandatory smoke)
#       3. notarize + staple the DMG, validate, Gatekeeper-assess it, smoke it again
#       4. write latest.json from the real .sig, stage DMG + Yap.app.tar.gz + .sig +
#          latest.json under <stage>/site/dist, and stage the build the host is
#          serving NOW under updates/rollback/ + its DMG under downloads/
#       5. verify the staged tree offline (signature against the committed pubkey)
#       6. --deploy only: vercel deploy --prod per docs/DEPLOY-SITE.md, then the
#          live verification below, which must see the new version AND the rollback
#
#   scripts/release-local.sh --verify-endpoint-or-name-the-reason [--endpoint URL]
#                            [--expect-version X.Y.Z] [--require-rollback]
#       Fetch latest.json from every plugins.updater.endpoints entry (or URL),
#       download the tarball each manifest names, and verify the manifest's
#       signature over those bytes against plugins.updater.pubkey — the same check
#       the installed app runs. Offline (no endpoint resolves or connects): exit 0
#       with "SKIP: offline — <reason>", never a silent pass — UNLESS
#       --expect-version or --require-rollback was given: an assertion that
#       nothing checked is a FAIL (exit 1), never a skip. Under --expect-version
#       a fallback that errors with anything but a definite HTTP 404 also FAILS:
#       what it offers is unknown.
#
#   scripts/release-local.sh --verify-staged <dir>      offline check of a staged
#                                                       site/dist (or its parent)
#   scripts/release-local.sh --stage-rollback <dir>     copy the live build into
#                                                       <dir>/updates/rollback + DMG
#
# Exit: 0 pass (or a NAMED skip), 1 failure, 64 usage.
#
# The updater key is never written in this file and never read from disk here:
# export TAURI_SIGNING_PRIVATE_KEY (the key's contents, or its path) and
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD before a release — docs/RELEASE.md says how.
# Needs: bash, curl, python3 (stdlib only — the Ed25519/minisign check is pure
# Python), and for a release: npm, xcrun, codesign, hdiutil, spctl, vercel (--deploy).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/desktop"
CONF="$DESKTOP/src-tauri/tauri.conf.json"
SITE="$ROOT/site/dist"
VERCEL_FILE_LIMIT=$((100 * 1024 * 1024))
CURL_TIMEOUT="${YAP_RELEASE_CURL_TIMEOUT:-120}"

die() { echo "FATAL: $*" >&2; exit 1; }
usage() { sed -n '2,45p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 64; }

# ── python helper: config, manifest, minisign/Ed25519 verification ──────────────
py() {
  python3 -I - "$@" <<'PY'
import base64, hashlib, json, os, re, sys
from datetime import datetime, timezone
from urllib.parse import urlparse

# RFC 8032 Ed25519 verification (reference algorithm, stdlib only).
P = 2**255 - 19
Q = 2**252 + 27742317777372353535851937790883648493
D = -121665 * pow(121666, P - 2, P) % P
SQRT_M1 = pow(2, (P - 1) // 4, P)

def _recover_x(y, sign):
    if y >= P:
        return None
    x2 = (y * y - 1) * pow(D * y * y + 1, P - 2, P) % P
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (P + 3) // 8, P)
    if (x * x - x2) % P:
        x = x * SQRT_M1 % P
    if (x * x - x2) % P:
        return None
    if (x & 1) != sign:
        x = P - x
    return x

def _add(a, b):
    A = (a[1] - a[0]) * (b[1] - b[0]) % P
    B = (a[1] + a[0]) * (b[1] + b[0]) % P
    C = 2 * a[3] * b[3] * D % P
    Dd = 2 * a[2] * b[2] % P
    E, F, G, H = B - A, Dd - C, Dd + C, B + A
    return (E * F % P, G * H % P, F * G % P, E * H % P)

def _mul(s, pt):
    acc = (0, 1, 1, 0)
    while s:
        if s & 1:
            acc = _add(acc, pt)
        pt = _add(pt, pt)
        s >>= 1
    return acc

def _eq(a, b):
    return (a[0] * b[2] - b[0] * a[2]) % P == 0 and (a[1] * b[2] - b[1] * a[2]) % P == 0

_GY = 4 * pow(5, P - 2, P) % P
_GX = _recover_x(_GY, 0)
BASE = (_GX, _GY, 1, _GX * _GY % P)

def _decompress(s):
    if len(s) != 32:
        return None
    y = int.from_bytes(s, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign)
    return None if x is None else (x, y, 1, x * y % P)

def ed25519_verify(pub, msg, sig):
    if len(sig) != 64:
        return False
    A, R = _decompress(pub), _decompress(sig[:32])
    if A is None or R is None:
        return False
    s = int.from_bytes(sig[32:], "little")
    if s >= Q:
        return False
    h = int.from_bytes(hashlib.sha512(sig[:32] + pub + msg).digest(), "little") % Q
    return _eq(_mul(s, BASE), _add(R, _mul(h, A)))

def fail(msg):
    print(f"FAIL: {msg}")
    sys.exit(1)

def b64_lines(blob, what):
    try:
        text = base64.b64decode(blob.strip(), validate=True).decode()
    except Exception as e:
        fail(f"{what} is not base64 of a minisign file ({e})")
    return text.splitlines()

def load_conf():
    with open(os.environ["YAP_CONF"]) as f:
        return json.load(f)

def pubkey():
    lines = b64_lines(load_conf()["plugins"]["updater"]["pubkey"], "plugins.updater.pubkey")
    raw = base64.b64decode(lines[1])
    if len(raw) != 42 or raw[:2] != b"Ed":
        fail("plugins.updater.pubkey is not a minisign Ed25519 public key")
    return raw[2:10], raw[10:]

def minisign_verify(sig_b64, data_path):
    """The check tauri-plugin-updater runs (minisign-verify): key id, the
    signature over the (prehashed) bytes, and the trusted-comment signature."""
    key_id, pk = pubkey()
    lines = b64_lines(sig_b64, "signature")
    if len(lines) < 4 or not lines[2].startswith("trusted comment: "):
        fail("signature is not a 4-line minisign signature")
    raw = base64.b64decode(lines[1])
    if len(raw) != 74 or raw[:2] not in (b"Ed", b"ED"):
        fail("signature line is not a minisign Ed25519 signature")
    if raw[2:10] != key_id:
        fail(f"signed by key id {raw[2:10][::-1].hex().upper()}, the app trusts {key_id[::-1].hex().upper()}")
    with open(data_path, "rb") as f:
        if raw[:2] == b"ED":            # prehashed: Ed25519 over BLAKE2b-512(file)
            h = hashlib.blake2b(digest_size=64)
            for chunk in iter(lambda: f.read(1 << 20), b""):
                h.update(chunk)
            msg = h.digest()
        else:                           # legacy: Ed25519 over the file itself
            msg = f.read()
    if not ed25519_verify(pk, msg, raw[10:]):
        fail(f"signature does NOT verify over {os.path.basename(data_path)} with plugins.updater.pubkey")
    trusted = lines[2][len("trusted comment: "):].encode()
    if not ed25519_verify(pk, raw[10:] + trusted, base64.b64decode(lines[3])):
        fail("trusted-comment signature does NOT verify")
    return key_id[::-1].hex().upper(), trusted.decode()

def semver(v):
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?", v)
    if not m:
        return None
    return (int(m[1]), int(m[2]), int(m[3]), 0 if m[4] else 1, m[4] or "")

def check_manifest(path, expect):
    with open(path) as f:
        try:
            m = json.load(f)
        except ValueError as e:
            fail(f"{path} is not JSON ({e}) — an SPA catch-all or an error page, not a manifest")
    if not isinstance(m, dict):
        fail(f"{path} is a JSON {type(m).__name__}, not a manifest object")
    v = str(m.get("version", ""))
    if v.startswith("v") or semver(v) is None:
        fail(f"version {v!r} is not plain semver (no leading v)")
    if expect not in ("", "-") and v != expect:
        fail(f"manifest announces {v}, expected {expect}")
    try:
        datetime.fromisoformat(str(m.get("pub_date", "")).replace("Z", "+00:00"))
    except ValueError:
        fail(f"pub_date {m.get('pub_date')!r} is not RFC 3339 — the app's parse fails")
    plats = m.get("platforms")
    if not isinstance(plats, dict):
        fail(f"platforms is {type(plats).__name__}, not an object — the app cannot deserialize this manifest")
    if "darwin-aarch64" not in plats:
        fail("no darwin-aarch64 entry — every Apple-silicon client sees 'no update'")
    for name, e in plats.items():
        if not isinstance(e, dict):
            fail(f"{name}: entry is {type(e).__name__}, not an object with url + signature — the app cannot deserialize this manifest")
        for k in ("signature", "url"):
            if not isinstance(e.get(k), str) or not e[k].strip():
                fail(f"{name}: {k} must be a non-empty string")
        if any(c.isspace() for c in e["url"]):
            fail(f"{name}: url {e['url']!r} contains whitespace")
    return m

cmd, args = sys.argv[1], sys.argv[2:]
if cmd == "conf":
    c = load_conf()
    eps = c["plugins"]["updater"]["endpoints"]
    base = eps[0].rsplit("/updates/", 1)[0] if "/updates/" in eps[0] else ""
    key_id, _ = pubkey()
    print(f"VERSION={c['version']}")
    print(f"PRODUCT={c['productName']}")
    print(f"BASE={base}")
    print(f"KEYID={key_id[::-1].hex().upper()}")
    for i, e in enumerate(eps):
        print(f"ENDPOINT_{i}={e}")
elif cmd == "urls":                      # manifest -> "<platform> <url>" lines
    m = check_manifest(args[0], "-")
    for name, e in m["platforms"].items():
        print(name, e["url"])
elif cmd == "field":                     # manifest field
    with open(args[0]) as f:
        print(json.load(f).get(args[1], ""))
elif cmd == "verify":                    # manifest platform tarball [expected-url]
    m = check_manifest(args[0], "-")
    e = m["platforms"][args[1]]
    if len(args) > 3 and e["url"] != args[3]:
        fail(f"{args[1]}: url {e['url']} != {args[3]}")
    size = os.path.getsize(args[2])
    if size >= int(os.environ.get("YAP_FILE_LIMIT", "104857600")):
        fail(f"{args[1]}: tarball is {size} bytes, over the host's static file limit")
    kid, trusted = minisign_verify(e["signature"], args[2])
    sha = hashlib.sha256(open(args[2], "rb").read()).hexdigest()
    print(f"OK {args[1]} v{m['version']}: signature verifies with plugins.updater.pubkey "
          f"(key {kid}); {size} bytes sha256 {sha[:16]}…; trusted comment: {trusted[:60]}")
elif cmd == "sigfile":                   # manifest platform sigfile: equal byte for byte?
    m = check_manifest(args[0], "-")
    with open(args[2]) as f:
        if f.read().strip() != m["platforms"][args[1]]["signature"].strip():
            fail(f"{args[1]}: {args[2]} differs from the manifest's signature")
elif cmd == "newer":                     # a b: exit 0 iff a > b
    a, b = semver(args[0]), semver(args[1])
    if a is None or b is None:
        fail(f"cannot compare {args[0]!r} and {args[1]!r}")
    sys.exit(0 if a > b else 1)
elif cmd == "newest":                    # versions...: print the greatest
    vs = [(semver(v), v) for v in args]
    bad = [v for s_, v in vs if s_ is None]
    if bad or not vs:
        fail(f"cannot compare versions {bad or args!r}")
    print(max(vs)[1])
elif cmd == "write":                     # out version notes-file sig-file url
    out, ver, notes_file, sig_file, url = args
    notes = open(notes_file).read().strip() if notes_file != "-" else f"Yap {ver}"
    sig = open(sig_file).read().strip()
    doc = {"version": ver, "notes": notes,
           "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
           "platforms": {"darwin-aarch64": {"signature": sig, "url": url}}}
    with open(out, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
elif cmd == "rewrite":                   # manifest new-url: every platform -> new-url
    with open(args[0]) as f:
        m = json.load(f)
    for e in m["platforms"].values():
        e["url"] = args[1]
    with open(args[0], "w") as f:
        json.dump(m, f, indent=2)
        f.write("\n")
else:
    sys.exit(f"unknown helper command {cmd}")
PY
}
export YAP_CONF="$CONF" YAP_FILE_LIMIT="$VERCEL_FILE_LIMIT"

load_conf() {
  local line
  while IFS= read -r line; do
    case "$line" in
      VERSION=*) CONF_VERSION="${line#VERSION=}" ;;
      PRODUCT=*) PRODUCT="${line#PRODUCT=}" ;;
      BASE=*)    BASE="${line#BASE=}" ;;
      KEYID=*)   KEYID="${line#KEYID=}" ;;
      ENDPOINT_*) ENDPOINTS+=("${line#*=}") ;;
    esac
  done < <(py conf)
  [[ -n "${BASE:-}" ]] || die "the primary updater endpoint in $CONF is not <host>/updates/latest.json"
}
ENDPOINTS=()
load_conf
TMPROOT="$(mktemp -d)"
trap 'rm -rf "$TMPROOT"' EXIT

# curl into a file; prints the curl exit code on stdout instead of dying, so the
# caller can tell "offline" (6 resolve, 7 connect, 28 timeout) from "broken".
# The final HTTP status (after redirects) lands in $TMPROOT/fetch.code — "000"
# when no response arrived — so a caller can tell a definite 404 from anything
# else (this curl reports a 404 over HTTP/2 as 56, not 22, so rc alone cannot).
fetch() {
  local url="$1" out="$2" rc=0
  curl -sS -f -L --max-time "$CURL_TIMEOUT" -w '%{http_code}' -o "$out" "$url" \
    >"$TMPROOT/fetch.code" 2>"$TMPROOT/fetch.err" || rc=$?
  echo "$rc"
}
fetch_code() { local c; c="$(cat "$TMPROOT/fetch.code" 2>/dev/null)"; echo "${c:-000}"; }
is_offline_rc() { [[ "$1" == 6 || "$1" == 7 || "$1" == 28 ]]; }

# Download the manifest's tarball(s) and verify each against the pubkey.
# Fails CLOSED: the manifest check's exit status is read (never thrown away by a
# process substitution), and a manifest that yields no platform line fails.
verify_manifest_bytes() {
  local manifest="$1" dir="$2" plat url rc urls n=0
  if ! urls="$(py urls "$manifest" 2>&1)"; then
    echo "${urls:-FAIL: $manifest: the manifest check exited non-zero}"
    echo "FAIL: $manifest is not a manifest the app can use — nothing was verified"
    return 1
  fi
  while read -r plat url; do
    [[ -n "$plat" ]] || continue
    n=$((n + 1))
    rc="$(fetch "$url" "$dir/$plat.tar.gz")"
    [[ "$rc" == 0 ]] || { echo "FAIL: $plat: $url did not download (curl $rc: $(cat "$TMPROOT/fetch.err"))"; return 1; }
    py verify "$manifest" "$plat" "$dir/$plat.tar.gz" || return 1
  done <<< "$urls"
  [[ $n -gt 0 ]] || { echo "FAIL: $manifest names no platform — nothing was verified"; return 1; }
}

# ── --verify-endpoint-or-name-the-reason ────────────────────────────────────────
verify_endpoint() {
  local expect="" require_rb=0 only=() tmp i url rc reachable=0 failures=0 primary_ok=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --endpoint) only+=("${2:?--endpoint needs a URL}"); shift 2 ;;
      --expect-version) expect="${2:?}"; shift 2 ;;
      --require-rollback) require_rb=1; shift ;;
      *) echo "unknown option $1" >&2; exit 64 ;;
    esac
  done
  local eps=("${ENDPOINTS[@]}")
  [[ ${#only[@]} -gt 0 ]] && eps=("${only[@]}")
  tmp="$(mktemp -d "$TMPROOT/verify.XXXXXX")"
  echo "pubkey: plugins.updater.pubkey in desktop/src-tauri/tauri.conf.json (key $KEYID)"
  local offline_reasons=() versions=()
  for i in "${!eps[@]}"; do
    url="${eps[$i]}"
    mkdir -p "$tmp/$i"
    rc="$(fetch "$url" "$tmp/$i/latest.json")"
    if is_offline_rc "$rc"; then
      offline_reasons+=("$url: curl $rc ($(tr -d '\n' < "$TMPROOT/fetch.err"))")
      echo "endpoint[$i] $url: UNREACHABLE (curl $rc)"
      continue
    fi
    reachable=1
    if [[ "$rc" != 0 ]]; then
      if [[ $i -eq 0 ]]; then
        echo "FAIL: endpoint[0] $url (primary): curl $rc — $(tr -d '\n' < "$TMPROOT/fetch.err")"
        failures=$((failures + 1))
      elif [[ -n "$expect" && "$(fetch_code)" != 404 ]]; then
        # Only a definite 404 means "this endpoint offers nothing". A 5xx, a TLS
        # error or a reset leaves its offer unknown, so the effective offer the
        # app would install cannot be confirmed as v$expect.
        echo "FAIL: --expect-version v$expect: fallback endpoint[$i] $url: curl $rc, HTTP $(fetch_code) — not a definite 404, so what it offers is unknown: $(tr -d '\n' < "$TMPROOT/fetch.err")"
        failures=$((failures + 1))
      else
        echo "WARN: endpoint[$i] $url (fallback): curl $rc — $(tr -d '\n' < "$TMPROOT/fetch.err"). Expected while the repo is private; the primary carries updates."
      fi
      continue
    fi
    echo "endpoint[$i] $url: latest.json v$(py field "$tmp/$i/latest.json" version)"
    if [[ $i -eq 0 && -n "$expect" ]]; then
      [[ "$(py field "$tmp/$i/latest.json" version)" == "$expect" ]] \
        || { echo "FAIL: primary announces v$(py field "$tmp/$i/latest.json" version), expected v$expect"; failures=$((failures + 1)); continue; }
    fi
    if verify_manifest_bytes "$tmp/$i/latest.json" "$tmp/$i"; then
      [[ $i -eq 0 ]] && primary_ok=1
      versions+=("$(py field "$tmp/$i/latest.json" version)")
    else
      if [[ $i -eq 0 ]]; then failures=$((failures + 1)); else echo "WARN: fallback endpoint[$i] does not verify — the app offers the newest VERIFIED-looking version, so this one is a liability"; failures=$((failures + 1)); fi
    fi
  done
  if [[ $reachable -eq 0 ]]; then
    if [[ -n "$expect" || $require_rb -eq 1 ]]; then
      # An explicit assertion (the rollback confirmation, the --deploy final
      # gate) must never pass with nothing checked.
      echo "FAIL: no updater endpoint resolved or connected, so${expect:+ --expect-version v$expect}$([[ $require_rb -eq 1 ]] && echo ' --require-rollback') could NOT be checked: ${offline_reasons[*]}"
      echo "verify-endpoint: FAILED (offline with an assertion given)"
      return 1
    fi
    echo "SKIP: offline — no updater endpoint resolved or connected, so the live manifest and its signature were NOT verified: ${offline_reasons[*]}"
    return 0
  fi
  if [[ ${#offline_reasons[@]} -gt 0 && $primary_ok -eq 0 && ${#only[@]} -eq 0 ]]; then
    echo "FAIL: the network is up (another endpoint answered) but the primary did not connect: ${offline_reasons[*]}"
    failures=$((failures + 1))
  fi
  if [[ ${#versions[@]} -ge 2 && "${versions[0]}" != "${versions[1]}" ]] && py newer "${versions[1]}" "${versions[0]}" >/dev/null; then
    echo "WARN: the fallback announces v${versions[1]}, newer than the primary's v${versions[0]} — re-stage the primary (docs/DEPLOY-SITE.md)"
  fi
  # The app asks EVERY endpoint and installs the NEWEST offer (lib.rs
  # check_every_endpoint -> pick_newest_offer), so --expect-version is checked
  # against that effective offer, not the primary alone — a rollback that leaves
  # a newer build on the fallback has not stopped it spreading.
  if [[ -n "$expect" ]]; then
    if [[ ${#offline_reasons[@]} -gt 0 ]]; then
      echo "FAIL: --expect-version v$expect: endpoint(s) did not answer, so the version the app is offered is unknown: ${offline_reasons[*]}"
      failures=$((failures + 1))
    fi
    if [[ ${#versions[@]} -gt 0 ]]; then
      local offered
      if ! offered="$(py newest "${versions[@]}")"; then
        echo "$offered"; failures=$((failures + 1))
      elif [[ "$offered" != "$expect" ]]; then
        echo "FAIL: the app is offered v$offered (the newest across all endpoints: ${versions[*]}), expected v$expect — every endpoint must stop offering a newer build (docs/RELEASE.md Rollback)"
        failures=$((failures + 1))
      else
        echo "effective offer: v$offered (newest across all verified endpoints) == expected"
      fi
    fi
  fi
  # Rollback: the previous build kept on the primary host (docs/RELEASE.md).
  if [[ ${#only[@]} -eq 0 && $primary_ok -eq 1 ]]; then
    mkdir -p "$tmp/rb"
    rc="$(fetch "$BASE/updates/rollback/latest.json" "$tmp/rb/latest.json")"
    if [[ "$rc" == 0 ]]; then
      local rbv; rbv="$(py field "$tmp/rb/latest.json" version)"
      if verify_manifest_bytes "$tmp/rb/latest.json" "$tmp/rb" \
         && py newer "${versions[0]}" "$rbv" >/dev/null \
         && [[ "$(fetch "$BASE/downloads/$PRODUCT-$rbv-arm64.dmg" "$tmp/rb/rb.dmg")" == 0 ]]; then
        echo "rollback: OK — v$rbv manifest + tarball verify, and $BASE/downloads/$PRODUCT-$rbv-arm64.dmg is served"
      else
        echo "FAIL: rollback on $BASE is present but broken (manifest/tarball/DMG for v$rbv, or not older than v${versions[0]})"
        failures=$((failures + 1))
      fi
    elif [[ $require_rb -eq 1 ]]; then
      echo "FAIL: rollback: $BASE/updates/rollback/latest.json is not served (curl $rc) and --require-rollback was given"
      failures=$((failures + 1))
    else
      echo "WARN: rollback: NOT PRESENT on $BASE (curl $rc on /updates/rollback/latest.json) — reason: the host was last staged before scripts/release-local.sh existed; the next release-local.sh stage writes it, and its --deploy run requires it"
    fi
  fi
  if [[ $failures -gt 0 ]]; then echo "verify-endpoint: FAILED ($failures)"; return 1; fi
  echo "verify-endpoint: OK"
}

# ── --verify-staged <dir> ───────────────────────────────────────────────────────
verify_staged() {
  local dir="${1:?--verify-staged needs a directory}" expect="${2:-}" out rbv
  [[ -d "$dir/site/dist" ]] && dir="$dir/site/dist"
  out="$dir/updates"
  [[ -f "$out/latest.json" ]] || die "$out/latest.json is not staged"
  local v; v="$(py field "$out/latest.json" version)"
  [[ -z "$expect" || "$v" == "$expect" ]] || die "staged manifest is v$v, expected v$expect"
  [[ -f "$dir/downloads/$PRODUCT-$v-arm64.dmg" ]] || die "staged manifest is v$v but $dir/downloads/$PRODUCT-$v-arm64.dmg is not staged"
  [[ $(stat -f %z "$dir/downloads/$PRODUCT-$v-arm64.dmg") -lt $VERCEL_FILE_LIMIT ]] || die "DMG is over the host's static file limit"
  py sigfile "$out/latest.json" darwin-aarch64 "$out/$PRODUCT.app.tar.gz.sig"
  py verify "$out/latest.json" darwin-aarch64 "$out/$PRODUCT.app.tar.gz" "$BASE/updates/$PRODUCT.app.tar.gz"
  if [[ -f "$out/rollback/latest.json" ]]; then
    rbv="$(py field "$out/rollback/latest.json" version)"
    py newer "$v" "$rbv" || die "rollback v$rbv is not older than v$v"
    py sigfile "$out/rollback/latest.json" darwin-aarch64 "$out/rollback/$PRODUCT.app.tar.gz.sig"
    py verify "$out/rollback/latest.json" darwin-aarch64 "$out/rollback/$PRODUCT.app.tar.gz" "$BASE/updates/rollback/$PRODUCT.app.tar.gz"
    [[ -f "$dir/downloads/$PRODUCT-$rbv-arm64.dmg" ]] || die "rollback DMG $PRODUCT-$rbv-arm64.dmg is not staged"
    echo "staged rollback: v$rbv"
  else
    echo "staged rollback: NONE — the host will keep no previous build"
  fi
  echo "verify-staged: OK — v$v in $dir"
}

# ── --stage-rollback <dir>: the build the host serves NOW becomes the rollback ───
stage_rollback() {
  local dir="${1:?--stage-rollback needs a directory}" new="${2:-}" rb tmp rc lv
  [[ -d "$dir/site/dist" ]] && dir="$dir/site/dist"
  local rc2=0 f
  rb="$dir/updates/rollback"; tmp="$(mktemp -d "$TMPROOT/rollback.XXXXXX")"
  rc="$(fetch "$BASE/updates/latest.json" "$tmp/latest.json")"
  [[ "$rc" == 0 ]] || die "cannot read the live manifest at $BASE/updates/latest.json (curl $rc) — a release with no rollback is refused"
  lv="$(py field "$tmp/latest.json" version)"
  if [[ -n "$new" ]] && ! py newer "$new" "$lv"; then
    die "the host already serves v$lv; v$new is not newer — bump the version (the app only installs a strictly newer one)"
  fi
  verify_manifest_bytes "$tmp/latest.json" "$tmp" || die "the live v$lv does not verify against plugins.updater.pubkey — refusing to keep it as the known-good rollback"
  mkdir -p "$rb" "$dir/downloads"
  mv "$tmp/darwin-aarch64.tar.gz" "$rb/$PRODUCT.app.tar.gz"
  python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["platforms"]["darwin-aarch64"]["signature"].strip())' "$tmp/latest.json" > "$rb/$PRODUCT.app.tar.gz.sig"
  cp "$tmp/latest.json" "$rb/latest.json"
  py rewrite "$rb/latest.json" "$BASE/updates/rollback/$PRODUCT.app.tar.gz"
  rc="$(fetch "$BASE/downloads/$PRODUCT-$lv-arm64.dmg" "$dir/downloads/$PRODUCT-$lv-arm64.dmg")"
  if [[ "$rc" != 0 ]]; then
    gh release download "v$lv" --repo wilsonguenther-dev/wilson-voice --clobber \
      --pattern "${PRODUCT}_${lv}_aarch64.dmg" --pattern "$PRODUCT-$lv-arm64.dmg" --dir "$tmp/gh" >/dev/null 2>&1 || rc2=$?
    f="$(ls "$tmp"/gh/*.dmg 2>/dev/null | head -1 || true)"
    [[ "$rc2" == 0 && -n "$f" ]] || die "the live v$lv DMG is neither on $BASE/downloads (curl $rc) nor on GitHub release v$lv — no known-good DMG to keep"
    mv "$f" "$dir/downloads/$PRODUCT-$lv-arm64.dmg"
  fi
  echo "staged rollback: v$lv (manifest, tarball, .sig, DMG) under $dir"
}

# ── release ─────────────────────────────────────────────────────────────────────
release() {
  local tag="" stage="" notes="-" deploy=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --stage) stage="${2:?}"; shift 2 ;;
      --notes-file) notes="${2:?}"; shift 2 ;;
      --deploy) deploy=1; shift ;;
      v*) tag="$1"; shift ;;
      *) echo "unknown argument $1" >&2; exit 64 ;;
    esac
  done
  [[ -n "$tag" ]] || usage
  local ver="${tag#v}"
  [[ "$(uname -s)" == Darwin ]] || die "a release is built on the Mac that holds the Developer ID certificate"
  [[ "$ver" == "$CONF_VERSION" ]] || die "tag $tag != tauri.conf.json version $CONF_VERSION"
  local pkgv; pkgv="$(python3 -I -c 'import json,sys;print(json.load(open(sys.argv[1]))["version"])' "$DESKTOP/package.json")"
  [[ "$ver" == "$pkgv" ]] || die "tag $tag != desktop/package.json version $pkgv (they must match)"
  [[ -z "$(git -C "$ROOT" status --porcelain)" ]] || die "working tree is dirty — a release is built from a commit"
  grep -q "/downloads/$PRODUCT-$ver-arm64.dmg" "$SITE/index.html" \
    || die "site/dist/index.html does not link /downloads/$PRODUCT-$ver-arm64.dmg — bump the three hrefs first (docs/DEPLOY-SITE.md)"
  [[ "$notes" == "-" || -f "$notes" ]] || die "notes file $notes not found"
  : "${TAURI_SIGNING_PRIVATE_KEY:?export the updater key (contents or path of ~/.tauri/wilson-voice-v2.key) — without it tauri skips Yap.app.tar.gz and .sig}"
  [[ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD+set}" ]] || die "export TAURI_SIGNING_PRIVATE_KEY_PASSWORD (empty if the key has none)"
  local notary=()
  if [[ -n "${APPLE_API_KEY:-}" && -n "${APPLE_API_ISSUER:-}" && -n "${APPLE_API_KEY_PATH:-}" ]]; then
    notary=(--key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER")
  elif [[ -n "${APPLE_ID:-}" && -n "${APPLE_PASSWORD:-}" && -n "${APPLE_TEAM_ID:-}" ]]; then
    notary=(--apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID")
  else
    die "no notarization credentials: export APPLE_ID + APPLE_PASSWORD (app-specific) + APPLE_TEAM_ID, or APPLE_API_KEY + APPLE_API_ISSUER + APPLE_API_KEY_PATH. tauri notarizes the .app with them; this script notarizes the DMG."
  fi
  local profile; profile="$(bash "$ROOT/scripts/sign-local.sh" --identity)"
  [[ "$profile" == RELEASE* ]] || die "sign-local.sh resolved '$profile' — a release is signed with the Developer ID (SEC-A) identity only"
  echo "$profile"
  [[ -n "$stage" ]] || stage="$(mktemp -d)/yap-site"
  local out="$stage/site/dist"
  [[ ! -e "$out" ]] || die "$out already exists — stage into a fresh directory"
  mkdir -p "$out/downloads" "$out/updates"
  # Rollback first: it reads the live host, and refuses a version that is not newer.
  stage_rollback "$stage" "$ver"

  # app + dmg, never 'dmg updater': tauri-bundler (2.11.4 bundle.rs) builds the
  # updater tarball + .sig only when the app bundle is a package type, and
  # deletes Yap.app when it is not. createUpdaterArtifacts=true adds the triple.
  (cd "$DESKTOP" && npm run desktop:build -- --bundles app dmg) || die "desktop:build failed (build, signature verification or the mandatory release smoke) — not a release"
  local app bundle tarball dmg
  app="$(bash "$ROOT/scripts/sign-local.sh" --which-app)"
  bundle="$(dirname "$(dirname "$app")")"
  tarball="$bundle/macos/$PRODUCT.app.tar.gz"
  dmg="$bundle/dmg/${PRODUCT}_${ver}_aarch64.dmg"
  [[ -f "$tarball" && -f "$tarball.sig" ]] || die "no $tarball(.sig) — the bundler skipped the updater artifacts (key not picked up?)"
  [[ -f "$dmg" ]] || die "no $dmg"
  xcrun stapler validate "$app" || die "$app is not notarized + stapled — the tarball would install an app Gatekeeper has never seen"

  xcrun notarytool submit "$dmg" "${notary[@]}" --wait || die "DMG notarization failed"
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  spctl -a -vvv -t install "$dmg" || die "Gatekeeper rejects the stapled DMG"
  local smoke=(--app "$app" --dmg "$dmg")
  [[ -n "${YAP_SMOKE_COMPARE:-}" ]] && smoke+=(--compare "$YAP_SMOKE_COMPARE")
  bash "$ROOT/scripts/qa/release-smoke.sh" "${smoke[@]}" || die "release smoke failed on the stapled DMG — not a release"

  cp "$SITE"/*.html "$SITE"/*.css "$SITE"/*.woff2 "$SITE/DepartureMono-LICENSE.txt" "$SITE/vercel.json" "$out/"
  cp "$dmg" "$out/downloads/$PRODUCT-$ver-arm64.dmg"
  cp "$tarball" "$tarball.sig" "$out/updates/"
  py write "$out/updates/latest.json" "$ver" "$notes" "$tarball.sig" "$BASE/updates/$PRODUCT.app.tar.gz"
  verify_staged "$stage" "$ver"
  echo "staged: $out (DMG + $PRODUCT.app.tar.gz + .sig + latest.json, rollback under updates/rollback)"

  if [[ $deploy -eq 0 ]]; then
    echo "NOT DEPLOYED (no --deploy). Deploy $stage per docs/DEPLOY-SITE.md, then:"
    echo "  scripts/release-local.sh --verify-endpoint-or-name-the-reason --expect-version $ver --require-rollback"
    return 0
  fi
  local acct="$HOME/.config/drivia/drivia-accounts.env"
  [[ -f "$acct" ]] || die "$acct not found (VERCEL_TOKEN / VERCEL_TEAM_ID)"
  set -a
  # shellcheck source=/dev/null
  . "$acct"
  set +a
  unset VERCEL_PROJECT_ID
  (
    cd "$stage"
    vercel link --yes --project yap --scope wilson-guenthers-projects --token "$VERCEL_TOKEN" >/dev/null
    rm -f .env.local site/dist/.env.local
    vercel deploy --prod --yes --archive=tgz --scope wilson-guenthers-projects --token "$VERCEL_TOKEN"
  ) || die "vercel deploy failed — the host still serves the previous build"
  verify_endpoint --expect-version "$ver" --require-rollback
}

case "${1:-}" in
  --verify-endpoint-or-name-the-reason) shift; verify_endpoint "$@" ;;
  --verify-staged) shift; verify_staged "$@" ;;
  --stage-rollback) shift; stage_rollback "$@" ;;
  -h|--help|"") usage ;;
  *) release "$@" ;;
esac
