#!/usr/bin/env bash
# scripts/smoke-dmg.sh — Y7-E: smoke the shipped DMG the way a first-time user meets it.
#
# Read-only. Mounts the DMG -readonly -nobrowse on a temp mountpoint, inspects the Yap.app
# INSIDE it, downloads the updater payload into a temp dir, and installs nothing: it never
# copies into /Applications, never launches the app and never touches TCC.
#
#   smoke-dmg.sh [--profile release|local] [--team-id ID] [--updater-manifest URL|FILE]...
#                [--no-updater "<named reason>"] <path-to-dmg>
#   smoke-dmg.sh --require-artifact-or-name-the-reason
#       Smoke $YAP_SMOKE_DMG, or the newest DMG under the bundle dirs below, for real. With no
#       artifact it prints `smoke-dmg: SKIP reason=no-artifact (...)` naming every place it
#       looked, then runs --self-test so the checks themselves are still proven able to fail.
#   smoke-dmg.sh --self-test
#       Builds ad-hoc fixture DMGs (bad entitlements, renamed bundle id, dylib-injection
#       entitlement, missing usage string) and a throwaway Ed25519 key, and asserts every
#       check FAILS on the defect it exists for and passes where it should.
#
# PROFILES (SEC-A's two signing profiles, docs/RELEASE.md "Code signing"):
#   release (default) — Developer ID Application, team pinned, hardened runtime,
#       spctl --assess "accepted, source=Notarized Developer ID", a stapled ticket on the app or
#       the DMG, and the updater triple (latest.json + .app.tar.gz + .sig) verified against the
#       pubkey compiled into the shipped binary.
#   local — Apple Development, unnotarized. spctl is run and REPORTED, never asserted (a local
#       build cannot be notarized, so asserting it would fail a correct build by construction).
#       The updater is checked only when --updater-manifest is given.
#   Both profiles assert: not ad-hoc, a stable designated requirement (identifier + certificate,
#   no cdhash), the bundle id, entitlement VALUES, both sidecars present and signed, the three
#   usage strings, and no resource-fork / Finder-info / quarantine detritus inside the bundle.
#
# OUTPUT: one `check=<name> ok|fail|skip (detail)` line per check, then
#   `smoke-dmg: OK profile=<p>` (exit 0) or `smoke-dmg: FAIL profile=<p> checks=<a,b>` (exit 1).
#   Exit 2 = usage / the DMG could not be mounted.
# NOTHING SECRET IS PRINTED: no raw codesign output, no certificate serial or subject, no key
#   material. Signing identities print as their TYPE only ("Developer ID Application"); the team
#   id, CDHashes and the updater key id are public (every signed binary / manifest carries them).
#   Safe to paste into a PR.
set -u

SCRIPT_PATH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"
REPO_ROOT="$(cd "$(dirname "$SCRIPT_PATH")/.." && pwd)"
BUNDLE_ID="com.wilsonguenther.wilson-voice"   # IRREVERSIBLE: a rename resets every user's TCC grants
TEAM_ID="${YAP_TEAM_ID:-VHYV8C2JNU}"          # same pin as scripts/qa/release-smoke.sh; docs/RELEASE.md
SIDECARS="yap-polish yap-diarize"
USAGE_KEYS="NSMicrophoneUsageDescription NSAudioCaptureUsageDescription NSAppleEventsUsageDescription"
DYLIB_INJECTION="com.apple.security.cs.disable-library-validation com.apple.security.cs.allow-dyld-environment-variables"
PUBKEY_PREFIX="dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6"  # base64("untrusted comment: minisign public key:")
PLISTBUDDY=/usr/libexec/PlistBuddy

PROFILE=release
NO_UPDATER=""
MANIFESTS=()
DMG=""
FAILED=""
WORK=""
MNT=""

usage() {
  printf 'smoke-dmg: %s\n' "$1" >&2
  printf 'usage: smoke-dmg.sh [--profile release|local] [--team-id ID] [--updater-manifest URL|FILE]... [--no-updater REASON] <dmg>\n' >&2
  printf '       smoke-dmg.sh --require-artifact-or-name-the-reason | --self-test\n' >&2
  exit 2
}

ok() { printf 'check=%s ok%s\n' "$1" "${2:+ ($2)}"; }
skip() { printf 'check=%s skip (%s)\n' "$1" "$2"; }
fail() {
  printf 'check=%s fail (%s)\n' "$1" "$2"
  case ",$FAILED," in *",$1,"*) ;; *) FAILED="${FAILED:+$FAILED,}$1" ;; esac
}

cleanup() {
  if [ -n "$MNT" ] && [ -d "$MNT" ]; then
    hdiutil detach "$MNT" -quiet >/dev/null 2>&1 || hdiutil detach "$MNT" -force -quiet >/dev/null 2>&1
    rmdir "$MNT" 2>/dev/null
  fi
  [ -n "$WORK" ] && [ -d "$WORK" ] && rm -rf "$WORK"
}
trap cleanup EXIT

