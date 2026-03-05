"""
Surrogate model: predicts game-win probability from per-turn features.

Trains a LightGBM classifier on historical turn data labelled with the
game outcome.  The model is used for **reporting only** — feature importance
and quality metrics.  Optimisation uses a separate counterfactual objective
(see optimizer.py).
"""

import logging
from typing import Any

import numpy as np
import pandas as pd
from lightgbm import LGBMClassifier
from sklearn.metrics import roc_auc_score, accuracy_score
from sklearn.model_selection import train_test_split

logger = logging.getLogger(__name__)

# Feature columns used for the surrogate model.
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
    # Derived features
    "health_ratio",
    "length_ratio",
    "max_safety",
]

TARGET_COL = "won"


def _add_derived_features(df: pd.DataFrame) -> pd.DataFrame:
    """Add interaction / context features to the frame."""
    out = df.copy()
    out["health_ratio"] = out["health"] / 100.0
    enemy_max = out["max_enemy_length"].replace(0, 1)
    out["length_ratio"] = out["length"] / enemy_max
    out["max_safety"] = out[
        ["up_safety", "down_safety", "left_safety", "right_safety"]
    ].max(axis=1)
    return out


def _prepare(df: pd.DataFrame) -> tuple[pd.DataFrame, pd.Series]:
    """Extract feature matrix X and target vector y."""
    enriched = _add_derived_features(df)
    available = [c for c in FEATURE_COLS if c in enriched.columns]
    X = enriched[available].copy()
    for col in X.columns:
        if X[col].dtype == "bool":
            X[col] = X[col].astype(int)
    y = enriched[TARGET_COL].astype(int)
    return X, y


def train_model(
    df: pd.DataFrame,
    n_estimators: int = 300,
    max_depth: int = 6,
    learning_rate: float = 0.05,
) -> dict[str, Any]:
    """
    Train a LightGBM classifier and return a dict with:
      - model: the fitted estimator
      - auc_roc: AUC-ROC on the holdout test set
      - accuracy: accuracy on the holdout test set
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

    # 80/20 holdout split for honest evaluation
    X_train, X_test, y_train, y_test = train_test_split(
        X, y, test_size=0.2, random_state=42, stratify=y
    )

    clf = LGBMClassifier(
        n_estimators=n_estimators,
        max_depth=max_depth,
        learning_rate=learning_rate,
        subsample=0.8,
        is_unbalance=True,
        random_state=42,
        verbosity=-1,
    )

    clf.fit(X_train, y_train)

    # Evaluate on held-out test set
    y_prob = clf.predict_proba(X_test)[:, 1]
    y_pred = clf.predict(X_test)
    auc = float(roc_auc_score(y_test, y_prob))
    acc = float(accuracy_score(y_test, y_pred))

    importance = dict(zip(X.columns, clf.feature_importances_))

    logger.info(
        "Model trained — AUC-ROC (holdout): %.4f, Accuracy (holdout): %.4f",
        auc,
        acc,
    )

    return {
        "model": clf,
        "auc_roc": auc,
        "accuracy": acc,
        "feature_importance": importance,
    }


def predict_win_rate(model: LGBMClassifier, X: pd.DataFrame) -> float:
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
