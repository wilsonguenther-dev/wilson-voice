#!/usr/bin/env python3
"""ASR worker for Wilson Voice.

Modes:
  One-shot (legacy):
    asr_worker.py <wav> [--model M] [--language en]

  Warm daemon (preferred — model stays in memory via mlx_whisper.ModelHolder):
    asr_worker.py --serve
    stdin JSON lines:
      {"cmd":"ping"}
      {"cmd":"preload","model":"...","language":"en"}
      {"cmd":"transcribe","wav":"...","model":"...","language":"en"}
      {"cmd":"status"}
      {"cmd":"quit"}
    stdout: one JSON object per line

Stack honesty: base weights are OpenAI Whisper via MLX community conversions —
not proprietary Wilson foundation models. Personalization lives in the app
dictionary/SQL layer.

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


_LAST_MODEL: str | None = None
_PRELOAD_AT: float | None = None


def _ensure_model_loaded(model: str) -> float:
    """Map weights into ModelHolder without a full silence decode when possible."""
    global _LAST_MODEL, _PRELOAD_AT
    t0 = time.time()
    import mlx.core as mx
    from mlx_whisper.load_models import load_model
    from mlx_whisper.transcribe import ModelHolder

    dtype = mx.float16
    # Reuse ModelHolder cache — same process, no reload
    if ModelHolder.model is None or ModelHolder.model_path != model:
        ModelHolder.model = load_model(model, dtype=dtype)
        ModelHolder.model_path = model
    _LAST_MODEL = model
    _PRELOAD_AT = time.time()
    return round(time.time() - t0, 3)


def _transcribe_inprocess(wav: Path, model: str, language: str) -> tuple[str, float]:
    """Transcribe with warm ModelHolder; single temperature for speed."""
    global _LAST_MODEL
    t0 = time.time()
    from mlx_whisper import transcribe

    # Force weights warm before decode (no-op if already loaded)
    _ensure_model_loaded(model)

    result = transcribe(
        str(wav),
        path_or_hf_repo=model,
        language=language if language and language != "auto" else None,
        verbose=False,
        # Dictation: greedy only — multi-temp ladder burns Metal for little gain
        temperature=0.0,
        condition_on_previous_text=False,
        # Slightly less eager silence rejection for short holds
        no_speech_threshold=0.55,
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
            backend = "mlx-warm"
        except Exception as e1:
            try:
                text, seconds = _transcribe_cli_fallback(path, model, language)
                backend = "mlx-cli"
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
            "backend": backend,
            "seconds": seconds,
            "model": model,
            "warm": _LAST_MODEL == model,
        }
    except Exception as e:
        return {"ok": False, "error": f"{type(e).__name__}: {e}"}


def do_preload(model: str, language: str) -> dict:
    """Load weights into ModelHolder so first real Dictate skips import+map tax."""
    t0 = time.time()
    try:
        load_s = _ensure_model_loaded(model)
        # Tiny silence decode once so mel/decode path is also warm
        import struct
        import wave
        import tempfile

        fd, name = tempfile.mkstemp(suffix=".wav", dir=str(TMP))
        os.close(fd)
        with wave.open(name, "w") as w:
            w.setnchannels(1)
            w.setsampwidth(2)
            w.setframerate(16000)
            n = 1600  # 0.1s
            w.writeframes(struct.pack("<" + "h" * n, *([0] * n)))
        # May return empty — that is fine for silence
        try:
            from mlx_whisper import transcribe

            transcribe(
                name,
                path_or_hf_repo=model,
                language=language if language and language != "auto" else None,
                verbose=False,
                temperature=0.0,
            )
        except Exception:
            pass
        try:
            os.unlink(name)
        except OSError:
            pass
        return {
            "ok": True,
            "text": "",
            "backend": "mlx-preload",
            "seconds": round(time.time() - t0, 3),
            "load_seconds": load_s,
            "model": model,
            "note": "weights + decode path warm",
        }
    except Exception as e:
        return {"ok": False, "error": f"preload: {e}"}


def do_status() -> dict:
    from mlx_whisper.transcribe import ModelHolder

    return {
        "ok": True,
        "cmd": "status",
        "model": _LAST_MODEL or ModelHolder.model_path,
        "loaded": ModelHolder.model is not None,
        "preload_at": _PRELOAD_AT,
        "pid": os.getpid(),
    }


def serve() -> int:
    """JSON-lines protocol on stdin/stdout. Never buffers a full request batch."""
    try:
        sys.stdout.reconfigure(line_buffering=True)  # type: ignore[attr-defined]
    except Exception:
        pass

    print(
        json.dumps(
            {
                "ok": True,
                "cmd": "ready",
                "backend": "mlx-daemon",
                "stack": "openai-whisper-via-mlx",
            }
        ),
        flush=True,
    )

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
                json.dumps(
                    {
                        "ok": True,
                        "cmd": "pong",
                        "model": _LAST_MODEL,
                        "loaded": _LAST_MODEL is not None,
                    }
                ),
                flush=True,
            )
            continue
        if cmd == "status":
            print(json.dumps(do_status()), flush=True)
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
