//! Runtime-tunable heuristic parameters for move evaluation.
//!
//! All scoring penalties, bonuses, thresholds, and weight ratios used by
//! [`crate::logic::get_move`] are collected here so the ML training pipeline
//! can push optimised values at runtime via `POST /config`.
//!
//! On startup the snake tries to load `/data/params.json`; if the file is
//! missing or corrupt it falls back to [`HeuristicParams::default()`] which
//! mirrors the original hard-coded values.

use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use utoipa::ToSchema;

/// Every tunable constant used in the move-decision algorithm.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HeuristicParams {
    // ── hazard penalties ─────────────────────────────────────────────────────
    /// Safety penalty applied when stepping onto a hazard tile and health
    /// is **below** `hazard_health_threshold`.
    pub hazard_penalty_low_health: u8,
    /// Safety penalty applied when stepping onto a hazard tile and health
    /// is **at or above** `hazard_health_threshold`.
    pub hazard_penalty_high_health: u8,
    /// Health boundary that switches between the two hazard penalties.
    pub hazard_health_threshold: u32,

    // ── edge proximity ───────────────────────────────────────────────────────
    /// Safety penalty (per axis) when the move lands within
    /// `edge_proximity_distance` tiles of a board edge.
    pub edge_proximity_penalty: u8,
    /// How many tiles from the edge triggers the penalty (inclusive).
    /// Default `1` means the two outermost columns/rows.
    pub edge_proximity_distance: i8,

    // ── head-to-head ─────────────────────────────────────────────────────────
    /// Maximum Manhattan distance to an enemy head that triggers
    /// head-to-head scoring adjustments.
    pub h2h_detection_radius: u8,
    /// Desirability bonus awarded when we are **longer** and an enemy head
    /// is exactly 1 tile away (aggressive chase).
    pub h2h_aggression_bonus: u8,
    /// Safety penalty when we are **shorter/equal** and an enemy head is
    /// ≤ 1 tile away (likely fatal collision).
    pub h2h_penalty_close: u8,
    /// Safety penalty when we are **shorter/equal** and an enemy head is
    /// 2 tiles away (risky proximity).
    pub h2h_penalty_medium: u8,

    // ── body proximity ───────────────────────────────────────────────────────
    /// Safety penalty per adjacent (distance = 1) body segment of any snake.
    pub body_proximity_penalty: u8,

    // ── flood fill ───────────────────────────────────────────────────────────
    /// Safety penalty applied when the reachable space (flood fill) is
    /// smaller than our body length — i.e. we would trap ourselves.
    pub flood_fill_trap_penalty: u8,

    // ── food scoring ─────────────────────────────────────────────────────────
    /// Base value for food desirability.  Actual score per move is
    /// `food_desirability_base - manhattan_distance_to_food`.
    pub food_desirability_base: u8,

    // ── health-based weight tiers ────────────────────────────────────────────
    /// Health **below** this value triggers "desperate" weights.
    pub health_threshold_desperate: u32,
    /// Health **below** this value (but above `health_threshold_desperate`)
    /// triggers "balanced" weights.
    pub health_threshold_balanced: u32,

    // ── weight triplets (safety, food, space) ────────────────────────────────
    /// Weights when health < `health_threshold_desperate`.
    pub weight_desperate_safety: u16,
    pub weight_desperate_food: u16,
    pub weight_desperate_space: u16,

    /// Weights when health is in the balanced range.
    pub weight_balanced_safety: u16,
    pub weight_balanced_food: u16,
    pub weight_balanced_space: u16,

    /// Weights when health ≥ `health_threshold_balanced`.
    pub weight_healthy_safety: u16,
    pub weight_healthy_food: u16,
    pub weight_healthy_space: u16,
}

