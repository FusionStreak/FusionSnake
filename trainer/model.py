"""
Surrogate model: predicts game-win probability from per-turn features.

Trains a gradient-boosted classifier on historical turn data labelled with
the game outcome.  The model acts as a cheap proxy for "how good are these
heuristic parameters?" so Optuna can evaluate hundreds of candidates without
running real games.
"""

import logging
from typing import Any

import numpy as np
import pandas as pd
from sklearn.ensemble import GradientBoostingClassifier
from sklearn.metrics import roc_auc_score, accuracy_score
from sklearn.model_selection import cross_val_score

logger = logging.getLogger(__name__)

# Feature columns used for the surrogate model.
# These are the per-direction scores (which depend on the heuristic params)
# plus contextual board features.
FEATURE_COLS: list[str] = [
    "health",
    "length",
    "board_width",
    "board_height",
    "num_snakes",
    "num_food",
    "num_hazards",
    "hazard_damage_per_turn",
    "target_food_distance",
    "target_food_contested",
    "max_enemy_length",
    "min_enemy_length",
    "length_advantage",
    "up_safety",
    "up_desirability",
    "up_space",
    "down_safety",
    "down_desirability",
    "down_space",
    "left_safety",
    "left_desirability",
    "left_space",
    "right_safety",
    "right_desirability",
    "right_space",
    "safety_weight",
    "food_weight",
    "space_weight",
]

TARGET_COL = "won"


def _prepare(df: pd.DataFrame) -> tuple[pd.DataFrame, pd.Series]:
    """Extract feature matrix X and target vector y."""
    available = [c for c in FEATURE_COLS if c in df.columns]
    X = df[available].copy()
    # Ensure boolean columns are numeric
    for col in X.columns:
        if X[col].dtype == "bool":
            X[col] = X[col].astype(int)
    y = df[TARGET_COL].astype(int)
    return X, y


def train_model(
    df: pd.DataFrame,
    n_estimators: int = 200,
    max_depth: int = 5,
    learning_rate: float = 0.1,
) -> dict[str, Any]:
    """
    Train a GBM classifier and return a dict with:
      - model: the fitted estimator
      - auc_roc: mean cross-validated AUC-ROC
      - accuracy: mean cross-validated accuracy
      - feature_importance: dict of {feature: importance}
    """
    X, y = _prepare(df)

    if len(y.unique()) < 2:
        logger.warning(
            "Only one class present in training data; skipping model training"
        )
        return {
            "model": None,
            "auc_roc": 0.0,
            "accuracy": 0.0,
            "feature_importance": {},
        }

    clf = GradientBoostingClassifier(
        n_estimators=n_estimators,
        max_depth=max_depth,
        learning_rate=learning_rate,
        subsample=0.8,
        random_state=42,
    )

    # Cross-validated metrics
    cv_auc = cross_val_score(clf, X, y, cv=5, scoring="roc_auc")
    cv_acc = cross_val_score(clf, X, y, cv=5, scoring="accuracy")

    # Final fit on all data
    clf.fit(X, y)

    importance = dict(zip(X.columns, clf.feature_importances_))

    logger.info(
        "Model trained — AUC-ROC: %.4f ± %.4f, Accuracy: %.4f ± %.4f",
        cv_auc.mean(),
        cv_auc.std(),
        cv_acc.mean(),
        cv_acc.std(),
    )

    return {
        "model": clf,
        "auc_roc": float(cv_auc.mean()),
        "auc_roc_std": float(cv_auc.std()),
        "accuracy": float(cv_acc.mean()),
        "accuracy_std": float(cv_acc.std()),
        "feature_importance": importance,
    }


def predict_win_rate(model: GradientBoostingClassifier, X: pd.DataFrame) -> float:
    """Return mean predicted win probability across all rows."""
    if model is None:
        return 0.0
    available = [c for c in FEATURE_COLS if c in X.columns]
    Xf = X[available].copy()
    for col in Xf.columns:
        if Xf[col].dtype == "bool":
            Xf[col] = Xf[col].astype(int)
    probs = model.predict_proba(Xf)[:, 1]
    return float(np.mean(probs))
