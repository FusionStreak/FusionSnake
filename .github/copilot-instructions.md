# FusionSnake - Battlesnake AI Agent Instructions

## Project Overview

This is a Battlesnake bot written in Rust using the Rocket web framework. The snake responds to REST API calls during games on play.battlesnake.com, making move decisions based on board state analysis.

## Architecture & Data Flow

### Core Components

- **`main.rs`**: Rocket HTTP server with 4 endpoints (`/`, `/start`, `/move`, `/end`)
- **`game_objects.rs`**: Serde-based data structures matching Battlesnake API spec
- **`logic.rs`**: Decision engine with safety scoring and food desirability algorithms

### Request Flow

1. Battlesnake engine sends JSON GameState to `/move` endpoint
2. `handle_move()` deserializes into `GameState` struct
3. `logic::get_move()` evaluates all 4 directions using `PotentialMoves`
4. Returns JSON with chosen direction: `{"move": "up|down|left|right"}`

### Decision Algorithm (in `logic.rs`)

The bot uses a **weighted scoring system** with two metrics per move:

- **`safety_score`**: Starts at 255, penalized for walls/snakes/edges/proximity
- **`desirability_score`**: Distance to nearest food (inverted: 200 - distance)

Key scoring logic:

```rust
// Immediate death = 0 safety (walls, snake bodies)
// Edge proximity: -1 penalty if within 1 tile of board edge
// Enemy head proximity: -4 if within 2 tiles
// Body proximity: -2 if within 2 tiles
// Health-based weighting: <30 health → prioritize food (1:2), else safety (2:1)
```

## Critical Patterns & Conventions

### Coordinate System

- Uses `i8` for coordinates (can be negative during bounds checking)
- `Coord.distance_to()` returns Manhattan distance as `u8`
- Board origin (0,0) is **bottom-left**; y increases upward

### Struct Visibility

All fields in `game_objects.rs` use `pub(super)` visibility - accessible only within parent module. When adding fields, follow this pattern.

### Move Evaluation Pattern

Always iterate through `PotentialMoves` in this order:

1. Filter out immediate death (safety_score = 0)
2. Apply graduated penalties (not binary death)
3. Choose with `choose_best_move_weighted()` - never pick a 0-safety move

### Environment Variables

- `PORT`: Server port (default 6666)
- `RUST_LOG`: Log level (default "info")

## Development Workflows

### Local Testing

```bash
cargo run  # Starts server on localhost:6666
curl http://localhost:6666  # Returns snake metadata
```

### Docker Deployment

```bash
docker compose up -d  # Production deployment
./update.sh          # Git pull + rebuild + restart
```

### Common Tasks

- **Add new penalty**: Modify safety_score in `logic.rs` move evaluation loop
- **Change appearance**: Edit `logic::info()` JSON (color/head/tail must match Battlesnake API)
- **Adjust aggression**: Modify `safety_weight`/`food_weight` ratio (currently 2:1 or 1:2)

## Testing Considerations

- Test moves at board edges (x/y = 0 or width/height-1)
- Verify behavior when health < 30 (should chase food aggressively)
- Check collision avoidance with snake bodies that persist after death
- Battlesnake API expects 500ms response time - profile with `RUST_LOG=debug`

## Project-Specific Notes

- Edition 2024 in Cargo.toml (uses latest Rust features)
- No async decision logic - all move calculations are synchronous
- Rocket 0.5.1 with custom server header for identification
- Original starter: BattlesnakeOfficial/starter-snake-rust
