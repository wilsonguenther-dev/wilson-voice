//! Bundled ASR model catalog + verified downloader (YV30).
//!
//! `catalog.json` is compiled into the binary (`include_str!`) so the app
//! ships a complete model list with zero network access. Entries are copied
//! verbatim from Handy's generated catalog — the top-2 recommended GGUF models
//! plus Whisper Tiny (smallest, used by tests) — each with a pinned revision,
//! per-quant byte sizes and sha256 hashes. The hashes compiled into the binary
//! are the trust anchor: every download is sha256-verified before the
//! `.partial` file is renamed into place, regardless of which host served it.
//!
//! YV32 reads this from the dictation path (which catalog model is downloaded
//! and where its file lives) and from the headless CLI, which auto-downloads
//! the smallest entry when nothing is on disk yet.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const CATALOG_JSON: &str = include_str!("catalog.json");

/// Emitted on every download progress step (typed payload below).
pub const MODEL_DOWNLOAD_PROGRESS_EVENT: &str = "model_download_progress";

/// How long a single chunk read may hang before the attempt is abandoned.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Full passes over the URL list before giving up (the `.partial` file is kept
/// so a later call resumes where this one died).
const MAX_ATTEMPTS: u32 = 4;
/// Progress events are throttled to one per this many bytes (plus the final one).
const PROGRESS_EMIT_STEP: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// Catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    /// Base URLs tried after Hugging Face. The full file URL is
    /// `{mirror}/{repo_id}/{revision}/{filename}` — the same three values that
    /// form the HF resolve URL, so a mirror is a plain static host.
    pub mirrors: Vec<String>,
    pub models: Vec<CatalogModel>,
    /// Optional LLM polish models for the `yap-polish` sidecar (YV60). A
    /// separate list on purpose: these are NOT ASR models and must never appear
    /// in the transcription model manager or be picked by `recommended_model`.
    #[serde(default)]
    pub polish_models: Vec<PolishCatalogModel>,
}

/// One model as written in `catalog.json`. Only the fields we need are
/// declared; serde ignores the rest (slug, languages, scores, …).
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogModel {
    /// HF repo id, e.g. `handy-computer/whisper-tiny-gguf`.
    pub id: String,
    /// Commit sha the catalog's sizes/hashes were generated from. Both HF and
    /// mirror URLs pin it, so downloaded bytes provably match the hashes
    /// regardless of source.
    pub revision: String,
    pub name: String,
    pub description: String,
    pub architecture: String,
    /// ISO codes the model transcribes, as the catalog declares them. YV93's
    /// English-only meeting gate (plan finding #38) reads this: the Notetaker
    /// refuses honestly on a model that cannot do English rather than handing a
    /// Spanish lecture to an English-only Parakeet. `default` because a catalog
    /// entry that omits it means "unstated", not "no languages".
    #[serde(default)]
    pub languages: Vec<String>,
    pub files: Vec<ModelFile>,
    pub default_quant: Option<String>,
    #[serde(default)]
    pub recommended: bool,
    pub recommended_rank: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelFile {
    pub filename: String,
    pub quant: String,
    pub size_bytes: u64,
    pub sha256: String,
}

impl CatalogModel {
    /// The file for `default_quant`, falling back to the first listed file.
    pub fn default_file(&self) -> Option<&ModelFile> {
        self.default_quant
            .as_deref()
            .and_then(|q| self.files.iter().find(|f| f.quant == q))
            .or_else(|| self.files.first())
    }
}

/// The bundled catalog, parsed once.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON)
            .expect("bundled catalog.json is valid JSON matching the catalog schema")
    })
}

/// Look up a catalog model by repo id.
pub fn catalog_model(model_id: &str) -> Option<&'static CatalogModel> {
    catalog().models.iter().find(|m| m.id == model_id)
}

/// The catalog's top recommendation (lowest `recommended_rank`) — the model a
/// fresh install selects (YV31's `native_model` default).
pub fn recommended_model() -> &'static CatalogModel {
    catalog()
        .models
        .iter()
        .filter(|m| m.recommended)
        .min_by_key(|m| m.recommended_rank.unwrap_or(u32::MAX))
        .or_else(|| catalog().models.first())
        .expect("bundled catalog is never empty")
}

