"""python -m wilson_voice → menubar app."""
from __future__ import annotations

import sys


def main() -> None:
    # Menubar needs to run on main thread
    from .app import run_app

    try:
        run_app()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
