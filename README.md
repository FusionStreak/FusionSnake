# FusionSnake

FusionSnake is a [Battlesnake](https://play.battlesnake.com/) AI written in Rust (Actix Web), paired with a Python ML trainer that continuously optimises its heuristic parameters using Bayesian optimisation. The two components form a closed loop: the snake plays games and records data to SQLite; the trainer reads that data, fits a surrogate model, and pushes improved parameters back to the running snake.

## Architecture

```
Battlesnake engine  -->  POST /move  -->  logic::get_move()  -->  MoveResponse
                                               |
                                        TrainingLogger  -->  SQLite

Every 24 h (Tokio interval):
  FusionSnake  -->  POST /train  -->  trainer/server.py
                                            |
                                      train.py pipeline
                                            |
                                      POST /config  -->  HeuristicParams updated
```

## Project Structure

```
fusionsnake/src/
  main.rs              — Actix server, route registration, 24 h trainer trigger
  logic.rs             — Decision engine: scores all 4 moves and picks the best
  heuristic_params.rs  — All tunable scoring constants (loaded from params.json)
  game_objects.rs      — Serde structs matching the Battlesnake API spec
  db.rs                — SQLite schema and async queries
  training.rs          — TrainingLogger: fire-and-forget Tokio writes
  auth.rs              — X-API-Key header extractor
  responses.rs         — utoipa-annotated response structs for OpenAPI
  stats.rs             — ActiveGames tracking (Arc<Mutex>)

trainer/
  train.py             — Full 5-step ML pipeline (load, model, optimise, report, push)
  server.py            — Flask server; POST /train triggers the pipeline
  data_loader.py       — Fetches training data from the snake's REST API
  model.py             — GBM surrogate win-rate model
  optimizer.py         — Optuna Bayesian optimisation
  param_schema.py      — Parameter bounds and defaults
  report.py            — PDF report generation
```

## Running

### Local development

Prerequisites: [Rust](https://www.rust-lang.org/tools/install) 1.92+ (pinned via `rust-toolchain.toml`).

```bash
cargo run
```

The server listens on `0.0.0.0:6666` by default.

To run the Python trainer once against a local snake:

```bash
cd trainer
pip install -r requirements.txt
SNAKE_URL=http://localhost:6666 API_KEY=<key> python train.py

# Optimise without pushing params:
python train.py --dry-run
```

### Docker Compose (production)

```bash
docker compose up -d
```

This starts two containers:

- `snek` — the Rust snake server on port 6666
- `snake-trainer` — the Python trainer; triggers a training run on startup and exposes `POST /train` on port 5050

## API Endpoints

| Route | Auth | Description |
|---|---|---|
| `GET /` | — | Snake metadata (color, head, tail) |
| `POST /start`, `/move`, `/end` | — | Battlesnake game lifecycle |
| `GET /stats`, `/stats/history` | — | Aggregate and per-game statistics |
| `GET /training/turns`, `/training/outcomes`, `/training/summary` | API key | ML training data |
| `GET /config` | API key | Current heuristic parameters |
| `POST /config` | API key | Update parameters (persists to disk) |
| `POST /config/reset` | API key | Revert to defaults |
| `GET /api-doc/openapi.json` | — | OpenAPI spec |

## Environment Variables

### Snake

| Variable | Default | Description |
|---|---|---|
| `PORT` | `6666` | HTTP listen port |
| `RUST_LOG` | `info` | Log level |
| `DATABASE_URL` | `sqlite:./data/fusion_snake.db` | SQLite path |
| `API_KEY` | _(unset)_ | Protects training and config endpoints |
| `PARAMS_FILE` | `./data/params.json` | Heuristic params persistence path |
| `TRAINER_URL` | _(unset)_ | Trainer base URL; enables the 24 h trigger |

### Trainer

| Variable | Default | Description |
|---|---|---|
| `SNAKE_URL` | `http://localhost:6666` | Snake base URL |
| `API_KEY` | _(unset)_ | Passed in the X-API-Key header |
| `TRAINER_PORT` | `5050` | Flask server port |
| `MIN_TRAINING_ROWS` | `500` | Minimum rows before optimisation runs |
| `MIN_IMPROVEMENT` | `0.02` | Win-rate delta required before pushing params |
| `N_TRIALS` | `200` | Number of Optuna trials per run |

## Linting and Formatting

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all
```

`clippy::pedantic` is enabled as a warning. All new code must pass without suppression.

## CI and Container Images

GitHub Actions run Clippy on every push and PR to `main`, and build and publish Docker images to GHCR on `main` and on version tags.

```bash
docker pull ghcr.io/fusionstreak/fusionsnake:latest
docker pull ghcr.io/fusionstreak/fusionsnake-trainer:latest
```

## Credits

Based on [starter-snake-rust](https://github.com/BattlesnakeOfficial/starter-snake-rust) by [@BattlesnakeOfficial](https://github.com/BattlesnakeOfficial).
