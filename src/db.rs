//! `SQLite` database layer (async via `sqlx`).
//!
//! A single `fusion_snake.db` file stores:
//! - **`turns`** — one row per move decision (training features)
//! - **`outcomes`** — one row per completed game (labels for ML)
//! - **`game_stats`** — a single aggregate row (replaces the old JSON file)
//!
//! All writes are async and use WAL journal mode so they never block
//! the critical move-response path.

use log::{error, info, warn};
use serde::Deserialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::env;
use std::path::Path;
use std::str::FromStr;

use crate::logic::MoveFeatures;
use crate::responses;

// ---------------------------------------------------------------------------
// Pool initialisation
// ---------------------------------------------------------------------------

/// Create the connection pool and run migrations (idempotent `CREATE TABLE`).
///
/// The DB path defaults to `./data/fusion_snake.db` and can be overridden
/// with the `DATABASE_URL` environment variable.
pub async fn init() -> SqlitePool {
    let url =
        env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:./data/fusion_snake.db".to_string());

    // Make sure the directory exists before SQLite tries to create the file
    if let Some(path) = url.strip_prefix("sqlite:")
        && let Some(parent) = Path::new(path).parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        error!("Cannot create DB directory {}: {e}", parent.display());
    }

    let opts = SqliteConnectOptions::from_str(&url)
        .expect("Invalid DATABASE_URL")
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("Failed to open SQLite database");

    run_migrations(&pool).await;
    migrate_json_stats(&pool).await;
    info!("Database ready: {url}");
    pool
}

/// Create tables if they do not already exist.
async fn run_migrations(pool: &SqlitePool) {
    sqlx::query(
        r"
        CREATE TABLE IF NOT EXISTS turns (
            id                    INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id               TEXT    NOT NULL,
            turn                  INTEGER NOT NULL,
            -- snake
            health                INTEGER NOT NULL,
            length                INTEGER NOT NULL,
            head_x                INTEGER NOT NULL,
            head_y                INTEGER NOT NULL,
            -- board
            board_width           INTEGER NOT NULL,
            board_height          INTEGER NOT NULL,
            num_snakes            INTEGER NOT NULL,
            num_food              INTEGER NOT NULL,
            num_hazards           INTEGER NOT NULL,
            hazard_damage_per_turn INTEGER NOT NULL,
            -- food
            target_food_distance  INTEGER NOT NULL,
            target_food_contested INTEGER NOT NULL,
            -- competition
            max_enemy_length      INTEGER NOT NULL,
            min_enemy_length      INTEGER NOT NULL,
            length_advantage      INTEGER NOT NULL,
            -- per-direction scores
            up_safety             INTEGER NOT NULL,
            up_desirability       INTEGER NOT NULL,
            up_space              INTEGER NOT NULL,
            down_safety           INTEGER NOT NULL,
            down_desirability     INTEGER NOT NULL,
            down_space            INTEGER NOT NULL,
            left_safety           INTEGER NOT NULL,
            left_desirability     INTEGER NOT NULL,
            left_space            INTEGER NOT NULL,
            right_safety          INTEGER NOT NULL,
            right_desirability    INTEGER NOT NULL,
            right_space           INTEGER NOT NULL,
            -- decision
            chosen_move           TEXT    NOT NULL,
            safety_weight         INTEGER NOT NULL,
            food_weight           INTEGER NOT NULL,
            space_weight          INTEGER NOT NULL,
            -- metadata
            recorded_at           TEXT    NOT NULL
        );
        ",
    )
    .execute(pool)
    .await
    .expect("Failed to create turns table");

    // Index for the most common query patterns
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_turns_game_id ON turns (game_id);")
        .execute(pool)
        .await
        .expect("Failed to create turns index");

    sqlx::query(
        r"
        CREATE TABLE IF NOT EXISTS outcomes (
            game_id         TEXT    PRIMARY KEY,
            won             INTEGER NOT NULL,
            is_draw         INTEGER NOT NULL,
            total_turns     INTEGER NOT NULL,
            total_food_eaten INTEGER NOT NULL,
            recorded_at     TEXT    NOT NULL
        );
        ",
    )
    .execute(pool)
    .await
    .expect("Failed to create outcomes table");

    sqlx::query(
        r"
        CREATE TABLE IF NOT EXISTS game_stats (
            id               INTEGER PRIMARY KEY CHECK (id = 1),
            total_games      INTEGER NOT NULL DEFAULT 0,
            wins             INTEGER NOT NULL DEFAULT 0,
            losses           INTEGER NOT NULL DEFAULT 0,
            draws            INTEGER NOT NULL DEFAULT 0,
            total_turns      INTEGER NOT NULL DEFAULT 0,
            longest_game     INTEGER NOT NULL DEFAULT 0,
            shortest_game    INTEGER NOT NULL DEFAULT 2147483647,
            total_food_eaten INTEGER NOT NULL DEFAULT 0,
            last_played      TEXT
        );
        ",
    )
    .execute(pool)
    .await
    .expect("Failed to create game_stats table");

    // Ensure there is always exactly one row in game_stats
    sqlx::query("INSERT OR IGNORE INTO game_stats (id) VALUES (1);")
        .execute(pool)
        .await
        .expect("Failed to seed game_stats row");
}

