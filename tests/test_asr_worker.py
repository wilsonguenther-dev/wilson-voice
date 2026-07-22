"""Tests for the Tauri ASR sidecar — python/asr_worker.py.

Fast, deterministic, numpy-only checks for the WAV decoder and polish logic run
in CI. The full MLX transcription (which proves the ffmpeg-free path end to end)
is marked ``heavy`` and skipped in CI — it needs a cached model + a real
recording, and is run locally / in the dev loop instead.
"""

from __future__ import annotations

import struct
import sys
import wave
from pathlib import Path

import numpy as np
import pytest

# Import the sidecar the same way the app seeds it (as a top-level module).
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "python"))

from asr_worker import WHISPER_SR, _load_wav_f32, _polish  # noqa: E402


def _write_wav(path: Path, samples_i16, sr: int = 16000, channels: int = 1) -> None:
    with wave.open(str(path), "w") as w:
        w.setnchannels(channels)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(struct.pack("<" + "h" * len(samples_i16), *samples_i16))


# --- WAV decode (the ffmpeg-free path) --------------------------------------


def test_decode_16k_mono_s16(tmp_path: Path) -> None:
    p = tmp_path / "a.wav"
    _write_wav(p, [0, 16384, -16384, 32767, -32768])
    a = _load_wav_f32(p)
    assert a.dtype == np.float32 and a.ndim == 1
    assert a.shape[0] == 5
    assert -1.01 <= a.min() and a.max() <= 1.01
    assert abs(a[1] - 0.5) < 1e-4  # 16384 / 32768


def test_decode_stereo_downmix(tmp_path: Path) -> None:
    p = tmp_path / "s.wav"
    _write_wav(p, [100, 300, 100, 300], channels=2)  # L=100 R=300 → mean 200
    a = _load_wav_f32(p)
    assert a.shape[0] == 2
    assert abs(a[0] - 200 / 32768) < 1e-4


def test_decode_resamples_to_16k(tmp_path: Path) -> None:
    p = tmp_path / "r.wav"
    _write_wav(p, [0] * 8000, sr=8000)  # 1s @ 8k → ~16000 samples @ 16k
    a = _load_wav_f32(p)
    assert abs(a.shape[0] - WHISPER_SR) <= 2


def test_decode_empty(tmp_path: Path) -> None:
    p = tmp_path / "e.wav"
    _write_wav(p, [])
    a = _load_wav_f32(p)
    assert a.size == 0 and a.dtype == np.float32


# --- polish (must not corrupt real dictation) -------------------------------


def test_polish_keeps_real_words() -> None:
    # Regression: the old polish stripped "you know"/"i mean" anywhere, eating
    # real dictated words. They must survive now.
    assert "you know" in _polish("you know the drill").lower()
    assert "i mean it" in _polish("i mean it").lower()


def test_polish_strips_leading_and_bracketed_filler() -> None:
    assert _polish("um hello there").lower().startswith("hello")
    assert "uh" not in _polish("well uh okay").lower().split()


def test_polish_capitalizes_first_letter() -> None:
    assert _polish("hello world")[0].isupper()


# --- full transcription (proves ffmpeg-free E2E) — heavy, skipped in CI ------


@pytest.mark.heavy
def test_transcribe_without_ffmpeg(monkeypatch: pytest.MonkeyPatch) -> None:
    """The in-process ASR path must transcribe with ffmpeg absent from PATH.

    Targets `_transcribe_inprocess` directly (the ffmpeg-free ndarray path);
    `do_transcribe`'s TCC guard rejects the repo's Desktop path, so it can't be
    the unit under test here.
    """
    import shutil

    pytest.importorskip("mlx_whisper")
    rec_dir = Path.home() / "Library/Application Support/WilsonVoice/recordings"
    wavs = sorted(rec_dir.glob("*.wav")) if rec_dir.exists() else []
    if not wavs:
        pytest.skip("no local recording available")
    # Pick a recording with real signal.
    wav = max(wavs, key=lambda f: float(abs(_load_wav_f32(f)).max()) if f.stat().st_size else 0.0)

    monkeypatch.setenv("PATH", "/usr/bin:/bin")  # ffmpeg unreachable (bundle condition)
    assert shutil.which("ffmpeg") is None, "test invalid: ffmpeg still reachable"

    from asr_worker import _transcribe_inprocess

    text, seconds = _transcribe_inprocess(
        wav, "mlx-community/whisper-large-v3-turbo", "en",
        prompt="Drivia JAX Supabase",
    )
    assert text, "empty transcript from ffmpeg-free path"
    assert seconds > 0
