"""
Parameter schema: single source of truth for the 22 tunable heuristic values.

Each entry defines the parameter name (matching the Rust HeuristicParams struct),
its Python type, the current default, and the Optuna search bounds
(min, max, step).  Integer parameters use suggest_int; float would use
suggest_float, but all current params are integral.
"""

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class ParamSpec:
    """Specification for one tunable parameter."""

    name: str
    default: int | float
    low: int | float
    high: int | float
    step: int | float | None = None
    description: str = ""
    tunable: bool = True


# fmt: off
PARAM_SPECS: list[ParamSpec] = [
    # ── hazard penalties (frozen in training data — not tunable in Phase 1) ─
    ParamSpec("hazard_penalty_low_health",  20,   5,  60, 1, "Safety penalty on hazard when health < threshold", tunable=False),
    ParamSpec("hazard_penalty_high_health", 10,   2,  40, 1, "Safety penalty on hazard when health >= threshold", tunable=False),
    ParamSpec("hazard_health_threshold",    50,  20,  80, 5, "Health boundary for hazard penalty graduation", tunable=False),

    # ── edge proximity (frozen) ───────────────────────────────────────────
    ParamSpec("edge_proximity_penalty",      1,   0,  10, 1, "Per-axis safety penalty near board edge", tunable=False),
    ParamSpec("edge_proximity_distance",     1,   0,   3, 1, "Tile distance from edge that triggers penalty", tunable=False),

    # ── head-to-head (frozen) ─────────────────────────────────────────────
    ParamSpec("h2h_detection_radius",        2,   1,   4, 1, "Max Manhattan dist for head-to-head scoring", tunable=False),
    ParamSpec("h2h_aggression_bonus",       15,   0,  40, 1, "Desirability bonus when longer & enemy head dist=1", tunable=False),
    ParamSpec("h2h_penalty_close",          30,   5,  80, 1, "Safety penalty when shorter & enemy head dist<=1", tunable=False),
    ParamSpec("h2h_penalty_medium",          8,   1,  30, 1, "Safety penalty when shorter & enemy head dist=2", tunable=False),

    # ── body proximity (frozen) ───────────────────────────────────────────
    ParamSpec("body_proximity_penalty",      2,   0,  10, 1, "Per-adjacent-body-segment safety penalty", tunable=False),

    # ── flood fill (frozen) ───────────────────────────────────────────────
    ParamSpec("flood_fill_trap_penalty",    50,  10, 120, 5, "Safety penalty when reachable < body length", tunable=False),

    # ── food scoring (frozen) ─────────────────────────────────────────────
    ParamSpec("food_desirability_base",    200, 100, 255, 5, "Base value for food desirability (score = base - dist)", tunable=False),

    # ── health thresholds for weight tiers ────────────────────────────────
    # NOTE: desperate max (45) must stay below balanced min (50) to prevent
    # Optuna from sampling invalid combinations (balanced <= desperate).
    ParamSpec("health_threshold_desperate",  30,  10,  45, 5, "Below this → desperate weights"),
    ParamSpec("health_threshold_balanced",   60,  50,  90, 5, "Below this (above desperate) → balanced weights"),

    # ── weight triplets ───────────────────────────────────────────────────
    ParamSpec("weight_desperate_safety",     1,   1,  10, 1, "Safety weight when desperate"),
    ParamSpec("weight_desperate_food",       3,   1,  10, 1, "Food weight when desperate"),
    ParamSpec("weight_desperate_space",      1,   1,  10, 1, "Space weight when desperate"),
    ParamSpec("weight_balanced_safety",      2,   1,  10, 1, "Safety weight when balanced"),
    ParamSpec("weight_balanced_food",        2,   1,  10, 1, "Food weight when balanced"),
    ParamSpec("weight_balanced_space",       1,   1,  10, 1, "Space weight when balanced"),
    ParamSpec("weight_healthy_safety",       3,   1,  10, 1, "Safety weight when healthy"),
    ParamSpec("weight_healthy_food",         1,   1,  10, 1, "Food weight when healthy"),
    ParamSpec("weight_healthy_space",        2,   1,  10, 1, "Space weight when healthy"),
]
# fmt: on


def defaults_dict() -> dict[str, Any]:
    """Return a dict of {name: default} for all parameters."""
    return {p.name: p.default for p in PARAM_SPECS}


def spec_by_name() -> dict[str, ParamSpec]:
    """Return a dict of {name: ParamSpec} for easy lookup."""
    return {p.name: p for p in PARAM_SPECS}


def tunable_specs() -> list[ParamSpec]:
    """Return only specs that are tunable in the current phase."""
    return [p for p in PARAM_SPECS if p.tunable]


def full_params_dict(tuned: dict[str, Any]) -> dict[str, Any]:
    """Merge tuned params with defaults for non-tunable params (full 22-param dict)."""
    out = defaults_dict()
    out.update(tuned)
    return out