# OpenSSL 3 is the Ed25519 verifier (macOS's LibreSSL cannot do `pkeyutl -rawin`).
find_openssl3() {
  local c
  for c in "${YAP_OPENSSL:-}" openssl /opt/homebrew/bin/openssl /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/bin/openssl; do
    [ -n "$c" ] || continue
    command -v "$c" >/dev/null 2>&1 || continue
    case "$("$c" version 2>/dev/null)" in "OpenSSL 3"*) command -v "$c"; return 0 ;; esac
  done
  return 1
}

# ── minisign (Tauri updater signatures), in python3 + OpenSSL 3 ─────────────
# verify  OPENSSL PUBKEY_B64 SIG_B64 FILE  -> prints `keyid=... alg=... trusted=...`, exit 0 ok / 1 bad
# pubkey  BINARY                            -> prints the base64 pubkey compiled into BINARY
# selftest OPENSSL                          -> signs with a throwaway key; good passes, tamper fails
read -r -d '' MINISIGN_PY <<'PY'
import base64, hashlib, os, re, subprocess, sys, tempfile

def b64(s):
    return base64.b64decode(s.strip(), validate=True)

def parse_pub(pub_b64):
    lines = [l for l in b64(pub_b64).decode().splitlines() if l.strip()]
    raw = b64(lines[1])
    if raw[:2] != b"Ed" or len(raw) != 42:
        raise ValueError("not a minisign Ed25519 public key")
    return raw[2:10], raw[10:]

def parse_sig(sig_b64):
    lines = [l for l in b64(sig_b64).decode().splitlines() if l.strip()]
    raw = b64(lines[1])
    if len(raw) != 74 or raw[:2] not in (b"Ed", b"ED"):
        raise ValueError("not a minisign Ed25519 signature")
    if not lines[2].startswith("trusted comment: "):
        raise ValueError("no trusted comment")
    return raw[:2], raw[2:10], raw[10:], lines[2][len("trusted comment: "):], b64(lines[3])

def ed(openssl, op, key_path, msg, sig=None):
    with tempfile.TemporaryDirectory() as d:
        m = os.path.join(d, "m"); open(m, "wb").write(msg)
        args = [openssl, "pkeyutl", "-" + op, "-rawin", "-in", m]
        if op == "verify":
            s = os.path.join(d, "s"); open(s, "wb").write(sig)
            args += ["-pubin", "-inkey", key_path, "-sigfile", s]
            return subprocess.run(args, capture_output=True).returncode == 0
        args += ["-inkey", key_path]
        return subprocess.run(args, capture_output=True, check=True).stdout

def pub_pem(pk, d):
    p = os.path.join(d, "pub.pem")
    der = bytes.fromhex("302a300506032b6570032100") + pk
    open(p, "w").write("-----BEGIN PUBLIC KEY-----\n" + base64.b64encode(der).decode() + "\n-----END PUBLIC KEY-----\n")
    return p

def verify(openssl, pub_b64, sig_b64, path):
    keyid, pk = parse_pub(pub_b64)
    alg, skeyid, sig, trusted, gsig = parse_sig(sig_b64)
    kid = skeyid[::-1].hex().upper()
    if skeyid != keyid:
        print("keyid mismatch: signature key=%s shipped key=%s" % (kid, keyid[::-1].hex().upper())); return 1
    data = open(path, "rb").read()
    msg = hashlib.blake2b(data, digest_size=64).digest() if alg == b"ED" else data
    with tempfile.TemporaryDirectory() as d:
        p = pub_pem(pk, d)
        if not ed(openssl, "verify", p, msg, sig):
            print("signature does NOT verify over the file (keyid=%s)" % kid); return 1
        if not ed(openssl, "verify", p, sig + trusted.encode(), gsig):
            print("trusted-comment signature does NOT verify (keyid=%s)" % kid); return 1
    print("keyid=%s alg=%s trusted=%s" % (kid, alg.decode(), trusted)); return 0

def pubkey_from_binary(path):
    data = open(path, "rb").read()
    pat = re.compile(rb"^untrusted comment: minisign public key: [0-9A-F]{16}\n[A-Za-z0-9+/]{56}\n?$")
    prefix = b"dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6"
    found = []
    i = data.find(prefix)
    while i >= 0:
        j = i
        while j < len(data) and data[j:j+1] in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=":
            j += 1
        run = data[i:j]
        for n in range(len(run) - len(run) % 4, 0, -4):
            try:
                if pat.match(base64.b64decode(run[:n], validate=True)):
                    found.append(run[:n].decode()); break
            except Exception:
                pass
        i = data.find(prefix, i + 1)
    uniq = sorted(set(found))
    if len(uniq) != 1:
        print("found %d distinct minisign public keys in the binary" % len(uniq)); return 1
    print(uniq[0]); return 0