// ---------------------------------------------------------------------------
// Polish models (YV60) — the `yap-polish` sidecar's GGUF, installed the same
// resumable + sha256-verified way as the ASR models. NOTHING SHIPS IN THE DMG:
// with no file on disk the parent never spawns the sidecar and the polish stage
// stays the no-op it is today.
// ---------------------------------------------------------------------------

/// One polish model. Unlike [`CatalogModel`] there is exactly one quant per
/// entry (Q4_K_M — the size/quality point the latency budget is written
/// against), so the id names the file rather than the repo.
#[derive(Debug, Clone, Deserialize)]
pub struct PolishCatalogModel {
    /// Stable local id, e.g. `qwen2.5-1.5b-instruct-q4_k_m` — what the
    /// `polish_model` setting stores.
    pub id: String,
    /// Hugging Face repo the file is fetched from.
    pub repo: String,
    /// Commit sha the size + hash below were taken from. Pinned, so the bytes
    /// are reproducible even if the repo's `main` moves.
    pub revision: String,
    pub name: String,
    pub parameters: String,
    /// SPDX id. Apache-2.0 for both Qwen2.5 entries — a non-OSI license (Gemma,
    /// LFM) would have to be passed downstream by an app heading to open source.
    pub license: String,
    pub description: String,
    pub file: ModelFile,
    #[serde(default)]
    pub recommended: bool,
    pub recommended_rank: Option<u32>,
}

/// Every polish model in the bundled catalog.
pub fn polish_models() -> &'static [PolishCatalogModel] {
    &catalog().polish_models
}

/// Look up a polish model by its catalog id.
pub fn polish_model(id: &str) -> Option<&'static PolishCatalogModel> {
    catalog().polish_models.iter().find(|m| m.id == id)
}

/// The default polish model (lowest `recommended_rank`) — the 1.5B primary.
pub fn recommended_polish_model() -> Option<&'static PolishCatalogModel> {
    catalog()
        .polish_models
        .iter()
        .filter(|m| m.recommended)
        .min_by_key(|m| m.recommended_rank.unwrap_or(u32::MAX))
}

/// Where a polish model's file lives, whether or not it exists — the same
/// models directory the ASR GGUFs use.
pub fn polish_model_path(model: &PolishCatalogModel) -> PathBuf {
    models_dir().join(&model.file.filename)
}

/// Present at its full expected size (the sha256 was verified at download time,
/// before the `.partial` was renamed into place).
pub fn is_polish_downloaded(model: &PolishCatalogModel) -> bool {
    std::fs::metadata(polish_model_path(model))
        .map(|m| m.is_file() && m.len() == model.file.size_bytes)
        .unwrap_or(false)
}

/// Download URLs for a polish model: Hugging Face at the pinned revision only.
/// The mirrors in this catalog host Handy's ASR repos, not Qwen's — pointing at
/// them would just add a 404 to every attempt.
pub fn polish_download_urls(model: &PolishCatalogModel) -> Vec<String> {
    vec![format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        model.repo, model.revision, model.file.filename
    )]
}

/// Fetch a polish model through the same resumable, sha256-verified path as the
/// ASR models. Returns the verified on-disk path.
pub async fn download_polish_model_with<F>(id: &str, progress: F) -> Result<PathBuf, String>
where
    F: FnMut(u64, u64),
{
    let model = polish_model(id).ok_or_else(|| format!("unknown polish model '{id}'"))?;
    let urls = polish_download_urls(model);
    let dest = polish_model_path(model);
    download_file(
        &urls,
        &dest,
        model.file.size_bytes,
        &model.file.sha256,
        progress,
    )
    .await
}

/// The catalog's smallest download — what the headless `--transcribe-file` mode
/// auto-fetches when nothing is on disk yet (fastest possible cold gate).
pub fn smallest_model() -> &'static CatalogModel {
    catalog()
        .models
        .iter()
        .filter(|m| m.default_file().is_some())
        .min_by_key(|m| m.default_file().map(|f| f.size_bytes).unwrap_or(u64::MAX))
        .or_else(|| catalog().models.first())
        .expect("bundled catalog is never empty")
}

/// Where a model's default-quant file lives on disk, whether or not it exists.
pub fn model_path(model: &CatalogModel) -> Option<PathBuf> {
    model.default_file().map(|f| models_dir().join(&f.filename))
}

