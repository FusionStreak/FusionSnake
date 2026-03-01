"""
Data loader: fetches training data from the Battlesnake REST API.

Paginates through /training/turns and /training/outcomes, joins them
on game_id, and returns a single pandas DataFrame ready for modelling.
"""

import logging
import os
from pathlib import Path

import pandas as pd
import requests

logger = logging.getLogger(__name__)

SNAKE_URL = os.environ.get("SNAKE_URL", "http://localhost:6666")
API_KEY = os.environ.get("API_KEY", "")
CACHE_DIR = Path(os.environ.get("CACHE_DIR", "/data/trainer_cache"))


def _headers() -> dict[str, str]:
    h: dict[str, str] = {"Accept": "application/json"}
    if API_KEY:
        h["X-API-Key"] = API_KEY
    return h


def _paginate(endpoint: str, extra_params: dict | None = None) -> list[dict]:
    """Fetch all records from a paginated Battlesnake API endpoint."""
    url = f"{SNAKE_URL}{endpoint}"
    params = dict(extra_params or {})
    params.setdefault("limit", 1000)
    params.setdefault("offset", 0)
    all_records: list[dict] = []

    while True:
        logger.info("GET %s  offset=%s", url, params["offset"])
        resp = requests.get(url, params=params, headers=_headers(), timeout=30)
        resp.raise_for_status()
        body = resp.json()
        data = body.get("data", [])
        if not data:
            break
        all_records.extend(data)
        if len(data) < int(params["limit"]):
            break
        params["offset"] = int(params["offset"]) + len(data)

    return all_records


def fetch_turns(game_id: str | None = None) -> pd.DataFrame:
    """Fetch all turn-level feature records."""
    extra = {}
    if game_id:
        extra["game_id"] = game_id
    records = _paginate("/training/turns", extra)
    if not records:
        return pd.DataFrame()
    return pd.DataFrame(records)


def fetch_outcomes() -> pd.DataFrame:
    """Fetch all game outcome records."""
    records = _paginate("/training/outcomes")
    if not records:
        return pd.DataFrame()
    return pd.DataFrame(records)


def load_training_data() -> pd.DataFrame:
    """
    Fetch turns + outcomes, join on game_id, and return a modelling-ready
    DataFrame.  Each row is one turn with a ``won`` label column.

    Results are cached to parquet; the cache is invalidated when the remote
    data has grown (checked via /training/summary).
    """
    CACHE_DIR.mkdir(parents=True, exist_ok=True)
    cache_file = CACHE_DIR / "training_data.parquet"
    meta_file = CACHE_DIR / "meta.txt"

    # Quick size check via summary endpoint
    try:
        summary = requests.get(
            f"{SNAKE_URL}/training/summary", headers=_headers(), timeout=10
        ).json()
        remote_turns = summary.get("total_turns", 0)
    except Exception:
        remote_turns = -1  # force refresh

    cached_turns = 0
    if meta_file.exists():
        try:
            cached_turns = int(meta_file.read_text().strip())
        except ValueError:
            cached_turns = 0

    if cache_file.exists() and cached_turns >= remote_turns > 0:
        logger.info(
            "Using cached training data (%d turns, %d remote)",
            cached_turns,
            remote_turns,
        )
        return pd.read_parquet(cache_file)

    logger.info("Fetching fresh training data from %s …", SNAKE_URL)
    turns_df = fetch_turns()
    outcomes_df = fetch_outcomes()

    if turns_df.empty or outcomes_df.empty:
        logger.warning("No training data available")
        return pd.DataFrame()

    # Join: keep only turns whose game has a recorded outcome
    outcomes_df = outcomes_df.rename(
        columns={
            "total_turns": "game_total_turns",
            "total_food_eaten": "game_food_eaten",
        }
    )
    df = turns_df.merge(
        outcomes_df[
            ["game_id", "won", "is_draw", "game_total_turns", "game_food_eaten"]
        ],
        on="game_id",
        how="inner",
    )

    # Convert boolean-ish columns
    for col in ("won", "is_draw", "target_food_contested"):
        if col in df.columns:
            df[col] = df[col].astype(bool)

    # Cache
    df.to_parquet(cache_file, index=False)
    meta_file.write_text(str(len(df)))
    logger.info("Cached %d training rows", len(df))

    return df
