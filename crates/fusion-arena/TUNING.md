# Tuning the standard engine

This guide explains how the offline tuning system works and how to take a set of parameters from a tuning run to the live snake.

Run everything here on a development machine. Tuning uses as much CPU as it is given and would slow the live snake's move responses if it ran on the same server. The server only runs the deployed snake.

## Overview

```text
live params ──► tune ──► best.json ──► match ──► promote ──► promoted.json
                                     (quick     (statistical      │
                                      check)     decision)        ▼
ladder results ◄── params_hash on each outcome ◄── POST /standard/config
```

| Step | Command | Question it answers |
| --- | --- | --- |
| Search | `tune` | Which parameters look promising? |
| Quick check | `match` | Does the result beat the current params on new seeds? |
| Decision | `promote` | Is there enough evidence to replace the current params? |
| External check | `gauntlet` | Does it hold up against other people's snakes? |
| Apply | `POST /standard/config` | Manual. Nothing in this system changes the live snake. |
| Confirm | `outcomes` table | Did ladder results improve? |

## How it works

### Reproducible games

Each move is limited to a fixed number of board evaluations instead of a time budget. A game therefore depends only on its seed, the engine build, and the params. Machine load cannot change a result, so two runs with the same inputs produce the same games.

Each seed is replayed with the measured engine in every seat, so a starting position cannot favour one side. Two identical engines score exactly 50% in Duels and 25% in 4-player games.

The referee in `src/rules.rs` is separate from the engine's simulator, so an engine rules bug cannot hide by also existing in the referee. `verify-rules` checks turn resolution against logs from the official CLI.

### What is tuned

`tune` searches 14 parameters.

| Parameters | Range | Search scale |
| --- | --- | --- |
| Health, food, length, aggression, trap, body proximity, edge, and head-to-head weights | 1–5000 | Multiplicative |
| `eval_desperate_food_mult`, `eval_healthy_area_mult`, `eval_healthy_length_mult` | 1–10 | Multiplicative |
| `eval_food_contest_discount` | 0–100 | Additive |
| `health_threshold_desperate` | 5–50 | Additive |
| `health_threshold_balanced` | 30–95 | Additive, always above the desperate threshold |

Two parameters are not tuned:

- `eval_area_weight` is fixed at 100 because only the ratios between weights affect move choice. Starting params are rescaled to match, so tuned weights look about 10 times larger than the defaults. This is expected.
- `search_time_pct` is copied from the starting params. It controls real time, which the fixed evaluation budget does not measure.

### The search

`tune` uses separable CMA-ES, an optimizer that needs only a score for each candidate and tolerates noise. Each generation:

1. Samples 11 candidates around the current mean.
2. Plays each candidate against an opponent pool on the same new seeds.
3. Moves the mean toward the best-scoring candidates and adapts the step size.
4. Saves the new mean.

The pool is the starting params plus up to four earlier means, with one added every five generations. Playing past means reduces the risk of learning to beat only the starting params. Seeds change every generation, so the tuner cannot overfit a fixed set of games.

A candidate's fitness weights the two formats equally, with 4-player scores doubled so an even result is 0.5 in both:

```text
fitness = (duel_score + 2 × four_player_score) / 2
```

### The promotion test

Tuning scores are noisy, and the selected candidate always looks better than it is. `promote` measures it separately on seeds the tuner did not use:

1. **Duels:** a sequential probability ratio test compares "no better" (50%) with "a real gain" (52%), using 5% error rates. It stops as soon as the evidence is strong enough, or gives up at `--max-games`.
2. **4-player:** 400 games by default. The candidate is rejected if the upper end of its 95% interval is below the even score of 25%.

The candidate is copied to `--out` only if it passes both. If it fails, the command exits with an error and writes nothing.

## Workflow

The examples use the git-ignored `runs/` directory.

### 1. Build and save the current params

```bash
cargo build --release -p fusion-arena
mkdir -p runs/first

export SNAKE_URL=https://your-snake.example.com
export API_KEY=your-key
curl -fsS -H "X-API-Key: $API_KEY" "$SNAKE_URL/standard/config" -o runs/first/champion.json
```

Keep `champion.json`. It is both the baseline to beat and your rollback file.

The build you tune must contain the same engine code that will serve the params. If the live server runs an older version, deploy this code first, or compare params only on the build you intend to deploy.

### 2. Check the setup

This takes a few minutes and catches path and build problems before a long run.

```bash
target/release/fusion-arena tune --out runs/smoke \
  --start runs/first/champion.json --generations 2 --evaluations 200 --seeds 1
```

The `failed` column must be 0.

### 3. Run the tuner

```bash
target/release/fusion-arena tune --out runs/first \
  --start runs/first/champion.json \
  --generations 40 --evaluations 2000 --seeds 4 --threads 10 \
  | tee runs/first/tune.log
```