/// A model counts as downloaded when its default-quant file is present at the
/// full expected size — the cheap check (the sha256 was already verified at
/// download time, before the `.partial` was renamed into place).
pub fn is_downloaded(model: &CatalogModel) -> bool {
    let Some(file) = model.default_file() else {
        return false;
    };
    std::fs::metadata(models_dir().join(&file.filename))
        .map(|m| m.is_file() && m.len() == file.size_bytes)
        .unwrap_or(false)
}

/// Remove a downloaded model's file and any interrupted `.partial` sibling.
pub fn delete_downloaded(model: &CatalogModel) -> Result<(), String> {
    let file = model
        .default_file()
        .ok_or_else(|| format!("catalog model '{}' lists no files", model.id))?;
    let dest = models_dir().join(&file.filename);
    let partial = partial_path(&dest);
    if let Err(e) = std::fs::remove_file(&dest) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("delete {}: {e}", dest.display()));
        }
    }
    let _ = std::fs::remove_file(partial);
    Ok(())
}

/// Ordered download URLs for one file: Hugging Face `resolve/<sha>` first
/// (immutable, CDN-friendly), then each mirror at the same pinned revision.
pub fn download_urls(model: &CatalogModel, file: &ModelFile) -> Vec<String> {
    let mut urls = vec![format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        model.id, model.revision, file.filename
    )];
    for mirror in &catalog().mirrors {
        urls.push(format!(
            "{}/{}/{}/{}",
            mirror.trim_end_matches('/'),
            model.id,
            model.revision,
            file.filename
        ));
    }
    urls
}

/// Where downloaded models live: `<data_dir>/WilsonVoice/models` (same
/// Application Support root as the rest of the app — never ~/Desktop).
pub fn models_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("WilsonVoice")
        .join("models")
}

// ---------------------------------------------------------------------------
// sha256 verification
// ---------------------------------------------------------------------------

/// Streaming sha256 of a file, as lowercase hex.
pub fn sha256_hex(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("open {} for hashing: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read {} for hashing: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verify a file's sha256 against the catalog's expected hex digest.
pub fn verify_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let actual = sha256_hex(path)?;
    if actual.eq_ignore_ascii_case(expected_hex) {
        Ok(())
    } else {
        Err(format!(
            "sha256 mismatch for {}: expected {expected_hex}, got {actual}",
            path.display()
        ))
    }
}

// ---------------------------------------------------------------------------
// Resume / rename logic (sync, unit-tested; the async downloader drives it)
// ---------------------------------------------------------------------------

/// The in-progress sibling of `dest`: same name with `.partial` appended.
pub fn partial_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".partial");
    PathBuf::from(name)
}

