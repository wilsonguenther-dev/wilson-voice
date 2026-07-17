#!/usr/bin/env bash
# Full end-to-end test without relying on GUI visibility.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PY="$ROOT/.venv/bin/python"
export PYTHONPATH="$ROOT"
cd "$ROOT"

echo "1) venv arch"
"$PY" -c "import platform; assert platform.machine()=='arm64'; print('arm64 ok', platform.python_version())"

echo "2) imports"
"$PY" -c "import rumps,numpy,sounddevice,mlx_whisper,wilson_voice; print('imports ok')"

echo "3) ASR jfk"
"$PY" - <<'PY'
from pathlib import Path
from wilson_voice.asr import transcribe
from wilson_voice.config import Config
jfk = Path('/opt/homebrew/Cellar/whisper-cpp/1.9.1/share/whisper-cpp/jfk.wav')
c = Config.load()
text, backend = transcribe(jfk, model=c.model, language='en', use_fallback=False)
assert backend == 'mlx' and ('country' in text.lower() or 'american' in text.lower())
print('ASR ok:', text[:90])
PY

echo "4) TTS→ASR coding phrases"
"$PY" - <<'PY'
import subprocess, tempfile
from pathlib import Path
from wilson_voice.asr import transcribe
from wilson_voice.polish import polish
from wilson_voice.config import Config
c = Config.load()
phrases = [
    "Fix the login button and push to main.",
    "What is the status of the GPU server?",
    "Deploy Drivia to production now.",
]
ok = 0
for phrase in phrases:
    with tempfile.TemporaryDirectory() as td:
        td = Path(td)
        aiff, wav = td/'t.aiff', td/'t.wav'
        subprocess.run(['say','-o',str(aiff), phrase], check=True)
        subprocess.run(['ffmpeg','-y','-i',str(aiff),'-ac','1','-ar','16000',str(wav)],
                       check=True, capture_output=True)
        text, backend = transcribe(wav, model=c.model, language='en', use_fallback=False)
        keys = [w for w in phrase.lower().replace('?','').replace('.','').split() if len(w)>3]
        hits = sum(1 for k in keys if k in text.lower())
        good = backend=='mlx' and hits >= max(1, len(keys)//3)
        print(('OK' if good else 'FAIL'), phrase, '→', text[:70], f'hits={hits}/{len(keys)}')
        ok += int(good)
print(f'phrase score {ok}/{len(phrases)}')
assert ok == len(phrases)
PY

echo "5) mic list + 1s capture (may be silent)"
"$PY" - <<'PY'
import time
from wilson_voice.audio import list_input_devices, Recorder
from wilson_voice.config import Config
c = Config.load()
devs = list_input_devices()
print('mics:', [(d['index'], d['name']) for d in devs])
assert devs, 'no input devices'
rec = Recorder(c.sample_rate, c.preferred_mic_substrings)
rec.start()
time.sleep(1.0)
audio = rec.stop()
print('captured samples', len(audio), 'peak', float(abs(audio).max()) if len(audio) else 0)
# Don't fail on silence — sandbox may block mic — but recording must not throw
assert len(audio) > 0, 'zero samples — mic permission denied for this process'
print('mic capture ok')
PY

echo "6) clipboard"
"$PY" -c "from wilson_voice.paste import copy_text; assert copy_text('wilson-voice-e2e'); print('clipboard ok')"

echo "7) process / app bundle"
test -x "$HOME/Applications/Wilson Voice.app/Contents/MacOS/wilson-voice"
echo "app bundle executable ok"

echo "=== ALL E2E CHECKS PASSED ==="
