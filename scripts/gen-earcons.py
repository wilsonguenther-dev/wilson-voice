#!/usr/bin/env python3
"""Y8-C — regenerate Yap's four earcons, procedurally, from nothing but stdlib.

    python3 scripts/gen-earcons.py

Writes desktop/src-tauri/assets/sounds/{dictation-start,dictation-stop,paste,achievement}.wav
(44.1 kHz, 16-bit, mono). The outputs are COMMITTED: the app embeds them with
include_bytes! and lists them in tauri.conf.json bundle resources, so this script
is a dev-time tool only — nothing runs it at build time or at runtime.

The voice is Yappy's: a pixel pet, so the timbre is a soft square wave (a chip
tune "blip", band-limited by a one-pole low-pass so it is chunky, not harsh),
short notes, quick attack, no reverb, no swells. start/stop are under 200 ms.
"""
import math
import os
import struct
import wave

RATE = 44_100
PEAK = 0.32  # about -10 dBFS: the system volume scales it, never a blast

OUT = os.path.join(os.path.dirname(__file__), "..", "desktop", "src-tauri", "assets", "sounds")


def note(freq_start, freq_end, ms, duty=0.5):
    """One square-wave note with an exponential pitch glide and a click-free envelope."""
    n = int(RATE * ms / 1000)
    out = []
    phase = 0.0
    attack = max(1, int(RATE * 0.004))
    for i in range(n):
        t = i / max(1, n - 1)
        f = freq_start * (freq_end / freq_start) ** t
        phase = (phase + f / RATE) % 1.0
        s = 1.0 if phase < duty else -1.0
        env = min(1.0, i / attack) * (1.0 - t) ** 1.6
        out.append(s * env)
    return out


def silence(ms):
    return [0.0] * int(RATE * ms / 1000)


def lowpass(samples, cutoff=4200.0):
    rc = 1.0 / (2 * math.pi * cutoff)
    a = (1.0 / RATE) / (rc + 1.0 / RATE)
    y, out = 0.0, []
    for s in samples:
        y += a * (s - y)
        out.append(y)
    return out


def write(name, samples):
    samples = lowpass(samples)
    peak = max(abs(s) for s in samples) or 1.0
    frames = b"".join(
        struct.pack("<h", int(max(-1.0, min(1.0, s / peak * PEAK)) * 32767)) for s in samples
    )
    path = os.path.join(OUT, name)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(frames)
    print(f"{name}: {len(samples) / RATE * 1000:.0f} ms")


def main():
    os.makedirs(OUT, exist_ok=True)
    # "I'm listening": two rising blips, a perk of the ears. 150 ms.
    write("dictation-start.wav", note(523, 587, 60, 0.5) + silence(10) + note(784, 880, 80, 0.5))
    # "Got it": the same two blips, falling — the shape mirrors start. 150 ms.
    write("dictation-stop.wav", note(880, 784, 60, 0.5) + silence(10) + note(587, 494, 80, 0.5))
    # "Dropped it in": one short pop with a downward chirp. 70 ms.
    write("paste.wav", note(1319, 880, 70, 0.25))
    # "Level up": a C-E-G-C arpeggio, the only earcon allowed to be longer.
    write(
        "achievement.wav",
        note(523, 523, 70) + note(659, 659, 70) + note(784, 784, 70) + note(1047, 1047, 160),
    )


if __name__ == "__main__":
    main()
