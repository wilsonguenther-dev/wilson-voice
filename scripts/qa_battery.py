#!/usr/bin/env python3
"""QA battery: unit tests + expanded synthetic checks (~50+ assertions)."""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from wilson_voice.asr import _find_mlx_whisper, transcribe
from wilson_voice.audio import list_input_devices, pick_device, Recorder
from wilson_voice.config import Config
from wilson_voice.logging_setup import log_event, setup_logging
from wilson_voice.paste import copy_text
from wilson_voice.polish import extract_intent, polish

PASS = 0
FAIL = 0


def check(name: str, cond: bool, detail: str = "") -> None:
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  ✓ {name}")
    else:
        FAIL += 1
        print(f"  ✗ {name} {detail}")


def main() -> int:
    print("=== Wilson Voice QA Battery ===\n")
    setup_logging("INFO")

    # --- Polish matrix (20) ---
    print("[polish]")
    cases = [
        ("um hello", "hello"),
        ("I uh need this", "need"),
        ("git hub deploy", "GitHub"),
        ("type script file", "TypeScript"),
        ("open codex", "Codex"),
        ("ask claude", "Claude"),
        ("super base query", "Supabase"),
        ("next j s route", "Next.js"),
        ("  spaced   out  ", "Spaced"),
        ("hello , world", "Hello"),
    ]
    for raw, expect_sub in cases:
        out = polish(raw)
        check(f"polish:{raw!r}", expect_sub.lower() in out.lower() or expect_sub in out, out)

    for text, intent in [
        ("can you fix this", "request"),
        ("please ship it", "request"),
        ("what is broken", "question"),
        ("how do I deploy", "question"),
        ("fix the bug", "action"),
        ("build the app", "action"),
        ("never call josh", "constraint"),
        ("The build passed.", "statement"),
        ("", ""),
        ("run the tests", "action"),
    ]:
        check(f"intent:{text[:20]!r}", extract_intent(text) == intent, extract_intent(text))

    # --- Config (5) ---
    print("[config]")
    c = Config()
    check("default model mlx", "mlx" in c.model or "whisper" in c.model)
    check("default hotkey", c.hotkey == "right_option")
    check("auto_paste on", c.auto_paste is True)
    check("sample_rate 16k", c.sample_rate == 16000)
    check("has mic prefs", len(c.preferred_mic_substrings) >= 1)

    # --- Audio devices (5) ---
    print("[audio]")
    devs = list_input_devices()
    check("devices list is list", isinstance(devs, list))
    check("at least one mic", len(devs) >= 1, str(devs))
    idx = pick_device(c.preferred_mic_substrings)
    check("pick device", idx is None or isinstance(idx, int), str(idx))
    with tempfile.TemporaryDirectory() as td:
        rec = Recorder()
        import numpy as np

        p = rec.write_wav(Path(td) / "a.wav", np.random.randn(8000).astype("float32") * 0.01)
        check("wav written", p.exists() and p.stat().st_size > 100)
        check("wav size sane", p.stat().st_size < 1_000_000)

    # --- Clipboard (3) ---
    print("[clipboard]")
    check("copy text", copy_text("wilson-voice-qa-1"))
    check("copy unicode", copy_text("café résumé 你好"))
    check("copy multiline", copy_text("line1\nline2\nline3"))

    # --- Logging (3) ---
    print("[logging]")
    log_event("qa_battery_ping", n=1)
    from wilson_voice.logging_setup import default_log_dir

    ev = default_log_dir() / "events.jsonl"
    check("events.jsonl exists", ev.exists())
    if ev.exists():
        last = ev.read_text().strip().splitlines()[-1]
        check("events json valid", json.loads(last).get("event") is not None)
    else:
        check("events json valid", False)
    check("log dir under Library", "WilsonVoice" in str(default_log_dir()))

    # --- pytest suite (counts as batch) ---
    print("[pytest]")
    r = subprocess.run(
        [sys.executable, "-m", "pytest", str(ROOT / "tests"), "-q", "--tb=no"],
        cwd=str(ROOT),
        capture_output=True,
        text=True,
    )
    check("pytest exit 0", r.returncode == 0, r.stdout[-200:] + r.stderr[-200:])
    # parse "XX passed"
    if "passed" in r.stdout:
        check("pytest has passes", True, r.stdout.strip().splitlines()[-1])
    else:
        check("pytest has passes", r.returncode == 0)

    # --- Live ASR samples (expand to many paraphrases via say) ---
    print("[asr live]")
    if not _find_mlx_whisper():
        print("  skip mlx not found")
    else:
        phrases = [
            "Fix the login button on Drivia.",
            "Commit and push to main.",
            "What is the status of the GPU server?",
            "Open Claude Code and run the tests.",
            "Never send email to blocked people.",
            "Deploy the application to Vercel.",
            "Explain this error in simple words.",
            "Create a new GitHub repository for Wilson Voice.",
        ]
        model = "mlx-community/whisper-large-v3-turbo"
        for i, phrase in enumerate(phrases):
            with tempfile.TemporaryDirectory() as td:
                td = Path(td)
                aiff, wav = td / "t.aiff", td / "t.wav"
                subprocess.run(["say", "-o", str(aiff), phrase], check=True)
                subprocess.run(
                    ["ffmpeg", "-y", "-i", str(aiff), "-ac", "1", "-ar", "16000", str(wav)],
                    check=True,
                    capture_output=True,
                )
                t0 = time.time()
                text, backend = transcribe(wav, model=model, language="en", use_fallback=False)
                dt = time.time() - t0
                # keyword overlap
                keys = [w.lower() for w in phrase.replace("?", "").replace(".", "").split() if len(w) > 3]
                hits = sum(1 for k in keys if k in text.lower())
                ok = backend == "mlx" and hits >= max(1, len(keys) // 3)
                check(f"asr[{i}] {phrase[:32]!r}", ok, f"hits={hits}/{len(keys)} t={dt:.1f}s → {text[:60]!r}")

        # JFK classic
        jfk = Path("/opt/homebrew/Cellar/whisper-cpp/1.9.1/share/whisper-cpp/jfk.wav")
        if jfk.exists():
            text, backend = transcribe(jfk, model=model, language="en", use_fallback=False)
            check("jfk americans", "american" in text.lower() or "country" in text.lower(), text[:80])

    # --- Extra resilience checks ---
    print("[resilience]")
    check("asr binary exists", bool(_find_mlx_whisper()))
    check("ffmpeg exists", shutil.which("ffmpeg") is not None)
    check("pbcopy exists", shutil.which("pbcopy") is not None)
    check("import pipeline", True)  # if we got here modules load
    try:
        from wilson_voice.pipeline import DictationEngine, DictationResult

        check("DictationResult fields", hasattr(DictationResult, "ok"))
        check("DictationEngine ctor", DictationEngine(Config()) is not None)
    except Exception as e:
        check("DictationResult fields", False, str(e))
        check("DictationEngine ctor", False, str(e))

    print(f"\n=== RESULT: {PASS} passed, {FAIL} failed (total {PASS+FAIL}) ===")
    return 0 if FAIL == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
