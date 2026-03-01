"""
Bayesian optimiser: uses Optuna TPE to search over the 22-parameter heuristic
space, scoring each candidate via the surrogate win-rate model.

For each trial the optimiser:
  1. Samples 22 candidate param values from the search space.
  2. Re-computes the *chosen move* that those weights would produce using the
     per-direction scores already stored in the training data.
  3. Recomputes the weight columns to match the candidate weights.
  4. Feeds the modified feature rows into the surrogate model.
  5. Returns the predicted win-rate as the objective to maximise.
"""

import logging
from typing import Any

import numpy as np
import optuna
import pandas as pd

from model import FEATURE_COLS, predict_win_rate
from param_schema import PARAM_SPECS, ParamSpec, defaults_dict

logger = logging.getLogger(__name__)

# Silence Optuna's verbose trial logs
optuna.logging.set_verbosity(optuna.logging.WARNING)

# Direction columns per move
_DIRECTIONS = ["up", "down", "left", "right"]
_SCORE_TYPES = ["safety", "desirability", "space"]


def _recompute_weights(df: pd.DataFrame, params: dict[str, Any]) -> pd.DataFrame:
    """
    Given candidate heuristic weight params, recompute the safety_weight /
    food_weight / space_weight columns based on the health thresholds, then
    re-derive which move would be chosen.
    """
    out = df.copy()

    desperate_thresh = params["health_threshold_desperate"]
    balanced_thresh = params["health_threshold_balanced"]

    mask_desperate = out["health"] < desperate_thresh
    mask_balanced = (~mask_desperate) & (out["health"] < balanced_thresh)
    mask_healthy = out["health"] >= balanced_thresh

    out.loc[mask_desperate, "safety_weight"] = params["weight_desperate_safety"]
    out.loc[mask_desperate, "food_weight"] = params["weight_desperate_food"]
    out.loc[mask_desperate, "space_weight"] = params["weight_desperate_space"]

    out.loc[mask_balanced, "safety_weight"] = params["weight_balanced_safety"]
    out.loc[mask_balanced, "food_weight"] = params["weight_balanced_food"]
    out.loc[mask_balanced, "space_weight"] = params["weight_balanced_space"]

    out.loc[mask_healthy, "safety_weight"] = params["weight_healthy_safety"]
    out.loc[mask_healthy, "food_weight"] = params["weight_healthy_food"]
    out.loc[mask_healthy, "space_weight"] = params["weight_healthy_space"]

    # Re-derive chosen_move using the weighted score formula:
    #   score = safety*sw + desirability*fw + space*spw
    # Pick direction with highest score (among those with safety > 0)
    sw = out["safety_weight"].values
    fw = out["food_weight"].values
    spw = out["space_weight"].values

    scores: dict[str, np.ndarray] = {}
    safeties: dict[str, np.ndarray] = {}
    for d in _DIRECTIONS:
        s = out[f"{d}_safety"].values.astype(np.float64)
        des = out[f"{d}_desirability"].values.astype(np.float64)
        sp = out[f"{d}_space"].values.astype(np.float64)
        scores[d] = s * sw + des * fw + sp * spw
        safeties[d] = s

    # Build (n_rows, 4) score matrix; mask out directions with safety=0
    score_mat = np.column_stack([scores[d] for d in _DIRECTIONS])
    safety_mat = np.column_stack([safeties[d] for d in _DIRECTIONS])

    # Where safety > 0, use score; otherwise -inf so it's never picked
    masked = np.where(safety_mat > 0, score_mat, -np.inf)

    # If ALL directions have safety 0, fall back to raw score (desperation)
    all_zero = np.all(safety_mat == 0, axis=1)
    masked[all_zero] = score_mat[all_zero]

    best_idx = np.argmax(masked, axis=1)
    dir_names = np.array(_DIRECTIONS)
    out["chosen_move"] = dir_names[best_idx]

    return out


def _sample_params(trial: optuna.Trial) -> dict[str, Any]:
    """Sample all 22 parameters from Optuna's search space."""
    params: dict[str, Any] = {}
    for spec in PARAM_SPECS:
        # All current params are integer
        params[spec.name] = trial.suggest_int(
            spec.name, int(spec.low), int(spec.high), step=int(spec.step or 1)
        )
    return params


def optimise(
    df: pd.DataFrame,
    surrogate_model: Any,
    n_trials: int = 200,
    min_improvement: float = 0.02,
) -> dict[str, Any]:
    """
    Run Bayesian optimisation and return the best parameter set.

    Returns a dict with:
      - best_params: dict of {name: value}
      - best_win_rate: predicted win-rate of best params
      - baseline_win_rate: predicted win-rate of current defaults
      - improvement: best - baseline
      - n_trials: number of trials run
      - study: the Optuna study object (for visualisation)
      - should_apply: bool — True if improvement exceeds min_improvement
    """
    # Compute baseline win-rate with current defaults
    defaults = defaults_dict()
    baseline_df = _recompute_weights(df, defaults)
    baseline_wr = predict_win_rate(surrogate_model, baseline_df[FEATURE_COLS])
    logger.info("Baseline predicted win-rate: %.4f", baseline_wr)

    def objective(trial: optuna.Trial) -> float:
        params = _sample_params(trial)
        # Enforce health_threshold_balanced > health_threshold_desperate
        if params["health_threshold_balanced"] <= params["health_threshold_desperate"]:
            raise optuna.TrialPruned()
        modified = _recompute_weights(df, params)
        return predict_win_rate(surrogate_model, modified[FEATURE_COLS])

    study = optuna.create_study(
        direction="maximize",
        sampler=optuna.samplers.TPESampler(seed=42),
        study_name="heuristic_tuning",
    )

    # Seed the study with current defaults
    study.enqueue_trial(defaults)

    study.optimize(objective, n_trials=n_trials, show_progress_bar=True)

    best = study.best_trial
    improvement = best.value - baseline_wr
    should_apply = improvement >= min_improvement

    logger.info(
        "Optimisation complete — best win-rate: %.4f (Δ %.4f), apply: %s",
        best.value,
        improvement,
        should_apply,
    )

    return {
        "best_params": best.params,
        "best_win_rate": best.value,
        "baseline_win_rate": baseline_wr,
        "improvement": improvement,
        "n_trials": n_trials,
        "study": study,
        "should_apply": should_apply,
    }
