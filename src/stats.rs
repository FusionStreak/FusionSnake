//! In-memory tracking of active games.
//!
//! Aggregate stats are now stored in `SQLite` (see [`crate::db`]).  This module
//! only tracks games that are currently in progress so `/end` can compute
//! per-game totals (turns survived, food eaten).

use log::{info, warn};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use fusion_models::{GameSource, GameType};

/// Tracks a currently active game.
#[derive(Debug, Clone)]
pub struct ActiveGame {
    /// Last turn number we participated in.
    pub last_turn: u32,
    /// When the game started.
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Starting length of our snake.
    pub starting_length: u32,
    /// Highest length reached during the game.
    pub max_length_reached: u32,
    /// Where the game request originated from.
    pub source: GameSource,
    /// The game mode inferred from ruleset and board size.
    pub game_type: GameType,
}

/// Identifies one of our snakes in one game: `(engine name, game id)`.
///
/// Keyed by engine as well as game so two of our engines playing in the
/// same game are tracked independently.
pub type ActiveGameKey = (&'static str, String);

/// Type alias for shared active games state.
pub type ActiveGames = Arc<Mutex<HashMap<ActiveGameKey, ActiveGame>>>;

/// Create a new shared active games tracker.
pub fn create_active_games() -> ActiveGames {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Clean up stale games that haven't been updated in a while.
/// Games older than the specified duration (in seconds) will be removed.
pub fn cleanup_stale_games(active_games: &ActiveGames, max_age_seconds: i64) {
    if let Ok(mut games) = active_games.lock() {
        let now = chrono::Utc::now();
        let initial_count = games.len();

        games.retain(|(engine, game_id), game| {
            let age = now.signed_duration_since(game.started_at);
            if age.num_seconds() > max_age_seconds {
                warn!(
                    "Cleaning up stale game {game_id} (engine={engine}, age: {} seconds)",
                    age.num_seconds()
                );
                false
            } else {
                true
            }
        });

        let removed = initial_count - games.len();
        if removed > 0 {
            info!("Cleaned up {removed} stale games");
        }
    }
}
