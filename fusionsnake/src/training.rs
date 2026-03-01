//! Async training-data collector backed by `SQLite`.
//!
//! Every call to `/move` produces a [`MoveFeatures`] and every `/end` produces
//! a game outcome.  Both are written to the `turns` and `outcomes` tables via
//! the shared `SqlitePool` from [`crate::db`].
//!
//! Writes are fire-and-forget: the handler spawns a Tokio task that does the
//! async insert so the HTTP response is never delayed by disk I/O.

use sqlx::SqlitePool;

use crate::db;
use crate::logic::MoveFeatures;

/// Cheap-to-clone handle for asynchronously recording training data.
#[derive(Clone)]
pub struct TrainingLogger {
    pool: SqlitePool,
}

impl TrainingLogger {
    /// Wrap an existing pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Fire-and-forget: insert a turn record in the background.
    pub fn log_turn(&self, game_id: String, features: MoveFeatures) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            db::insert_turn(&pool, &game_id, &features).await;
        });
    }

    /// Fire-and-forget: insert the game outcome in the background.
    pub fn log_outcome(
        &self,
        game_id: String,
        won: bool,
        is_draw: bool,
        total_turns: u32,
        total_food_eaten: u32,
    ) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            db::insert_outcome(&pool, &game_id, won, is_draw, total_turns, total_food_eaten).await;
        });
    }

    /// Fire-and-forget: update the aggregate `game_stats` row.
    pub fn log_game_stats(&self, turns: u32, food_eaten: u32, won: bool, is_draw: bool) {
        let pool = self.pool.clone();
        tokio::spawn(async move {
            db::record_game(&pool, turns, food_eaten, won, is_draw).await;
        });
    }
}
