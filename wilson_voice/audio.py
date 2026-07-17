"""Microphone capture with sounddevice; crash-safe write to WAV."""
from __future__ import annotations

import logging
import wave
from pathlib import Path
from typing import Optional

import numpy as np

from .logging_setup import log_event, log_exception

log = logging.getLogger("wilson_voice.audio")


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
                log.info("selected mic %s (index %s) match=%r", d["name"], d["index"], sub)
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
        self.preferred = preferred or ["Microphone"]
        self._stream = None
        self._chunks: list[np.ndarray] = []
        self._device: Optional[int] = None
        self._recording = False

    @property
    def is_recording(self) -> bool:
        return self._recording

    def start(self) -> None:
        import sounddevice as sd

        if self._recording:
            log.warning("start() called while already recording")
            return
        self._chunks = []
        self._device = pick_device(self.preferred)

        def callback(indata, frames, time_info, status):  # noqa: ARG001
            if status:
                log.warning("audio status: %s", status)
            if self._recording:
                self._chunks.append(indata.copy())

        try:
            self._stream = sd.InputStream(
                samplerate=self.sample_rate,
                channels=1,
                dtype="float32",
                device=self._device,
                callback=callback,
            )
            self._stream.start()
            self._recording = True
            log_event("record_start", device=self._device, sr=self.sample_rate)
            log.info("recording started device=%s", self._device)
        except Exception as e:
            log_exception("recorder_start", e)
            self._recording = False
            raise

    def stop(self) -> np.ndarray:
        import sounddevice as sd

        self._recording = False
        try:
            if self._stream is not None:
                self._stream.stop()
                self._stream.close()
        except Exception as e:
            log_exception("recorder_stop_stream", e)
        finally:
            self._stream = None

        if not self._chunks:
            log.warning("no audio chunks captured")
            log_event("record_empty")
            return np.zeros((0,), dtype=np.float32)

        audio = np.concatenate(self._chunks, axis=0).reshape(-1)
        # Peak normalize gently
        peak = float(np.max(np.abs(audio))) if audio.size else 0.0
        if peak > 1e-4:
            audio = audio / min(peak * 1.2, 1.0)
        duration = len(audio) / float(self.sample_rate)
        log_event("record_stop", samples=int(audio.size), duration_s=round(duration, 3), peak=round(peak, 4))
        log.info("recording stopped duration=%.2fs samples=%d peak=%.3f", duration, audio.size, peak)
        return audio.astype(np.float32)

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
