"""Global hotkey listener (hold-to-talk).

Default: Right Option (⌥). Pure FN is a modifier-only key on Mac and is
unreliable as a sole trigger; map FN→F18 via Karabiner if you want FN.
"""
from __future__ import annotations

import logging
import threading
from typing import Callable, Optional

from .logging_setup import log_event, log_exception

log = logging.getLogger("wilson_voice.hotkey")

# Map config names → pynput Key
_KEY_MAP = {
    "right_option": "Key.alt_r",
    "left_option": "Key.alt_l",
    "right_command": "Key.cmd_r",
    "left_command": "Key.cmd_l",
    "right_control": "Key.ctrl_r",
    "left_control": "Key.ctrl_l",
    "right_shift": "Key.shift_r",
    "f13": "Key.f13",
    "f14": "Key.f14",
    "f15": "Key.f15",
    "f16": "Key.f16",
    "f17": "Key.f17",
    "f18": "Key.f18",
    "f19": "Key.f19",
    "f20": "Key.f20",
}


class HotkeyService:
    def __init__(
        self,
        hotkey_name: str,
        on_press: Callable[[], None],
        on_release: Callable[[], None],
    ):
        self.hotkey_name = hotkey_name
        self.on_press = on_press
        self.on_release = on_release
        self._listener = None
        self._down = False
        self._lock = threading.Lock()

    def start(self) -> None:
        from pynput import keyboard
        from pynput.keyboard import Key

        target = self._resolve_key(keyboard, Key)
        log.info("hotkey listener starting target=%s name=%s", target, self.hotkey_name)
        log_event("hotkey_start", name=self.hotkey_name)

        def on_press(key):
            try:
                if key == target:
                    with self._lock:
                        if not self._down:
                            self._down = True
                            log_event("hotkey_press", name=self.hotkey_name)
                            self.on_press()
            except Exception as e:
                log_exception("hotkey_press", e)

        def on_release(key):
            try:
                if key == target:
                    with self._lock:
                        if self._down:
                            self._down = False
                            log_event("hotkey_release", name=self.hotkey_name)
                            self.on_release()
            except Exception as e:
                log_exception("hotkey_release", e)

        self._listener = keyboard.Listener(on_press=on_press, on_release=on_release)
        self._listener.daemon = True
        self._listener.start()

    def stop(self) -> None:
        try:
            if self._listener:
                self._listener.stop()
        except Exception as e:
            log_exception("hotkey_stop", e)
        self._listener = None
        log_event("hotkey_stop")

    def _resolve_key(self, keyboard, Key):
        name = self.hotkey_name.lower().strip()
        if name == "ctrl_shift_space":
            # For combo we only listen for space with modifiers checked — simplified: use f18
            log.warning("ctrl_shift_space not fully implemented; using f18")
            return Key.f18
        attr = _KEY_MAP.get(name)
        if not attr:
            log.warning("unknown hotkey %r — defaulting right_option", name)
            return Key.alt_r
        # attr like Key.alt_r
        key_name = attr.split(".", 1)[1]
        return getattr(Key, key_name)
