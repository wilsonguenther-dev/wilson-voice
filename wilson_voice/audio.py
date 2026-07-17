"""Microphone capture — sounddevice primary, ffmpeg avfoundation fallback.

macOS TCC: the host process (Python / Wilson Voice) must be allowed under
System Settings → Privacy & Security → Microphone.
"""
from __future__ import annotations

import logging
import subprocess
import wave
from pathlib import Path
from typing import Optional

import numpy as np

from .logging_setup import log_event, log_exception

log = logging.getLogger("wilson_voice.audio")


def request_mic_permission() -> None:
    """Ask macOS for microphone access via AVFoundation (shows system dialog)."""
    try:
        from AVFoundation import AVCaptureDevice, AVMediaTypeAudio
        from Foundation import NSRunLoop, NSDate

        # Synchronous-ish wait for permission dialog
        done = {"ok": None}

        def handler(granted: bool) -> None:
            done["ok"] = bool(granted)
            log.info("AVFoundation mic granted=%s", granted)
            log_event("mic_permission", granted=bool(granted))

        AVCaptureDevice.requestAccessForMediaType_completionHandler_(
            AVMediaTypeAudio, handler
        )
        # Spin run loop briefly so dialog can appear
        for _ in range(50):
            if done["ok"] is not None:
                break
            NSRunLoop.currentRunLoop().runUntilDate_(
                NSDate.dateWithTimeIntervalSinceNow_(0.1)
            )
    except Exception as e:
        log_exception("request_mic_permission", e)


def list_input_devices() -> list[dict]:
    import sounddevice as sd

    devices = []
    for i, d in enumerate(sd.query_devices()):
        if d.get("max_input_channels", 0) > 0:
            devices.append(
                {
                    "index": i,
                    "name": d["name"],
                    "channels": d["max_input_channels"],
                    "default_sr": d.get("default_samplerate"),
                }
            )
    return devices


def pick_device(preferred_substrings: list[str]) -> Optional[int]:
    import sounddevice as sd

    devices = list_input_devices()
    log.debug("input devices: %s", devices)
    for sub in preferred_substrings:
        for d in devices:
            if sub.lower() in d["name"].lower():
                log.info(
                    "selected mic %s (index %s) match=%r", d["name"], d["index"], sub
                )
                return d["index"]
    try:
        default = sd.default.device
        if isinstance(default, (list, tuple)):
            return int(default[0]) if default[0] is not None else None
        return int(default) if default is not None else None
    except Exception as e:
        log_exception("pick_device_default", e)
        return None