// ---------------------------------------------------------------------------
// One-time migration: JSON stats file → SQLite
// ---------------------------------------------------------------------------

/// Legacy stats structure from the old JSON file (`/data/game_stats.json`).
#[derive(Debug, Deserialize)]
struct LegacyStats {
    total_games: i64,
    wins: i64,
    losses: i64,
    draws: i64,
    total_turns: i64,
    longest_game: i64,
    shortest_game: i64,
    total_food_eaten: i64,
    last_played: Option<String>,
}

/// If the legacy `game_stats.json` file exists **and** the DB row is still at
/// its default (zero games), import the JSON values into `SQLite` and rename the
/// file to `.json.migrated` so the import is never repeated.
async fn migrate_json_stats(pool: &SqlitePool) {
    let json_path = env::var("STATS_FILE").unwrap_or_else(|_| "./data/game_stats.json".to_string());
    let path = Path::new(&json_path);

    if !path.exists() {
        return; // nothing to migrate
    }

    // Only migrate if the DB row is still pristine (total_games == 0)
    let total: i64 = sqlx::query_scalar("SELECT total_games FROM game_stats WHERE id = 1")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if total != 0 {
        info!("game_stats already populated ({total} games) — skipping JSON migration");
        return;
    }

    // Read & parse the legacy file
    let contents = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => {
            warn!("Could not read legacy stats file {json_path}: {e}");
            return;
        }
    };

    let legacy: LegacyStats = match serde_json::from_str(&contents) {
        Ok(s) => s,
        Err(e) => {
            warn!("Could not parse legacy stats file {json_path}: {e}");
            return;
        }
    };

    // Write into SQLite
    if let Err(e) = sqlx::query(
        r"
        UPDATE game_stats SET
            total_games      = ?1,
            wins             = ?2,
            losses           = ?3,
            draws            = ?4,
            total_turns      = ?5,
            longest_game     = ?6,
            shortest_game    = ?7,
            total_food_eaten = ?8,
            last_played      = ?9
        WHERE id = 1
        ",
    )
    .bind(legacy.total_games)
    .bind(legacy.wins)
    .bind(legacy.losses)
    .bind(legacy.draws)
    .bind(legacy.total_turns)
    .bind(legacy.longest_game)
    .bind(legacy.shortest_game)
    .bind(legacy.total_food_eaten)
    .bind(&legacy.last_played)
    .execute(pool)
    .await
    {
        error!("Failed to migrate legacy stats into SQLite: {e}");
        return;
    }

    // Rename the file so we never import it again
    let migrated = format!("{json_path}.migrated");
    if let Err(e) = tokio::fs::rename(path, &migrated).await {
        warn!("Could not rename legacy stats file to {migrated}: {e}");
    } else {
        info!(
            "Migrated legacy stats from {json_path} → SQLite ({} games, {} wins)",
            legacy.total_games, legacy.wins
        );
    }
}

// ---------------------------------------------------------------------------
// Training data writes
// ---------------------------------------------------------------------------

