# FusionSnake - Battlesnake AI Agent Instructions

## Architecture Overview

Two components that form a closed ML optimization loop:

- **`fusionsnake/`** — Rust/Actix Web server that plays Battlesnake and records training data to SQLite
- **`trainer/`** — Python ML pipeline (Flask + Optuna + GBM) that reads training data, optimizes heuristic parameters, and pushes them back to the running snake via `POST /config`; exposes `POST /train` as an HTTP trigger

### Request Flow

```
Battlesnake engine → POST /move → logic::get_move(&params) → MoveResponse
                                         ↓ (fire-and-forget)
                               training::TrainingLogger → SQLite (turns + outcomes)

Every 24 h (Tokio interval task in main.rs):
  FusionSnake server → POST /train → trainer/server.py → 202 Accepted
                                           ↓ (background thread)
                                     trainer/train.py (Bayesian optimisation)
                                           ↓
                                POST /config → snake HeuristicParams updated
```

### Source Files

| File                  | Role                                                                                                                |
| --------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `main.rs`             | Actix server, all route registrations, `SecurityHeaders` middleware, 24 h trainer trigger (`spawn_trainer_trigger`) |
| `logic.rs`            | Decision engine: `get_move()` returns `(MoveResponse, MoveFeatures)`                                                |
| `heuristic_params.rs` | All tunable scoring constants (`HeuristicParams`), loads from `/data/params.json`                                   |
| `game_objects.rs`     | Serde structs matching Battlesnake API spec; fields use `pub(super)`                                                |
| `db.rs`               | SQLite schema + async queries (`turns`, `outcomes`, `game_stats` tables)                                            |
| `training.rs`         | `TrainingLogger` — cheap clone handle, fire-and-forget Tokio spawns                                                 |
| `auth.rs`             | `ApiKey` extractor — validates `X-API-Key` header via constant-time compare                                         |
| `responses.rs`        | `utoipa`-annotated response structs for OpenAPI generation                                                          |
| `stats.rs`            | `ActiveGames` (Arc<Mutex>) tracking in-progress games                                                               |

**Trainer (`trainer/`):**

| File            | Role                                                                         |
| --------------- | ---------------------------------------------------------------------------- |
| `server.py`     | Flask web server; `POST /train` triggers pipeline in a background thread     |
| `train.py`      | Full 5-step ML pipeline (load → model → optimise → report → push)            |
| `entrypoint.sh` | Waits for snake readiness, runs one initial training pass, then starts Flask |

## Decision Algorithm (`logic.rs`)

`get_move()` scores all 4 directions using three metrics per `Move`:

- **`safety_score`** (`u8`, starts at `u8::MAX`): zero = dead cell; penalties applied for hazards, edge proximity, head-to-head, body proximity, and flood-fill trapping
- **`desirability_score`** (`u8`): `food_desirability_base - manhattan_distance_to_food`
- **`space_score`** (`u16`): BFS flood fill from candidate cell counting reachable open tiles

`choose_best_move_weighted()` picks the highest `(safety * sw) + (desirability * fw) + (space * xw)`, filtered to `safety_score > 0` first. Falls back to least-bad if all moves are lethal.

**Three health-tier weight regimes** (all values in `HeuristicParams`):

- `health < health_threshold_desperate` → desperate weights (high food priority)
- `health_threshold_desperate ≤ health < health_threshold_balanced` → balanced
- `health ≥ health_threshold_balanced` → healthy (high safety priority)

**Tail movement**: a snake that just ate has identical last two body segments — `tail_will_move()` detects this to avoid falsely blocking the vacating tail cell.

## Runtime-Tunable Parameters

All scoring constants live in `HeuristicParams` (`heuristic_params.rs`). On startup the server loads `/data/params.json` (override with `PARAMS_FILE`), falling back to `HeuristicParams::default()`. Stored in `Arc<RwLock<HeuristicParams>>` (aliased as `SharedParams`) and shared via Actix `Data<>`.