class Recorder:
    def __init__(self, sample_rate: int = 16000, preferred: list[str] | None = None):
        self.sample_rate = sample_rate
        # Prefer built-in mic first — external mics often fail if offline
        self.preferred = preferred or [
            "MacBook Pro Microphone",
            "MacBook Air Microphone",
            "Built-in",
            "Wilson G",
            "Microphone",
        ]
        self._stream = None
        self._chunks: list[np.ndarray] = []
        self._device: Optional[int] = None
        self._recording = False
        self._ffmpeg: Optional[subprocess.Popen] = None
        self._ffmpeg_wav: Optional[Path] = None
        self._backend = "none"

    @property
    def is_recording(self) -> bool:
        return self._recording

    def start(self) -> None:
        if self._recording:
            log.warning("start() while already recording")
            return
        self._chunks = []
        # Try sounddevice first
        try:
            self._start_sounddevice()
            return
        except Exception as e:
            log.warning("sounddevice start failed: %s — trying ffmpeg", e)
            log_event("record_sounddevice_fail", error=str(e))
        self._start_ffmpeg()

    def _start_sounddevice(self) -> None:
        import sounddevice as sd

        self._device = pick_device(self.preferred)

        def callback(indata, frames, time_info, status):  # noqa: ARG001
            if status:
                log.warning("audio status: %s", status)
            if self._recording:
                self._chunks.append(indata.copy())

        self._stream = sd.InputStream(
            samplerate=self.sample_rate,
            channels=1,
            dtype="float32",
            device=self._device,
            callback=callback,
        )
        self._stream.start()
        self._recording = True
        self._backend = "sounddevice"
        log_event("record_start", backend="sounddevice", device=self._device)
        log.info("recording started backend=sounddevice device=%s", self._device)

    def _start_ffmpeg(self) -> None:
        """Fallback: ffmpeg avfoundation :0 (default mic)."""
        import tempfile

        wav = Path(tempfile.mkdtemp(prefix="wv-ff-")) / "cap.wav"
        # :0 is first audio capture device on macOS avfoundation
        cmd = [
            "ffmpeg",
            "-y",
            "-f",
            "avfoundation",
            "-i",
            ":0",
            "-ac",
            "1",
            "-ar",
            str(self.sample_rate),
            str(wav),
        ]
        self._ffmpeg = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        self._ffmpeg_wav = wav
        self._recording = True
        self._backend = "ffmpeg"
        log_event("record_start", backend="ffmpeg", device=":0")
        log.info("recording started backend=ffmpeg path=%s", wav)

    def stop(self) -> np.ndarray:
        self._recording = False
        if self._backend == "ffmpeg":
            return self._stop_ffmpeg()
        return self._stop_sounddevice()

    def _stop_sounddevice(self) -> np.ndarray:
        try:
            if self._stream is not None:
                self._stream.stop()
                self._stream.close()
        except Exception as e:
            log_exception("recorder_stop_stream", e)
        finally:
            self._stream = None

        if not self._chunks:
            log.warning("no audio chunks captured (sounddevice)")
            log_event("record_empty", backend="sounddevice")
            return np.zeros((0,), dtype=np.float32)

        audio = np.concatenate(self._chunks, axis=0).reshape(-1)
        peak = float(np.max(np.abs(audio))) if audio.size else 0.0
        if peak > 1e-4:
            audio = audio / min(peak * 1.2, 1.0)
        duration = len(audio) / float(self.sample_rate)
        log_event(
            "record_stop",
            backend="sounddevice",
            samples=int(audio.size),
            duration_s=round(duration, 3),
            peak=round(peak, 4),
        )
        log.info(
            "recording stopped backend=sounddevice duration=%.2fs samples=%d peak=%.3f",
            duration,
            audio.size,
            peak,
        )
        return audio.astype(np.float32)

    def _stop_ffmpeg(self) -> np.ndarray:
        proc = self._ffmpeg
        wav = self._ffmpeg_wav
        self._ffmpeg = None
        if proc is None:
            return np.zeros((0,), dtype=np.float32)
        try:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=2)
        except Exception as e:
            log_exception("ffmpeg_stop", e)

        if wav is None or not wav.exists() or wav.stat().st_size < 1000:
            err = ""
            try:
                if proc.stderr:
                    err = proc.stderr.read()[-500:]
            except Exception:
                pass
            log.error("ffmpeg produced no audio: %s", err)
            log_event("record_empty", backend="ffmpeg", stderr=err[-200:])
            return np.zeros((0,), dtype=np.float32)

        # Read wav to float32
        with wave.open(str(wav), "rb") as wf:
            n = wf.getnframes()
            raw = wf.readframes(n)
            pcm = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
        duration = len(pcm) / float(self.sample_rate)
        peak = float(np.max(np.abs(pcm))) if pcm.size else 0.0
        log_event(
            "record_stop",
            backend="ffmpeg",
            samples=int(pcm.size),
            duration_s=round(duration, 3),
            peak=round(peak, 4),
        )
        log.info(
            "recording stopped backend=ffmpeg duration=%.2fs samples=%d peak=%.3f",
            duration,
            pcm.size,
            peak,
        )
        return pcm

    def write_wav(self, path: Path, audio: np.ndarray) -> Path:
        path = Path(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        pcm = np.clip(audio, -1.0, 1.0)
        pcm_i16 = (pcm * 32767.0).astype(np.int16)
        with wave.open(str(path), "wb") as wf:
            wf.setnchannels(1)
            wf.setsampwidth(2)
            wf.setframerate(self.sample_rate)
            wf.writeframes(pcm_i16.tobytes())
        log.debug("wrote wav %s bytes=%d", path, path.stat().st_size)
        return path
