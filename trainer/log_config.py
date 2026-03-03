"""
Shared JSON logging configuration for the FusionSnake trainer.

Call ``setup_logging()`` once at process startup (in server.py and train.py).
All child loggers created with ``logging.getLogger(__name__)`` inherit the
handler automatically.

Output format mirrors the Rust tracing-subscriber JSON layout:
  {"timestamp": "…", "level": "INFO", "target": "trainer.server", "message": "…"}
"""

import logging
import os

from pythonjsonlogger.json import JsonFormatter


def setup_logging() -> None:
    """Configure the root logger to emit one JSON object per line."""
    level_name = os.environ.get("LOG_LEVEL", "INFO").upper()
    level = logging.getLevelName(level_name)

    formatter = JsonFormatter(
        fmt="%(asctime)s %(levelname)s %(name)s %(message)s",
        rename_fields={
            "asctime": "timestamp",
            "levelname": "level",
            "name": "target",
        },
        datefmt="%Y-%m-%dT%H:%M:%SZ",
    )

    handler = logging.StreamHandler()
    handler.setFormatter(formatter)

    root = logging.getLogger()
    # Replace any handlers added by a previous basicConfig call
    root.handlers.clear()
    root.addHandler(handler)
    root.setLevel(level)

    # Silence noisy third-party loggers that don't need DEBUG output
    for noisy in ("optuna", "matplotlib", "PIL", "werkzeug"):
        logging.getLogger(noisy).setLevel(logging.WARNING)
