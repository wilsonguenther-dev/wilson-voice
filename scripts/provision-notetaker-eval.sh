#!/usr/bin/env bash
# Provision the two notetaker eval inputs ONCE, at fixed paths, verified by hash.
#
#   model:  $YAP_MODEL_DIR/<Q8_0 filename>   default $HOME/code/wilson-voice-loop/cache/models
#   corpus: $YAP_EVAL_CORPUS                 default $HOME/yap-eval-corpus/meetings
#
# Every model-backed eval gate points at these two paths explicitly — never at Wilson's real
# install, never a skip. Idempotent: a second run verifies and prints "model ok" / "corpus ok".
#
# The pin (id, revision, filename, size, sha256) is read from desktop/src-tauri/src/catalog.json,
# the one source of truth; this script holds no second copy of the hash. The installed copy under
# ~/Library/Application Support/WilsonVoice/models is only ever READ and APFS-cloned (cp -c), never
# moved or modified. The corpus manifest is never rewritten to match a regrown corpus.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CATALOG="$ROOT/desktop/src-tauri/src/catalog.json"
MANIFEST_SHA="$ROOT/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.sha256"
MANIFEST_JSON="$ROOT/desktop/src-tauri/tests/fixtures/meeting_eval_manifest.json"
MODEL_ID="handy-computer/parakeet-unified-en-0.6b-gguf"
QUANT="Q8_0"
INSTALLED_DIR="$HOME/Library/Application Support/WilsonVoice/models"

MODEL_DIR="${YAP_MODEL_DIR:-$HOME/code/wilson-voice-loop/cache/models}"
CORPUS_DIR="${YAP_EVAL_CORPUS:-$HOME/yap-eval-corpus/meetings}"

die() { echo "provision-notetaker-eval: $*" >&2; exit 1; }
sha_of() { shasum -a 256 "$1" | cut -c1-64; }

command -v jq >/dev/null || die "jq is required (ships with macOS 15+)"
[[ -f "$CATALOG" ]] || die "catalog not found: $CATALOG"
[[ -f "$MANIFEST_SHA" ]] || die "corpus manifest not found: $MANIFEST_SHA"

# ── a. the pin, from the catalog ───────────────────────────────────────────────
REVISION="$(jq -er --arg id "$MODEL_ID" '.models[] | select(.id == $id) | .revision' "$CATALOG")" \
  || die "catalog has no model $MODEL_ID"
FILE_ROW="$(jq -ec --arg id "$MODEL_ID" --arg q "$QUANT" \
  '.models[] | select(.id == $id) | .files[] | select(.quant == $q)' "$CATALOG")" \
  || die "catalog model $MODEL_ID has no $QUANT file"
