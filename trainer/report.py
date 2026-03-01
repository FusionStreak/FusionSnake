"""
PDF report generator for daily training runs.

Produces a multi-page PDF saved to /data/reports/report_YYYY-MM-DD.pdf with:
  - Header: date, data volume summary
  - Current vs. proposed parameters (side-by-side table with delta)
  - Surrogate model metrics: AUC-ROC, accuracy, feature importance chart
  - Optuna optimisation: history plot, parameter importance
  - Win-rate trend (from /stats/history)
"""

import logging
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import matplotlib

matplotlib.use("Agg")  # non-interactive backend
import matplotlib.pyplot as plt
import numpy as np
import optuna
import requests
from fpdf import FPDF

from param_schema import PARAM_SPECS, defaults_dict

logger = logging.getLogger(__name__)

SNAKE_URL = os.environ.get("SNAKE_URL", "http://localhost:6666")
API_KEY = os.environ.get("API_KEY", "")
REPORTS_DIR = Path(os.environ.get("REPORTS_DIR", "/data/reports"))


def _headers() -> dict[str, str]:
    h: dict[str, str] = {"Accept": "application/json"}
    if API_KEY:
        h["X-API-Key"] = API_KEY
    return h


class TrainingReport(FPDF):
    """Custom PDF with header/footer."""

    report_date: str = ""

    def header(self):
        self.set_font("Helvetica", "B", 14)
        self.cell(
            0,
            10,
            f"FusionSnake Training Report | {self.report_date}",
            ln=True,
            align="C",
        )
        self.ln(4)

    def footer(self):
        self.set_y(-15)
        self.set_font("Helvetica", "I", 8)
        self.cell(0, 10, f"Page {self.page_no()}/{{nb}}", align="C")


def _add_summary_page(
    pdf: TrainingReport,
    total_turns: int,
    total_games: int,
    model_metrics: dict[str, Any],
):
    """Page 1: Data volume & model metrics."""
    pdf.add_page()
    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "1. Data Summary", ln=True)
    pdf.set_font("Helvetica", "", 10)
    pdf.cell(0, 6, f"Total training turns: {total_turns:,}", ln=True)
    pdf.cell(0, 6, f"Total games with outcomes: {total_games:,}", ln=True)
    pdf.ln(4)

    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "2. Surrogate Model Performance", ln=True)
    pdf.set_font("Helvetica", "", 10)
    auc = model_metrics.get("auc_roc", 0)
    auc_std = model_metrics.get("auc_roc_std", 0)
    acc = model_metrics.get("accuracy", 0)
    acc_std = model_metrics.get("accuracy_std", 0)
    pdf.cell(0, 6, f"AUC-ROC (5-fold CV): {auc:.4f} ± {auc_std:.4f}", ln=True)
    pdf.cell(0, 6, f"Accuracy (5-fold CV): {acc:.4f} ± {acc_std:.4f}", ln=True)


def _add_feature_importance(
    pdf: TrainingReport,
    feature_importance: dict[str, float],
    tmp_dir: Path,
):
    """Feature importance bar chart."""
    if not feature_importance:
        return

    pdf.ln(6)
    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "3. Feature Importance", ln=True)

    # Sort by importance descending
    sorted_feats = sorted(feature_importance.items(), key=lambda x: x[1], reverse=True)
    names = [f[0] for f in sorted_feats[:15]]
    values = [f[1] for f in sorted_feats[:15]]

    fig, ax = plt.subplots(figsize=(7, 4))
    ax.barh(names[::-1], values[::-1], color="#BF360C")
    ax.set_xlabel("Importance")
    ax.set_title("Top 15 Feature Importances")
    plt.tight_layout()

    img_path = tmp_dir / "feature_importance.png"
    fig.savefig(img_path, dpi=150)
    plt.close(fig)
    pdf.image(str(img_path), x=10, w=190)


def _add_params_table(
    pdf: TrainingReport,
    current_params: dict[str, Any],
    proposed_params: dict[str, Any],
):
    """Side-by-side current vs. proposed parameter table with delta."""
    pdf.add_page()
    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "4. Parameter Comparison: Current vs. Proposed", ln=True)
    pdf.ln(2)

    # Table header
    pdf.set_font("Helvetica", "B", 8)
    col_w = [70, 30, 30, 30]
    headers = ["Parameter", "Current", "Proposed", "Delta"]
    for w, h_text in zip(col_w, headers):
        pdf.cell(w, 6, h_text, border=1, align="C")
    pdf.ln()

    # Table rows
    pdf.set_font("Helvetica", "", 8)
    for spec in PARAM_SPECS:
        cur = current_params.get(spec.name, spec.default)
        prop = proposed_params.get(spec.name, spec.default)
        delta = prop - cur
        delta_str = f"{delta:+}" if isinstance(delta, int) else f"{delta:+.2f}"

        pdf.cell(col_w[0], 5, spec.name, border=1)
        pdf.cell(col_w[1], 5, str(cur), border=1, align="C")
        pdf.cell(col_w[2], 5, str(prop), border=1, align="C")

        # Colour delta: green for improvement direction, red otherwise
        if delta != 0:
            pdf.set_text_color(0, 128, 0) if delta != 0 else None
        pdf.cell(col_w[3], 5, delta_str, border=1, align="C")
        pdf.set_text_color(0, 0, 0)
        pdf.ln()


