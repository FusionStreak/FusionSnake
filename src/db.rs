//! `SQLite` database layer (async via `sqlx`).
//!
//! A single `fusion_snake.db` file stores:
//! - **`turns`** — one row per move decision (training features)
//! - **`outcomes`** — one row per completed game (labels for ML)
//! - **`game_stats`** — one aggregate row per engine (replaces the old JSON file)
//!
//! Every row is attributed to the engine that played it, so two of our
//! engines can share a game (same `game_id`) without clobbering each other.
//!
//! All writes are async and use WAL journal mode so they never block
//! the critical move-response path.

use fusion_core::MoveFeatures;
use log::{error, info, warn};
use serde::Deserialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use std::env;
use std::path::Path;
use std::str::FromStr;

use crate::game_objects::{GameSource, GameType};
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

/// Create tables if they do not already exist, then apply column migrations.
async fn run_migrations(pool: &SqlitePool) {
    migrate_turns_table(pool).await;
    create_supporting_tables(pool).await;
    migrate_outcomes_source_column(pool).await;
    migrate_outcomes_engine(pool).await;
    migrate_game_stats_engine(pool).await;
}

/// Whether `table` currently has a column named `column`.
async fn has_column(pool: &SqlitePool, table: &str, column: &str) -> bool {
    sqlx::query_scalar("SELECT COUNT(*) > 0 FROM pragma_table_info(?1) WHERE name = ?2")
        .bind(table)
        .bind(column)
        .fetch_one(pool)
        .await
        .unwrap_or(false)
}

/// Rebuild `table` with `create_sql`, copying rows across with `copy_sql`.
///
/// Used for changes `ALTER TABLE` cannot express (e.g. primary keys). Runs in
/// a single transaction so a failure leaves the original table untouched.
///
/// All arguments are compile-time constants from this module, never user
/// input, which is what makes the dynamic SQL below safe.
async fn rebuild_table(
    pool: &SqlitePool,
    table: &'static str,
    create_sql: &'static str,
    copy_sql: &'static str,
) {
    let old = format!("_{table}_old");
    let mut tx = pool
        .begin()
        .await
        .expect("Failed to begin migration transaction");
    for sql in [
        format!("ALTER TABLE {table} RENAME TO {old}"),
        create_sql.to_string(),
        copy_sql.replace("{old}", &old),
        format!("DROP TABLE {old}"),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|e| panic!("Failed to rebuild {table} table: {e}"));
    }
    tx.commit()
        .await
        .unwrap_or_else(|e| panic!("Failed to commit {table} table rebuild: {e}"));
}

/// Migrate the `turns` table from the legacy schema to the current one, or create it fresh.
async fn migrate_turns_table(pool: &SqlitePool) {
    // If the table has legacy columns (e.g. target_food_distance) from the
    // pre-search era, recreate it.  SQLite lacks DROP COLUMN on older versions,
    // so we rename → create → copy → drop.
    let has_legacy: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('turns') WHERE name = 'target_food_distance'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if has_legacy {
        sqlx::query("ALTER TABLE turns RENAME TO _turns_old")
            .execute(pool)
            .await
            .expect("Failed to rename old turns table");
    }

    sqlx::query(
        r"
        CREATE TABLE IF NOT EXISTS turns (
            id                    INTEGER PRIMARY KEY AUTOINCREMENT,
            -- rows predating multi-engine support were all played by 'standard'
            engine                TEXT    NOT NULL DEFAULT 'standard',
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
            -- competition
            max_enemy_length      INTEGER NOT NULL,
            min_enemy_length      INTEGER NOT NULL,
            length_advantage      INTEGER NOT NULL,
            -- search metadata
            chosen_move           TEXT    NOT NULL,
            search_depth          INTEGER NOT NULL,
            eval_score            INTEGER NOT NULL,
            search_time_ms        INTEGER NOT NULL,
            -- metadata
            recorded_at           TEXT    NOT NULL
        );
        ",
    )
    .execute(pool)
    .await
    .expect("Failed to create turns table");

    if has_legacy {
        // Copy rows that exist in both schemas; new search columns get defaults.
        let _ = sqlx::query(
            r"
            INSERT INTO turns (
                game_id, turn, health, length, head_x, head_y,
                board_width, board_height, num_snakes, num_food, num_hazards,
                hazard_damage_per_turn, max_enemy_length, min_enemy_length,
                length_advantage, chosen_move, search_depth, eval_score,
                search_time_ms, recorded_at
            )
            SELECT
                game_id, turn, health, length, head_x, head_y,
                board_width, board_height, num_snakes, num_food, num_hazards,
                hazard_damage_per_turn, max_enemy_length, min_enemy_length,
                length_advantage, chosen_move, 0, 0,
                0, recorded_at
            FROM _turns_old
            ",
        )
        .execute(pool)
        .await;
        let _ = sqlx::query("DROP TABLE _turns_old").execute(pool).await;
    }

    if !has_column(pool, "turns", "engine").await {
        sqlx::query("ALTER TABLE turns ADD COLUMN engine TEXT NOT NULL DEFAULT 'standard'")
            .execute(pool)
            .await
            .expect("Failed to add engine column to turns");
        info!("Migrated turns table: added 'engine' column");
    }

    // Indexes for the most common query patterns
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_turns_game_id ON turns (game_id);")
        .execute(pool)
        .await
        .expect("Failed to create turns index");
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_turns_engine_game_id ON turns (engine, game_id);")
        .execute(pool)
        .await
        .expect("Failed to create turns engine index");
}