FILENAME="$(jq -er '.filename' <<<"$FILE_ROW")"
SIZE_BYTES="$(jq -er '.size_bytes' <<<"$FILE_ROW")"
SHA256="$(jq -er '.sha256' <<<"$FILE_ROW")"
[[ "$SHA256" =~ ^[0-9a-f]{64}$ ]] || die "catalog sha256 for $FILENAME is malformed: $SHA256"
[[ "$FILENAME" != */* && "$FILENAME" != .* ]] || die "catalog filename is not a plain name: $FILENAME"
MIRRORS=()
while IFS= read -r m; do [[ -n "$m" ]] && MIRRORS+=("${m%/}"); done < <(jq -r '.mirrors[]?' "$CATALOG")

MODEL_PATH="$MODEL_DIR/$FILENAME"
PARTIAL="$MODEL_PATH.partial"

verified() { [[ -f "$1" ]] && [[ "$(stat -f '%z' "$1")" == "$SIZE_BYTES" ]] && [[ "$(sha_of "$1")" == "$SHA256" ]]; }

# ── b. the model ────────────────────────────────────────────────────────────────
mkdir -p "$MODEL_DIR"
if verified "$MODEL_PATH"; then
  echo "model ok"
else
  [[ -e "$MODEL_PATH" ]] && { echo "cached $FILENAME fails its catalog sha256; replacing it"; rm -f "$MODEL_PATH"; }
  rm -f "$PARTIAL"
  INSTALLED="$INSTALLED_DIR/$FILENAME"
  if verified "$INSTALLED"; then
    # APFS clone of the installed copy: copy-on-write, the source is only read.
    cp -c "$INSTALLED" "$PARTIAL" || cp "$INSTALLED" "$PARTIAL"
    echo "cloned the installed copy (read-only source): $INSTALLED"
  else
    URLS=("https://huggingface.co/$MODEL_ID/resolve/$REVISION/$FILENAME")
    for m in ${MIRRORS[@]+"${MIRRORS[@]}"}; do URLS+=("$m/$MODEL_ID/$REVISION/$FILENAME"); done
    got=""
    for u in "${URLS[@]}"; do
      echo "downloading $u"
      if curl -fL --retry 3 --retry-delay 2 --connect-timeout 20 -o "$PARTIAL" "$u"; then got="$u"; break; fi
      rm -f "$PARTIAL"
    done
    [[ -n "$got" ]] || die "every download URL failed for $FILENAME"
  fi
  # Verify BEFORE the atomic rename; a mismatch deletes the partial and fails.
  if ! verified "$PARTIAL"; then
    actual="$(sha_of "$PARTIAL")"; rm -f "$PARTIAL"
    die "sha256 mismatch for $FILENAME: expected $SHA256, got $actual — partial deleted"
  fi
  mv -f "$PARTIAL" "$MODEL_PATH"
  verified "$MODEL_PATH" || die "model failed verification after rename: $MODEL_PATH"
  echo "model ok (provisioned)"
fi

# ── c. the corpus ───────────────────────────────────────────────────────────────
corpus_verifies() { [[ -d "$CORPUS_DIR" ]] && (cd "$CORPUS_DIR" && shasum -a 256 -c --quiet "$1" >/dev/null 2>&1); }

if corpus_verifies "$MANIFEST_SHA"; then
  echo "corpus ok"
else
  echo "corpus at $CORPUS_DIR missing or failing the manifest; growing it with the shipped generator"
  TRIPLE="$(rustc -vV | awk '/^host:/ {print $2}')"
  for bin in yap-polish yap-diarize; do
    [[ -x "$ROOT/desktop/src-tauri/binaries/$bin-$TRIPLE" ]] \
      || die "sidecar not staged: desktop/src-tauri/binaries/$bin-$TRIPLE (bundle.externalBin makes it a precondition of every cargo build of the app; build it with 'cargo build -p $bin --release' and copy it there)"
  done
  # The generator rewrites BOTH committed manifests from whatever it rendered. Snapshot them and
  # verify against the snapshot: the committed manifest is the truth, never the regrown bytes.
  SNAP="$(mktemp -d)"
  cp "$MANIFEST_SHA" "$SNAP/manifest.sha256"; cp "$MANIFEST_JSON" "$SNAP/manifest.json"
  restore() { cp "$SNAP/manifest.sha256" "$MANIFEST_SHA"; cp "$SNAP/manifest.json" "$MANIFEST_JSON"; }
  trap restore EXIT INT TERM
  mkdir -p "$CORPUS_DIR"
  gen_rc=0
  (cd "$ROOT/desktop/src-tauri" && \
    YAP_DATA_DIR="$(mktemp -d)/yap-state" YAP_EVAL_CORPUS="$CORPUS_DIR" \
    cargo test --features custom-protocol --test meeting_eval meeting_eval_generate_corpus -- --ignored --nocapture) || gen_rc=$?
  restore; trap - EXIT INT TERM
  [[ "$gen_rc" == 0 ]] || die "corpus generator exited $gen_rc"
  if ! corpus_verifies "$SNAP/manifest.sha256"; then
    (cd "$CORPUS_DIR" && shasum -a 256 -c "$SNAP/manifest.sha256" 2>&1 | grep -v ': OK$' | head -20) >&2 || true
    die "the regrown corpus disagrees with the committed manifest (a different macOS say voice build); the manifest is NOT rewritten to match"
  fi
  rm -rf "$SNAP"
  echo "corpus ok (grown)"
fi

# ── d. both verified ────────────────────────────────────────────────────────────
echo "YAP_MODEL_DIR=$MODEL_DIR"
echo "model:  $MODEL_PATH"
echo "YAP_EVAL_CORPUS=$CORPUS_DIR"
echo "corpus: $CORPUS_DIR"