def _add_optuna_plots(
    pdf: TrainingReport,
    study: Any,
    tmp_dir: Path,
):
    """Optuna optimisation history plot."""
    if study is None:
        return

    pdf.add_page()
    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "5. Optimisation History", ln=True)

    # Plot optimisation history manually (avoid optuna.visualization dependency)
    trials = [t for t in study.trials if t.value is not None]
    if not trials:
        pdf.set_font("Helvetica", "", 10)
        pdf.cell(0, 6, "No completed trials to plot.", ln=True)
        return

    trial_nums = [t.number for t in trials]
    values = [t.value for t in trials]

    # Running best
    running_best = []
    best_so_far = -np.inf
    for v in values:
        best_so_far = max(best_so_far, v)
        running_best.append(best_so_far)

    fig, ax = plt.subplots(figsize=(7, 4))
    ax.scatter(
        trial_nums, values, alpha=0.3, s=8, label="Trial win-rate", color="#BF360C"
    )
    ax.plot(trial_nums, running_best, color="#1B5E20", linewidth=2, label="Best so far")
    ax.set_xlabel("Trial")
    ax.set_ylabel("Predicted Win Rate")
    ax.set_title("Optuna Optimisation History")
    ax.legend()
    plt.tight_layout()

    img_path = tmp_dir / "optuna_history.png"
    fig.savefig(img_path, dpi=150)
    plt.close(fig)
    pdf.image(str(img_path), x=10, w=190)

    # Parameter importance (which params matter most)
    pdf.ln(6)
    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "6. Parameter Importance (Optuna)", ln=True)

    try:
        importances = optuna.importance.get_param_importances(study)
        if importances:
            sorted_imp = sorted(importances.items(), key=lambda x: x[1], reverse=True)
            names = [x[0] for x in sorted_imp[:15]]
            vals = [x[1] for x in sorted_imp[:15]]

            fig2, ax2 = plt.subplots(figsize=(7, 4))
            ax2.barh(names[::-1], vals[::-1], color="#1565C0")
            ax2.set_xlabel("Importance")
            ax2.set_title("Top 15 Hyperparameter Importances")
            plt.tight_layout()

            img_path2 = tmp_dir / "param_importance.png"
            fig2.savefig(img_path2, dpi=150)
            plt.close(fig2)
            pdf.image(str(img_path2), x=10, w=190)
    except Exception as e:
        logger.warning("Could not compute param importance: %s", e)
        pdf.set_font("Helvetica", "", 10)
        pdf.cell(0, 6, f"Could not compute parameter importance: {e}", ln=True)


def _add_win_rate_trend(pdf: TrainingReport, tmp_dir: Path):
    """Fetch /stats/history and plot rolling win-rate."""
    pdf.add_page()
    pdf.set_font("Helvetica", "B", 12)
    pdf.cell(0, 8, "7. Win-Rate Trend", ln=True)

    try:
        resp = requests.get(
            f"{SNAKE_URL}/stats/history",
            params={"limit": 1000},
            headers=_headers(),
            timeout=15,
        )
        resp.raise_for_status()
        history = resp.json().get("data", [])
    except Exception as e:
        pdf.set_font("Helvetica", "", 10)
        pdf.cell(0, 6, f"Could not fetch stats history: {e}", ln=True)
        return

    if not history:
        pdf.set_font("Helvetica", "", 10)
        pdf.cell(0, 6, "No stats history available yet.", ln=True)
        return

    games = list(range(1, len(history) + 1))
    win_rates = [float(h.get("cumulative_win_rate", 0)) for h in history]

    fig, ax = plt.subplots(figsize=(7, 3.5))
    ax.plot(games, win_rates, color="#BF360C", linewidth=1.5)
    ax.set_xlabel("Game #")
    ax.set_ylabel("Cumulative Win Rate (%)")
    ax.set_title("Win-Rate Over Time")
    ax.set_ylim(0, 100)
    plt.tight_layout()

    img_path = tmp_dir / "win_rate_trend.png"
    fig.savefig(img_path, dpi=150)
    plt.close(fig)
    pdf.image(str(img_path), x=10, w=190)


def generate_report(
    model_metrics: dict[str, Any],
    optimisation_results: dict[str, Any],
    total_turns: int,
    total_games: int,
) -> Path:
    """
    Generate the daily PDF report and return the file path.
    """
    REPORTS_DIR.mkdir(parents=True, exist_ok=True)
    today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
    output_path = REPORTS_DIR / f"report_{today}.pdf"
    tmp_dir = REPORTS_DIR / ".tmp"
    tmp_dir.mkdir(parents=True, exist_ok=True)

    # Fetch current live params
    try:
        resp = requests.get(f"{SNAKE_URL}/config", headers=_headers(), timeout=10)
        resp.raise_for_status()
        current_params = resp.json()
    except Exception:
        current_params = defaults_dict()

    proposed_params = optimisation_results.get("best_params", defaults_dict())

    pdf = TrainingReport()
    pdf.report_date = today
    pdf.alias_nb_pages()

    # Page 1: summary + model metrics + feature importance
    _add_summary_page(pdf, total_turns, total_games, model_metrics)
    _add_feature_importance(pdf, model_metrics.get("feature_importance", {}), tmp_dir)

    # Page 2: parameter table
    _add_params_table(pdf, current_params, proposed_params)

    # Page 3: Optuna plots
    _add_optuna_plots(pdf, optimisation_results.get("study"), tmp_dir)

    # Page 4: win-rate trend
    _add_win_rate_trend(pdf, tmp_dir)

    pdf.output(str(output_path))
    logger.info("Report saved to %s", output_path)

    # Clean up temp images
    for f in tmp_dir.glob("*.png"):
        f.unlink()

    return output_path