def selftest(openssl):
    fails = 0
    with tempfile.TemporaryDirectory() as d:
        def keypair(name):
            k = os.path.join(d, name)
            subprocess.run([openssl, "genpkey", "-algorithm", "ed25519", "-out", k], check=True, capture_output=True)
            der = subprocess.run([openssl, "pkey", "-in", k, "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout
            return k, der[-32:]
        k, pk = keypair("k1"); _, other_pk = keypair("k2")
        keyid = os.urandom(8)
        def pubtxt(pk_):
            return base64.b64encode(("untrusted comment: minisign public key: %s\n%s\n" % (
                keyid[::-1].hex().upper(), base64.b64encode(b"Ed" + keyid + pk_).decode())).encode()).decode()
        f = os.path.join(d, "Yap.app.tar.gz"); open(f, "wb").write(os.urandom(4096))
        sig = ed(openssl, "sign", k, hashlib.blake2b(open(f, "rb").read(), digest_size=64).digest())
        trusted = "timestamp:0\tfile:Yap.app.tar.gz"
        gsig = ed(openssl, "sign", k, sig + trusted.encode())
        sigtxt = base64.b64encode(("untrusted comment: signature from tauri secret key\n%s\ntrusted comment: %s\n%s\n" % (
            base64.b64encode(b"ED" + keyid + sig).decode(), trusted, base64.b64encode(gsig).decode())).encode()).decode()
        bin_ = os.path.join(d, "bin"); open(bin_, "wb").write(os.urandom(64) + pubtxt(pk).encode() + b"https\x00junk")
        def expect(label, want, got):
            nonlocal fails
            print("self-test minisign %-34s %s" % (label, "ok" if want == got else "FAIL (want %d got %d)" % (want, got)))
            fails += want != got
        expect("good signature verifies", 0, verify(openssl, pubtxt(pk), sigtxt, f))
        expect("wrong public key is rejected", 1, verify(openssl, pubtxt(other_pk), sigtxt, f))
        open(f, "ab").write(b"x")
        expect("tampered tarball is rejected", 1, verify(openssl, pubtxt(pk), sigtxt, f))
        import io, contextlib
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = pubkey_from_binary(bin_)
        expect("pubkey extracted from a binary", 0, 0 if rc == 0 and buf.getvalue().strip() == pubtxt(pk) else 1)
    return 1 if fails else 0

cmd = sys.argv[1]
try:
    if cmd == "verify": sys.exit(verify(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5]))
    if cmd == "pubkey": sys.exit(pubkey_from_binary(sys.argv[2]))
    if cmd == "selftest": sys.exit(selftest(sys.argv[2]))
except (ValueError, IndexError, UnicodeDecodeError, base64.binascii.Error) as e:
    print("malformed: %s" % e); sys.exit(1)
sys.exit(64)
PY
minisign() { python3 -I -c "$MINISIGN_PY" "$@"; }

# ── signature helpers (never echo raw codesign output) ──────────────────────
sig_info() { codesign -dv --verbose=4 "$1" 2>&1; }
leaf_type() { # -> "Developer ID Application" | "Apple Development" | "adhoc" | "none" | other
  local info="$1" leaf
  if grep -q '^Signature=adhoc' <<<"$info" || grep -q '^CodeDirectory .*flags=[^ ]*adhoc' <<<"$info"; then
    echo adhoc; return
  fi
  leaf="$(sed -n 's/^Authority=\([^:]*\):.*/\1/p' <<<"$info" | head -1)"
  [ -n "$leaf" ] && echo "$leaf" || echo none
}
team_of() { sed -n 's/^TeamIdentifier=//p' <<<"$1" | head -1; }
has_runtime() { grep -q '^CodeDirectory .*flags=0x[0-9a-f]*([^)]*runtime' <<<"$1"; }
cdhash_of() { sed -n 's/^CDHash=//p' <<<"$1" | head -1; }

# Entitlements as an XML plist in $2 (empty file = none).
entitlements_to() {
  codesign -d --entitlements - --xml "$1" >"$2" 2>/dev/null
  if [ ! -s "$2" ]; then codesign -d --entitlements :- "$1" >"$2" 2>/dev/null; fi
  [ -s "$2" ] || : >"$2"
}
ent_value() { # $1 plist file, $2 key -> true|false|absent
  local v
  [ -s "$1" ] || { echo absent; return; }
  v="$("$PLISTBUDDY" -c "Print :$2" "$1" 2>/dev/null)" || { echo absent; return; }
  echo "$v"
}

# Expected signing identity per profile; prints the reason on mismatch.
identity_ok() { # $1 label, $2 info -> 0 ok
  local t team
  t="$(leaf_type "$2")"; team="$(team_of "$2")"
  case "$t" in
    adhoc) echo "$1 is ad-hoc signed (Signature=adhoc): no stable designated requirement, TCC grants die every build"; return 1 ;;
    none) echo "$1 has no Authority chain (unsigned)"; return 1 ;;
  esac
  if [ "$PROFILE" = release ]; then
    [ "$t" = "Developer ID Application" ] || { echo "$1 identity is '$t', release needs 'Developer ID Application'"; return 1; }
    [ "$team" = "$TEAM_ID" ] || { echo "$1 TeamIdentifier=${team:-none} is not the pinned team $TEAM_ID"; return 1; }
  else
    [ "$t" = "Apple Development" ] || [ "$t" = "Developer ID Application" ] ||
      { echo "$1 identity is '$t', local needs 'Apple Development' (or Developer ID)"; return 1; }
    { [ -n "$team" ] && [ "$team" != "not set" ]; } || { echo "$1 has no TeamIdentifier"; return 1; }
  fi
  has_runtime "$2" || { echo "$1 lacks the hardened runtime (flags=0x10000(runtime))"; return 1; }
  echo "$1: $t, team=$team, runtime"
}

