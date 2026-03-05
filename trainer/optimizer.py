"""
Bayesian optimiser: uses Optuna TPE to search over the tunable heuristic
weight / threshold parameters using a **counterfactual agreement-rate**
objective.

For each trial the optimiser:
  1. Samples candidate weight + threshold values from the search space.
  2. Re-derives the chosen move that those weights would produce using the
     per-direction scores already stored in the training data.
  3. Compares the trial's chosen moves against the *original* chosen moves
     within each game.
  4. Computes a counterfactual score that rewards agreement with the original
     decision on turns from **won** games and disagreement on turns from
     **lost** games.
  5. Returns the score as the objective to maximise.

This replaces the previous surrogate-model-prediction objective, which
barely moved because the frozen per-direction scores dominate the feature
space.
"""

import logging
from typing import Any

import numpy as np
import optuna
import pandas as pd

from param_schema import defaults_dict, full_params_dict, tunable_specs

logger = logging.getLogger(__name__)

# Silence Optuna's verbose trial logs
optuna.logging.set_verbosity(optuna.logging.WARNING)

_DIRECTIONS = ["up", "down", "left", "right"]


# ── Vectorised move re-derivation ─────────────────────────────────────────


def _choose_moves(df: pd.DataFrame, params: dict[str, Any]) -> np.ndarray:
    """
    Return an int array of chosen-move indices (0-3 for up/down/left/right)
    for every row, applying the candidate weight + threshold params to the
    stored per-direction scores.
    """
    health = df["health"].values

    desperate_thresh = params["health_threshold_desperate"]
    balanced_thresh = params["health_threshold_balanced"]

    mask_desp = health < desperate_thresh
    mask_bal = (~mask_desp) & (health < balanced_thresh)
    mask_heal = health >= balanced_thresh

    n = len(df)
    sw = np.empty(n, dtype=np.float64)
    fw = np.empty(n, dtype=np.float64)
    spw = np.empty(n, dtype=np.float64)

    sw[mask_desp] = params["weight_desperate_safety"]
    fw[mask_desp] = params["weight_desperate_food"]
    spw[mask_desp] = params["weight_desperate_space"]

    sw[mask_bal] = params["weight_balanced_safety"]
    fw[mask_bal] = params["weight_balanced_food"]
    spw[mask_bal] = params["weight_balanced_space"]

    sw[mask_heal] = params["weight_healthy_safety"]
    fw[mask_heal] = params["weight_healthy_food"]
    spw[mask_heal] = params["weight_healthy_space"]

    # (n, 4) matrices of safety / desirability / space
    safety_mat = np.column_stack(
        [df[f"{d}_safety"].values.astype(np.float64) for d in _DIRECTIONS]
    )
    desir_mat = np.column_stack(
        [df[f"{d}_desirability"].values.astype(np.float64) for d in _DIRECTIONS]
    )
    space_mat = np.column_stack(
        [df[f"{d}_space"].values.astype(np.float64) for d in _DIRECTIONS]
    )

    score_mat = (
        safety_mat * sw[:, None] + desir_mat * fw[:, None] + space_mat * spw[:, None]
    )

    # Mask out directions with safety == 0 (dead moves)
    masked = np.where(safety_mat > 0, score_mat, -np.inf)

    # If ALL directions are lethal, fall back to raw score
    all_zero = np.all(safety_mat == 0, axis=1)
    masked[all_zero] = score_mat[all_zero]

    return np.argmax(masked, axis=1)


# ── Counterfactual objective ──────────────────────────────────────────────


def _counterfactual_score(
    trial_moves: np.ndarray,
    baseline_moves: np.ndarray,
    won: np.ndarray,
) -> float:
    """
    Compute a counterfactual agreement-rate score.

    For each turn:
      - If won=True: reward +1 for *agreeing* with the baseline move
        (don't break a winning pattern).
      - If won=False: reward +1 for *disagreeing* with the baseline move
        (change a losing pattern).

    Returns the mean score (0..1).
    """
    agree = trial_moves == baseline_moves
    score = np.where(won, agree, ~agree)
    return float(np.mean(score))


# ── Optuna sampling ──────────────────────────────────────────────────────


def _sample_params(trial: optuna.Trial) -> dict[str, Any]:
    """Sample only the tunable parameters from Optuna's search space."""
    params: dict[str, Any] = {}
    for spec in tunable_specs():
        params[spec.name] = trial.suggest_int(
            spec.name, int(spec.low), int(spec.high), step=int(spec.step or 1)
        )
    return params


def _tunable_defaults() -> dict[str, Any]:
    """Return a dict of {name: default} for tunable params only (for enqueue)."""
    return {spec.name: spec.default for spec in tunable_specs()}


# ── Public API ────────────────────────────────────────────────────────────


def optimise(
    df: pd.DataFrame,
    n_trials: int = 200,
    min_improvement: float = 0.02,
) -> dict[str, Any]:
    """
    Run Bayesian optimisation and return the best parameter set.

    Returns a dict with:
      - best_params: full 22-param dict (tuned + defaults for frozen)
      - best_win_rate: counterfactual score of best params
      - baseline_win_rate: counterfactual score of current defaults
      - improvement: best - baseline
      - n_trials: number of trials run
      - study: the Optuna study object (for visualisation)
      - should_apply: bool — True if improvement exceeds min_improvement
    """
    defaults = defaults_dict()
    won = df["won"].values.astype(bool)

    # Baseline: moves chosen with default params
    baseline_moves = _choose_moves(df, defaults)
    baseline_wr = _counterfactual_score(baseline_moves, baseline_moves, won)
    logger.info("Baseline counterfactual score: %.4f", baseline_wr)

    def objective(trial: optuna.Trial) -> float:
        tuned = _sample_params(trial)
        # Enforce health_threshold_balanced > health_threshold_desperate
        if tuned["health_threshold_balanced"] <= tuned["health_threshold_desperate"]:
            raise optuna.TrialPruned()
        params = full_params_dict(tuned)
        trial_moves = _choose_moves(df, params)
        return _counterfactual_score(trial_moves, baseline_moves, won)

    study = optuna.create_study(
        direction="maximize",
        sampler=optuna.samplers.TPESampler(seed=42),
        study_name="heuristic_tuning",
    )

    # Seed with current defaults
    study.enqueue_trial(_tunable_defaults())

    study.optimize(objective, n_trials=n_trials, show_progress_bar=True)

    best = study.best_trial
    improvement = best.value - baseline_wr
    should_apply = improvement >= min_improvement

    best_full = full_params_dict(best.params)

    logger.info(
        "Optimisation complete — best score: %.4f (Δ %.4f), apply: %s",
        best.value,
        improvement,
        should_apply,
    )

    return {
        "best_params": best_full,
        "best_win_rate": best.value,
        "baseline_win_rate": baseline_wr,
        "improvement": improvement,
        "n_trials": n_trials,
        "study": study,
        "should_apply": should_apply,
    }
