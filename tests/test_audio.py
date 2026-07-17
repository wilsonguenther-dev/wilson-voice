import numpy as np

from wilson_voice.audio import list_input_devices, pick_device, Recorder


def test_list_devices_nonempty():
    devs = list_input_devices()
    assert isinstance(devs, list)
    # CI may be headless; on Wilson's Mac must have mics
    if devs:
        assert "name" in devs[0]
        assert "index" in devs[0]


def test_pick_device_returns_int_or_none():
    idx = pick_device(["MacBook", "Microphone", "Wilson"])
    assert idx is None or isinstance(idx, int)


def test_write_wav(tmp_path):
    rec = Recorder(sample_rate=16000)
    audio = np.zeros(16000, dtype=np.float32)
    audio[100:200] = 0.1
    path = rec.write_wav(tmp_path / "t.wav", audio)
    assert path.exists()
    assert path.stat().st_size > 100


def test_write_wav_empty(tmp_path):
    rec = Recorder()
    path = rec.write_wav(tmp_path / "e.wav", np.zeros(0, dtype=np.float32))
    assert path.exists()
