use serde::Serialize;

/// All observable features captured during a single move decision.
/// Returned alongside the chosen move so callers can log it for ML training.
#[derive(Debug, Clone, Serialize)]
pub struct MoveFeatures {
    // ── turn state ───────────────────────────────────────────────────────────
    pub turn: i32,
    pub health: u32,
    pub length: u32,
    pub head_x: i8,
    pub head_y: i8,
    // ── board ────────────────────────────────────────────────────────────────
    pub board_width: u8,
    pub board_height: u8,
    pub num_snakes: usize,
    pub num_food: usize,
    pub num_hazards: usize,
    pub hazard_damage_per_turn: u32,
    // ── competition ──────────────────────────────────────────────────────────
    pub max_enemy_length: u32,
    pub min_enemy_length: u32,
    /// `you.length - max_enemy_length`; negative means we are at a disadvantage.
    pub length_advantage: i32,
    // ── search metadata ──────────────────────────────────────────────────────
    pub chosen_move: &'static str,
    pub search_depth: u8,
    pub eval_score: i32,
    pub search_time_ms: u64,
}

pub struct EngineMoveDecision {
    pub chosen_move: &'static str,
    pub features: MoveFeatures,
}