const CREATE_OUTCOMES: &str = r"
    CREATE TABLE IF NOT EXISTS outcomes (
        engine          TEXT    NOT NULL,
        game_id         TEXT    NOT NULL,
        won             INTEGER NOT NULL,
        is_draw         INTEGER NOT NULL,
        total_turns     INTEGER NOT NULL,
        total_food_eaten INTEGER NOT NULL,
        source          TEXT,
        game_type       TEXT,
        max_length_reached INTEGER NOT NULL DEFAULT 0,
        recorded_at     TEXT    NOT NULL,
        PRIMARY KEY (engine, game_id)
    );
";

const CREATE_GAME_STATS: &str = r"
    CREATE TABLE IF NOT EXISTS game_stats (
        engine           TEXT    PRIMARY KEY,
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
";

/// Create the `outcomes` and `game_stats` tables.
async fn create_supporting_tables(pool: &SqlitePool) {
    sqlx::query(CREATE_OUTCOMES)
        .execute(pool)
        .await
        .expect("Failed to create outcomes table");

    sqlx::query(CREATE_GAME_STATS)
        .execute(pool)
        .await
        .expect("Failed to create game_stats table");
}

/// Re-key `outcomes` by `(engine, game_id)` for databases created before
/// multi-engine support. Existing rows are attributed to `standard`.
async fn migrate_outcomes_engine(pool: &SqlitePool) {
    if has_column(pool, "outcomes", "engine").await {
        return;
    }
    rebuild_table(
        pool,
        "outcomes",
        CREATE_OUTCOMES,
        r"
        INSERT INTO outcomes (
            engine, game_id, won, is_draw, total_turns, total_food_eaten,
            source, game_type, max_length_reached, recorded_at
        )
        SELECT
            'standard', game_id, won, is_draw, total_turns, total_food_eaten,
            source, game_type, max_length_reached, recorded_at
        FROM {old}
        ",
    )
    .await;
    info!("Migrated outcomes table: keyed by (engine, game_id)");
}

/// Convert the legacy single-row `game_stats` table into one row per engine.
/// The existing aggregate is attributed to `standard`.
async fn migrate_game_stats_engine(pool: &SqlitePool) {
    if has_column(pool, "game_stats", "engine").await {
        return;
    }
    rebuild_table(
        pool,
        "game_stats",
        CREATE_GAME_STATS,
        r"
        INSERT INTO game_stats (
            engine, total_games, wins, losses, draws, total_turns,
            longest_game, shortest_game, total_food_eaten, last_played
        )
        SELECT
            'standard', total_games, wins, losses, draws, total_turns,
            longest_game, shortest_game, total_food_eaten, last_played
        FROM {old}
        WHERE total_games > 0
        ",
    )
    .await;
    info!("Migrated game_stats table: one row per engine");
}

/// Add schema columns to `outcomes` for existing databases.
///
/// New databases get them from the `CREATE TABLE IF NOT EXISTS` above, but
/// older databases need `ALTER TABLE` migrations.
async fn migrate_outcomes_source_column(pool: &SqlitePool) {
    let has_source: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('outcomes') WHERE name = 'source'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_source {
        sqlx::query("ALTER TABLE outcomes ADD COLUMN source TEXT")
            .execute(pool)
            .await
            .expect("Failed to add source column to outcomes");
        info!("Migrated outcomes table: added 'source' column");
    }

    let has_game_type: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('outcomes') WHERE name = 'game_type'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_game_type {
        sqlx::query("ALTER TABLE outcomes ADD COLUMN game_type TEXT")
            .execute(pool)
            .await
            .expect("Failed to add game_type column to outcomes");
        info!("Migrated outcomes table: added 'game_type' column");
    }

    let has_max_length_reached: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('outcomes') WHERE name = 'max_length_reached'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);

    if !has_max_length_reached {
        sqlx::query(
            "ALTER TABLE outcomes ADD COLUMN max_length_reached INTEGER NOT NULL DEFAULT 0",
        )
        .execute(pool)
        .await
        .expect("Failed to add max_length_reached column to outcomes");
        info!("Migrated outcomes table: added 'max_length_reached' column");
    }
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

    // Only migrate if no games have been recorded yet
    let total: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(total_games), 0) FROM game_stats")
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
        INSERT OR REPLACE INTO game_stats (
            engine, total_games, wins, losses, draws, total_turns,
            longest_game, shortest_game, total_food_eaten, last_played
        ) VALUES ('standard', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
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
pub async fn insert_turn(pool: &SqlitePool, engine: &str, game_id: &str, f: &MoveFeatures) {
    let recorded_at = chrono::Utc::now().to_rfc3339();

    if let Err(e) = sqlx::query(
        r"
        INSERT INTO turns (
            game_id, turn,
            health, length, head_x, head_y,
            board_width, board_height, num_snakes, num_food, num_hazards,
            hazard_damage_per_turn,
            max_enemy_length, min_enemy_length, length_advantage,
            chosen_move, search_depth, eval_score, search_time_ms,
            recorded_at, engine
        ) VALUES (
            ?1, ?2,
            ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11,
            ?12,
            ?13, ?14, ?15,
            ?16, ?17, ?18, ?19,
            ?20, ?21
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
    .bind(f.max_enemy_length as i64)
    .bind(f.min_enemy_length as i64)
    .bind(f.length_advantage)
    .bind(f.chosen_move)
    .bind(i32::from(f.search_depth))
    .bind(f.eval_score)
    .bind(f.search_time_ms as i64)
    .bind(&recorded_at)
    .bind(engine)
    .execute(pool)
    .await
    {
        error!("Failed to insert turn: {e}");
    }
}

#[derive(Clone, Copy)]
pub struct OutcomeRecord {
    pub won: bool,
    pub is_draw: bool,
    pub total_turns: u32,
    pub total_food_eaten: u32,
    pub source: GameSource,
    pub game_type: GameType,
    pub max_length_reached: u32,
}

impl OutcomeRecord {
    #[must_use]
    pub fn new(
        won: bool,
        is_draw: bool,
        total_turns: u32,
        total_food_eaten: u32,
        source: GameSource,
        game_type: GameType,
        max_length_reached: u32,
    ) -> Self {
        Self {
            won,
            is_draw,
            total_turns,
            total_food_eaten,
            source,
            game_type,
            max_length_reached,
        }
    }
}

/// Insert a game outcome row.
pub async fn insert_outcome(
    pool: &SqlitePool,
    engine: &str,
    game_id: &str,
    outcome: OutcomeRecord,
) {
    let recorded_at = chrono::Utc::now().to_rfc3339();

    if let Err(e) = sqlx::query(
        r"
        INSERT OR REPLACE INTO outcomes
            (game_id, won, is_draw, total_turns, total_food_eaten, source, game_type, max_length_reached, recorded_at, engine)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
        ",
    )
    .bind(game_id)
    .bind(outcome.won)
    .bind(outcome.is_draw)
    .bind(i64::from(outcome.total_turns))
    .bind(i64::from(outcome.total_food_eaten))
    .bind(outcome.source.as_str())
    .bind(outcome.game_type.as_str())
    .bind(i64::from(outcome.max_length_reached))
    .bind(&recorded_at)
    .bind(engine)
    .execute(pool)
    .await
    {
        error!("Failed to insert outcome: {e}");
    }
}

// ---------------------------------------------------------------------------
// Aggregate game stats
// ---------------------------------------------------------------------------

/// Atomically update `engine`'s `game_stats` row after a game ends,
/// creating it on the engine's first game.
pub async fn record_game(
    pool: &SqlitePool,
    engine: &str,
    turns: u32,
    food_eaten: u32,
    won: bool,
    is_draw: bool,
) {
    let now = chrono::Utc::now().to_rfc3339();

    let win_inc: i64 = i64::from(won && !is_draw);
    let loss_inc: i64 = i64::from(!won && !is_draw);
    let draw_inc: i64 = i64::from(is_draw);

    if let Err(e) = sqlx::query(
        r"
        INSERT INTO game_stats (
            engine, total_games, wins, losses, draws, total_turns,
            total_food_eaten, longest_game, shortest_game, last_played
        ) VALUES (?7, 1, ?1, ?2, ?3, ?4, ?5, ?4, ?4, ?6)
        ON CONFLICT (engine) DO UPDATE SET
            total_games      = total_games + 1,
            wins             = wins + excluded.wins,
            losses           = losses + excluded.losses,
            draws            = draws + excluded.draws,
            total_turns      = total_turns + excluded.total_turns,
            total_food_eaten = total_food_eaten + excluded.total_food_eaten,
            longest_game     = MAX(longest_game, excluded.longest_game),
            shortest_game    = MIN(shortest_game, excluded.shortest_game),
            last_played      = excluded.last_played
        ",
    )
    .bind(win_inc)
    .bind(loss_inc)
    .bind(draw_inc)
    .bind(i64::from(turns))
    .bind(i64::from(food_eaten))
    .bind(&now)
    .bind(engine)
    .execute(pool)
    .await
    {
        error!("Failed to update game_stats: {e}");
    }
}

// ---------------------------------------------------------------------------
// Query helpers (for API endpoints)
// ---------------------------------------------------------------------------

/// Fetch aggregate stats for one engine, or summed across all engines.
#[allow(clippy::cast_precision_loss)]
pub async fn get_stats(
    pool: &SqlitePool,
    engine: Option<&str>,
) -> Result<responses::StatsResponse, String> {
    match sqlx::query(
        r"
        SELECT
            COALESCE(SUM(total_games), 0)       AS total_games,
            COALESCE(SUM(wins), 0)              AS wins,
            COALESCE(SUM(losses), 0)            AS losses,
            COALESCE(SUM(draws), 0)             AS draws,
            COALESCE(SUM(total_turns), 0)       AS total_turns,
            COALESCE(MAX(longest_game), 0)      AS longest_game,
            COALESCE(MIN(shortest_game), 2147483647) AS shortest_game,
            COALESCE(SUM(total_food_eaten), 0)  AS total_food_eaten,
            MAX(last_played)                    AS last_played
        FROM game_stats
        WHERE ?1 IS NULL OR engine = ?1
        ",
    )
    .bind(engine)
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

/// Paginated turn records, optionally filtered by `engine` and/or `game_id`.
pub async fn get_turns(
    pool: &SqlitePool,
    engine: Option<&str>,
    game_id: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<responses::PaginatedTurns, String> {
    let result = sqlx::query(
        r"
        SELECT * FROM turns
        WHERE (?1 IS NULL OR engine = ?1) AND (?2 IS NULL OR game_id = ?2)
        ORDER BY id DESC LIMIT ?3 OFFSET ?4
        ",
    )
    .bind(engine)
    .bind(game_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await;

    match result {
        Ok(rows) => {
            let records: Vec<responses::TurnRecord> = rows
                .iter()
                .map(|r| responses::TurnRecord {
                    id: r.get("id"),
                    engine: r.get("engine"),
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
                    max_enemy_length: r.get("max_enemy_length"),
                    min_enemy_length: r.get("min_enemy_length"),
                    length_advantage: r.get("length_advantage"),
                    chosen_move: r.get("chosen_move"),
                    search_depth: r.get("search_depth"),
                    eval_score: r.get("eval_score"),
                    search_time_ms: r.get("search_time_ms"),
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

/// Paginated game outcomes, optionally filtered by `engine`.
pub async fn get_outcomes(
    pool: &SqlitePool,
    engine: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<responses::PaginatedOutcomes, String> {
    match sqlx::query(
        r"
        SELECT * FROM outcomes
        WHERE ?1 IS NULL OR engine = ?1
        ORDER BY recorded_at DESC LIMIT ?2 OFFSET ?3
        ",
    )
    .bind(engine)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let records: Vec<responses::OutcomeRecord> = rows
                .iter()
                .map(|r| responses::OutcomeRecord {
                    engine: r.get("engine"),
                    game_id: r.get("game_id"),
                    won: r.get("won"),
                    is_draw: r.get("is_draw"),
                    total_turns: r.get("total_turns"),
                    total_food_eaten: r.get("total_food_eaten"),
                    source: r
                        .try_get::<Option<String>, _>("source")
                        .unwrap_or(None)
                        .into(),
                    game_type: r
                        .try_get::<Option<String>, _>("game_type")
                        .unwrap_or(None)
                        .into(),
                    max_length_reached: r.try_get("max_length_reached").unwrap_or(0),
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
        avg_search_depth: row.try_get("avg_search_depth").unwrap_or(0.0),
        avg_eval_score: row.try_get("avg_eval_score").unwrap_or(0.0),
        avg_search_time_ms: row.try_get("avg_search_time_ms").unwrap_or(0.0),
        avg_length_advantage: row.try_get("avg_length_advantage").unwrap_or(0.0),
    }
}

/// Aggregate summary useful for dashboards and quick ML feature analysis:
/// average scores per direction, win-correlated averages, etc.
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub async fn get_training_summary(
    pool: &SqlitePool,
    engine: Option<&str>,
) -> responses::TrainingSummary {
    // Overall averages across all turns
    let overall = sqlx::query(
        r"
        SELECT
            COUNT(*)                    AS total_turns,
            COUNT(DISTINCT game_id)     AS total_games,
            AVG(health)                 AS avg_health,
            AVG(length)                 AS avg_length,
            AVG(search_depth)           AS avg_search_depth,
            AVG(eval_score)             AS avg_eval_score,
            AVG(search_time_ms)         AS avg_search_time_ms,
            AVG(length_advantage)       AS avg_length_advantage
        FROM turns
        WHERE ?1 IS NULL OR engine = ?1
        ",
    )
    .bind(engine)
    .fetch_one(pool)
    .await;

    // Averages for turns in games that were won
    let won_avg = sqlx::query(
        r"
        SELECT
            COUNT(*)                    AS total_turns,
            AVG(t.health)               AS avg_health,
            AVG(t.length)               AS avg_length,
            AVG(t.search_depth)         AS avg_search_depth,
            AVG(t.eval_score)           AS avg_eval_score,
            AVG(t.search_time_ms)       AS avg_search_time_ms,
            AVG(t.length_advantage)     AS avg_length_advantage
        FROM turns t
        JOIN outcomes o ON t.engine = o.engine AND t.game_id = o.game_id
        WHERE o.won = 1 AND (?1 IS NULL OR t.engine = ?1)
        ",
    )
    .bind(engine)
    .fetch_one(pool)
    .await;

    // Averages for turns in games that were lost
    let lost_avg = sqlx::query(
        r"
        SELECT
            COUNT(*)                    AS total_turns,
            AVG(t.health)               AS avg_health,
            AVG(t.length)               AS avg_length,
            AVG(t.search_depth)         AS avg_search_depth,
            AVG(t.eval_score)           AS avg_eval_score,
            AVG(t.search_time_ms)       AS avg_search_time_ms,
            AVG(t.length_advantage)     AS avg_length_advantage
        FROM turns t
        JOIN outcomes o ON t.engine = o.engine AND t.game_id = o.game_id
        WHERE o.won = 0 AND o.is_draw = 0 AND (?1 IS NULL OR t.engine = ?1)
        ",
    )
    .bind(engine)
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
    engine: Option<&str>,
    limit: i64,
) -> Result<responses::PaginatedStatsHistory, String> {
    match sqlx::query(
        r"
        SELECT
            o.engine,
            o.game_id,
            o.won,
            o.is_draw,
            o.total_turns,
            o.total_food_eaten,
            o.source,
            o.game_type,
            o.max_length_reached,
            o.recorded_at,
            -- running aggregates
            SUM(o.won) OVER (ORDER BY o.recorded_at ROWS UNBOUNDED PRECEDING) AS cumulative_wins,
            COUNT(*)   OVER (ORDER BY o.recorded_at ROWS UNBOUNDED PRECEDING) AS cumulative_games
        FROM outcomes o
        WHERE ?1 IS NULL OR o.engine = ?1
        ORDER BY o.recorded_at DESC
        LIMIT ?2
        ",
    )
    .bind(engine)
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
                        engine: r.get("engine"),
                        game_id: r.get("game_id"),
                        won: r.get("won"),
                        is_draw: r.get("is_draw"),
                        total_turns: r.get("total_turns"),
                        total_food_eaten: r.get("total_food_eaten"),
                        source: r
                            .try_get::<Option<String>, _>("source")
                            .unwrap_or(None)
                            .into(),
                        game_type: r
                            .try_get::<Option<String>, _>("game_type")
                            .unwrap_or(None)
                            .into(),
                        max_length_reached: r.try_get("max_length_reached").unwrap_or(0),
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
