# AGENTS.md

Guidance for AI coding agents working in this repository. Human-facing docs are in [README.md](./README.md). Engine internals are in each engine crate's README.

## What this is

FusionSnake is a Rust (edition 2024, Actix Web) [Battlesnake](https://docs.battlesnake.com/api) server that hosts several **engines**. Each engine is its own snake, served under `/<engine-name>` and registered separately on play.battlesnake.com. The server owns HTTP, auth, game tracking and SQLite persistence. Engines are plain library crates that only choose moves.

## Layout

| Path | Contents |
| --- | --- |
| `src/` | Server binary `fusion-snake`: routing, middleware, DB, stats/training endpoints |
| `src/engine_registry.rs` | The list of engines; primes, validates names, mounts routes, adds OpenAPI schemas |
| `src/engine_routes.rs` | Generic `/<engine>/{,start,move,end,config}` handlers over `E: SnakeEngine` |
| `src/db.rs` | SQLite schema, idempotent startup migrations, all queries |
| `crates/fusion-core/` | `SnakeEngine` trait, `EngineInfo`, `EngineMoveDecision`, `MoveFeatures` |
| `crates/fusion-models/` | Battlesnake API types (`GameState`, `Board`, …), `GameType`, `GameSource` |
| `crates/fusion-engine-standard/` | `standard` engine (Standard + Duels): search, evaluation, simulation, benches. See its [README](./crates/fusion-engine-standard/README.md) |
| `data/` | Runtime state (git-ignored). Never commit |
| `scratch/` | Personal notes (git-ignored) |

Dependency direction: `fusion-snake` → engine crates → `fusion-core` → `fusion-models`. Engine crates must not depend on actix, sqlx or the server crate.

## Commands

Run these before considering work done. CI does **not** lint or test ordinary PRs; it only builds the Docker image.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --locked --workspace
cargo run                                  # server on 0.0.0.0:6666
cargo bench -p fusion-engine-standard      # after touching search/evaluation/simulation
```

Manual smoke test: run the server with throwaway state, e.g. `DATABASE_URL=sqlite:/tmp/fs/test.db STANDARD_ENGINE_PARAMS_FILE=/tmp/fs/params.json cargo run`. Then `POST` a Battlesnake `GameState` JSON to `/standard/start`, `/standard/move` and `/standard/end`. A `GameState` must include `game.ruleset.settings.royale` and `.squad`, or deserialization returns 400. Use `127.0.0.1` rather than `localhost` if another service might be bound on IPv6.

## Architecture rules

- **The engine is chosen by URL, never inferred.** Don't add game-type-based dispatch. A new game mode gets a new engine, which gets its own route and its own Battlesnake registration.
- **Engines stay transport-agnostic.** An engine implements `SnakeEngine` (`prime`, `name`, `info`, `get_move`, `get_params`/`set_params`/`reset_params`). Routing, config endpoints, auth, OpenAPI, game tracking and DB logging all live in `src/` and are shared by every engine. Don't add per-engine HTTP handlers.
- **Adding an engine:**
  1. Create a crate under `crates/` and implement `SnakeEngine`.
  2. In `EngineRegistry`, add a field and wire it into `prime()`, `names()`, `configure()` and `merge_openapi()`.
  3. Add the crate to the workspace `members` and as a dependency of the root package.
  4. Write a README for the crate.
  5. Add a row to the engine table in the root README.
- **Engine names** are route prefixes and DB keys. They must be lowercase `[a-z0-9_-]`, unique, and not `stats`, `training` or `api-doc` (checked at startup). Never rename an existing engine: its stats and training rows are keyed by the name.
- **Everything is attributed to an engine.** `turns`, `outcomes` and `game_stats` carry an `engine` column. `ActiveGames` is keyed by `(engine, game_id)`. New per-game data must follow the same pattern. New stats/training endpoints take an optional `?engine=` filter.
- **Don't block the move response on I/O.** DB writes go through `TrainingLogger`, which `tokio::spawn`s them (fire-and-forget).
- **`MoveFeatures` is shared by all engines.** Changing it means changing the `turns` table, `db::insert_turn`, `responses::TurnRecord` and every engine.

## Conventions

- **Lints:** `clippy::pedantic` is enabled workspace-wide and CI-equivalent runs use `-D warnings`. Fix lints rather than suppressing them. If an `#[allow(...)]` is truly needed, scope it to the item and say why in a comment.
- **Integer conversions:** prefer `From`/`TryFrom` and `.cast_signed()`/`.cast_unsigned()`. Use `as` only with a targeted `#[allow(clippy::cast_*)]`.
- **Coordinates:** `i8`, origin `(0, 0)` bottom-left, `y` increases upward (Battlesnake convention).
- **Tunable constants:** the `standard` engine's scoring constants live only in `HeuristicParams`. Add a field, give it a `Default` value, and add `validate()` bounds. Don't hard-code new constants in `evaluation.rs` or `search.rs`.
- **Database changes:** all schema changes go in `db.rs`'s `run_migrations()`. Migrations must be idempotent: guard with `has_column()`. Use `rebuild_table()` for changes `ALTER TABLE` can't express, such as primary keys. They also must preserve existing production data. There is no migration framework and no down-migrations.
- **SQL:** `sqlx` 0.9 rejects non-literal SQL. Use parameter binding for values. Wrap a dynamic string in `sqlx::AssertSqlSafe` only when it is built purely from compile-time constants.
- **OpenAPI:** every endpoint needs `#[utoipa::path(...)]` and an entry in `ApiDoc` in `main.rs`, and every response body needs a `ToSchema` struct in `responses.rs`. Engine params schemas are added via `EngineRegistry::merge_openapi()`.
- **Commits:** `release-please` builds `CHANGELOG.md` and version bumps from commits on `main`. Use [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `chore:`, `docs:`, `refactor:`, …) for anything that lands on `main`, including squash-merge PR titles.
- **Docs:** when you change routes, env vars, the engine contract or the layout, update `README.md`, the affected engine README and this file in the same change.
