#!/usr/bin/env python3
"""
FusionSnake ML Trainer — main entry point.

Orchestrates the full daily training pipeline:
  1. Load training data from the Battlesnake REST API
  2. Train a surrogate win-rate model (GBM)
  3. Run Bayesian optimisation (Optuna) over heuristic parameters
  4. Generate a PDF report
  5. Push optimised parameters to the snake (if improvement exceeds threshold)
"""

import argparse
import json
import logging
import os
import sys
from datetime import datetime, timezone

import requests

from data_loader import load_training_data
from model import train_model
from optimizer import optimise
from param_schema import defaults_dict
from report import generate_report

from log_config import setup_logging

setup_logging()
logger = logging.getLogger("train")

SNAKE_URL = os.environ.get("SNAKE_URL", "http://localhost:6666")
API_KEY = os.environ.get("API_KEY", "")
MIN_TRAINING_ROWS = int(os.environ.get("MIN_TRAINING_ROWS", "500"))
MIN_IMPROVEMENT = float(os.environ.get("MIN_IMPROVEMENT", "0.02"))
N_TRIALS = int(os.environ.get("N_TRIALS", "200"))


def _headers() -> dict[str, str]:
    h: dict[str, str] = {"Content-Type": "application/json"}
    if API_KEY:
        h["X-API-Key"] = API_KEY
    return h


def push_params(params: dict) -> bool:
    """POST optimised parameters to the snake's /config endpoint."""
    try:
        resp = requests.post(
            f"{SNAKE_URL}/config",
            json=params,
            headers=_headers(),
            timeout=10,
        )
        if resp.ok:
            logger.info("Successfully pushed params to snake: %s", resp.json())
            return True
        else:
            logger.error("Failed to push params: %s %s", resp.status_code, resp.text)
            return False
    except Exception as e:
        logger.error("Error pushing params: %s", e)
        return False


def main(dry_run: bool = False) -> int:
    logger.info("=" * 60)
    logger.info(
        "FusionSnake ML Training Pipeline — %s", datetime.now(timezone.utc).isoformat()
    )
    logger.info("=" * 60)

    # ── 1. Load data ──────────────────────────────────────────────────────
    logger.info("Step 1/5: Loading training data from %s", SNAKE_URL)
    df = load_training_data()

    if df.empty:
        logger.warning("No training data available. Exiting.")
        return 1

    total_turns = len(df)
    total_games = df["game_id"].nunique() if "game_id" in df.columns else 0

    logger.info("Loaded %d turns across %d games", total_turns, total_games)

    if total_turns < MIN_TRAINING_ROWS:
        logger.warning(
            "Insufficient data (%d rows < %d minimum). "
            "Generating report but skipping optimisation.",
            total_turns,
            MIN_TRAINING_ROWS,
        )
        # Still generate a report with model metrics only
        model_results = train_model(df)
        opt_results = {
            "best_params": defaults_dict(),
            "best_win_rate": 0.0,
            "baseline_win_rate": 0.0,
            "improvement": 0.0,
            "n_trials": 0,
            "study": None,
            "should_apply": False,
        }
        if not dry_run:
            generate_report(model_results, opt_results, total_turns, total_games)
        return 0

    # ── 2. Train surrogate model (reporting only) ─────────────────────────
    logger.info("Step 2/5: Training surrogate model (for reporting)")
    model_results = train_model(df)

    if model_results["model"] is None:
        logger.warning(
            "Model training failed (single-class data?). Metrics will be empty."
        )
    else:
        logger.info(
            "Surrogate model: AUC-ROC=%.4f, Accuracy=%.4f",
            model_results["auc_roc"],
            model_results["accuracy"],
        )

    # ── 3. Optimise (counterfactual objective) ────────────────────────────
    logger.info("Step 3/5: Running Bayesian optimisation (%d trials)", N_TRIALS)
    opt_results = optimise(
        df,
        n_trials=N_TRIALS,
        min_improvement=MIN_IMPROVEMENT,
    )

    logger.info(
        "Best counterfactual score: %.4f (baseline: %.4f, Δ: %.4f)",
        opt_results["best_win_rate"],
        opt_results["baseline_win_rate"],
        opt_results["improvement"],
    )

    # ── 4. Generate report ────────────────────────────────────────────────
    logger.info("Step 4/5: Generating PDF report")
    if not dry_run:
        report_path = generate_report(
            model_results, opt_results, total_turns, total_games
        )
        logger.info("Report: %s", report_path)
    else:
        logger.info("[DRY RUN] Skipping report generation")

    # ── 5. Push params ────────────────────────────────────────────────────
    logger.info("Step 5/5: Parameter deployment")
    if opt_results["should_apply"]:
        logger.info(
            "Improvement %.4f >= %.4f threshold — pushing new params",
            opt_results["improvement"],
            MIN_IMPROVEMENT,
        )
        if dry_run:
            logger.info(
                "[DRY RUN] Would push: %s",
                json.dumps(opt_results["best_params"], indent=2),
            )
        else:
            success = push_params(opt_results["best_params"])
            if not success:
                logger.error(
                    "Failed to push params — snake will continue with current values"
                )
    else:
        logger.info(
            "Improvement %.4f < %.4f threshold — keeping current params",
            opt_results["improvement"],
            MIN_IMPROVEMENT,
        )

    logger.info("Pipeline complete.")
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="FusionSnake ML Training Pipeline")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Run pipeline without pushing params or writing reports",
    )
    args = parser.parse_args()
    sys.exit(main(dry_run=args.dry_run))