/// Insert a turn record. Runs async — the caller should `tokio::spawn` this
/// if they want fire-and-forget semantics.
#[allow(clippy::too_many_lines)]
#[allow(clippy::cast_lossless)]
#[allow(clippy::cast_possible_wrap)]
pub async fn insert_turn(pool: &SqlitePool, game_id: &str, f: &MoveFeatures) {
    let recorded_at = chrono::Utc::now().to_rfc3339();

    if let Err(e) = sqlx::query(
        r"
        INSERT INTO turns (
            game_id, turn,
            health, length, head_x, head_y,
            board_width, board_height, num_snakes, num_food, num_hazards,
            hazard_damage_per_turn,
            target_food_distance, target_food_contested,
            max_enemy_length, min_enemy_length, length_advantage,
            up_safety, up_desirability, up_space,
            down_safety, down_desirability, down_space,
            left_safety, left_desirability, left_space,
            right_safety, right_desirability, right_space,
            chosen_move, safety_weight, food_weight, space_weight,
            recorded_at
        ) VALUES (
            ?1, ?2,
            ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11,
            ?12,
            ?13, ?14,
            ?15, ?16, ?17,
            ?18, ?19, ?20,
            ?21, ?22, ?23,
            ?24, ?25, ?26,
            ?27, ?28, ?29,
            ?30, ?31, ?32, ?33,
            ?34
        )
        ",
    )
    .bind(game_id)
    .bind(f.turn)
    .bind(i64::from(f.health))
    .bind(i64::from(f.length))
    .bind(i32::from(f.head_x))
    .bind(i32::from(f.head_y))
    .bind(i32::from(f.board_width))
    .bind(i32::from(f.board_height))
    .bind(f.num_snakes as i64)
    .bind(f.num_food as i64)
    .bind(f.num_hazards as i64)
    .bind(f.hazard_damage_per_turn as i64)
    .bind(i32::from(f.target_food_distance))
    .bind(f.target_food_contested)
    .bind(f.max_enemy_length as i64)
    .bind(f.min_enemy_length as i64)
    .bind(f.length_advantage)
    .bind(i32::from(f.up_safety))
    .bind(i32::from(f.up_desirability))
    .bind(i64::from(f.up_space))
    .bind(i32::from(f.down_safety))
    .bind(i32::from(f.down_desirability))
    .bind(i64::from(f.down_space))
    .bind(i32::from(f.left_safety))
    .bind(i32::from(f.left_desirability))
    .bind(i64::from(f.left_space))
    .bind(i32::from(f.right_safety))
    .bind(i32::from(f.right_desirability))
    .bind(i64::from(f.right_space))
    .bind(f.chosen_move)
    .bind(i32::from(f.safety_weight))
    .bind(i32::from(f.food_weight))
    .bind(i32::from(f.space_weight))
    .bind(&recorded_at)
    .execute(pool)
    .await
    {
        error!("Failed to insert turn: {e}");
    }
}

