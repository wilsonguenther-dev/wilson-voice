"""CLI entry: file or timed record without menubar."""
from __future__ import annotations

import argparse
import logging
import sys
import tempfile
import time
from pathlib import Path

from .asr import transcribe
from .audio import Recorder
from .config import Config
from .logging_setup import setup_logging
from .paste import copy_and_paste
from .polish import extract_intent, polish

log = logging.getLogger("wilson_voice.cli")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="wilson-voice-cli")
    ap.add_argument("file", nargs="?", help="Audio file")
    ap.add_argument("--seconds", type=float, default=0)
    ap.add_argument("--raw", action="store_true")
    ap.add_argument("--intent", action="store_true")
    ap.add_argument("--no-copy", action="store_true")
    ap.add_argument("--paste", action="store_true")
    ap.add_argument("--model", default=None)
    args = ap.parse_args(argv)

    cfg = Config.load()
    if args.model:
        cfg.model = args.model
    setup_logging(cfg.log_level)

    with tempfile.TemporaryDirectory(prefix="wv-cli-") as td:
        td_path = Path(td)
        if args.file:
            wav = Path(args.file).expanduser().resolve()
            if not wav.exists():
                print(f"not found: {wav}", file=sys.stderr)
                return 2
            if wav.suffix.lower() != ".wav":
                import subprocess

                out = td_path / "in.wav"
                subprocess.run(
                    ["ffmpeg", "-y", "-i", str(wav), "-ac", "1", "-ar", "16000", str(out)],
                    check=True,
                    capture_output=True,
                )
                wav = out
        else:
            rec = Recorder(cfg.sample_rate, cfg.preferred_mic_substrings)
            rec.start()
            secs = args.seconds or 5.0
            print(f"Recording {secs}s…", flush=True)
            time.sleep(secs)
            audio = rec.stop()
            wav = rec.write_wav(td_path / "mic.wav", audio)

        text, backend = transcribe(
            wav,
            model=cfg.model,
            language=cfg.language,
            timeout=cfg.asr_timeout_seconds,
            use_fallback=cfg.fallback_whisper_cli,
        )
        if not text:
            print("empty transcript", file=sys.stderr)
            return 1
        final = text if args.raw else polish(text)
        print(final)
        if args.intent:
            print(f"[intent] {extract_intent(final)}", file=sys.stderr)
        if not args.no_copy:
            copy_and_paste(final, do_paste=args.paste)
        log.info("cli done backend=%s", backend)
    return 0
