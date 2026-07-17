"""Structured logging: console + rotating file + JSON event stream."""
from __future__ import annotations

import json
import logging
import os
import sys
import traceback
from datetime import datetime, timezone
from logging.handlers import RotatingFileHandler
from pathlib import Path
from typing import Any


def default_log_dir() -> Path:
    base = Path.home() / "Library" / "Logs" / "WilsonVoice"
    base.mkdir(parents=True, exist_ok=True)
    return base


def setup_logging(level: str = "DEBUG", log_dir: Path | None = None) -> Path:
    log_dir = log_dir or default_log_dir()
    log_dir.mkdir(parents=True, exist_ok=True)

    root = logging.getLogger("wilson_voice")
    root.handlers.clear()
    root.setLevel(getattr(logging, level.upper(), logging.DEBUG))
    root.propagate = False

    fmt = logging.Formatter(
        "%(asctime)s | %(levelname)-7s | %(name)s | %(message)s",
        datefmt="%Y-%m-%d %H:%M:%S",
    )

    # Console
    ch = logging.StreamHandler(sys.stderr)
    ch.setLevel(logging.INFO)
    ch.setFormatter(fmt)
    root.addHandler(ch)

    # Rotating text log
    fh = RotatingFileHandler(
        log_dir / "wilson-voice.log",
        maxBytes=5_000_000,
        backupCount=5,
        encoding="utf-8",
    )
    fh.setLevel(logging.DEBUG)
    fh.setFormatter(fmt)
    root.addHandler(fh)

    # JSON event stream (one object per line) for machine diagnosis
    jh = RotatingFileHandler(
        log_dir / "events.jsonl",
        maxBytes=5_000_000,
        backupCount=5,
        encoding="utf-8",
    )
    jh.setLevel(logging.DEBUG)
    jh.setFormatter(logging.Formatter("%(message)s"))
    root.addHandler(jh)

    root.debug("logging initialized dir=%s", log_dir)
    return log_dir


def log_event(event: str, **fields: Any) -> None:
    """Write a structured JSONL event (also appears as DEBUG text)."""
    payload = {
        "ts": datetime.now(timezone.utc).isoformat(),
        "event": event,
        "pid": os.getpid(),
        **fields,
    }
    line = json.dumps(payload, default=str, ensure_ascii=False)
    logger = logging.getLogger("wilson_voice.events")
    # Only JSON handler format is message-only — attach to root handlers carefully
    logging.getLogger("wilson_voice").debug("EVENT %s", line)
    # Append raw to events.jsonl via dedicated logger name that only has JSON handler
    # Simpler: write directly
    try:
        path = default_log_dir() / "events.jsonl"
        with open(path, "a", encoding="utf-8") as f:
            f.write(line + "\n")
    except OSError as e:
        logging.getLogger("wilson_voice").error("failed to write event log: %s", e)


def log_exception(where: str, exc: BaseException) -> None:
    logging.getLogger("wilson_voice").error(
        "%s: %s: %s\n%s",
        where,
        type(exc).__name__,
        exc,
        traceback.format_exc(),
    )
    log_event(
        "exception",
        where=where,
        error_type=type(exc).__name__,
        error=str(exc),
        traceback=traceback.format_exc()[-2000:],
    )
