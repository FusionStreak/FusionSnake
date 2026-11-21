# fusion-engine-standard

The `standard` engine for FusionSnake. It plays the Battlesnake **Standard** and **Duels** modes using iterative-deepening paranoid minimax with alpha-beta pruning and a Voronoi-based evaluation function.

- Served at: `https://<host>/standard`
- Appearance: color `#f54a00`, head `pixel-round`, tail `mlh-gene`
- Implements: [`SnakeEngine`](../fusion-core/src/lib.rs) (see [`engine.rs`](./src/engine.rs))

This crate has no HTTP or database code. The server in [`src/`](../../src) mounts its routes, persists its training data, and exposes its params over `/standard/config`.

## Module Layout

```txt
src/
  engine.rs            StandardEngine: SnakeEngine impl, get_move_with_params(), MoveFeatures capture
  simulation.rs        SimBoard / SimSnake: compact cloneable state + full Standard rules
  search.rs            Iterative-deepening paranoid minimax with alpha-beta pruning
  evaluation.rs        Static leaf evaluation (area, traps, food, length, aggression, ...)
  heuristic_params.rs  HeuristicParams: tunable weights, validation, load/save, SharedParams
  lib.rs               Module exports (+ `game_objects` re-export of fusion-models)
benches/
  common/mod.rs        Shared helpers for building test boards
  simulation_bench.rs  SimBoard::apply_moves() throughput
  evaluation_bench.rs  evaluate() scoring
  search_bench.rs      End-to-end search at various depths
```

## Decision Algorithm

`StandardEngine::get_move()` takes a read lock on the live params, then calls `get_move_with_params()`:

1. **Build the simulation board.** `SimBoard::from_game_state()` converts the API types into a compact, `Clone`-able board. Our snake is always index 0. `SimBoard::apply_moves()` implements the full Standard ruleset: movement, health decay, eating and growth, hazard damage, wall and body collisions, and head-to-head resolution.
2. **Set the time budget.** Budget = `game.timeout × search_time_pct / 100`, with a minimum of 50 ms.
3. **Search.** `search::search()` runs iterative deepening:
   - Root moves are pre-ordered by a 1-ply evaluation to improve pruning.
   - Depth starts at 1 and goes up to 50. No new depth starts after 60% of the budget has been used. A depth that hits the hard deadline (checked every 512 nodes) is thrown away, and the last completed depth's move is used.
   - **Paranoid assumption:** we maximize. For each of our moves, the nearest enemy is a true minimizing player and all of its replies are searched. Any other enemies play the reply that looks worst for us one ply ahead, which limits branching with 3-4 snakes.
   - Alpha-beta pruning over the resulting tree. Forced losses and wins are scored by distance, so the search survives as long as possible and takes the fastest win.
4. **Evaluate.** `evaluation::evaluate()` scores each leaf from our perspective (higher is better; dead = `LOSS_SCORE`, won = `WIN_SCORE`):

   | Term | Effect | Params |
   | --- | --- | --- |
   | Voronoi area control | + tiles we reach before any enemy (simultaneous BFS) | `eval_area_weight` |
   | Trap detection | − proportional to how far flood-fill reachable space falls short of our length | `eval_trap_penalty` |
   | Body proximity | − per snake body segment adjacent to our head | `eval_body_proximity_penalty` |
   | Edge proximity | − per axis on which our head touches a wall | `eval_edge_penalty` |
   | Health | + health, tripled below 20 | `eval_health_weight` |
   | Food proximity | + closer to the nearest food. Discounted when every food is contested by an equal-or-longer enemy. Multiplied when desperate | `eval_food_weight`, `eval_food_contest_discount`, `eval_desperate_food_mult` |
   | Length advantage | + our length − longest enemy's length | `eval_length_weight` |
   | Aggression | + when within 2 of a shorter enemy's head | `eval_aggression_bonus` |
   | Head-to-head danger | − when adjacent to an equal-or-longer enemy's head | `eval_h2h_danger_penalty` |

   Weights also scale by health tier:
   - **Desperate** (health below `health_threshold_desperate`): food × `eval_desperate_food_mult` (default 3).
   - **Balanced** (health below `health_threshold_balanced`): no scaling.
   - **Healthy** (otherwise): area × `eval_healthy_area_mult` and length × `eval_healthy_length_mult` (default 2 each).

5. **Return.** The result is the chosen move plus `MoveFeatures` (board summary, search depth, eval score, search time), which the server logs as a training row.

## Configuration

All scoring constants live in [`HeuristicParams`](./src/heuristic_params.rs). Never hard-code a new constant in `evaluation.rs` or `search.rs`. Add a field with a doc comment, give it a value in `Default` (the baseline behaviour), and add bounds to `validate()` if needed.

**Persistence.** On startup the engine loads its params file and falls back to `HeuristicParams::default()` if the file is missing or invalid. If no env override is set and the default path doesn't exist, a legacy `./data/params.json` or `/data/params.json` is migrated to the new path.

| Variable | Default | Description |
| --- | --- | --- |
| `STANDARD_ENGINE_PARAMS_FILE` | `./data/engines/standard/params.json` | Params file path |
| `PARAMS_FILE` | _(unset)_ | Legacy alias, used when `STANDARD_ENGINE_PARAMS_FILE` is unset |

**Runtime tuning** (requires `X-API-Key`). Updates are validated, written to disk, then swapped into the shared `Arc<RwLock<HeuristicParams>>`, so the next move uses them:

| Route | Description |
| --- | --- |
| `GET /standard/config` | Current params |
| `POST /standard/config` | Replace params (full `HeuristicParams` JSON body). Returns 400 on validation failure |
| `POST /standard/config/reset` | Restore defaults |

Validation rules: at least one of the area/health/food/length weights must be > 0, `search_time_pct` must be 1–90, `eval_food_contest_discount` must be 0–100, `health_threshold_balanced` must be greater than `health_threshold_desperate`, and the three tier multipliers must be 1–10. Fields missing from a params file or request body take their default value.

## Benchmarks

```bash
cargo bench -p fusion-engine-standard                      # all benches
cargo bench -p fusion-engine-standard --bench search_bench # one bench
```

The `Benchmarks` workflow (manual `workflow_dispatch`) runs them and publishes the results with `github-action-benchmark`. Run them before and after any change to `simulation.rs`, `search.rs` or `evaluation.rs`: search depth, and therefore play strength, depends directly on throughput.
