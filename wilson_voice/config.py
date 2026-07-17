"""Config load/save with safe defaults."""
from __future__ import annotations

import logging
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

import yaml

log = logging.getLogger("wilson_voice.config")

DEFAULT_MODEL = "mlx-community/whisper-large-v3-turbo"
# Hold-to-talk: right option is reliable on Mac.
# Pure FN is a modifier-only key and needs special handling — we support
# "right_option" (default), "right_command", "f13"–"f20", "ctrl_shift_space".
DEFAULT_HOTKEY = "right_option"


def config_path() -> Path:
    p = Path.home() / "Library" / "Application Support" / "WilsonVoice"
    p.mkdir(parents=True, exist_ok=True)
    return p / "config.yaml"


@dataclass
class Config:
    model: str = DEFAULT_MODEL
    language: str = "en"
    hotkey: str = DEFAULT_HOTKEY  # see README for keys
    auto_paste: bool = True
    auto_copy: bool = True
    polish: bool = True
    show_intent: bool = False
    sample_rate: int = 16000
    # Prefer "Wilson G Microphone" if present, else system default
    preferred_mic_substrings: list[str] = field(
        default_factory=lambda: ["Wilson G", "MacBook Pro Microphone", "Microphone"]
    )
    min_record_seconds: float = 0.25
    max_record_seconds: float = 120.0
    asr_timeout_seconds: float = 120.0
    fallback_whisper_cli: bool = True
    whisper_cli_model: str = "large-v3-turbo"  # if ggml available
    log_level: str = "DEBUG"

    @classmethod
    def load(cls) -> "Config":
        path = config_path()
        if not path.exists():
            cfg = cls()
            cfg.save()
            log.info("wrote default config %s", path)
            return cfg
        try:
            raw: dict[str, Any] = yaml.safe_load(path.read_text()) or {}
            known = {f.name for f in cls.__dataclass_fields__.values()}  # type: ignore
            filtered = {k: v for k, v in raw.items() if k in known}
            cfg = cls(**filtered)
            log.info("loaded config %s", path)
            return cfg
        except Exception as e:
            log.exception("config load failed, using defaults: %s", e)
            return cls()

    def save(self) -> None:
        path = config_path()
        path.write_text(yaml.safe_dump(asdict(self), sort_keys=False))
        log.debug("saved config %s", path)
