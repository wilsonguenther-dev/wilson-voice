#!/usr/bin/env bash
# Provision the two notetaker eval inputs ONCE, at fixed paths, verified by hash.
#
#   model:  $YAP_MODEL_DIR/<Q8_0 filename>   default $HOME/code/wilson-voice-loop/cache/models
#   notes:  $YAP_MODEL_DIR/<qwen2.5-1.5b Q4_K_M filename>  (yap24-NT4 R1 — the default notes model,
#           pinned in the catalog's polish_models; tests/meeting_notes_recall_eval.rs reads it here)
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

# ── b2. the notes model (yap24-NT4 R1) ─────────────────────────────────────────
# The default notes model (summarize::DEFAULT_SUMMARY_MODEL), pinned in the catalog's
# polish_models. Same rules as the ASR model above: the pin comes from the catalog, the
# installed copy is only read and APFS-cloned, a download is verified before its rename.
NOTES_ID="qwen2.5-1.5b-instruct-q4_k_m"
NOTES_ROW="$(jq -ec --arg id "$NOTES_ID" '.polish_models[] | select(.id == $id)' "$CATALOG")" \
  || die "catalog has no polish model $NOTES_ID"
NOTES_REPO="$(jq -er '.repo' <<<"$NOTES_ROW")"
NOTES_REV="$(jq -er '.revision' <<<"$NOTES_ROW")"
NOTES_FILE="$(jq -er '.file.filename' <<<"$NOTES_ROW")"
NOTES_SIZE="$(jq -er '.file.size_bytes' <<<"$NOTES_ROW")"
NOTES_SHA="$(jq -er '.file.sha256' <<<"$NOTES_ROW")"
[[ "$NOTES_SHA" =~ ^[0-9a-f]{64}$ ]] || die "catalog sha256 for $NOTES_FILE is malformed: $NOTES_SHA"
[[ "$NOTES_FILE" != */* && "$NOTES_FILE" != .* ]] || die "catalog filename is not a plain name: $NOTES_FILE"
NOTES_PATH="$MODEL_DIR/$NOTES_FILE"
NOTES_PARTIAL="$NOTES_PATH.partial"
notes_verified() { [[ -f "$1" ]] && [[ "$(stat -f '%z' "$1")" == "$NOTES_SIZE" ]] && [[ "$(sha_of "$1")" == "$NOTES_SHA" ]]; }

if notes_verified "$NOTES_PATH"; then
  echo "notes model ok"
else
  [[ -e "$NOTES_PATH" ]] && { echo "cached $NOTES_FILE fails its catalog sha256; replacing it"; rm -f "$NOTES_PATH"; }
  rm -f "$NOTES_PARTIAL"
  NOTES_INSTALLED="$INSTALLED_DIR/$NOTES_FILE"
  if notes_verified "$NOTES_INSTALLED"; then
    cp -c "$NOTES_INSTALLED" "$NOTES_PARTIAL" || cp "$NOTES_INSTALLED" "$NOTES_PARTIAL"
    echo "cloned the installed notes model (read-only source): $NOTES_INSTALLED"
  else
    URLS=("https://huggingface.co/$NOTES_REPO/resolve/$NOTES_REV/$NOTES_FILE")
    for m in ${MIRRORS[@]+"${MIRRORS[@]}"}; do URLS+=("$m/$NOTES_REPO/$NOTES_REV/$NOTES_FILE"); done
    got=""
    for u in "${URLS[@]}"; do
      echo "downloading $u"
      if curl -fL --retry 3 --retry-delay 2 --connect-timeout 20 -o "$NOTES_PARTIAL" "$u"; then got="$u"; break; fi
      rm -f "$NOTES_PARTIAL"
    done
    [[ -n "$got" ]] || die "every download URL failed for $NOTES_FILE"
  fi
  if ! notes_verified "$NOTES_PARTIAL"; then
    actual="$(sha_of "$NOTES_PARTIAL")"; rm -f "$NOTES_PARTIAL"
    die "sha256 mismatch for $NOTES_FILE: expected $NOTES_SHA, got $actual — partial deleted"
  fi
  mv -f "$NOTES_PARTIAL" "$NOTES_PATH"
  notes_verified "$NOTES_PATH" || die "notes model failed verification after rename: $NOTES_PATH"
  echo "notes model ok (provisioned)"
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
