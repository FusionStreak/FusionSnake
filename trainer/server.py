#!/usr/bin/env python3
"""
FusionSnake Trainer — HTTP trigger server.

Exposes a single endpoint:
    POST /train  →  202 Accepted + starts the ML pipeline in a background
                    thread. Concurrent calls while a run is in-flight are
                    silently ignored (returns 202 with a note).

A GET /health endpoint is also provided as a liveness probe.
"""

import logging
import os
import threading
from datetime import datetime, timezone

from flask import Flask, jsonify

from log_config import setup_logging
from train import main as run_pipeline

setup_logging()
logger = logging.getLogger("trainer.server")

app = Flask(__name__)

# Lock prevents two pipeline runs from overlapping
_pipeline_lock = threading.Lock()


def _run_pipeline_bg() -> None:
    """Run the training pipeline; silently skip if already running."""
    if not _pipeline_lock.acquire(blocking=False):
        logger.info("Pipeline already running — skipping duplicate trigger")
        return
    try:
        started = datetime.now(timezone.utc).isoformat()
        logger.info("Pipeline triggered at %s", started)
        exit_code = run_pipeline(dry_run=False)
        logger.info("Pipeline finished with exit code %d", exit_code)
    except Exception:
        logger.exception("Unhandled error in training pipeline")
    finally:
        _pipeline_lock.release()


@app.get("/health")
def health():
    """Liveness probe — always returns 200 OK."""
    return jsonify({"status": "ok"}), 200


@app.post("/train")
def trigger_train():
    """
    Trigger a training run.

    Returns 202 Accepted immediately and starts (or notes a skipped)
    pipeline execution in a background daemon thread.
    """
    running = not _pipeline_lock.acquire(blocking=False)
    if running:
        return jsonify({"status": "skipped", "reason": "pipeline already running"}), 202
    _pipeline_lock.release()  # release probe acquisition; background thread will re-acquire

    thread = threading.Thread(target=_run_pipeline_bg, daemon=True, name="trainer-bg")
    thread.start()
    return jsonify({"status": "accepted"}), 202


if __name__ == "__main__":
    port = int(os.environ.get("TRAINER_PORT", "5050"))
    logger.info("FusionSnake trainer server listening on :%d", port)
    # Use threaded=False so the GIL is shared cleanly with the pipeline thread
    app.run(host="0.0.0.0", port=port, threaded=True)