impl Default for HeuristicParams {
    /// Original hard-coded values — guaranteed baseline behaviour.
    fn default() -> Self {
        Self {
            // hazard
            hazard_penalty_low_health: 20,
            hazard_penalty_high_health: 10,
            hazard_health_threshold: 50,
            // edge
            edge_proximity_penalty: 1,
            edge_proximity_distance: 1,
            // head-to-head
            h2h_detection_radius: 2,
            h2h_aggression_bonus: 15,
            h2h_penalty_close: 30,
            h2h_penalty_medium: 8,
            // body
            body_proximity_penalty: 2,
            // flood fill
            flood_fill_trap_penalty: 50,
            // food
            food_desirability_base: 200,
            // health thresholds
            health_threshold_desperate: 30,
            health_threshold_balanced: 60,
            // desperate weights (s/f/sp)
            weight_desperate_safety: 1,
            weight_desperate_food: 3,
            weight_desperate_space: 1,
            // balanced weights
            weight_balanced_safety: 2,
            weight_balanced_food: 2,
            weight_balanced_space: 1,
            // healthy weights
            weight_healthy_safety: 3,
            weight_healthy_food: 1,
            weight_healthy_space: 2,
        }
    }
}

impl HeuristicParams {
    /// Attempt to load parameters from a JSON file.
    /// Returns `None` if the file is missing or cannot be parsed.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<Self>(&contents) {
                Ok(params) => {
                    info!("Loaded heuristic params from {}", path.display());
                    Some(params)
                }
                Err(e) => {
                    warn!(
                        "Failed to parse heuristic params from {}: {e}",
                        path.display()
                    );
                    None
                }
            },
            Err(_) => None,
        }
    }

    /// Persist the current parameters to a JSON file for crash recovery.
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        std::fs::write(path, json).map_err(|e| e.to_string())?;
        info!("Saved heuristic params to {}", path.display());
        Ok(())
    }

    /// Validate that all values are within sane bounds.
    /// Returns a list of validation errors (empty = valid).
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        if self.hazard_health_threshold == 0 {
            errors.push("hazard_health_threshold must be > 0".into());
        }
        if self.health_threshold_desperate == 0 {
            errors.push("health_threshold_desperate must be > 0".into());
        }
        if self.health_threshold_balanced <= self.health_threshold_desperate {
            errors.push("health_threshold_balanced must be > health_threshold_desperate".into());
        }
        if self.weight_desperate_safety == 0
            && self.weight_desperate_food == 0
            && self.weight_desperate_space == 0
        {
            errors.push("At least one desperate weight must be > 0".into());
        }
        if self.weight_balanced_safety == 0
            && self.weight_balanced_food == 0
            && self.weight_balanced_space == 0
        {
            errors.push("At least one balanced weight must be > 0".into());
        }
        if self.weight_healthy_safety == 0
            && self.weight_healthy_food == 0
            && self.weight_healthy_space == 0
        {
            errors.push("At least one healthy weight must be > 0".into());
        }

        errors
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Async-friendly shared handle to the live heuristic parameters.
/// Readers (`get_move`) take a cheap `read()` lock; writers (`POST /config`)
/// take an exclusive `write()` lock and persist to disk.
pub type SharedParams = Arc<RwLock<HeuristicParams>>;

/// Default path for the persisted parameters file.
pub const PARAMS_FILE: &str = "/data/params.json";

/// Create the shared parameter store.
/// Tries to load from `PARAMS_FILE`, falling back to `Default`.
/// If no file exists, persists the defaults immediately so they survive
/// the next redeploy without requiring a trainer push first.
pub fn create_shared_params() -> SharedParams {
    let path = std::env::var("PARAMS_FILE").unwrap_or_else(|_| PARAMS_FILE.to_string());
    let params = HeuristicParams::load_from_file(Path::new(&path)).unwrap_or_else(|| {
        info!("Using default heuristic parameters");
        let defaults = HeuristicParams::default();
        if let Err(e) = defaults.save_to_file(Path::new(&path)) {
            warn!("Could not persist default params to {path}: {e}");
        }
        defaults
    });
    Arc::new(RwLock::new(params))
}
