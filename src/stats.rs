use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Get the stats file path, checking environment variable or using default
fn get_stats_file() -> String {
    env::var("STATS_FILE").unwrap_or_else(|_| "./data/stats.json".to_string())
}

/// Tracks a currently active game
#[derive(Debug, Clone)]
pub struct ActiveGame {
    /// Last turn number we participated in
    pub last_turn: u32,
    /// When the game started
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Starting length of our snake
    pub starting_length: u32,
}

/// Type alias for shared active games state
pub type ActiveGames = Arc<Mutex<HashMap<String, ActiveGame>>>;

/// Create a new shared active games tracker
pub fn create_active_games() -> ActiveGames {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Clean up stale games that haven't been updated in a while
/// Games older than the specified duration (in seconds) will be removed
pub fn cleanup_stale_games(active_games: &ActiveGames, max_age_seconds: i64) {
    if let Ok(mut games) = active_games.lock() {
        let now = chrono::Utc::now();
        let initial_count = games.len();

        games.retain(|game_id, game| {
            let age = now.signed_duration_since(game.started_at);
            if age.num_seconds() > max_age_seconds {
                warn!(
                    "Cleaning up stale game {} (age: {} seconds)",
                    game_id,
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

/// Game statistics structure that tracks aggregate performance metrics
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct GameStats {
    /// Total number of games played
    pub total_games: u64,
    /// Total number of wins
    pub wins: u64,
    /// Total number of losses
    pub losses: u64,
    /// Total number of draws (ties)
    pub draws: u64,
    /// Sum of all turns survived across all games (accurate turns participated)
    pub total_turns: u64,
    /// Longest game survived (in turns)
    pub longest_game: u32,
    /// Shortest game survived (in turns)
    pub shortest_game: u32,
    /// Total food eaten across all games
    pub total_food_eaten: u64,
    /// ISO 8601 timestamp of last game played
    pub last_played: Option<String>,
}

impl GameStats {
    /// Create a new `GameStats` with default values
    pub fn new() -> Self {
        Self {
            total_games: 0,
            wins: 0,
            losses: 0,
            draws: 0,
            total_turns: 0,
            longest_game: 0,
            shortest_game: u32::MAX,
            total_food_eaten: 0,
            last_played: None,
        }
    }

    /// Load stats from JSON file, or create new if file doesn't exist
    pub fn load_or_create() -> Self {
        let stats_file = get_stats_file();

        // Ensure data directory exists
        if let Some(parent) = Path::new(&stats_file).parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            error!("Failed to create data directory: {e}");
            return Self::new();
        }

        if let Ok(contents) = fs::read_to_string(&stats_file) {
            match serde_json::from_str(&contents) {
                Ok(stats) => {
                    info!("Loaded stats from {stats_file}");
                    stats
                }
                Err(e) => {
                    error!("Failed to parse stats file: {e}. Creating new stats.");
                    Self::new()
                }
            }
        } else {
            info!("Stats file not found. Creating new stats.");
            Self::new()
        }
    }

    /// Save stats to JSON file atomically (write to temp file, then rename)
    pub fn save(&self) -> Result<(), std::io::Error> {
        let stats_file = get_stats_file();
        let json = serde_json::to_string_pretty(self)?;
        let temp_file = format!("{stats_file}.tmp");

        // Write to temporary file
        let mut file = fs::File::create(&temp_file)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;

        // Atomic rename
        fs::rename(&temp_file, &stats_file)?;
        info!("Stats saved to {stats_file}");
        Ok(())
    }

    /// Record a game result
    pub fn record_game(&mut self, turns: u32, food_eaten: u32, won: bool, is_draw: bool) {
        self.total_games += 1;
        self.total_turns += u64::from(turns);
        self.total_food_eaten += u64::from(food_eaten);

        if is_draw {
            self.draws += 1;
        } else if won {
            self.wins += 1;
        } else {
            self.losses += 1;
        }

        // Update longest/shortest game
        if turns > self.longest_game {
            self.longest_game = turns;
        }
        if turns < self.shortest_game {
            self.shortest_game = turns;
        }

        // Update timestamp (ISO 8601 format)
        self.last_played = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Calculate win rate as a percentage
    #[allow(clippy::cast_precision_loss)]
    pub fn win_rate(&self) -> f64 {
        if self.total_games == 0 {
            return 0.0;
        }
        (self.wins as f64 / self.total_games as f64) * 100.0
    }

    /// Calculate average turns per game
    #[allow(clippy::cast_precision_loss)]
    pub fn average_turns(&self) -> f64 {
        if self.total_games == 0 {
            return 0.0;
        }
        self.total_turns as f64 / self.total_games as f64
    }

    /// Calculate average food eaten per game
    #[allow(clippy::cast_precision_loss)]
    pub fn average_food_eaten(&self) -> f64 {
        if self.total_games == 0 {
            return 0.0;
        }
        self.total_food_eaten as f64 / self.total_games as f64
    }
}

impl Default for GameStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Type alias for shared stats state
pub type SharedStats = Arc<Mutex<GameStats>>;

/// Create a new shared stats instance, loading from file if available
pub fn create_shared_stats() -> SharedStats {
    Arc::new(Mutex::new(GameStats::load_or_create()))
}
