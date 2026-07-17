"""ASR backends: MLX Whisper primary, whisper-cli fallback. Isolated subprocesses."""
from __future__ import annotations

import logging
import shutil
import subprocess
import tempfile
import time
from pathlib import Path

from .logging_setup import log_event, log_exception

log = logging.getLogger("wilson_voice.asr")


class ASRError(Exception):
    pass


def _find_mlx_whisper() -> str | None:
    return shutil.which("mlx_whisper")


def _find_whisper_cli() -> str | None:
    return shutil.which("whisper-cli")


def transcribe_mlx(
    wav: Path,
    model: str,
    language: str = "en",
    timeout: float = 120.0,
) -> str:
    mlx = _find_mlx_whisper()
    if not mlx:
        raise ASRError("mlx_whisper not found on PATH")

    out_dir = Path(tempfile.mkdtemp(prefix="wv-asr-"))
    cmd = [
        mlx,
        str(wav),
        "--model",
        model,
        "--output-dir",
        str(out_dir),
        "--output-format",
        "txt",
        "--verbose",
        "False",
        "--language",
        language,
    ]
    log.info("mlx_whisper start model=%s wav=%s", model, wav)
    log_event("asr_start", backend="mlx", model=model, wav=str(wav))
    t0 = time.time()
    try:
        r = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as e:
        log_exception("asr_mlx_timeout", e)
        log_event("asr_timeout", backend="mlx", timeout=timeout)
        raise ASRError(f"mlx_whisper timed out after {timeout}s") from e
    except Exception as e:
        log_exception("asr_mlx_run", e)
        raise ASRError(f"mlx_whisper failed to run: {e}") from e

    elapsed = time.time() - t0
    if r.returncode != 0:
        log.error("mlx_whisper rc=%s stderr=%s", r.returncode, (r.stderr or "")[-800:])
        log_event("asr_fail", backend="mlx", rc=r.returncode, stderr=(r.stderr or "")[-500:])
        raise ASRError(f"mlx_whisper exit {r.returncode}: {(r.stderr or '')[-300:]}")

    texts = list(out_dir.glob("*.txt"))
    if not texts:
        log_event("asr_fail", backend="mlx", reason="no_txt")
        raise ASRError("mlx_whisper produced no .txt output")

    text = texts[0].read_text(encoding="utf-8").strip()
    log.info("mlx_whisper ok %.1fs chars=%d", elapsed, len(text))
    log_event("asr_ok", backend="mlx", seconds=round(elapsed, 2), chars=len(text))
    return text


def transcribe_whisper_cli(
    wav: Path,
    model_name: str = "large-v3-turbo",
    language: str = "en",
    timeout: float = 120.0,
) -> str:
    cli = _find_whisper_cli()
    if not cli:
        raise ASRError("whisper-cli not found on PATH")

    # whisper-cli prints to stdout by default with -otxt
    out_base = wav.with_suffix("")
    cmd = [
        cli,
        "-m",
        model_name,  # may need full ggml path — try and fail soft
        "-l",
        language,
        "-f",
        str(wav),
        "-nt",  # no timestamps in stdout
        "-np",  # no prints progress if supported
    ]
    log.info("whisper-cli start model=%s", model_name)
    log_event("asr_start", backend="whisper_cli", model=model_name)
    t0 = time.time()
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, check=False)
    except subprocess.TimeoutExpired as e:
        raise ASRError("whisper-cli timed out") from e
    except Exception as e:
        raise ASRError(f"whisper-cli failed: {e}") from e

    elapsed = time.time() - t0
    if r.returncode != 0:
        log_event("asr_fail", backend="whisper_cli", rc=r.returncode, stderr=(r.stderr or "")[-400:])
        raise ASRError(f"whisper-cli exit {r.returncode}: {(r.stderr or r.stdout)[-300:]}")

    text = (r.stdout or "").strip()
    # Some versions write file
    txt_file = Path(str(out_base) + ".txt")
    if not text and txt_file.exists():
        text = txt_file.read_text(encoding="utf-8").strip()
    log_event("asr_ok", backend="whisper_cli", seconds=round(elapsed, 2), chars=len(text))
    return text


def transcribe(
    wav: Path,
    model: str,
    language: str = "en",
    timeout: float = 120.0,
    use_fallback: bool = True,
) -> tuple[str, str]:
    """Return (text, backend_used). Never raises if both fail — returns ("", "none")."""
    try:
        return transcribe_mlx(wav, model=model, language=language, timeout=timeout), "mlx"
    except ASRError as e:
        log.warning("primary ASR failed: %s", e)
        if not use_fallback:
            return "", "mlx_failed"
        try:
            return (
                transcribe_whisper_cli(wav, language=language, timeout=timeout),
                "whisper_cli",
            )
        except ASRError as e2:
            log.error("fallback ASR failed: %s", e2)
            log_event("asr_total_fail", primary=str(e), fallback=str(e2))
            return "", "none"
