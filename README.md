# FusionSnake

FusionSnake is a [Battlesnake](https://play.battlesnake.com/) AI written in Rust (Actix Web). It uses iterative-deepening paranoid minimax search with alpha-beta pruning to choose moves, and records game data to SQLite for analysis.

## Architecture

```
Battlesnake engine  -->  POST /move  -->  logic::get_move()
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

```
src/
  main.rs              — Actix server, route registration, SecurityHeaders middleware
  lib.rs               — Public module re-exports for benchmarks
  logic.rs             — Decision engine: get_move() returns (MoveResponse, MoveFeatures)
  simulation.rs        — SimBoard: compact cloneable game state for tree search
  evaluation.rs        — Static board evaluation (Voronoi, health, food, length, aggression)
  search.rs            — Iterative-deepening paranoid minimax with alpha-beta pruning
  heuristic_params.rs  — All tunable scoring constants (loaded from params.json)
  game_objects.rs      — Serde structs matching the Battlesnake API spec
  db.rs                — SQLite schema and async queries
  training.rs          — TrainingLogger: fire-and-forget game data recording
  auth.rs              — X-API-Key header extractor
  responses.rs         — utoipa-annotated response structs for OpenAPI
  stats.rs             — ActiveGames tracking (Arc<Mutex>)
benches/
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
|---|---|---|
| `GET /` | — | Snake metadata (color, head, tail) |
| `POST /start`, `/move`, `/end` | — | Battlesnake game lifecycle |
| `GET /stats`, `/stats/history` | — | Aggregate and per-game statistics |
| `GET /training/turns`, `/training/outcomes`, `/training/summary` | API key | Game data and analytics |
| `GET /config` | API key | Current heuristic parameters |
| `POST /config` | API key | Update parameters (persists to disk) |
| `POST /config/reset` | API key | Revert to defaults |
| `GET /api-doc/openapi.json` | — | OpenAPI spec |

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `PORT` | `6666` | HTTP listen port |
| `RUST_LOG` | `info` | Log level |
| `DATABASE_URL` | `sqlite:./data/fusion_snake.db` | SQLite path (WAL mode) |
| `API_KEY` | _(unset)_ | Protects data and config endpoints |
| `PARAMS_FILE` | `./data/params.json` | Heuristic params persistence path |

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
