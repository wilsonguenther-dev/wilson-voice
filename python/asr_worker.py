#!/usr/bin/env python3
"""ASR worker for Wilson Voice.

Modes:
  One-shot (legacy):
    asr_worker.py <wav> [--model M] [--language en]

  Warm daemon (preferred — model stays in memory):
    asr_worker.py --serve
    stdin JSON lines:
      {"cmd":"ping"}
      {"cmd":"preload","model":"...","language":"en"}
      {"cmd":"transcribe","wav":"...","model":"...","language":"en"}
    stdout: one JSON object per line

Never touch ~/Desktop (macOS TCC Files & Folders spam).
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path

# --- isolate caches off Desktop before any HF import ---
SUPPORT = Path.home() / "Library" / "Application Support" / "WilsonVoice"
CACHE = SUPPORT / "cache"
HF = CACHE / "huggingface"
TMP = SUPPORT / "tmp"
for d in (SUPPORT, CACHE, HF, TMP, HF / "hub"):
    d.mkdir(parents=True, exist_ok=True)

os.environ.setdefault("HF_HOME", str(HF))
os.environ.setdefault("HUGGINGFACE_HUB_CACHE", str(HF / "hub"))
os.environ.setdefault("TRANSFORMERS_CACHE", str(HF / "transformers"))
os.environ.setdefault("XDG_CACHE_HOME", str(CACHE))
os.environ.setdefault("TMPDIR", str(TMP))
try:
    os.chdir(SUPPORT)
except OSError:
    pass


def _refuse_desktop(path: str) -> str | None:
    p = path or ""
    if "/Desktop/" in p or p.rstrip("/").endswith("/Desktop"):
        return f"path on Desktop blocked for TCC: {p}"
    return None


def _polish(text: str) -> str:
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
    return text


# Cache loaded models in-process (warm daemon)
_MODEL_CACHE: dict[str, object] = {}
_LAST_MODEL: str | None = None


def _transcribe_inprocess(wav: Path, model: str, language: str) -> tuple[str, float]:
    """Load model once per process; reuse weights across utterances."""
    global _LAST_MODEL
    t0 = time.time()

    # Prefer mlx_whisper.transcribe which caches in its own layer when repeated
    from mlx_whisper import transcribe

    result = transcribe(
        str(wav),
        path_or_hf_repo=model,
        language=language if language and language != "auto" else None,
        # verbose False reduces I/O
        verbose=False,
    )
    text = _polish((result.get("text") or "").strip())
    _LAST_MODEL = model
    return text, round(time.time() - t0, 3)


def _transcribe_cli_fallback(wav: Path, model: str, language: str) -> tuple[str, float]:
    import subprocess
    import tempfile

    t0 = time.time()
    venv_cli = Path(sys.executable).resolve().parent / "mlx_whisper"
    out_dir = Path(tempfile.mkdtemp(prefix="wv-asr-", dir=str(TMP)))
    if not venv_cli.is_file():
        raise RuntimeError("mlx_whisper CLI missing and in-process failed")
    cmd = [
        str(venv_cli),
        str(wav),
        "--model",
        model,
        "--language",
        language,
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
        cwd=str(SUPPORT),
        env=os.environ.copy(),
    )
    if r.returncode != 0:
        raise RuntimeError((r.stderr or r.stdout or "mlx_whisper failed")[-500:])
    txts = list(out_dir.glob("*.txt"))
    if not txts:
        raise RuntimeError("no transcript file")
    text = _polish(txts[0].read_text(encoding="utf-8").strip())
    return text, round(time.time() - t0, 3)


def do_transcribe(wav: str, model: str, language: str) -> dict:
    for p in (sys.executable, wav, __file__):
        err = _refuse_desktop(p)
        if err:
            return {"ok": False, "error": err}

    path = Path(wav)
    if not path.exists():
        return {"ok": False, "error": f"missing file: {wav}"}

    try:
        try:
            text, seconds = _transcribe_inprocess(path, model, language)
        except Exception as e1:
            # Fall back to CLI once
            try:
                text, seconds = _transcribe_cli_fallback(path, model, language)
            except Exception as e2:
                return {
                    "ok": False,
                    "error": f"{type(e1).__name__}: {e1} | cli: {e2}",
                }

        if not text:
            return {"ok": False, "error": "Empty transcript"}
        return {
            "ok": True,
            "text": text,
            "backend": "mlx-warm" if _LAST_MODEL else "mlx",
            "seconds": seconds,
            "model": model,
        }
    except Exception as e:
        return {"ok": False, "error": f"{type(e).__name__}: {e}"}


def do_preload(model: str, language: str) -> dict:
    """Touch-load weights so first real Dictate is fast."""
    t0 = time.time()
    try:
        # 0.3s silence is enough to force weight load without meaningful decode cost
        import struct
        import wave
        import tempfile

        fd, name = tempfile.mkstemp(suffix=".wav", dir=str(TMP))
        os.close(fd)
        with wave.open(name, "w") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(16000)
            n = 4800  # 0.3s
            w.writeframes(struct.pack("<" + "h" * n, *([0] * n)))
        out = do_transcribe(name, model, language)
        try:
            os.unlink(name)
        except OSError:
            pass
        out["preload_seconds"] = round(time.time() - t0, 3)
        out["cmd"] = "preload"
        # Empty/silence may fail empty transcript — still counts as warm if model mapped
        if not out.get("ok") and "Empty" in str(out.get("error", "")):
            return {
                "ok": True,
                "text": "",
                "backend": "mlx-preload",
                "seconds": round(time.time() - t0, 3),
                "model": model,
                "note": "weights warm; silence empty is fine",
            }
        return out
    except Exception as e:
        return {"ok": False, "error": f"preload: {e}"}


def serve() -> int:
    """JSON-lines protocol on stdin/stdout. Never buffers a full request batch."""
    # Line-buffered stdout
    try:
        sys.stdout.reconfigure(line_buffering=True)  # type: ignore[attr-defined]
    except Exception:
        pass

    print(json.dumps({"ok": True, "cmd": "ready", "backend": "mlx-daemon"}), flush=True)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
        except json.JSONDecodeError as e:
            print(json.dumps({"ok": False, "error": f"bad json: {e}"}), flush=True)
            continue

        cmd = (req.get("cmd") or "transcribe").lower()
        if cmd == "ping":
            print(
                json.dumps({"ok": True, "cmd": "pong", "model": _LAST_MODEL}),
                flush=True,
            )
            continue
        if cmd == "quit":
            print(json.dumps({"ok": True, "cmd": "bye"}), flush=True)
            return 0
        if cmd == "preload":
            model = req.get("model") or "mlx-community/whisper-small-mlx"
            language = req.get("language") or "en"
            print(json.dumps(do_preload(model, language)), flush=True)
            continue
        if cmd == "transcribe":
            wav = req.get("wav") or ""
            model = req.get("model") or "mlx-community/whisper-large-v3-turbo"
            language = req.get("language") or "en"
            print(json.dumps(do_transcribe(wav, model, language)), flush=True)
            continue
        print(json.dumps({"ok": False, "error": f"unknown cmd: {cmd}"}), flush=True)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("wav", nargs="?", default=None)
    ap.add_argument("--model", default="mlx-community/whisper-large-v3-turbo")
    ap.add_argument("--language", default="en")
    ap.add_argument(
        "--serve",
        action="store_true",
        help="Warm daemon: JSON lines on stdin/stdout",
    )
    args = ap.parse_args()

    if args.serve:
        return serve()

    if not args.wav:
        print(json.dumps({"ok": False, "error": "missing wav or --serve"}))
        return 1

    out = do_transcribe(args.wav, args.model, args.language)
    print(json.dumps(out))
    return 0 if out.get("ok") else 1


if __name__ == "__main__":
    raise SystemExit(main())