/// Byte offset to resume from. A partial at least as large as the expected
/// total can never complete into a valid file, so it is deleted and the
/// download restarts from zero.
pub fn resume_offset(partial: &Path, total_bytes: u64) -> u64 {
    match std::fs::metadata(partial) {
        Ok(m) if total_bytes > 0 && m.len() >= total_bytes => {
            let _ = std::fs::remove_file(partial);
            0
        }
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

/// MANDATORY verification gate: hash the completed `.partial`, and only on a
/// match rename it into place. A corrupted partial is deleted so the next
/// attempt starts clean — corrupt bytes are never left to "resume" into.
pub fn finalize_download(partial: &Path, dest: &Path, expected_sha256: &str) -> Result<(), String> {
    if let Err(e) = verify_sha256(partial, expected_sha256) {
        let _ = std::fs::remove_file(partial);
        return Err(e);
    }
    std::fs::rename(partial, dest).map_err(|e| {
        format!(
            "rename {} -> {} failed: {e}",
            partial.display(),
            dest.display()
        )
    })
}

// ---------------------------------------------------------------------------
// Async downloader
// ---------------------------------------------------------------------------

/// Typed payload for [`MODEL_DOWNLOAD_PROGRESS_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct ModelDownloadProgress {
    pub model_id: String,
    pub downloaded: u64,
    pub total: u64,
}

/// Download a catalog model's default-quant file into [`models_dir`], emitting
/// [`ModelDownloadProgress`] events. Returns the verified on-disk path.
pub async fn download_model(app: &AppHandle, model_id: &str) -> Result<PathBuf, String> {
    let app = app.clone();
    let id = model_id.to_string();
    let mut last_emitted: Option<u64> = None;
    download_model_with(model_id, |downloaded, total| {
        // Throttle: first, final, and one per PROGRESS_EMIT_STEP in between.
        let due = match last_emitted {
            None => true,
            Some(prev) => {
                downloaded >= total || downloaded.saturating_sub(prev) >= PROGRESS_EMIT_STEP
            }
        };
        if due {
            last_emitted = Some(downloaded);
            let _ = app.emit(
                MODEL_DOWNLOAD_PROGRESS_EVENT,
                &ModelDownloadProgress {
                    model_id: id.clone(),
                    downloaded,
                    total,
                },
            );
        }
    })
    .await
}

/// The AppHandle-free core of [`download_model`]: same resumable, sha256-verified
/// fetch with a caller-supplied progress sink. Used by the headless CLI, which
/// has no Tauri app to emit events into.
pub async fn download_model_with<F>(model_id: &str, progress: F) -> Result<PathBuf, String>
where
    F: FnMut(u64, u64),
{
    let model =
        catalog_model(model_id).ok_or_else(|| format!("unknown catalog model '{model_id}'"))?;
    let file = model
        .default_file()
        .ok_or_else(|| format!("catalog model '{model_id}' lists no files"))?;
    let urls = download_urls(model, file);
    let dest = models_dir().join(&file.filename);
    download_file(&urls, &dest, file.size_bytes, &file.sha256, progress).await
}

/// Resumable, verified download: tries each URL in order, resuming from the
/// `.partial` file via HTTP Range, retrying full passes with exponential
/// backoff, abandoning any attempt whose next chunk stalls past
/// [`STALL_TIMEOUT`]. The file only ever appears at `dest` after its sha256
/// matched `expected_sha256`.
pub async fn download_file<F>(
    urls: &[String],
    dest: &Path,
    size_bytes: u64,
    expected_sha256: &str,
    mut progress: F,
) -> Result<PathBuf, String>
where
    F: FnMut(u64, u64),
{
    // Already downloaded and intact — done (guards double-clicks and re-runs).
    if dest.is_file() && verify_sha256(dest, expected_sha256).is_ok() {
        progress(size_bytes, size_bytes);
        return Ok(dest.to_path_buf());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    let partial = partial_path(dest);
    let client = reqwest::Client::new();
    let mut last_err = "no download URLs configured".to_string();

    for attempt in 0..MAX_ATTEMPTS {
        if attempt > 0 {
            // 1s, 2s, 4s … between full passes over the URL list.
            tokio::time::sleep(Duration::from_secs(1 << (attempt - 1))).await;
        }
        for url in urls {
            match download_attempt(&client, url, &partial, size_bytes, &mut progress).await {
                Ok(()) => match finalize_download(&partial, dest, expected_sha256) {
                    Ok(()) => return Ok(dest.to_path_buf()),
                    // Verification failure (finalize deleted the partial) counts as a
                    // failed attempt — fall back to the remaining URLs/attempts.
                    Err(e) => {
                        log::warn!("model download verification failed ({url}): {e}");
                        last_err = e;
                    }
                },
                Err(e) => {
                    log::warn!("model download attempt failed ({url}): {e}");
                    last_err = e;
                }
            }
        }
    }
    // The .partial survives for a future resume.
    Err(format!(
        "download failed after {MAX_ATTEMPTS} attempts: {last_err}"
    ))
}

/// One streaming pass against one URL, appending to the `.partial` file.
async fn download_attempt<F>(
    client: &reqwest::Client,
    url: &str,
    partial: &Path,
    total: u64,
    progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(u64, u64),
{
    let mut downloaded = resume_offset(partial, total);
    let mut req = client.get(url);
    if downloaded > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={downloaded}-"));
    }
    let resp = tokio::time::timeout(STALL_TIMEOUT, req.send())
        .await
        .map_err(|_| format!("request to {url} timed out"))?
        .map_err(|e| format!("request to {url} failed: {e}"))?;
    let resp = resp
        .error_for_status()
        .map_err(|e| format!("{url} returned error status: {e}"))?;

    // Server honored the Range → append; anything else → restart from zero.
    let resuming = downloaded > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resuming {
        downloaded = 0;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(resuming)
        .write(true)
        .truncate(!resuming)
        .open(partial)
        .map_err(|e| format!("open {}: {e}", partial.display()))?;

    progress(downloaded, total);
    let mut resp = resp;
    loop {
        let chunk = tokio::time::timeout(STALL_TIMEOUT, resp.chunk())
            .await
            .map_err(|_| format!("download from {url} stalled at {downloaded} bytes"))?
            .map_err(|e| format!("read from {url} failed: {e}"))?;
        let Some(bytes) = chunk else { break };
        file.write_all(&bytes)
            .map_err(|e| format!("write {}: {e}", partial.display()))?;
        downloaded += bytes.len() as u64;
        progress(downloaded, total);
    }
    file.sync_all()
        .map_err(|e| format!("sync {}: {e}", partial.display()))?;

    if total > 0 && downloaded != total {
        return Err(format!(
            "short read from {url}: got {downloaded} of {total} bytes"
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests (no network)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-yv30-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn hex_of(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn catalog_parses_with_required_fields() {
        let cat = catalog();
        assert!(!cat.mirrors.is_empty(), "catalog must list a mirror");
        assert_eq!(cat.models.len(), 3, "top-2 recommended + whisper-tiny");
        for m in &cat.models {
            assert!(!m.id.is_empty());
            assert!(!m.name.is_empty());
            assert_eq!(m.revision.len(), 40, "{}: revision must be a pinned sha", m.id);
            assert!(!m.files.is_empty(), "{}: no files", m.id);
            for f in &m.files {
                assert!(f.filename.ends_with(".gguf"), "{}: {}", m.id, f.filename);
                assert!(f.size_bytes > 0, "{}: {} has no size", m.id, f.filename);
                assert_eq!(f.sha256.len(), 64, "{}: {} bad sha256", m.id, f.filename);
                assert!(f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            }
            assert!(m.default_file().is_some(), "{}: default_quant unresolvable", m.id);
        }
        // Handy's top-2 recommended set, plus the smallest whisper for tests.
        let ranks: Vec<Option<u32>> = cat.models.iter().map(|m| m.recommended_rank).collect();
        assert_eq!(ranks[0], Some(1));
        assert_eq!(ranks[1], Some(2));
        assert!(cat.models[2].id.contains("whisper-tiny"));
    }

    /// YV60: the polish entries are a SEPARATE list — an LLM that leaked into
    /// `models` would be offered as a transcription engine, and could even be
    /// picked by `smallest_model()` on a fresh headless run.
    #[test]
    fn polish_catalog_is_pinned_and_never_mixed_into_the_asr_list() {
        let polish = polish_models();
        assert_eq!(polish.len(), 2, "1.5B primary + 0.5B fast tier");
        for m in polish {
            assert!(!m.id.is_empty());
            assert_eq!(m.revision.len(), 40, "{}: revision must be a pinned sha", m.id);
            assert!(
                m.revision.chars().all(|c| c.is_ascii_hexdigit()),
                "{}: revision must be a sha",
                m.id
            );
            assert!(m.repo.starts_with("Qwen/"), "{}: unexpected repo", m.id);
            assert_eq!(m.license, "apache-2.0", "{}: OSI license required", m.id);
            assert!(m.file.filename.ends_with(".gguf"), "{}", m.file.filename);
            assert!(m.file.size_bytes > 0, "{}: no size", m.id);
            assert_eq!(m.file.sha256.len(), 64, "{}: bad sha256", m.id);
            assert!(m.file.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            // The id names the file it installs, so a settings value maps to
            // exactly one blob on disk.
            assert_eq!(m.file.filename, format!("{}.gguf", m.id));
            assert!(
                catalog_model(&m.id).is_none(),
                "{} must not be reachable as an ASR model",
                m.id
            );
        }
        // The documented pair, at the documented sizes (spec §2.2).
        let primary = recommended_polish_model().expect("a default polish model");
        assert_eq!(primary.id, "qwen2.5-1.5b-instruct-q4_k_m");
        assert_eq!(primary.file.size_bytes, 1_117_320_736);
        let fast = polish_model("qwen2.5-0.5b-instruct-q4_k_m").expect("fast tier in catalog");
        assert_eq!(fast.file.size_bytes, 491_400_032);
        assert!(fast.file.size_bytes < primary.file.size_bytes);
    }

    #[test]
    fn polish_download_url_pins_the_hf_revision() {
        let m = recommended_polish_model().expect("a default polish model");
        let urls = polish_download_urls(m);
        assert_eq!(
            urls,
            vec![format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                m.repo, m.revision, m.file.filename
            )]
        );
        // Installed alongside the ASR GGUFs, never bundled into the app.
        assert!(polish_model_path(m).starts_with(models_dir()));
        assert_eq!(
            polish_model_path(m).file_name().unwrap().to_string_lossy(),
            m.file.filename
        );
    }

    #[test]
    fn recommended_model_is_rank_one_and_resolves_to_a_path() {
        let m = recommended_model();
        assert!(m.recommended, "the default selection must be recommended");
        assert_eq!(m.recommended_rank, Some(1));
        // The path a download lands at / the manager loads from.
        let path = model_path(m).expect("recommended model resolves a file");
        assert!(path.starts_with(models_dir()));
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            m.default_file().unwrap().filename
        );
    }

    #[test]
    fn smallest_model_is_the_cheapest_download() {
        let m = smallest_model();
        let size = m.default_file().unwrap().size_bytes;
        for other in &catalog().models {
            if let Some(f) = other.default_file() {
                assert!(
                    size <= f.size_bytes,
                    "{} is smaller than {}",
                    other.id,
                    m.id
                );
            }
        }
        // Whisper Tiny today — the model the headless gate auto-downloads.
        assert!(
            m.id.contains("whisper-tiny"),
            "unexpected smallest: {}",
            m.id
        );
    }

    #[test]
    fn download_urls_are_hf_then_mirror_at_pinned_revision() {
        let m = catalog_model("handy-computer/whisper-tiny-gguf").expect("whisper-tiny in catalog");
        let f = m.default_file().unwrap();
        let urls = download_urls(m, f);
        assert!(urls.len() >= 2, "expected HF + at least one mirror");
        assert_eq!(
            urls[0],
            format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                m.id, m.revision, f.filename
            )
        );
        for u in &urls[1..] {
            assert!(u.starts_with("https://"), "bad mirror url {u}");
            assert!(u.contains(&m.revision), "mirror url must pin the revision");
            assert!(u.ends_with(&f.filename));
        }
    }

    #[test]
    fn sha256_verifier_rejects_corrupted_bytes() {
        let dir = temp_dir();
        let path = dir.join("fixture.bin");
        let payload = b"yap yv30 fixture payload";
        std::fs::write(&path, payload).unwrap();
        let expected = hex_of(payload);

        assert!(verify_sha256(&path, &expected).is_ok());
        // Flip bytes → same length, different content → must be rejected.
        std::fs::write(&path, b"yap yv30 fixture corrupt").unwrap();
        let err = verify_sha256(&path, &expected).unwrap_err();
        assert!(err.contains("sha256 mismatch"), "unexpected error: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn partial_path_appends_partial_suffix() {
        let dest = Path::new("/tmp/models/whisper-tiny-Q8_0.gguf");
        assert_eq!(
            partial_path(dest),
            PathBuf::from("/tmp/models/whisper-tiny-Q8_0.gguf.partial")
        );
    }

    #[test]
    fn resume_offset_resumes_valid_partial_and_resets_oversized() {
        let dir = temp_dir();
        let partial = dir.join("model.gguf.partial");

        // No partial → start at zero.
        assert_eq!(resume_offset(&partial, 100), 0);

        // Valid partial smaller than total → resume from its length.
        std::fs::write(&partial, vec![0u8; 40]).unwrap();
        assert_eq!(resume_offset(&partial, 100), 40);
        assert!(partial.is_file(), "valid partial must be kept");

        // Partial >= total can never verify → deleted, restart from zero.
        std::fs::write(&partial, vec![0u8; 100]).unwrap();
        assert_eq!(resume_offset(&partial, 100), 0);
        assert!(!partial.exists(), "oversized partial must be removed");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn finalize_renames_only_verified_bytes_into_place() {
        let dir = temp_dir();
        let dest = dir.join("model.gguf");
        let partial = partial_path(&dest);
        let payload = b"pretend this is a gguf model";
        let good_sha = hex_of(payload);

        // Corrupted partial: rejected, partial deleted, dest never appears.
        std::fs::write(&partial, b"corrupted download bytes!!!!").unwrap();
        assert!(finalize_download(&partial, &dest, &good_sha).is_err());
        assert!(!partial.exists(), "corrupt partial must be removed");
        assert!(!dest.exists(), "corrupt bytes must never land at dest");

        // Verified partial: renamed into place atomically.
        std::fs::write(&partial, payload).unwrap();
        finalize_download(&partial, &dest, &good_sha).expect("verified rename");
        assert!(!partial.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        std::fs::remove_dir_all(&dir).ok();
    }
}