# ── the checks ──────────────────────────────────────────────────────────────
check_spctl() { # $1 app, $2 dmg
  local out res src
  out="$(spctl --assess --type exec -vv "$1" 2>&1)"
  res="$(head -1 <<<"$out" | sed 's/^.*: //')"
  src="$(sed -n 's/^source=//p' <<<"$out" | head -1)"
  if [ "$PROFILE" = local ]; then
    skip gatekeeper "local profile: spctl --assess says '${res:-?}' source='${src:-none}' — reported, not asserted (unnotarized by design)"
    return
  fi
  if [ "$res" = accepted ] && [ "$src" = "Notarized Developer ID" ]; then
    ok gatekeeper "spctl --assess --type exec: accepted, source=Notarized Developer ID"
  else
    fail gatekeeper "spctl --assess --type exec: '${res:-?}' source='${src:-none}' — every user hits a Gatekeeper wall"
  fi
  local app_st=no dmg_st=no
  xcrun stapler validate "$1" >/dev/null 2>&1 && app_st=yes
  xcrun stapler validate "$2" >/dev/null 2>&1 && dmg_st=yes
  if [ "$app_st" = yes ] || [ "$dmg_st" = yes ]; then
    ok stapled "ticket stapled: app=$app_st dmg=$dmg_st"
  else
    fail stapled "no stapled notarization ticket on the app or the DMG (first launch offline is blocked)"
  fi
}

check_signature() { # $1 app
  local info msg dr verify
  info="$(sig_info "$1")"
  if msg="$(identity_ok app "$info")"; then ok signature "$msg"; else fail signature "$msg"; fi
  if verify="$(codesign --verify --deep --strict "$1" 2>&1)"; then
    ok seal "codesign --verify --deep --strict"
  else
    fail seal "codesign --verify --deep --strict: $(grep -m1 -o 'resource fork, Finder information, or similar detritus not allowed\|a sealed resource is missing or invalid\|code object is not signed at all\|invalid signature[^(]*\|not signed at all' <<<"$verify" || echo 'failed')"
  fi
  dr="$(codesign -d -r- "$1" 2>&1 | sed -n 's/^designated => //p')"
  if [ -z "$dr" ]; then
    fail requirement "no designated requirement"
  elif grep -q 'cdhash' <<<"$dr"; then
    fail requirement "designated requirement pins a cdhash: changes every build, TCC grants die"
  elif ! grep -q "identifier \"$BUNDLE_ID\"" <<<"$dr" || ! grep -q 'certificate' <<<"$dr"; then
    fail requirement "designated requirement does not name identifier \"$BUNDLE_ID\" plus a certificate"
  else
    ok requirement "stable: identifier \"$BUNDLE_ID\" + certificate chain, no cdhash"
  fi
}

check_entitlements() { # $1 app
  local f="$WORK/app-ent.plist" bad="" v k
  entitlements_to "$1" "$f"
  v="$(ent_value "$f" com.apple.security.app-sandbox)"
  [ "$v" = false ] || bad="${bad}com.apple.security.app-sandbox=$v (must be present and false); "
  v="$(ent_value "$f" com.apple.security.device.audio-input)"
  [ "$v" = true ] || bad="${bad}com.apple.security.device.audio-input=$v (must be true); "
  for k in $DYLIB_INJECTION; do
    v="$(ent_value "$f" "$k")"
    [ "$v" = absent ] || bad="${bad}${k#com.apple.security.}=$v (must be absent); "
  done
  if [ -z "$bad" ]; then
    ok entitlements "app-sandbox=false audio-input=true, dylib-injection pair absent"
  else
    fail entitlements "${bad%; }"
  fi
}

