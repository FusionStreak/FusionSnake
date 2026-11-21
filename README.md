# FusionSnake

FusionSnake is a [Battlesnake](https://play.battlesnake.com/) server written in Rust (Actix Web). One process hosts several **engines**. Each engine is its own snake with its own URL, appearance and stats. The server handles the Battlesnake API, runtime config, and recording every turn and game outcome to SQLite for analysis and ML training.

| Engine | Route | Game modes | Docs |
| --- | --- | --- | --- |
| `standard` | `/standard` | Standard, Duels | [`crates/fusion-engine-standard`](./crates/fusion-engine-standard/README.md) |

## Architecture

The server is a router plus shared bookkeeping. Engines are plain library crates that implement the [`SnakeEngine`](./crates/fusion-core/src/lib.rs) trait and know nothing about HTTP or SQLite.

```txt
fusion-snake (server binary, src/)
  ├── fusion-engine-standard   engine: move logic + tunable params
  │     ├── fusion-core        SnakeEngine trait, MoveFeatures, EngineInfo
  │     └── fusion-models      Battlesnake API types (serde + utoipa)
  ├── fusion-core
  └── fusion-models
```

At startup [`EngineRegistry`](./src/engine_registry.rs) primes each engine once and mounts it under `/<engine-name>`. Battlesnake picks the engine by URL: each engine is registered on play.battlesnake.com as a separate snake (e.g. `https://<host>/standard`). The server never infers the engine from the game type.

```txt
POST /standard/move
  │
  ▼
engine_routes::handle_move::<StandardEngine>    (src/engine_routes.rs)
  │
  ├─▶ StandardEngine::get_move()  ──▶  EngineMoveDecision { chosen_move, features }
  │
  ├─▶ ActiveGames[(engine, game_id)]  update last turn / max length   (in memory)
  ├─▶ TrainingLogger::log_turn()      fire-and-forget  ──▶ SQLite `turns`
  │
  ▼
{"move": "up"}
```

`/start` begins tracking the game in `ActiveGames`. `/end` works out win/loss/draw (`logic.rs`) and writes the `outcomes` row and the engine's `game_stats` row. Every row is tagged with the engine that played. In-progress games are keyed by `(engine, game_id)`, so two of our engines can play in the same game without clashing.

## Decision Algorithm

Move selection belongs to each engine. The server only:

1. Deserializes the Battlesnake request into `fusion-models` types.
2. Calls `SnakeEngine::get_move(game, turn, board, you)` on the engine that owns the route.
3. Returns `chosen_move` to Battlesnake and logs the returned `MoveFeatures` (board state, search depth, eval score, timing) as a training row.

For how an engine actually chooses its move, see its crate. The `standard` engine runs iterative-deepening paranoid minimax with alpha-beta pruning and Voronoi area evaluation: see [`crates/fusion-engine-standard/README.md`](./crates/fusion-engine-standard/README.md).

## Project Structure

```txt
src/                          server binary (fusion-snake)
  main.rs                     Actix app: middleware, stats/training/OpenAPI routes, startup
  engine_registry.rs          Engines this server runs; primes, validates names, mounts, documents
  engine_routes.rs            Generic per-engine routes /<engine>/{,start,move,end,config}
  logic.rs                    Shared game start/end handling (win/draw detection)
  stats.rs                    ActiveGames: in-memory tracking of in-progress games
  training.rs                 TrainingLogger: fire-and-forget SQLite writes
  db.rs                       SQLite schema, migrations, and queries
  responses.rs                utoipa-annotated response structs
  auth.rs                     X-API-Key validation (constant-time compare)
  logged_json.rs              JSON extractor that logs raw request bodies
crates/
  fusion-core/                SnakeEngine trait and shared engine types
  fusion-models/              Battlesnake API request/response models
  fusion-engine-standard/     Standard/Duels engine (search, evaluation, simulation, benches)
  fusion-arena/               Development-only match, tuning, and promotion tool
data/                         Runtime state (git-ignored): SQLite DB, engine params
```

Contributor and AI-agent conventions (commands, architecture rules, migrations, lint policy) are in [`AGENTS.md`](./AGENTS.md).

## Running

### Local development

Prerequisites: [rustup](https://www.rust-lang.org/tools/install). The toolchain is pinned in `rust-toolchain.toml` (currently 1.100.0), and rustup installs it automatically on first build.

```bash
cargo run
```

The server listens on `0.0.0.0:6666` by default.

### Docker Compose (production)

```bash
docker compose up -d
```

## API Endpoints

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

Engines live in their own crates under [`crates/`](./crates), each with its own README. The server handles routing, config endpoints, OpenAPI docs, game tracking, and stats/training logging; an engine only implements [`SnakeEngine`](./crates/fusion-core/src/lib.rs):

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
| `API_KEY` | _(unset)_ | Protects training and config endpoints. Unset means auth is disabled (dev only) |

Engines may define their own variables; see each engine's README (e.g. [standard](./crates/fusion-engine-standard/README.md#configuration)).

## Linting, Testing, and Benchmarks

These match CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTFLAGS="-D warnings" cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo bench --workspace          # Criterion benches live in engine crates
```

`clippy::pedantic` is enabled as a warning. All new code must pass without suppression.

## Tuning

Tune on a development machine, not the server hosting the live snake. [`fusion-arena`](./crates/fusion-arena/README.md) plays reproducible matches, tunes parameters with CMA-ES, and promotes a candidate only after a statistical test. Apply a promoted file with `POST /standard/config`. Each game outcome records a `params_hash` and `engine_version`, so ladder results can confirm the change.

See the [tuning guide](./crates/fusion-arena/TUNING.md) for how the system works and a step-by-step run.

## CI and Container Images

| Workflow | Trigger | What it does |
| --- | --- | --- |
| `ci.yml` | Every PR, push to `main` | Parallel `fmt`, `clippy`, `test`, `docs`, `deny` (licenses, bans, sources), `typos` and `machete` jobs; `ci-success` aggregates them (the required check) |
| `rust-canary.yml` | Weekly, manual | Clippy + tests on latest stable; opens a toolchain bump PR when they pass (not required) |
| `fusionsnake.yml` | Push/PR to `main` touching code | Builds the Docker image; publishes to GHCR on push |
| `cargo-deny-advisories.yml` | Dependency changes, weekly | Security advisories via `cargo deny` (not required; ignores in `deny.toml`) |
| `release-please.yml` | Push to `main` | Release PRs, version bumps and `CHANGELOG.md` from Conventional Commit messages |
| `release-docker.yml`, `release-check.yml` | Release published | Release image and checks |
| `benchmarks.yml` | Manual | Criterion benchmarks |

**Toolchain bumps:** when latest stable passes, `rust-canary` opens a PR updating `rust-toolchain.toml`, `rust-version` in the root `Cargo.toml`, and the `FROM rust:…` tag in `src/Dockerfile`. PRs it creates don't start CI automatically, so run `gh workflow run ci.yml --ref <branch>`. A failing `rust-canary` run means new lints or breakage need fixing first; apply that bump by hand with `.github/scripts/bump-rust-toolchain.sh <version>`.

```bash
docker pull ghcr.io/fusionstreak/fusionsnake:latest
```

## Credits

Based on [starter-snake-rust](https://github.com/BattlesnakeOfficial/starter-snake-rust) by [@BattlesnakeOfficial](https://github.com/BattlesnakeOfficial).