To change scoring behavior: call `POST /config` (requires `X-API-Key`) or edit `params.json`. Do **not** hardcode new constants — add them to `HeuristicParams` and its `Default` impl.

## API Endpoints

| Route                                                            | Auth    | Description                        |
| ---------------------------------------------------------------- | ------- | ---------------------------------- |
| `GET /`                                                          | —       | Snake metadata (color, head, tail) |
| `POST /start`, `/move`, `/end`                                   | —       | Battlesnake game lifecycle         |
| `GET /stats`, `/stats/history`                                   | —       | Aggregate + per-game statistics    |
| `GET /training/turns`, `/training/outcomes`, `/training/summary` | API key | ML training data                   |
| `GET /config`                                                    | API key | Current `HeuristicParams`          |
| `POST /config`                                                   | API key | Update params (persists to disk)   |
| `POST /config/reset`                                             | API key | Revert to defaults                 |
| `GET /api-doc/openapi.json`                                      | —       | OpenAPI spec                       |

## Environment Variables

| Variable       | Default                         | Description                                                            |
| -------------- | ------------------------------- | ---------------------------------------------------------------------- |
| `PORT`         | `6666`                          | HTTP listen port                                                       |
| `RUST_LOG`     | `info`                          | Log level (uses `tracing_subscriber` with JSON format)                 |
| `DATABASE_URL` | `sqlite:./data/fusion_snake.db` | SQLite path (WAL mode)                                                 |
| `API_KEY`      | _(unset = auth disabled)_       | Protects training/config endpoints                                     |
| `PARAMS_FILE`  | `./data/params.json`            | Heuristic params persistence path                                      |
| `TRAINER_URL`  | _(unset = trigger disabled)_    | Base URL of the trainer server; enables the 24 h `POST /train` trigger |

**Trainer environment variables** (set in the `trainer` container):

| Variable            | Default                 | Description                                            |
| ------------------- | ----------------------- | ------------------------------------------------------ |
| `TRAINER_PORT`      | `5050`                  | Port the Flask trigger server listens on               |
| `SNAKE_URL`         | `http://localhost:6666` | Base URL for fetching training data and pushing params |
| `MIN_TRAINING_ROWS` | `500`                   | Minimum rows before optimisation runs                  |
| `MIN_IMPROVEMENT`   | `0.02`                  | Win-rate Δ required before pushing new params          |
| `N_TRIALS`          | `200`                   | Number of Optuna trials per run                        |

## Development Workflows

```bash
# Local development
cargo run                       # Server on 0.0.0.0:6666
cargo clippy --all-targets --all-features --locked -- -D warnings  # CI lint
cargo fmt --all                 # Formatting

# Production
docker compose up -d            # Starts snek + snake-trainer containers

# Python trainer (standalone — runs the pipeline once then exits)
cd trainer && pip install -r requirements.txt
SNAKE_URL=http://localhost:6666 API_KEY=... python train.py
python train.py --dry-run       # Optimise but do not push params

# Python trainer server (standalone)
SNAKE_URL=http://localhost:6666 API_KEY=... python trainer/server.py
# Then trigger a run: curl -X POST http://localhost:5050/train
```

## Key Conventions

- **Coordinate system**: `i8` coords, origin `(0,0)` at bottom-left, y increases upward. Use `.cast_signed()` / `.cast_unsigned()` (Rust 2024 edition) — not `as i8`.
- **`game_objects.rs` fields**: always `pub(super)` — accessible within the crate only via the parent module.
- **All DB writes are fire-and-forget**: `TrainingLogger` spawns a Tokio task; never block the HTTP response path on I/O.
- **Rust edition 2024**: `let-else` chains and `.cast_signed()` are used throughout. Do not introduce `as` casts for integer conversions.
- **`clippy::pedantic`** is enabled as a warning — new code must pass without suppression unless a specific `#[allow]` with a comment is justified.
- **OpenAPI**: every new endpoint and response struct must have a `#[utoipa::path(...)]` annotation and be registered in `ApiDoc`.
