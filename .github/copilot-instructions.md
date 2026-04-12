# FusionSnake - Battlesnake AI Agent Instructions

## Architecture Overview

Single Rust service that plays Battlesnake using iterative-deepening paranoid minimax search with alpha-beta pruning. All source lives in `src/` with benchmarks in `benches/`.

### Request Flow

```
Battlesnake engine → POST /move → logic::get_move(&params)
                                         ↓
                        SimBoard::from_game_state() → search::search()
                        (iterative deepening minimax with alpha-beta)
                                         ↓
                                    MoveResponse
                                         ↓ (fire-and-forget)
                               training::TrainingLogger → SQLite (turns + outcomes)
```

### Source Files (`src/`)

| File                  | Role                                                                                                               |
| --------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `main.rs`             | Actix server, all route registrations, `SecurityHeaders` middleware                                                |
| `lib.rs`              | Public module re-exports for benchmarks (`evaluation`, `game_objects`, `heuristic_params`, `search`, `simulation`) |
| `logic.rs`            | Decision engine: `get_move()` returns `(MoveResponse, MoveFeatures)`                                               |
| `simulation.rs`       | `SimBoard` — compact cloneable game state for tree search, `apply_moves()` with full Battlesnake rule resolution   |
| `evaluation.rs`       | Static board evaluation: Voronoi area control, health, food proximity, length advantage, aggression bonus          |
| `search.rs`           | Iterative-deepening paranoid minimax with alpha-beta pruning and time management                                   |
| `heuristic_params.rs` | All tunable scoring constants (`HeuristicParams`), loads from `/data/params.json`                                  |
| `game_objects.rs`     | Serde structs matching Battlesnake API spec; fields use `pub(crate)`                                               |
| `db.rs`               | SQLite schema + async queries (`turns`, `outcomes`, `game_stats` tables)                                           |
| `training.rs`         | `TrainingLogger` — cheap clone handle, fire-and-forget Tokio spawns for game data recording                        |
| `auth.rs`             | `ApiKey` extractor — validates `X-API-Key` header via constant-time compare                                        |
| `responses.rs`        | `utoipa`-annotated response structs for OpenAPI generation                                                         |
| `stats.rs`            | `ActiveGames` (Arc<Mutex>) tracking in-progress games                                                              |

### Benchmarks (`benches/`)

| File                  | Role                                            |
| --------------------- | ----------------------------------------------- |
| `common/mod.rs`       | Shared helpers for building test boards         |
| `simulation_bench.rs` | `SimBoard::apply_moves()` throughput benchmarks |
| `evaluation_bench.rs` | `evaluate()` scoring benchmarks                 |
| `search_bench.rs`     | End-to-end search benchmarks at various depths  |

## Decision Algorithm

`get_move()` in `logic.rs` builds a `SimBoard` from the API game state, then delegates to `search::search()`:

1. **`SimBoard::from_game_state()`** converts the Battlesnake API types into a compact, `Clone`-able board representation with `VecDeque<Coord>` bodies.
2. **`search::search()`** runs iterative-deepening paranoid minimax with alpha-beta pruning:
   - Starts at depth 1, increases until the time budget is exhausted.
   - At each root move, enemies respond with the move that _minimises_ our evaluation (paranoid assumption, computed independently per enemy → 4×N branching, not 4^N).
   - Time is checked every 512 nodes; safe deadline is set to 60% of the configured budget.
3. **`evaluation::evaluate()`** scores leaf/terminal positions:
   - **Voronoi area control** (simultaneous BFS) — tiles we can reach before any opponent.
   - **Health** — weighted more heavily when below 20.
   - **Food proximity** — inverted Manhattan distance to nearest food, 3× urgency when health < 30.
   - **Length advantage** — difference vs longest enemy.
   - **Aggression bonus** — awarded when adjacent to a shorter enemy's head (kill opportunity).

## Runtime-Tunable Parameters

All scoring constants live in `HeuristicParams` (`heuristic_params.rs`). On startup the server loads `/data/params.json` (override with `PARAMS_FILE`), falling back to `HeuristicParams::default()`. Stored in `Arc<RwLock<HeuristicParams>>` (aliased as `SharedParams`) and shared via Actix `Data<>`.

To change scoring behavior: call `POST /config` (requires `X-API-Key`) or edit `params.json`. Do **not** hardcode new constants — add them to `HeuristicParams` and its `Default` impl.

## API Endpoints

| Route                                                            | Auth    | Description                        |
| ---------------------------------------------------------------- | ------- | ---------------------------------- |
| `GET /`                                                          | —       | Snake metadata (color, head, tail) |
| `POST /start`, `/move`, `/end`                                   | —       | Battlesnake game lifecycle         |
| `GET /stats`, `/stats/history`                                   | —       | Aggregate + per-game statistics    |
| `GET /training/turns`, `/training/outcomes`, `/training/summary` | API key | Game data / analytics              |
| `GET /config`                                                    | API key | Current `HeuristicParams`          |
| `POST /config`                                                   | API key | Update params (persists to disk)   |
| `POST /config/reset`                                             | API key | Revert to defaults                 |
| `GET /api-doc/openapi.json`                                      | —       | OpenAPI spec                       |

## Environment Variables

| Variable       | Default                         | Description                                            |
| -------------- | ------------------------------- | ------------------------------------------------------ |
| `PORT`         | `6666`                          | HTTP listen port                                       |
| `RUST_LOG`     | `info`                          | Log level (uses `tracing_subscriber` with JSON format) |
| `DATABASE_URL` | `sqlite:./data/fusion_snake.db` | SQLite path (WAL mode)                                 |
| `API_KEY`      | _(unset = auth disabled)_       | Protects data/config endpoints                         |
| `PARAMS_FILE`  | `./data/params.json`            | Heuristic params persistence path                      |

## Development Workflows

```bash
# Local development
cargo run                       # Server on 0.0.0.0:6666
cargo clippy --all-targets --all-features --locked -- -D warnings  # CI lint
cargo fmt --all                 # Formatting
cargo bench                     # Run Criterion benchmarks

# Production
docker compose up -d            # Starts snek container
```

## Key Conventions

- **Coordinate system**: `i8` coords, origin `(0,0)` at bottom-left, y increases upward. Use `.cast_signed()` / `.cast_unsigned()` (Rust 2024 edition) — not `as i8`.
- **`game_objects.rs` fields**: always `pub(crate)` — accessible within the crate only via the parent module.
- **All DB writes are fire-and-forget**: `TrainingLogger` spawns a Tokio task; never block the HTTP response path on I/O.
- **Rust edition 2024**: `let-else` chains and `.cast_signed()` are used throughout. Do not introduce `as` casts for integer conversions.
- **`clippy::pedantic`** is enabled as a warning — new code must pass without suppression unless a specific `#[allow]` with a comment is justified.
- **OpenAPI**: every new endpoint and response struct must have a `#[utoipa::path(...)]` annotation and be registered in `ApiDoc`.
