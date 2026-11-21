# fusion-arena

Offline match harness for measuring FusionSnake engines. Run it on a development machine, not on the server hosting the live snake, because it uses as much CPU as it is given.

## Commands

**`match`** plays two engine builds in-process with a fixed number of board evaluations per move. Games depend only on the seed, so results are reproducible and unaffected by machine load. Every seed is played with engine A in every seat.

```bash
cargo build --release -p fusion-arena
cp target/release/fusion-arena /tmp/arena-baseline   # keep a build to compare against

# After changing the engine and rebuilding:
target/release/fusion-arena match --b-bin /tmp/arena-baseline --evaluations 5000 --seeds 100
```

Use `--a-params` and `--b-params` to compare parameter files. Two identical engines score exactly 50% in Duels and 25% in 4-player games, because each seed replays the same game from every seat.

**`gauntlet`** plays a running snake against other running snakes over HTTP using the official [`battlesnake`](https://github.com/BattlesnakeOfficial/rules) CLI. Use it for final validation, for example against opponents started with [snake-zoo](https://github.com/BattlesnakeOfficial/snake-zoo). These games are timed, so watch the `req-fail` column: failed requests mean the machine is overloaded and the result is noisy.

```bash
fusion-arena gauntlet --candidate me=http://127.0.0.1:6666/standard \
  --opponent snork=http://127.0.0.1:5000 --timeout-ms 500 --concurrency 1
```

**`tune`** searches for better parameters with a diagonal CMA-ES. Each candidate plays Duels and 4-player games against the starting params and recent generation means, on seeds that change every generation. `eval_area_weight` is held fixed because only the ratios between weights matter, and `search_time_pct` is not tuned.

```bash
fusion-arena tune --out runs/first --generations 40 --evaluations 2000 --seeds 4
```

Each generation writes `mean-NNN.json` and updates `best.json`, so a run can be stopped at any time. Scores near 0.5 mean a candidate plays level with its opponents. Scores within a run are noisy and are not proof of improvement, so check `best.json` with `match` on fresh seeds before using it.

**`promote`** decides whether tuned params should replace the champion. It runs a sequential test on Duels (50% against 52%, with 5% error rates), then rejects candidates that are clearly worse in 4-player games. The candidate is copied to `--out` only if it passes. Applying it stays a manual `POST /standard/config`.

```bash
fusion-arena promote --candidate runs/first/best.json --out runs/first/promoted.json
```

**`verify-rules`** replays logs from `battlesnake play --output` and checks that the arena's referee reproduces every turn.

## Design notes

- The referee in `src/rules.rs` is separate from the engine's simulator, so an engine rules bug cannot hide by also being in the referee.
- Each side runs as a worker process, which lets builds from different commits play each other.
- Game setup approximates the standard map and is not a copy of it. Turn resolution is what `verify-rules` checks.
