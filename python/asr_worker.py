#!/usr/bin/env python3
"""ASR worker for Wilson Voice desktop (Tauri sidecar).

Usage:
  asr_worker.py <wav_path> [--model MODEL] [--language en]

Prints JSON to stdout:
  {"ok": true, "text": "...", "backend": "mlx", "seconds": 1.2}
"""
from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("wav")
    ap.add_argument("--model", default="mlx-community/whisper-large-v3-turbo")
    ap.add_argument("--language", default="en")
    args = ap.parse_args()

    wav = Path(args.wav)
    if not wav.exists():
        print(json.dumps({"ok": False, "error": f"missing file: {wav}"}))
        return 1

    t0 = time.time()
    try:
        # Prefer CLI next to this interpreter
        import os
        import subprocess
        import tempfile

        # Never let HF / caches touch ~/Desktop (macOS TCC Files & Folders spam).
        support = (
            Path.home()
            / "Library"
            / "Application Support"
            / "WilsonVoice"
        )
        cache = support / "cache"
        hf = cache / "huggingface"
        tmp = support / "tmp"
        for d in (cache, hf, tmp, support):
            d.mkdir(parents=True, exist_ok=True)
        os.environ.setdefault("HF_HOME", str(hf))
        os.environ.setdefault("HUGGINGFACE_HUB_CACHE", str(hf / "hub"))
        os.environ.setdefault("TRANSFORMERS_CACHE", str(hf / "transformers"))
        os.environ.setdefault("XDG_CACHE_HOME", str(cache))
        os.environ.setdefault("TMPDIR", str(tmp))
        try:
            os.chdir(support)
        except OSError:
            pass

        # Refuse Desktop-based interpreters / scripts
        for p in (sys.executable, str(wav), __file__):
            if "/Desktop/" in p or p.rstrip("/").endswith("/Desktop"):
                print(
                    json.dumps(
                        {
                            "ok": False,
                            "error": f"path on Desktop blocked for TCC: {p}",
                        }
                    )
                )
                return 1

        venv_cli = Path(sys.executable).resolve().parent / "mlx_whisper"
        out_dir = Path(tempfile.mkdtemp(prefix="wv-asr-", dir=str(tmp)))
        if venv_cli.is_file():
            cmd = [
                str(venv_cli),
                str(wav),
                "--model",
                args.model,
                "--language",
                args.language,
                "--output-dir",
                str(out_dir),
                "--output-format",
                "txt",
                "--verbose",
                "False",
            ]
            r = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=180,
                cwd=str(support),
                env=os.environ.copy(),
            )
            if r.returncode != 0:
                print(
                    json.dumps(
                        {
                            "ok": False,
                            "error": (r.stderr or r.stdout or "mlx_whisper failed")[
                                -500:
                            ],
                        }
                    )
                )
                return 1
            txts = list(out_dir.glob("*.txt"))
            if not txts:
                print(json.dumps({"ok": False, "error": "no transcript file"}))
                return 1
            text = txts[0].read_text(encoding="utf-8").strip()
        else:
            # In-process fallback
            from mlx_whisper import transcribe

            result = transcribe(
                str(wav), path_or_hf_repo=args.model, language=args.language
            )
            text = (result.get("text") or "").strip()

        # light polish
        import re

        for pat in [
            r"\b(um|uh|erm|hmm)\b",
            r"\byou know\b",
            r"\bi mean\b",
        ]:
            text = re.sub(pat, "", text, flags=re.I)
        text = re.sub(r"\s{2,}", " ", text).strip()
        if text and text[0].islower():
            text = text[0].upper() + text[1:]

        print(
            json.dumps(
                {
                    "ok": True,
                    "text": text,
                    "backend": "mlx",
                    "seconds": round(time.time() - t0, 3),
                }
            )
        )
        return 0
    except Exception as e:
        print(json.dumps({"ok": False, "error": f"{type(e).__name__}: {e}"}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
