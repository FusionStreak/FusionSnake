use log::{error, info};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Get the stats file path, checking environment variable or using default
fn get_stats_file() -> String {
    env::var("STATS_FILE").unwrap_or_else(|_| "./data/stats.json".to_string())
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
    /// Sum of all turns survived across all games
    pub total_turns: u64,
    /// Longest game survived (in turns)
    pub longest_game: u32,
    /// Shortest game survived (in turns)
    pub shortest_game: u32,
    /// ISO 8601 timestamp of last game played
    pub last_played: Option<String>,
}

impl GameStats {
    /// Create a new GameStats with default values
    pub fn new() -> Self {
        Self {
            total_games: 0,
            wins: 0,
            losses: 0,
            draws: 0,
            total_turns: 0,
            longest_game: 0,
            shortest_game: u32::MAX,
            last_played: None,
        }
    }

    /// Load stats from JSON file, or create new if file doesn't exist
    pub fn load_or_create() -> Self {
        let stats_file = get_stats_file();

        // Ensure data directory exists
        if let Some(parent) = Path::new(&stats_file).parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                error!("Failed to create data directory: {}", e);
                return Self::new();
            }
        }

        match fs::read_to_string(&stats_file) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(stats) => {
                    info!("Loaded stats from {}", stats_file);
                    stats
                }
                Err(e) => {
                    error!("Failed to parse stats file: {}. Creating new stats.", e);
                    Self::new()
                }
            },
            Err(_) => {
                info!("Stats file not found. Creating new stats.");
                Self::new()
            }
        }
    }

    /// Save stats to JSON file atomically (write to temp file, then rename)
    pub fn save(&self) -> Result<(), std::io::Error> {
        let stats_file = get_stats_file();
        let json = serde_json::to_string_pretty(self)?;
        let temp_file = format!("{}.tmp", stats_file);

        // Write to temporary file
        let mut file = fs::File::create(&temp_file)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;

        // Atomic rename
        fs::rename(&temp_file, &stats_file)?;
        info!("Stats saved to {}", stats_file);
        Ok(())
    }

    /// Record a game result
    pub fn record_game(&mut self, turns: u32, won: bool, is_draw: bool) {
        self.total_games += 1;
        self.total_turns += turns as u64;

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
    pub fn win_rate(&self) -> f64 {
        if self.total_games == 0 {
            return 0.0;
        }
        (self.wins as f64 / self.total_games as f64) * 100.0
    }

    /// Calculate average turns per game
    pub fn average_turns(&self) -> f64 {
        if self.total_games == 0 {
            return 0.0;
        }
        self.total_turns as f64 / self.total_games as f64
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
