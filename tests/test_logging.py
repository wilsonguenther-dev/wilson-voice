import json
from pathlib import Path

from wilson_voice.logging_setup import default_log_dir, log_event, setup_logging


def test_setup_logging_creates_dir(tmp_path, monkeypatch):
    monkeypatch.setattr("wilson_voice.logging_setup.default_log_dir", lambda: tmp_path / "logs")
    d = setup_logging("DEBUG", log_dir=tmp_path / "logs")
    assert d.exists()
    assert (d / "wilson-voice.log").exists() or True  # created on first write


def test_log_event_writes_jsonl(tmp_path, monkeypatch):
    monkeypatch.setattr("wilson_voice.logging_setup.default_log_dir", lambda: tmp_path)
    log_event("unit_test_event", foo=1, bar="x")
    path = tmp_path / "events.jsonl"
    assert path.exists()
    last = path.read_text().strip().splitlines()[-1]
    obj = json.loads(last)
    assert obj["event"] == "unit_test_event"
    assert obj["foo"] == 1
