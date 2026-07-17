"""Menubar tray app (rumps) + global hold-to-talk hotkey."""
from __future__ import annotations

import logging
import threading
from typing import Optional

import rumps

from .config import Config
from .hotkey import HotkeyService
from .logging_setup import log_event, log_exception, setup_logging
from .pipeline import DictationEngine, DictationResult

log = logging.getLogger("wilson_voice.app")


class WilsonVoiceApp(rumps.App):
    def __init__(self, cfg: Optional[Config] = None):
        super().__init__(
            "WV",
            title="WV",
            quit_button="Quit Wilson Voice",
        )
        self.cfg = cfg or Config.load()
        setup_logging(self.cfg.log_level)
        self.engine = DictationEngine(self.cfg)
        self._hotkey: Optional[HotkeyService] = None
        self._recording = False
        self._last: Optional[DictationResult] = None

        self.menu = [
            rumps.MenuItem(f"Hotkey: hold {self.cfg.hotkey}", callback=None),
            rumps.MenuItem("Status: idle", callback=None),
            None,
            rumps.MenuItem("Toggle auto-paste", callback=self.toggle_paste),
            rumps.MenuItem("Open logs", callback=self.open_logs),
            rumps.MenuItem("Test mic (3s)", callback=self.test_mic),
            None,
        ]
        # Keep references to mutable items
        self._status_item = self.menu["Status: idle"]
        self._hotkey_item = self.menu[f"Hotkey: hold {self.cfg.hotkey}"]

        log_event("app_start", hotkey=self.cfg.hotkey, model=self.cfg.model)
        log.info("Wilson Voice starting model=%s hotkey=%s", self.cfg.model, self.cfg.hotkey)

    def _set_status(self, text: str) -> None:
        try:
            self._status_item.title = f"Status: {text}"
            self.title = "●" if "record" in text.lower() else "WV"
        except Exception as e:
            log_exception("set_status", e)

    def _on_press(self) -> None:
        if self._recording or self.engine.busy:
            return
        try:
            self._recording = True
            self._set_status("recording…")
            self.engine.start_recording()
            rumps.notification(
                "Wilson Voice",
                "Listening",
                f"Hold {self.cfg.hotkey} — release to transcribe",
            )
        except Exception as e:
            self._recording = False
            self._set_status(f"mic error: {e}")
            rumps.notification("Wilson Voice", "Mic error", str(e))

    def _on_release(self) -> None:
        if not self._recording:
            return
        self._recording = False
        self._set_status("transcribing…")

        def work():
            try:
                result = self.engine.finish()
                self._last = result
                if result.ok:
                    preview = result.text[:80] + ("…" if len(result.text) > 80 else "")
                    self._set_status(f"ok · {result.backend} · {result.asr_s:.1f}s")
                    rumps.notification("Wilson Voice", "Pasted" if self.cfg.auto_paste else "Copied", preview)
                else:
                    self._set_status(f"fail: {result.error}")
                    rumps.notification("Wilson Voice", "Failed", result.error or "unknown")
            except Exception as e:
                log_exception("release_worker", e)
                self._set_status(f"error: {e}")
                rumps.notification("Wilson Voice", "Error", str(e))

        threading.Thread(target=work, daemon=True, name="wv-finish").start()

    def toggle_paste(self, _):
        self.cfg.auto_paste = not self.cfg.auto_paste
        self.cfg.save()
        rumps.notification(
            "Wilson Voice",
            "Auto-paste",
            "ON" if self.cfg.auto_paste else "OFF (clipboard only)",
        )
        log_event("toggle_paste", value=self.cfg.auto_paste)

    def open_logs(self, _):
        import subprocess
        from .logging_setup import default_log_dir

        d = default_log_dir()
        subprocess.run(["open", str(d)], check=False)

    def test_mic(self, _):
        """Record 3 seconds and run full pipeline without hotkey."""
        def work():
            try:
                self._set_status("test record 3s…")
                self.engine.start_recording()
                import time

                time.sleep(3.0)
                result = self.engine.finish()
                if result.ok:
                    self._set_status(f"test ok · {result.backend}")
                    rumps.notification("Wilson Voice", "Test OK", result.text[:100])
                else:
                    self._set_status(f"test fail: {result.error}")
                    rumps.notification("Wilson Voice", "Test failed", result.error)
            except Exception as e:
                log_exception("test_mic", e)
                rumps.notification("Wilson Voice", "Test error", str(e))

        threading.Thread(target=work, daemon=True).start()

    def run_hotkey(self) -> None:
        self._hotkey = HotkeyService(
            self.cfg.hotkey,
            on_press=self._on_press,
            on_release=self._on_release,
        )
        self._hotkey.start()

    def clean_up(self):  # rumps quit hook name varies
        try:
            if self._hotkey:
                self._hotkey.stop()
        except Exception as e:
            log_exception("cleanup", e)
        log_event("app_stop")


def run_app() -> None:
    cfg = Config.load()
    app = WilsonVoiceApp(cfg)
    app.run_hotkey()
    # rumps.App.run blocks
    app.run()
