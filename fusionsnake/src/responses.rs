//! Typed API response structures for `OpenAPI` documentation.
//!
//! Every HTTP response has a corresponding struct here so that `utoipa` can
//! generate accurate `OpenAPI` schemas.

use serde::Serialize;
use utoipa::ToSchema;

// ---------------------------------------------------------------------------
// Battlesnake API responses
// ---------------------------------------------------------------------------

/// Snake metadata returned by `GET /`.
#[derive(Serialize, ToSchema)]
pub struct InfoResponse {
    /// Battlesnake API version.
    pub apiversion: &'static str,
    /// Author of this Battlesnake.
    pub author: &'static str,
    /// Hex colour code.
    pub color: &'static str,
    /// Head customisation identifier.
    pub head: &'static str,
    /// Tail customisation identifier.
    pub tail: &'static str,
    /// Bot version string.
    pub version: &'static str,
}

/// Move decision returned by `POST /move`.
#[derive(Serialize, ToSchema)]
pub struct MoveResponse {
    /// The chosen direction: `"up"`, `"down"`, `"left"`, or `"right"`.
    #[serde(rename = "move")]
    pub chosen_move: &'static str,
}

// ---------------------------------------------------------------------------
// Common
// ---------------------------------------------------------------------------

/// Generic error envelope.
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Human-readable error description.
    pub error: String,
}

// ---------------------------------------------------------------------------
// Stats responses
// ---------------------------------------------------------------------------

/// Aggregate game statistics returned by `GET /stats`.
#[derive(Serialize, ToSchema)]
pub struct StatsResponse {
    pub total_games: i64,
    pub wins: i64,
    pub losses: i64,
    pub draws: i64,
    /// Win percentage formatted to one decimal place (e.g. `"63.2"`).
    pub win_rate: String,
    pub total_turns: i64,
    /// Average turns per game, one decimal place.
    pub average_turns: String,
    pub longest_game: i64,
    pub shortest_game: i64,
    pub total_food_eaten: i64,
    /// Average food eaten per game, one decimal place.
    pub average_food_eaten: String,
    /// RFC 3339 timestamp of the last completed game, or `null`.
    pub last_played: Option<String>,
}

/// Per-game stats with running cumulative aggregates.
#[derive(Serialize, ToSchema)]
pub struct StatsHistoryRecord {
    pub game_id: String,
    pub won: bool,
    pub is_draw: bool,
    pub total_turns: i64,
    pub total_food_eaten: i64,
    /// RFC 3339 timestamp.
    pub recorded_at: String,
    pub cumulative_wins: i64,
    pub cumulative_games: i64,
    /// Cumulative win rate formatted to one decimal place.
    pub cumulative_win_rate: String,
}

/// Paginated stats history.
#[derive(Serialize, ToSchema)]
pub struct PaginatedStatsHistory {
    pub data: Vec<StatsHistoryRecord>,
    pub count: usize,
}

// ---------------------------------------------------------------------------
// Training data responses
// ---------------------------------------------------------------------------

/// A single turn's feature snapshot from the training data.
#[derive(Serialize, ToSchema)]
pub struct TurnRecord {
    pub id: i64,
    pub game_id: String,
    pub turn: i32,
    pub health: i64,
    pub length: i64,
    pub head_x: i32,
    pub head_y: i32,
    pub board_width: i32,
    pub board_height: i32,
    pub num_snakes: i64,
    pub num_food: i64,
    pub num_hazards: i64,
    pub hazard_damage_per_turn: i64,
    pub max_enemy_length: i64,
    pub min_enemy_length: i64,
    pub length_advantage: i32,
    pub chosen_move: String,
    pub search_depth: i32,
    pub eval_score: i32,
    pub search_time_ms: i64,
    /// RFC 3339 timestamp.
    pub recorded_at: String,
}

/// A completed game's outcome.
#[derive(Serialize, ToSchema)]
pub struct OutcomeRecord {
    pub game_id: String,
    pub won: bool,
    pub is_draw: bool,
    pub total_turns: i64,
    pub total_food_eaten: i64,
    /// RFC 3339 timestamp.
    pub recorded_at: String,
}

/// Paginated list of turn records.
#[derive(Serialize, ToSchema)]
pub struct PaginatedTurns {
    pub data: Vec<TurnRecord>,
    pub count: usize,
}

/// Paginated list of game outcomes.
#[derive(Serialize, ToSchema)]
pub struct PaginatedOutcomes {
    pub data: Vec<OutcomeRecord>,
    pub count: usize,
}

/// Averaged feature values across a subset of turns (overall, won, or lost games).
#[derive(Serialize, ToSchema)]
pub struct TrainingAverages {
    pub total_turns: i64,
    pub avg_health: f64,
    pub avg_length: f64,
    pub avg_search_depth: f64,
    pub avg_eval_score: f64,
    pub avg_search_time_ms: f64,
    pub avg_length_advantage: f64,
}

/// Training data summary with per-category averages.
#[derive(Serialize, ToSchema)]
pub struct TrainingSummary {
    pub total_games: i64,
    pub total_turns: i64,
    pub overall: Option<TrainingAverages>,
    pub won_games: Option<TrainingAverages>,
    pub lost_games: Option<TrainingAverages>,
}
