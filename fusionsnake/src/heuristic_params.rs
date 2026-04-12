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
    // ── evaluation function weights (used by tree search) ────────────────
    /// Weight for Voronoi area control in the evaluation function.
    pub eval_area_weight: u16,
    /// Weight for health scoring in the evaluation function.
    pub eval_health_weight: u16,
    /// Weight for food proximity in the evaluation function.
    pub eval_food_weight: u16,
    /// Weight for length advantage in the evaluation function.
    pub eval_length_weight: u16,
    /// Bonus score when adjacent to a shorter enemy head (kill opportunity).
    pub eval_aggression_bonus: u16,

    // ── safety heuristics (ported from single-ply engine) ────────────────
    /// Penalty when flood-fill reachable area is less than our body length
    /// (self-trap detection).  Applied proportionally to the shortfall.
    pub eval_trap_penalty: u16,
    /// Penalty per body segment (any snake) at Manhattan distance 1 from
    /// our head.
    pub eval_body_proximity_penalty: u16,
    /// Penalty (per axis) when our head is within 1 tile of a board edge.
    pub eval_edge_penalty: u16,
    /// Penalty when our head is adjacent (distance ≤ 1) to an enemy head
    /// of equal or greater length (dangerous head-to-head situation).
    pub eval_h2h_danger_penalty: u16,

    // ── food targeting ───────────────────────────────────────────────────
    /// Discount (0–100%) applied to food proximity score when the nearest
    /// food is contested by a longer-or-equal enemy.
    pub eval_food_contest_discount: u16,

    // ── health-adaptive thresholds ───────────────────────────────────────
    /// Health below this triggers "desperate" mode (food score boosted).
    pub health_threshold_desperate: u16,
    /// Health below this (but above desperate) triggers "balanced" mode.
    pub health_threshold_balanced: u16,

    // ── search parameters ────────────────────────────────────────────────
    /// Fraction of the game timeout to use for search (1–90, as a percentage).
    /// Default 50 means use 50% of the timeout for search.
    pub search_time_pct: u8,
}

impl Default for HeuristicParams {
    /// Original hard-coded values — guaranteed baseline behaviour.
    fn default() -> Self {
        Self {
            // evaluation function
            eval_area_weight: 10,
            eval_health_weight: 2,
            eval_food_weight: 5,
            eval_length_weight: 8,
            eval_aggression_bonus: 30,
            // safety heuristics
            eval_trap_penalty: 50,
            eval_body_proximity_penalty: 3,
            eval_edge_penalty: 2,
            eval_h2h_danger_penalty: 30,
            // food targeting
            eval_food_contest_discount: 50,
            // health-adaptive thresholds
            health_threshold_desperate: 30,
            health_threshold_balanced: 60,
            // search
            search_time_pct: 50,
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

        if self.eval_area_weight == 0
            && self.eval_health_weight == 0
            && self.eval_food_weight == 0
            && self.eval_length_weight == 0
        {
            errors.push("At least one eval weight must be > 0".into());
        }
        if self.search_time_pct == 0 || self.search_time_pct > 90 {
            errors.push("search_time_pct must be between 1 and 90".into());
        }
        if self.eval_food_contest_discount > 100 {
            errors.push("eval_food_contest_discount must be 0–100".into());
        }
        if self.health_threshold_balanced <= self.health_threshold_desperate {
            errors.push("health_threshold_balanced must be > health_threshold_desperate".into());
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
