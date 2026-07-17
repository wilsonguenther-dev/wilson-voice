"""Live ASR smoke (uses cached MLX model). Skip if mlx_whisper missing."""
import shutil
import subprocess
from pathlib import Path

import pytest

from wilson_voice.asr import _find_mlx_whisper, transcribe

JFK = Path("/opt/homebrew/Cellar/whisper-cpp/1.9.1/share/whisper-cpp/jfk.wav")
MODEL = "mlx-community/whisper-large-v3-turbo"


@pytest.mark.skipif(not _find_mlx_whisper(), reason="mlx_whisper not installed")
@pytest.mark.skipif(not JFK.exists(), reason="jfk.wav sample missing")
def test_jfk_transcription_contains_americans():
    text, backend = transcribe(JFK, model=MODEL, language="en", use_fallback=False)
    assert backend == "mlx"
    assert text
    low = text.lower()
    assert "american" in low or "country" in low


@pytest.mark.skipif(not _find_mlx_whisper(), reason="mlx_whisper not installed")
def test_say_generated_phrase(tmp_path):
    aiff = tmp_path / "t.aiff"
    wav = tmp_path / "t.wav"
    phrase = "Deploy the Drivia platform to production now."
    subprocess.run(["say", "-o", str(aiff), phrase], check=True)
    subprocess.run(
        ["ffmpeg", "-y", "-i", str(aiff), "-ac", "1", "-ar", "16000", str(wav)],
        check=True,
        capture_output=True,
    )
    text, backend = transcribe(wav, model=MODEL, language="en", use_fallback=False)
    assert backend == "mlx"
    low = text.lower()
    # loose match — TTS + ASR may slightly alter wording
    assert any(w in low for w in ("deploy", "drivia", "production", "platform"))
