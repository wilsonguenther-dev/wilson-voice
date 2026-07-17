"""Clipboard + paste into frontmost app (Accessibility required)."""
from __future__ import annotations

import logging
import subprocess
import time

from .logging_setup import log_event, log_exception

log = logging.getLogger("wilson_voice.paste")


def copy_text(text: str) -> bool:
    try:
        r = subprocess.run(
            ["pbcopy"],
            input=text,
            text=True,
            check=False,
            capture_output=True,
        )
        ok = r.returncode == 0
        log_event("clipboard_copy", ok=ok, chars=len(text))
        if not ok:
            log.error("pbcopy failed: %s", r.stderr)
        return ok
    except Exception as e:
        log_exception("clipboard_copy", e)
        return False


def paste_frontmost(delay: float = 0.12) -> bool:
    """Simulate Cmd+V in the frontmost app."""
    time.sleep(delay)
    script = """
    tell application "System Events"
        keystroke "v" using command down
    end tell
    """
    try:
        r = subprocess.run(
            ["osascript", "-e", script],
            capture_output=True,
            text=True,
            check=False,
        )
        ok = r.returncode == 0
        log_event("paste", ok=ok, stderr=(r.stderr or "")[:200])
        if not ok:
            log.error("paste failed: %s", r.stderr)
            log.error(
                "Grant Accessibility to Terminal/Python/Wilson Voice in "
                "System Settings → Privacy & Security → Accessibility"
            )
        return ok
    except Exception as e:
        log_exception("paste", e)
        return False


def copy_and_paste(text: str, do_paste: bool = True) -> bool:
    if not copy_text(text):
        return False
    if do_paste:
        return paste_frontmost()
    return True