The default run plays 39,600 games and is likely to take several hours. Use the `seconds` column to project the finish, but expect generations to slow down until generation 21 as the opponent pool grows from one to five. `--threads` defaults to half the CPU cores.

| Option | Default | Effect of raising it |
| --- | --- | --- |
| `--generations` | 40 | More search |
| `--seeds` | 4 | Less noise per candidate, proportionally more games |
| `--evaluations` | 2000 | Deeper play, closer to live strength, slower games |
| `--population` | 11 | Broader search, proportionally more games |
| `--sigma` | 0.3 | Larger initial steps. Lower it to refine known-good params |
| `--seed` | 1 | Changes the run. Keep it below 900 to avoid the default `promote` seeds |

#### Reading the output

```text
gen  best    mean    sigma   failed  seconds
```

- `best` and `mean` are the top and average fitness in that generation. 0.5 means level with the opponent pool.
- These scores will not climb steadily, because the pool gets stronger and seeds change. A flat line does not mean the run failed.
- `sigma` is the search step size. A steady decline suggests the search is settling.
- `failed` must stay at 0. Otherwise, stop and investigate.

#### Output files

| File | Contents |
| --- | --- |
| `start.json` | Starting params, rescaled to the area weight of 100 |
| `mean-NNN.json` | The search mean after generation `NNN` |
| `best.json` | A copy of the **latest mean** |
| `candidate.json` | Temporary file for the candidate being played |

`best.json` is the latest mean rather than the highest-scoring single candidate, because a mean averages out lucky results.

You can stop a run at any time and use the latest `best.json`. A run cannot be resumed with its optimizer state. Passing `--start runs/first/best.json` with a new `--out` and a smaller `--sigma` starts a new search from that point. Use a separate `--out` for each concurrent run, because runs sharing a directory overwrite each other's files.

### 4. Check on new seeds

```bash
target/release/fusion-arena match \
  --a-params runs/first/best.json --b-params runs/first/champion.json \
  --evaluations 5000 --seeds 100 --base-seed 800000000
```

Engine A is the candidate. Look for a Duel score above 50% and a 4-player score above 25%. If it is clearly below either, skip promotion and adjust the run.

### 5. Promote

```bash
target/release/fusion-arena promote \
  --candidate runs/first/best.json --champion runs/first/champion.json \
  --out runs/first/promoted.json
```

Always pass `--champion` once the live params differ from the defaults, because it otherwise tests against the defaults. A small real gain can take thousands of Duels. An `inconclusive` result means the gain, if any, is too small to detect within `--max-games`, so keep the champion.

Use a new `--base-seed` for each attempt, for example by adding 10,000,000. Retrying on the same seeds gives a test that has already seen those games.

### 6. Optional: play other snakes

Self-play can miss weaknesses against different styles. [snake-zoo](https://github.com/BattlesnakeOfficial/snake-zoo) can launch community snakes in Docker, and `gauntlet` plays them using the official [`battlesnake`](https://github.com/BattlesnakeOfficial/rules) CLI. Run your candidate on a local server with throwaway state:

```bash
mkdir -p /tmp/fs
DATABASE_URL=sqlite:/tmp/fs/test.db STANDARD_ENGINE_PARAMS_FILE=runs/first/promoted.json \
  cargo run --release

target/release/fusion-arena gauntlet --candidate me=http://127.0.0.1:6666/standard \
  --opponent snork=http://127.0.0.1:5000 --timeout-ms 500 --concurrency 1
```

These games are timed. Non-zero `req-fail` means the machine is overloaded and the result is unreliable. snake-zoo builds the latest upstream code, so record each opponent's commit with your results.

### 7. Apply

```bash
curl -fsS -X POST "$SNAKE_URL/standard/config" \
  -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
  --data @runs/first/promoted.json
```

The server validates the params, saves them, and uses them from the next move. To roll back, post `champion.json` the same way. `POST /standard/config/reset` restores the defaults.

Set `API_KEY` on any public deployment. Without it, the config endpoints accept requests from anyone.

### 8. Confirm on the ladder

Each outcome records the hash of the params it was played with and the server version:

```sql
SELECT engine_version, params_hash, game_type,
       COUNT(*) AS games,
       ROUND(100.0 * AVG(won), 1) AS win_pct
FROM outcomes
WHERE engine = 'standard' AND params_hash IS NOT NULL
GROUP BY engine_version, params_hash, game_type
ORDER BY MIN(recorded_at);
```

Ladder opponents change over time and small samples swing widely, so treat this as a check for large regressions rather than proof of a small gain.

## Limits

- Tuning optimizes against this engine and its past versions, so `gauntlet` is the check for other play styles.
- The arena's game setup approximates the standard map and has no hazards, so hazard behaviour is not tuned.
- A fixed evaluation budget does not capture time-related effects. Use a timed `gauntlet` run after engine changes that affect speed.
- Tuned params are specific to the engine code. After changing `simulation.rs`, `search.rs`, or `evaluation.rs`, promote them again against that build.