check_bundle_id() { # $1 app
  local id
  id="$("$PLISTBUDDY" -c 'Print :CFBundleIdentifier' "$1/Contents/Info.plist" 2>/dev/null)"
  if [ "$id" = "$BUNDLE_ID" ]; then
    ok bundle_id "$BUNDLE_ID"
  else
    fail bundle_id "CFBundleIdentifier='${id:-missing}', expected $BUNDLE_ID — a rename resets every user's TCC grants"
  fi
}

check_info_plist() { # $1 app
  local k v missing=""
  for k in $USAGE_KEYS; do
    v="$("$PLISTBUDDY" -c "Print :$k" "$1/Contents/Info.plist" 2>/dev/null)"
    [ -n "$v" ] || missing="$missing $k"
  done
  if [ -z "$missing" ]; then
    ok info_plist "NSMicrophoneUsageDescription NSAudioCaptureUsageDescription NSAppleEventsUsageDescription present"
  else
    fail info_plist "missing or empty:$missing — a TCC failure with no dialog"
  fi
}

check_sidecars() { # $1 app
  local name p info msg f v bad="" good=""
  for name in $SIDECARS; do
    p="$(find "$1/Contents/MacOS" "$1/Contents/Resources" -maxdepth 2 -type f -name "$name" 2>/dev/null | head -1)"
    [ -n "$p" ] || p="$(find "$1/Contents/MacOS" "$1/Contents/Resources" -maxdepth 2 -type f -name "$name-*" 2>/dev/null | head -1)"
    if [ -z "$p" ]; then bad="${bad}$name: not in Contents/MacOS or Contents/Resources; "; continue; fi
    info="$(sig_info "$p")"
    if ! msg="$(identity_ok "$name" "$info")"; then bad="${bad}$msg; "; continue; fi
    if ! codesign --verify --strict "$p" >/dev/null 2>&1; then bad="${bad}$name: codesign --verify --strict failed; "; continue; fi
    f="$WORK/$name-ent.plist"; entitlements_to "$p" "$f"
    v="$(ent_value "$f" com.apple.security.app-sandbox)"
    [ "$v" = true ] && { bad="${bad}$name: app-sandbox=true; "; continue; }
    for k in $DYLIB_INJECTION; do
      v="$(ent_value "$f" "$k")"
      [ "$v" = absent ] || bad="${bad}$name: ${k#com.apple.security.}=$v; "
    done
    good="$good $name@${p#"$1"/}"
  done
  if [ -z "$bad" ]; then ok sidecars "signed:$good"; else fail sidecars "${bad%; }"; fi
}

check_detritus() { # $1 app, $2 dmg
  local names apple_double dmgq
  names="$(xattr -r "$1" 2>/dev/null | sed 's/^.*: //' | grep -E '^com\.apple\.(quarantine|FinderInfo|ResourceFork)$' | sort | uniq -c | awk '{printf "%s%s x%s", (NR>1?", ":""), $2, $1}')"
  apple_double="$(find "$1" \( -name '._*' -o -name '.DS_Store' -o -name $'Icon\r' \) 2>/dev/null | wc -l | tr -d ' ')"
  dmgq=absent; xattr "$2" 2>/dev/null | grep -qx com.apple.quarantine && dmgq=present
  if [ -z "$names" ] && [ "$apple_double" = 0 ]; then
    ok detritus "no quarantine / FinderInfo / ResourceFork xattrs and no AppleDouble files inside the bundle (resource-fork detritus: none); dmg quarantine=$dmgq (informational)"
  else
    fail detritus "inside the bundle: ${names:-no xattrs} ; AppleDouble/.DS_Store/Icon files=$apple_double — the 'resource fork, Finder information, or similar detritus' codesign flake"
  fi
}

