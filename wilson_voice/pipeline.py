"""End-to-end dictation pipeline with extreme error handling."""
from __future__ import annotations

import logging
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

from .asr import transcribe
from .audio import Recorder
from .config import Config
from .logging_setup import log_event, log_exception
from .paste import copy_and_paste
from .polish import extract_intent, polish

log = logging.getLogger("wilson_voice.pipeline")


@dataclass
class DictationResult:
    ok: bool
    raw: str = ""
    text: str = ""
    intent: str = ""
    backend: str = ""
    duration_s: float = 0.0
    asr_s: float = 0.0
    error: str = ""


class DictationEngine:
    def __init__(self, cfg: Config):
        self.cfg = cfg
        self.recorder = Recorder(
            sample_rate=cfg.sample_rate,
            preferred=cfg.preferred_mic_substrings,
        )
        self._busy = False

    @property
    def busy(self) -> bool:
        return self._busy

    def start_recording(self) -> None:
        if self._busy:
            log.warning("start_recording ignored — busy")
            return
        try:
            self.recorder.start()
        except Exception as e:
            log_exception("pipeline_start_recording", e)
            raise

    def finish(self) -> DictationResult:
        """Stop recording, ASR, polish, clipboard/paste."""
        if self._busy:
            return DictationResult(ok=False, error="busy")
        self._busy = True
        t0 = time.time()
        try:
            audio = self.recorder.stop()
            duration = len(audio) / float(self.cfg.sample_rate) if len(audio) else 0.0
            if duration < self.cfg.min_record_seconds:
                log.warning("recording too short: %.3fs", duration)
                log_event("dictation_too_short", duration_s=duration)
                return DictationResult(
                    ok=False,
                    duration_s=duration,
                    error=f"too short ({duration:.2f}s)",
                )
            if duration > self.cfg.max_record_seconds:
                log.warning("recording truncated conceptually at max (%.1fs)", duration)

            with tempfile.TemporaryDirectory(prefix="wv-sess-") as td:
                wav = Path(td) / "utterance.wav"
                self.recorder.write_wav(wav, audio)
                t_asr = time.time()
                text, backend = transcribe(
                    wav,
                    model=self.cfg.model,
                    language=self.cfg.language,
                    timeout=self.cfg.asr_timeout_seconds,
                    use_fallback=self.cfg.fallback_whisper_cli,
                )
                asr_s = time.time() - t_asr

            if not text:
                return DictationResult(
                    ok=False,
                    backend=backend,
                    duration_s=duration,
                    asr_s=asr_s,
                    error="empty transcript",
                )

            final = polish(text) if self.cfg.polish else text
            intent = extract_intent(final) if self.cfg.show_intent else ""

            if self.cfg.auto_copy:
                copy_and_paste(final, do_paste=self.cfg.auto_paste)

            result = DictationResult(
                ok=True,
                raw=text,
                text=final,
                intent=intent,
                backend=backend,
                duration_s=duration,
                asr_s=asr_s,
            )
            log_event(
                "dictation_ok",
                backend=backend,
                duration_s=round(duration, 3),
                asr_s=round(asr_s, 3),
                chars=len(final),
                intent=intent,
            )
            log.info(
                "dictation ok backend=%s audio=%.2fs asr=%.2fs chars=%d",
                backend,
                duration,
                asr_s,
                len(final),
            )
            return result
        except Exception as e:
            log_exception("pipeline_finish", e)
            return DictationResult(ok=False, error=str(e))
        finally:
            self._busy = False
            log.debug("pipeline finish total=%.2fs", time.time() - t0)
