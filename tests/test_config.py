from pathlib import Path

from wilson_voice.config import Config, config_path


def test_config_defaults():
    c = Config()
    assert "whisper" in c.model
    assert c.hotkey == "right_option"
    assert c.auto_paste is True


def test_config_roundtrip(tmp_path, monkeypatch):
    monkeypatch.setattr(
        "wilson_voice.config.config_path",
        lambda: tmp_path / "config.yaml",
    )
    c = Config(hotkey="f18", auto_paste=False)
    c.save()
    c2 = Config.load()
    assert c2.hotkey == "f18"
    assert c2.auto_paste is False


def test_config_path_under_app_support():
    p = config_path()
    assert "WilsonVoice" in str(p)