check_updater() { # $1 app
  local exe="$1/Contents/MacOS/$2" openssl pub keyid confpub appver endpoints=() e n=0 bad="" good=""
  if [ -n "$NO_UPDATER" ]; then skip updater "$NO_UPDATER"; return; fi
  if [ "$PROFILE" = local ] && [ ${#MANIFESTS[@]} -eq 0 ]; then
    skip updater "local profile: no --updater-manifest given (a local DMG has no published triple)"; return
  fi
  if ! openssl="$(find_openssl3)"; then
    fail updater "no Ed25519 verifier: OpenSSL 3 not found (brew install openssl@3, or set YAP_OPENSSL)"; return
  fi
  if ! pub="$(minisign pubkey "$exe")"; then
    fail updater "shipped pubkey: $pub"; return
  fi
  keyid="$(printf '%s' "$pub" | base64 -D 2>/dev/null | sed -n 's/^untrusted comment: minisign public key: //p')"
  confpub="$(sed -n 's/.*"pubkey": *"\([^"]*\)".*/\1/p' "$REPO_ROOT/desktop/src-tauri/tauri.conf.json" 2>/dev/null)"
  printf 'updater: shipped pubkey keyid=%s (%s tauri.conf.json at this checkout)\n' "$keyid" \
    "$([ "$pub" = "$confpub" ] && echo matches || echo DIFFERS from)"
  appver="$("$PLISTBUDDY" -c 'Print :CFBundleShortVersionString' "$1/Contents/Info.plist" 2>/dev/null)"
  if [ ${#MANIFESTS[@]} -gt 0 ]; then
    endpoints=("${MANIFESTS[@]}")
  else
    while IFS= read -r e; do [ -n "$e" ] && endpoints+=("$e"); done < <(
      LC_ALL=C grep -aoE 'https://[A-Za-z0-9.-]+/[A-Za-z0-9./_-]*latest\.json' "$exe" | awk '!seen[$0]++')
  fi
  if [ ${#endpoints[@]} -eq 0 ]; then fail updater "no updater endpoint found in the shipped binary"; return; fi
  for e in "${endpoints[@]}"; do
    n=$((n + 1))
    local d="$WORK/upd$n" mver url sig vout sibling
    mkdir -p "$d"
    if [ -f "$e" ]; then cp "$e" "$d/latest.json"
    elif ! curl -sfL --max-time 30 -o "$d/latest.json" "$e"; then bad="${bad}$e: does not resolve; "; continue; fi
    if ! jq -e '.version and .pub_date and .platforms."darwin-aarch64".url and .platforms."darwin-aarch64".signature' "$d/latest.json" >/dev/null 2>&1; then
      bad="${bad}$e: latest.json lacks version/pub_date/platforms.darwin-aarch64.{url,signature}; "; continue
    fi
    mver="$(jq -r .version "$d/latest.json")"
    url="$(jq -r '.platforms."darwin-aarch64".url' "$d/latest.json")"
    sig="$(jq -r '.platforms."darwin-aarch64".signature' "$d/latest.json")"
    if ! curl -sfL --max-time 600 -o "$d/payload.tar.gz" "$url"; then bad="${bad}$e: payload url does not resolve ($url); "; continue; fi
    if ! curl -sfL --max-time 30 -o "$d/payload.sig" "$url.sig"; then
      bad="${bad}$e: the .sig of the triple does not resolve ($url.sig); "; continue
    fi
    sibling="$(tr -d '\n' <"$d/payload.sig")"
    [ "$sibling" = "$sig" ] || { bad="${bad}$e: $url.sig differs from the manifest signature; "; continue; }
    if ! vout="$(minisign verify "$openssl" "$pub" "$sig" "$d/payload.tar.gz")"; then
      bad="${bad}$e: $vout; "; continue
    fi
    local note="manifest=$mver app=$appver"
    if [ "$mver" = "$appver" ]; then
      # Same version: the payload must be THIS build. Compare the main executable's CDHash.
      if tar -xzf "$d/payload.tar.gz" -C "$d" 2>/dev/null && [ -d "$d/$(basename "$1")" ]; then
        local a b
        a="$(cdhash_of "$(sig_info "$1")")"; b="$(cdhash_of "$(sig_info "$d/$(basename "$1")")")"
        if [ -n "$a" ] && [ "$a" = "$b" ]; then note="$note, payload CDHash == DMG app"
        else bad="${bad}$e: same version $mver but the updater payload is a different build than the DMG (CDHash differs); "; continue; fi
      else
        bad="${bad}$e: payload is not a tarball of $(basename "$1"); "; continue
      fi
    else
      note="$note (versions differ: the manifest is not yet published for this DMG, or this DMG is not the latest)"
    fi
    good="$good [$e -> $(basename "$url") + .sig verified, $vout, $note]"
  done
  if [ -z "$bad" ]; then ok updater "triple resolves and verifies against the shipped pubkey:$good"
  else fail updater "${bad%; }"; fi
}

smoke() { # $1 dmg
  local dmg="$1" app exe
  [ -f "$dmg" ] || usage "no DMG at $dmg"
  WORK="$(mktemp -d -t yap-smoke-dmg)" || exit 2
  MNT="$WORK/mnt"; mkdir -p "$MNT"
  if ! hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$MNT" "$dmg" >/dev/null 2>&1; then
    rmdir "$MNT" 2>/dev/null; MNT=""
    printf 'smoke-dmg: cannot mount %s read-only\n' "$dmg"; exit 2
  fi
  app="$(find "$MNT" -maxdepth 1 -name '*.app' -type d | head -1)"
  printf 'smoke-dmg: profile=%s dmg=%s app=%s (mounted read-only; nothing is installed)\n' "$PROFILE" "$(basename "$dmg")" "${app:+$(basename "$app")}"
  if [ -z "$app" ]; then fail bundle "no .app at the root of the DMG"; finish; return $?; fi
  exe="$("$PLISTBUDDY" -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist" 2>/dev/null)"
  printf 'smoke-dmg: version=%s executable=%s\n' "$("$PLISTBUDDY" -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist" 2>/dev/null)" "${exe:-?}"
  check_spctl "$app" "$dmg"
  check_signature "$app"
  check_entitlements "$app"
  check_bundle_id "$app"
  check_info_plist "$app"
  check_sidecars "$app"
  check_detritus "$app" "$dmg"
  check_updater "$app" "${exe:-wilson-voice}"
  finish
}

finish() {
  if [ -n "$FAILED" ]; then printf 'smoke-dmg: FAIL profile=%s checks=%s\n' "$PROFILE" "$FAILED"; return 1; fi
  printf 'smoke-dmg: OK profile=%s\n' "$PROFILE"; return 0
}

# ── --self-test ─────────────────────────────────────────────────────────────
make_fixture() { # $1 dir, $2 bundle id, $3 entitlements xml body, $4 usage keys -> $1/fixture.dmg
  local src="$1/src" app="$1/src/Yap.app" k s
  mkdir -p "$app/Contents/MacOS"
  {
    printf '<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict>\n'
    printf '<key>CFBundleIdentifier</key><string>%s</string><key>CFBundleExecutable</key><string>wilson-voice</string>\n' "$2"
    printf '<key>CFBundleShortVersionString</key><string>0.0.0</string><key>CFBundlePackageType</key><string>APPL</string>\n'
    for k in $4; do printf '<key>%s</key><string>fixture</string>\n' "$k"; done
    printf '</dict></plist>\n'
  } >"$app/Contents/Info.plist"
  printf '<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict>%s</dict></plist>\n' "$3" >"$1/ent.plist"
  for s in yap-polish yap-diarize; do
    cp /usr/bin/true "$app/Contents/MacOS/$s"
    codesign -f -s - --options runtime "$app/Contents/MacOS/$s" >/dev/null 2>&1 || return 1
  done
  cp /usr/bin/true "$app/Contents/MacOS/wilson-voice"
  codesign -f -s - --options runtime --entitlements "$1/ent.plist" "$app" >/dev/null 2>&1 || return 1
  hdiutil create -quiet -fs HFS+ -volname YapFixture -srcfolder "$src" -format UDZO "$1/fixture.dmg" >/dev/null 2>&1
}

self_test() {
  local root fails=0 out rc openssl
  local GOOD_ENT='<key>com.apple.security.app-sandbox</key><false/><key>com.apple.security.device.audio-input</key><true/>'
  root="$(mktemp -d -t yap-smoke-dmg-selftest)" || return 1
  expect_line() { # $1 label, $2 output, $3 regex
    if grep -qE "$3" <<<"$2"; then printf 'self-test %-58s ok\n' "$1"
    else printf 'self-test %-58s FAIL (no line matching /%s/)\n' "$1" "$3"; fails=$((fails + 1)); fi
  }
  run_case() { # $1 name, $2 bundle id, $3 ent, $4 usage keys -> sets out, rc
    mkdir -p "$root/$1"
    if ! make_fixture "$root/$1" "$2" "$3" "$4"; then out="fixture build failed"; rc=99; return; fi
    out="$(bash "$SCRIPT_PATH" --profile local --no-updater "self-test fixture" "$root/$1/fixture.dmg" 2>&1)"; rc=$?
  }

  # A: ad-hoc, sandbox ON, a usage string missing — the three "fails silently for a user" classes.
  run_case adhoc "$BUNDLE_ID" '<key>com.apple.security.app-sandbox</key><true/><key>com.apple.security.device.audio-input</key><true/>' \
    "NSMicrophoneUsageDescription NSAppleEventsUsageDescription"
  expect_line "A exits 1" "rc=$rc" '^rc=1$'
  expect_line "A ad-hoc signature fails" "$out" '^check=signature fail \(.*adhoc'
  expect_line "A sidecars fail (ad-hoc)" "$out" '^check=sidecars fail \(.*adhoc'
  expect_line "A app-sandbox=true fails on the VALUE" "$out" '^check=entitlements fail \(.*app-sandbox=true'
  expect_line "A missing NSAudioCaptureUsageDescription fails" "$out" '^check=info_plist fail \(.*NSAudioCaptureUsageDescription'
  expect_line "A correct bundle id passes" "$out" '^check=bundle_id ok'
  expect_line "A local profile reports, never asserts, spctl" "$out" '^check=gatekeeper skip \(local profile'
  expect_line "A named updater skip is carried" "$out" '^check=updater skip \(self-test fixture\)'
  expect_line "A clean bundle has no detritus" "$out" '^check=detritus ok'

  # B: renamed bundle id, otherwise correct.
  run_case renamed com.example.renamed "$GOOD_ENT" "$USAGE_KEYS"
  expect_line "B renamed bundle id fails" "$out" '^check=bundle_id fail \(CFBundleIdentifier=.com\.example\.renamed'
  expect_line "B correct entitlements pass" "$out" '^check=entitlements ok'
  expect_line "B all three usage strings pass" "$out" '^check=info_plist ok'

  # C: a dylib-injection entitlement, and app-sandbox key absent (absent != false).
  run_case dylib "$BUNDLE_ID" '<key>com.apple.security.device.audio-input</key><true/><key>com.apple.security.cs.disable-library-validation</key><true/>' "$USAGE_KEYS"
  expect_line "C disable-library-validation fails" "$out" '^check=entitlements fail \(.*cs\.disable-library-validation=true'
  expect_line "C absent app-sandbox key fails (absent is not false)" "$out" '^check=entitlements fail \(.*app-sandbox=absent'

  # D: release profile on an ad-hoc fixture — notarization and identity are asserted, not reported.
  out="$(bash "$SCRIPT_PATH" --no-updater "self-test fixture" "$root/renamed/fixture.dmg" 2>&1)"; rc=$?
  expect_line "D release profile asserts Gatekeeper" "$out" '^check=gatekeeper fail'
  expect_line "D release profile asserts a stapled ticket" "$out" '^check=stapled fail'

  # E: nothing secret on any line printed above.
  expect_line "E no secret-looking output" "$(printf '%s\n' "$out" | grep -ciE 'serial|password|BEGIN .*KEY' || true)" '^0$'

  # F: the updater signature verifier can fail.
  if openssl="$(find_openssl3)"; then
    if ! minisign selftest "$openssl"; then fails=$((fails + 1)); fi
  else
    printf 'self-test minisign skipped: OpenSSL 3 not found (named reason; the updater check fails closed without it)\n'
  fi

  rm -rf "$root"
  if [ "$fails" -ne 0 ]; then printf 'self-test: %d FAIL\n' "$fails"; return 1; fi
  printf 'self-test: all ok\n'
}

require_artifact() {
  local cands=() c newest="" looked=""
  if [ -n "${YAP_SMOKE_DMG:-}" ]; then
    [ -f "$YAP_SMOKE_DMG" ] || { printf 'smoke-dmg: YAP_SMOKE_DMG=%s is not a file\n' "$YAP_SMOKE_DMG"; return 1; }
    newest="$YAP_SMOKE_DMG"
  else
    for c in ${CARGO_TARGET_DIR:+"$CARGO_TARGET_DIR/release/bundle/dmg"} "$REPO_ROOT/desktop/src-tauri/target/release/bundle/dmg"; do
      looked="$looked $c"
      for f in "$c"/*.dmg; do [ -f "$f" ] && cands+=("$f"); done
    done
    [ ${#cands[@]} -gt 0 ] && newest="$(ls -t "${cands[@]}" | head -1)"
  fi
  if [ -n "$newest" ]; then
    PROFILE="${YAP_SMOKE_PROFILE:-release}"
    smoke "$newest"; return $?
  fi
  printf 'smoke-dmg: SKIP reason=no-artifact (no $YAP_SMOKE_DMG; no *.dmg in:%s). A signed, notarized DMG is minutes of build plus Apple'"'"'s servers and belongs to the release checklist (docs/RELEASE.md, "DMG smoke") and to the review pass with {dmg: true}, not to the per-item gate. Running --self-test so the checks are still proven able to fail.\n' "$looked"
  self_test
}

# ── argv ────────────────────────────────────────────────────────────────────
MODE=smoke
while [ $# -gt 0 ]; do
  case "$1" in
    --profile) [ $# -ge 2 ] || usage "--profile needs release|local"; PROFILE="$2"; shift 2 ;;
    --team-id) [ $# -ge 2 ] || usage "--team-id needs a value"; TEAM_ID="$2"; shift 2 ;;
    --updater-manifest) [ $# -ge 2 ] || usage "--updater-manifest needs a URL or file"; MANIFESTS+=("$2"); shift 2 ;;
    --no-updater) [ $# -ge 2 ] && [ -n "$2" ] || usage "--no-updater needs a named reason"; NO_UPDATER="$2"; shift 2 ;;
    --require-artifact-or-name-the-reason) MODE=require; shift ;;
    --self-test) MODE=self; shift ;;
    -h | --help) sed -n '2,36p' "$SCRIPT_PATH"; exit 0 ;;
    -*) usage "unknown flag $1" ;;
    *) [ -z "$DMG" ] || usage "one DMG only"; DMG="$1"; shift ;;
  esac
done
case "$PROFILE" in release | local) ;; *) usage "--profile must be release or local" ;; esac

case "$MODE" in
  self) self_test; exit $? ;;
  require) require_artifact; exit $? ;;
  smoke) [ -n "$DMG" ] || usage "no DMG given"; smoke "$DMG"; exit $? ;;
esac
