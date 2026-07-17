from pynput.keyboard import Key

from wilson_voice.hotkey import HotkeyService


def test_resolve_right_option():
    svc = HotkeyService("right_option", lambda: None, lambda: None)
    from pynput import keyboard

    k = svc._resolve_key(keyboard, Key)
    assert k == Key.alt_r


def test_resolve_f18():
    svc = HotkeyService("f18", lambda: None, lambda: None)
    from pynput import keyboard

    assert svc._resolve_key(keyboard, Key) == Key.f18


def test_resolve_unknown_defaults():
    svc = HotkeyService("not_a_key", lambda: None, lambda: None)
    from pynput import keyboard

    assert svc._resolve_key(keyboard, Key) == Key.alt_r


def test_resolve_right_command():
    svc = HotkeyService("right_command", lambda: None, lambda: None)
    from pynput import keyboard

    assert svc._resolve_key(keyboard, Key) == Key.cmd_r