/// Insert a game outcome row.
pub async fn insert_outcome(
    pool: &SqlitePool,
    game_id: &str,
    won: bool,
    is_draw: bool,
    total_turns: u32,
    total_food_eaten: u32,
) {
    let recorded_at = chrono::Utc::now().to_rfc3339();

    if let Err(e) = sqlx::query(
        r"
        INSERT OR REPLACE INTO outcomes
            (game_id, won, is_draw, total_turns, total_food_eaten, recorded_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ",
    )
    .bind(game_id)
    .bind(won)
    .bind(is_draw)
    .bind(i64::from(total_turns))
    .bind(i64::from(total_food_eaten))
    .bind(&recorded_at)
    .execute(pool)
    .await
    {
        error!("Failed to insert outcome: {e}");
    }
}

// ---------------------------------------------------------------------------
// Aggregate game stats
// ---------------------------------------------------------------------------

/// Atomically update the single `game_stats` row after a game ends.
pub async fn record_game(pool: &SqlitePool, turns: u32, food_eaten: u32, won: bool, is_draw: bool) {
    let now = chrono::Utc::now().to_rfc3339();

    let win_inc: i64 = i64::from(won && !is_draw);
    let loss_inc: i64 = i64::from(!won && !is_draw);
    let draw_inc: i64 = i64::from(is_draw);

    if let Err(e) = sqlx::query(
        r"
        UPDATE game_stats SET
            total_games      = total_games + 1,
            wins             = wins + ?1,
            losses           = losses + ?2,
            draws            = draws + ?3,
            total_turns      = total_turns + ?4,
            total_food_eaten = total_food_eaten + ?5,
            longest_game     = MAX(longest_game, ?4),
            shortest_game    = MIN(shortest_game, ?4),
            last_played      = ?6
        WHERE id = 1
        ",
    )
    .bind(win_inc)
    .bind(loss_inc)
    .bind(draw_inc)
    .bind(i64::from(turns))
    .bind(i64::from(food_eaten))
    .bind(&now)
    .execute(pool)
    .await
    {
        error!("Failed to update game_stats: {e}");
    }
}

// ---------------------------------------------------------------------------
// Query helpers (for API endpoints)
// ---------------------------------------------------------------------------

/// Fetch aggregate stats as a typed struct.
#[allow(clippy::cast_precision_loss)]
pub async fn get_stats(pool: &SqlitePool) -> Result<responses::StatsResponse, String> {
    match sqlx::query(
        r"
        SELECT total_games, wins, losses, draws, total_turns,
               longest_game, shortest_game, total_food_eaten, last_played
        FROM game_stats WHERE id = 1
        ",
    )
    .fetch_one(pool)
    .await
    {
        Ok(row) => {
            let total_games: i64 = row.get("total_games");
            let wins: i64 = row.get("wins");
            let losses: i64 = row.get("losses");
            let draws: i64 = row.get("draws");
            let total_turns: i64 = row.get("total_turns");
            let longest_game: i64 = row.get("longest_game");
            let shortest_game: i64 = row.get("shortest_game");
            let total_food_eaten: i64 = row.get("total_food_eaten");
            let last_played: Option<String> = row.get("last_played");

            let win_rate = if total_games > 0 {
                wins as f64 / total_games as f64 * 100.0
            } else {
                0.0
            };
            let avg_turns = if total_games > 0 {
                total_turns as f64 / total_games as f64
            } else {
                0.0
            };
            let avg_food = if total_games > 0 {
                total_food_eaten as f64 / total_games as f64
            } else {
                0.0
            };

            Ok(responses::StatsResponse {
                total_games,
                wins,
                losses,
                draws,
                win_rate: format!("{win_rate:.1}"),
                total_turns,
                average_turns: format!("{avg_turns:.1}"),
                longest_game,
                shortest_game: if shortest_game == i64::from(i32::MAX) {
                    0
                } else {
                    shortest_game
                },
                total_food_eaten,
                average_food_eaten: format!("{avg_food:.1}"),
                last_played,
            })
        }
        Err(e) => {
            error!("Failed to fetch game_stats: {e}");
            Err("Failed to fetch stats".to_string())
        }
    }
}

/// Paginated turn records, optionally filtered by `game_id`.
pub async fn get_turns(
    pool: &SqlitePool,
    game_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<responses::PaginatedTurns, String> {
    let result = if let Some(gid) = game_id {
        sqlx::query(
            r"
            SELECT * FROM turns WHERE game_id = ?1
            ORDER BY id DESC LIMIT ?2 OFFSET ?3
            ",
        )
        .bind(gid)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    } else {
        sqlx::query(
            r"
            SELECT * FROM turns
            ORDER BY id DESC LIMIT ?1 OFFSET ?2
            ",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    };

    match result {
        Ok(rows) => {
            let records: Vec<responses::TurnRecord> = rows
                .iter()
                .map(|r| responses::TurnRecord {
                    id: r.get("id"),
                    game_id: r.get("game_id"),
                    turn: r.get("turn"),
                    health: r.get("health"),
                    length: r.get("length"),
                    head_x: r.get("head_x"),
                    head_y: r.get("head_y"),
                    board_width: r.get("board_width"),
                    board_height: r.get("board_height"),
                    num_snakes: r.get("num_snakes"),
                    num_food: r.get("num_food"),
                    num_hazards: r.get("num_hazards"),
                    hazard_damage_per_turn: r.get("hazard_damage_per_turn"),
                    target_food_distance: r.get("target_food_distance"),
                    target_food_contested: r.get("target_food_contested"),
                    max_enemy_length: r.get("max_enemy_length"),
                    min_enemy_length: r.get("min_enemy_length"),
                    length_advantage: r.get("length_advantage"),
                    up_safety: r.get("up_safety"),
                    up_desirability: r.get("up_desirability"),
                    up_space: r.get("up_space"),
                    down_safety: r.get("down_safety"),
                    down_desirability: r.get("down_desirability"),
                    down_space: r.get("down_space"),
                    left_safety: r.get("left_safety"),
                    left_desirability: r.get("left_desirability"),
                    left_space: r.get("left_space"),
                    right_safety: r.get("right_safety"),
                    right_desirability: r.get("right_desirability"),
                    right_space: r.get("right_space"),
                    chosen_move: r.get("chosen_move"),
                    safety_weight: r.get("safety_weight"),
                    food_weight: r.get("food_weight"),
                    space_weight: r.get("space_weight"),
                    recorded_at: r.get("recorded_at"),
                })
                .collect();
            let count = records.len();
            Ok(responses::PaginatedTurns {
                data: records,
                count,
            })
        }
        Err(e) => {
            error!("Failed to query turns: {e}");
            Err("Failed to query turns".to_string())
        }
    }
}

/// Paginated game outcomes.
pub async fn get_outcomes(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<responses::PaginatedOutcomes, String> {
    match sqlx::query(
        r"
        SELECT * FROM outcomes
        ORDER BY recorded_at DESC LIMIT ?1 OFFSET ?2
        ",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let records: Vec<responses::OutcomeRecord> = rows
                .iter()
                .map(|r| responses::OutcomeRecord {
                    game_id: r.get("game_id"),
                    won: r.get("won"),
                    is_draw: r.get("is_draw"),
                    total_turns: r.get("total_turns"),
                    total_food_eaten: r.get("total_food_eaten"),
                    recorded_at: r.get("recorded_at"),
                })
                .collect();
            let count = records.len();
            Ok(responses::PaginatedOutcomes {
                data: records,
                count,
            })
        }
        Err(e) => {
            error!("Failed to query outcomes: {e}");
            Err("Failed to query outcomes".to_string())
        }
    }
}

/// Convert an aggregated SQL row into a [`TrainingAverages`] struct.
///
/// Uses `try_get` with defaults so that `NULL` values from empty tables
/// are gracefully handled instead of panicking.
fn row_to_averages(row: &sqlx::sqlite::SqliteRow) -> responses::TrainingAverages {
    responses::TrainingAverages {
        total_turns: row.try_get("total_turns").unwrap_or(0),
        avg_health: row.try_get("avg_health").unwrap_or(0.0),
        avg_length: row.try_get("avg_length").unwrap_or(0.0),
        avg_up_safety: row.try_get("avg_up_safety").unwrap_or(0.0),
        avg_down_safety: row.try_get("avg_down_safety").unwrap_or(0.0),
        avg_left_safety: row.try_get("avg_left_safety").unwrap_or(0.0),
        avg_right_safety: row.try_get("avg_right_safety").unwrap_or(0.0),
        avg_up_space: row.try_get("avg_up_space").unwrap_or(0.0),
        avg_down_space: row.try_get("avg_down_space").unwrap_or(0.0),
        avg_left_space: row.try_get("avg_left_space").unwrap_or(0.0),
        avg_right_space: row.try_get("avg_right_space").unwrap_or(0.0),
        avg_food_distance: row.try_get("avg_food_distance").unwrap_or(0.0),
        avg_length_advantage: row.try_get("avg_length_advantage").unwrap_or(0.0),
    }
}

/// Aggregate summary useful for dashboards and quick ML feature analysis:
/// average scores per direction, win-correlated averages, etc.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub async fn get_training_summary(pool: &SqlitePool) -> responses::TrainingSummary {
    // Overall averages across all turns
    let overall = sqlx::query(
        r"
        SELECT
            COUNT(*)                    AS total_turns,
            COUNT(DISTINCT game_id)     AS total_games,
            AVG(health)                 AS avg_health,
            AVG(length)                 AS avg_length,
            AVG(up_safety)              AS avg_up_safety,
            AVG(down_safety)            AS avg_down_safety,
            AVG(left_safety)            AS avg_left_safety,
            AVG(right_safety)           AS avg_right_safety,
            AVG(up_space)               AS avg_up_space,
            AVG(down_space)             AS avg_down_space,
            AVG(left_space)             AS avg_left_space,
            AVG(right_space)            AS avg_right_space,
            AVG(target_food_distance)   AS avg_food_distance,
            AVG(length_advantage)       AS avg_length_advantage
        FROM turns
        ",
    )
    .fetch_one(pool)
    .await;

    // Averages for turns in games that were won
    let won_avg = sqlx::query(
        r"
        SELECT
            COUNT(*)                    AS total_turns,
            AVG(t.health)               AS avg_health,
            AVG(t.length)               AS avg_length,
            AVG(t.up_safety)            AS avg_up_safety,
            AVG(t.down_safety)          AS avg_down_safety,
            AVG(t.left_safety)          AS avg_left_safety,
            AVG(t.right_safety)         AS avg_right_safety,
            AVG(t.up_space)             AS avg_up_space,
            AVG(t.down_space)           AS avg_down_space,
            AVG(t.left_space)           AS avg_left_space,
            AVG(t.right_space)          AS avg_right_space,
            AVG(t.target_food_distance) AS avg_food_distance,
            AVG(t.length_advantage)     AS avg_length_advantage
        FROM turns t
        JOIN outcomes o ON t.game_id = o.game_id
        WHERE o.won = 1
        ",
    )
    .fetch_one(pool)
    .await;

    // Averages for turns in games that were lost
    let lost_avg = sqlx::query(
        r"
        SELECT
            COUNT(*)                    AS total_turns,
            AVG(t.health)               AS avg_health,
            AVG(t.length)               AS avg_length,
            AVG(t.up_safety)            AS avg_up_safety,
            AVG(t.down_safety)          AS avg_down_safety,
            AVG(t.left_safety)          AS avg_left_safety,
            AVG(t.right_safety)         AS avg_right_safety,
            AVG(t.up_space)             AS avg_up_space,
            AVG(t.down_space)           AS avg_down_space,
            AVG(t.left_space)           AS avg_left_space,
            AVG(t.right_space)          AS avg_right_space,
            AVG(t.target_food_distance) AS avg_food_distance,
            AVG(t.length_advantage)     AS avg_length_advantage
        FROM turns t
        JOIN outcomes o ON t.game_id = o.game_id
        WHERE o.won = 0 AND o.is_draw = 0
        ",
    )
    .fetch_one(pool)
    .await;

    let mut summary = responses::TrainingSummary {
        total_games: 0,
        total_turns: 0,
        overall: None,
        won_games: None,
        lost_games: None,
    };

    match overall {
        Ok(ref row) => {
            summary.total_games = row.try_get("total_games").unwrap_or(0);
            summary.total_turns = row.try_get("total_turns").unwrap_or(0);
            summary.overall = Some(row_to_averages(row));
        }
        Err(e) => {
            error!("training summary (overall) failed: {e}");
        }
    }

    match won_avg {
        Ok(ref row) => summary.won_games = Some(row_to_averages(row)),
        Err(e) => {
            error!("training summary (won) failed: {e}");
        }
    }

    match lost_avg {
        Ok(ref row) => summary.lost_games = Some(row_to_averages(row)),
        Err(e) => {
            error!("training summary (lost) failed: {e}");
        }
    }

    summary
}

/// Recent per-game stats for win-rate-over-time visualisation.
#[allow(clippy::cast_precision_loss)]
pub async fn get_stats_history(
    pool: &SqlitePool,
    limit: i64,
) -> Result<responses::PaginatedStatsHistory, String> {
    match sqlx::query(
        r"
        SELECT
            o.game_id,
            o.won,
            o.is_draw,
            o.total_turns,
            o.total_food_eaten,
            o.recorded_at,
            -- running aggregates
            SUM(o.won) OVER (ORDER BY o.recorded_at ROWS UNBOUNDED PRECEDING) AS cumulative_wins,
            COUNT(*)   OVER (ORDER BY o.recorded_at ROWS UNBOUNDED PRECEDING) AS cumulative_games
        FROM outcomes o
        ORDER BY o.recorded_at DESC
        LIMIT ?1
        ",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let records: Vec<responses::StatsHistoryRecord> = rows
                .iter()
                .map(|r| {
                    let cum_wins: i64 = r.get("cumulative_wins");
                    let cum_games: i64 = r.get("cumulative_games");
                    let win_rate = if cum_games > 0 {
                        cum_wins as f64 / cum_games as f64 * 100.0
                    } else {
                        0.0
                    };
                    responses::StatsHistoryRecord {
                        game_id: r.get("game_id"),
                        won: r.get("won"),
                        is_draw: r.get("is_draw"),
                        total_turns: r.get("total_turns"),
                        total_food_eaten: r.get("total_food_eaten"),
                        recorded_at: r.get("recorded_at"),
                        cumulative_wins: cum_wins,
                        cumulative_games: cum_games,
                        cumulative_win_rate: format!("{win_rate:.1}"),
                    }
                })
                .collect();
            let count = records.len();
            Ok(responses::PaginatedStatsHistory {
                data: records,
                count,
            })
        }
        Err(e) => {
            error!("Failed to query stats history: {e}");
            Err("Failed to query stats history".to_string())
        }
    }
}
