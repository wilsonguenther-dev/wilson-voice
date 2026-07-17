"""Menubar tray app (rumps) + global hold-to-talk hotkey.

macOS note: must run as a proper GUI process (use Wilson Voice.app or
scripts/start.sh). Background nohup Python often has no visible status item.
"""
from __future__ import annotations

import logging
import os
import sys
import threading
from pathlib import Path
from typing import Optional

import rumps

from .config import Config
from .hotkey import HotkeyService
from .logging_setup import log_event, log_exception, setup_logging
from .pipeline import DictationEngine, DictationResult

log = logging.getLogger("wilson_voice.app")

# Plain ASCII title — most reliable menubar visibility (emoji can vanish /
# get swallowed into the "»" overflow on crowded bars).
TITLE_IDLE = "Voice"
TITLE_REC = "●REC"
TITLE_BUSY = "…Voice"


def _force_accessory_app() -> None:
    """Register as a UI app so the status item is allowed.

    Use Regular (not Accessory) so we also get a Dock icon — makes it
    obvious the app is running when the menu bar is crowded.
    """
    try:
        from AppKit import NSApplication, NSApplicationActivationPolicyRegular

        app = NSApplication.sharedApplication()
        # 0=regular (Dock + menu bar), 1=accessory (menu bar only)
        app.setActivationPolicy_(NSApplicationActivationPolicyRegular)
        app.activateIgnoringOtherApps_(True)
        log.info("NSApplication activation policy = Regular (Dock + menu bar)")
        log_event("nsapp_policy", policy="regular", ok=True)
    except Exception as e:
        log_exception("nsapp_policy", e)


class WilsonVoiceApp(rumps.App):
    def __init__(self, cfg: Optional[Config] = None):
        # Always use emoji title — most reliable visible menubar indicator.
        # Custom template icons often vanish in crowded/dark menu bars.
        super().__init__(
            name="Wilson Voice",
            title=TITLE_IDLE,
            quit_button="Quit Wilson Voice",
        )
        self.cfg = cfg or Config.load()
        # logging may already be initialized in run_app()
        if not logging.getLogger("wilson_voice").handlers:
            setup_logging(self.cfg.log_level)
        self.engine = DictationEngine(self.cfg)
        self._hotkey: Optional[HotkeyService] = None
        self._recording = False
        self._last: Optional[DictationResult] = None

        hotkey_label = f"Hotkey: hold {self.cfg.hotkey}"
        self.menu = [
            rumps.MenuItem("Wilson Voice — local dictation"),
            rumps.MenuItem(hotkey_label, callback=None),
            rumps.MenuItem("Status: idle", callback=None),
            None,
            rumps.MenuItem("Start 3s test recording", callback=self.test_mic),
            rumps.MenuItem("Toggle auto-paste", callback=self.toggle_paste),
            rumps.MenuItem("Open logs folder", callback=self.open_logs),
            rumps.MenuItem("Show config", callback=self.show_config),
            None,
        ]
        self._status_item = self.menu["Status: idle"]
        self._hotkey_item = self.menu[hotkey_label]

        log_event(
            "app_start",
            hotkey=self.cfg.hotkey,
            model=self.cfg.model,
            pid=os.getpid(),
            title=TITLE_IDLE,
        )
        log.info(
            "Wilson Voice UI starting model=%s hotkey=%s pid=%s",
            self.cfg.model,
            self.cfg.hotkey,
            os.getpid(),
        )

    def _set_status(self, text: str) -> None:
        try:
            self._status_item.title = f"Status: {text}"
            low = text.lower()
            if "record" in low:
                self.title = TITLE_REC
            elif "transcrib" in low or "busy" in low:
                self.title = TITLE_BUSY
            else:
                self.title = TITLE_IDLE
        except Exception as e:
            log_exception("set_status", e)

    def _on_press(self) -> None:
        if self._recording or self.engine.busy:
            log.debug("press ignored recording=%s busy=%s", self._recording, self.engine.busy)
            return
        try:
            self._recording = True
            self._set_status("recording…")
            self.engine.start_recording()
            try:
                rumps.notification(
                    title="Wilson Voice",
                    subtitle="Listening",
                    message=f"Hold {self.cfg.hotkey} — release to transcribe",
                )
            except Exception as e:
                log_exception("notification_listen", e)
        except Exception as e:
            self._recording = False
            self._set_status(f"mic error: {e}")
            log_exception("on_press", e)
            try:
                rumps.notification("Wilson Voice", "Mic error", str(e))
            except Exception:
                pass

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
                    try:
                        rumps.notification(
                            "Wilson Voice",
                            "Pasted" if self.cfg.auto_paste else "Copied",
                            preview,
                        )
                    except Exception as e:
                        log_exception("notification_ok", e)
                else:
                    self._set_status(f"fail: {result.error}")
                    try:
                        rumps.notification(
                            "Wilson Voice", "Failed", result.error or "unknown"
                        )
                    except Exception:
                        pass
            except Exception as e:
                log_exception("release_worker", e)
                self._set_status(f"error: {e}")

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

    def show_config(self, _):
        import subprocess

        from .config import config_path

        p = config_path()
        rumps.alert(
            title="Wilson Voice config",
            message=f"{p}\n\nhotkey={self.cfg.hotkey}\nmodel={self.cfg.model}\nauto_paste={self.cfg.auto_paste}",
        )
        subprocess.run(["open", "-R", str(p)], check=False)

    def test_mic(self, _):
        def work():
            try:
                self._set_status("test record 3s…")
                self.engine.start_recording()
                import time

                time.sleep(3.0)
                result = self.engine.finish()
                if result.ok:
                    self._set_status(f"test ok · {result.backend}")
                    rumps.notification("Wilson Voice", "Test OK", result.text[:120])
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

    @rumps.clicked("Quit Wilson Voice")
    def quit_app(self, _):
        try:
            if self._hotkey:
                self._hotkey.stop()
        except Exception as e:
            log_exception("quit_hotkey", e)
        log_event("app_stop")
        rumps.quit_application()


