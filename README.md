# FusionSnake

FusionSnake is a [Battlesnake](https://play.battlesnake.com/) AI written in Rust (Actix Web). It uses iterative-deepening paranoid minimax search with alpha-beta pruning to choose moves, and records game data to SQLite for analysis.

## Architecture

Each engine is served as its own Battlesnake under `/<engine>` (currently just `standard`) and registered separately on play.battlesnake.com, e.g. `https://<host>/standard`.

```txt
Battlesnake engine  -->  POST /standard/move  -->  StandardEngine::get_move()
                                               |
                                    SimBoard::from_game_state()
                                               |
                                    search::search() (iterative deepening
                                      paranoid minimax + alpha-beta)
                                               |
                                          MoveResponse
                                               |
                                    TrainingLogger  -->  SQLite
```

### Decision Algorithm

1. **`SimBoard::from_game_state()`** converts the Battlesnake API types into a compact, cloneable board representation.
2. **`search::search()`** runs iterative-deepening paranoid minimax with alpha-beta pruning:
   - Starts at depth 1, increases until the time budget is exhausted.
   - Enemies respond with the move that minimises our evaluation (paranoid assumption, computed independently per enemy — 4×N branching, not 4^N).
   - Time is checked every 512 nodes; safe deadline is set to 60% of the budget.
3. **`evaluation::evaluate()`** scores leaf/terminal positions using Voronoi area control, health, food proximity, length advantage, and aggression bonuses.

## Project Structure

```txt
src/
  main.rs              — Actix server, route registration, SecurityHeaders middleware
  lib.rs               — Public module re-exports
  engine_registry.rs   — The engines this server runs; primes and mounts each one
  engine_routes.rs     — Generic per-engine routes: /<engine>/{start,move,end,config}
  logged_json.rs       — JSON extractor that logs raw request bodies
  logic.rs             — Shared game start/end handling (win/draw detection)
  simulation.rs        — SimBoard: compact cloneable game state for tree search
  evaluation.rs        — Static board evaluation (Voronoi, health, food, length, aggression)
  search.rs            — Iterative-deepening paranoid minimax with alpha-beta pruning
  heuristic_params.rs  — Re-exported standard-engine heuristic params and shared state
  game_objects.rs      — Serde structs matching the Battlesnake API spec
  db.rs                — SQLite schema and async queries
  training.rs          — TrainingLogger: fire-and-forget game data recording
  auth.rs              — X-API-Key header extractor
  responses.rs         — utoipa-annotated response structs for OpenAPI
  stats.rs             — ActiveGames tracking (Arc<Mutex>)
crates/fusion-engine-standard/benches/
  common/mod.rs        — Shared helpers for building test boards
  simulation_bench.rs  — SimBoard::apply_moves() throughput benchmarks
  evaluation_bench.rs  — evaluate() scoring benchmarks
  search_bench.rs      — End-to-end search benchmarks at various depths
```

## Running

### Local development

Prerequisites: [Rust](https://www.rust-lang.org/tools/install) 1.92+ (pinned via `rust-toolchain.toml`).

```bash
cargo run
```

The server listens on `0.0.0.0:6666` by default.

### Docker Compose (production)

```bash
docker compose up -d
```

## API Endpoints

| Route | Auth | Description |
| --- | --- | --- |
Per-engine routes (`<engine>` is the engine name, e.g. `standard`):

| Route | Auth | Description |
| --- | --- | --- |
| `GET /<engine>` | — | Snake metadata (color, head, tail) |
| `POST /<engine>/start`, `/<engine>/move`, `/<engine>/end` | — | Battlesnake game lifecycle |
| `GET /<engine>/config` | API key | Current engine parameters |
| `POST /<engine>/config` | API key | Update engine parameters (persists to disk) |
| `POST /<engine>/config/reset` | API key | Revert engine parameters to defaults |

Server-wide routes:

| Route | Auth | Description |
| --- | --- | --- |
| `GET /stats`, `/stats/history` | — | Aggregate and per-game statistics |
| `GET /training/turns`, `/training/outcomes`, `/training/summary` | API key | Game data and analytics |
| `GET /api-doc/openapi.json` | — | OpenAPI spec |

Stats and training endpoints accept an optional `?engine=<name>` filter. Without it, `/stats` sums across all engines.

## Adding a New Engine

Engines live in their own crates under [`crates/`](./crates). The server handles routing, config endpoints, OpenAPI docs, game tracking, and stats/training logging; an engine only implements [`SnakeEngine`](./crates/fusion-core/src/lib.rs):

- `prime()`: construct/load engine state (params, models, caches, etc.)
- `name()`: stable identifier — the engine's route prefix (`/<name>`) and the `engine` value on its stats and training rows. Must be lowercase `[a-z0-9_-]` and must not be `stats`, `training`, or `api-doc`.
- `info()`: snake appearance (color, head, tail)
- `get_move(...)`: returns `EngineMoveDecision`
- `get_params()/set_params()/reset_params()`: runtime config lifecycle. `Params` must implement `Serialize`, `Deserialize` and `utoipa::ToSchema`.

Then register it in [`EngineRegistry`](./src/engine_registry.rs): add a field, prime it in `prime()`, list it in `names()`, mount it in `configure()`, and add its params schema in `merge_openapi()`. Finally, register a new snake on play.battlesnake.com pointing at `https://<host>/<name>`.

## Environment Variables

| Variable | Default | Description |
| --- | --- | --- |
| `PORT` | `6666` | HTTP listen port |
| `RUST_LOG` | `info` | Log level |
| `DATABASE_URL` | `sqlite:./data/fusion_snake.db` | SQLite path (WAL mode) |
| `API_KEY` | _(unset)_ | Protects data and config endpoints |
| `STANDARD_ENGINE_PARAMS_FILE` | `./data/engines/standard/params.json` | Standard-engine heuristic params path |
| `PARAMS_FILE` | _(legacy override)_ | Backward-compatible alias for standard-engine params path |

## Linting, Formatting, and Benchmarks

```bash
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo fmt --all
cargo bench
```

`clippy::pedantic` is enabled as a warning. All new code must pass without suppression.

## CI and Container Images

GitHub Actions run Clippy on every push and PR to `main`, and build and publish Docker images to GHCR on `main` and on version tags.

```bash
docker pull ghcr.io/fusionstreak/fusionsnake:latest
```

## Credits

Based on [starter-snake-rust](https://github.com/BattlesnakeOfficial/starter-snake-rust) by [@BattlesnakeOfficial](https://github.com/BattlesnakeOfficial).