def _probe_mic_permission(cfg: Config) -> None:
    """Request TCC mic access and prove capture works."""
    try:
        from .audio import request_mic_permission

        request_mic_permission()
    except Exception as e:
        log_exception("request_mic_permission", e)
    try:
        rec = DictationEngine(cfg).recorder
        rec.start()
        import time

        time.sleep(0.4)
        audio = rec.stop()
        peak = float(abs(audio).max()) if len(audio) else 0.0
        log.info("mic probe samples=%d peak=%.4f", len(audio), peak)
        log_event("mic_probe", samples=int(len(audio)), peak=peak)
        if len(audio) == 0 or peak < 1e-6:
            log.warning(
                "Mic captured silence/empty — enable Microphone for Python / "
                "Wilson Voice in System Settings → Privacy & Security → Microphone"
            )
    except Exception as e:
        log_exception("mic_probe", e)
        log_event("mic_probe_fail", error=str(e))


def run_app() -> None:
    # Logging first so policy/setup failures are visible
    cfg = Config.load()
    # Prefer built-in mic for reliability
    cfg.preferred_mic_substrings = [
        "MacBook Pro Microphone",
        "MacBook Air Microphone",
        "Built-in",
        "Wilson G",
        "Microphone",
    ]
    setup_logging(cfg.log_level)
    # Must set activation policy before rumps creates the status item
    _force_accessory_app()
    # Trigger TCC mic prompt so we appear in System Settings → Microphone
    _probe_mic_permission(cfg)
    app = WilsonVoiceApp(cfg)
    app.run_hotkey()
    log.info("entering rumps run loop — look for '%s' in menu bar + Dock", TITLE_IDLE)
    log.warning(
        "PERMISSIONS REQUIRED:\n"
        "  1) Microphone → enable **Python** (Homebrew) / Wilson Voice\n"
        "  2) Accessibility → enable **Python** / Wilson Voice\n"
        "  3) Input Monitoring → enable **Python** / Wilson Voice\n"
        "Hotkey: hold Right Option (⌥). Menu bar title: Voice"
    )
    log_event("rumps_run_enter", title=TITLE_IDLE)
    try:
        rumps.notification(
            "Wilson Voice",
            "Running",
            "Look for 'Voice' in menu bar. Hold Right ⌥ to talk. "
            "Enable Mic + Accessibility for Python if prompted.",
        )
    except Exception:
        pass
    # rumps.App.run blocks on NSApp run loop
    app.run()
