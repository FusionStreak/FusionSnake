// Welcome to
// __________         __    __  .__                               __
// \______   \_____ _/  |__/  |_|  |   ____   ______ ____ _____  |  | __ ____
//  |    |  _/\__  \\   __\   __\  | _/ __ \ /  ___//    \\__  \ |  |/ // __ \
//  |    |   \ / __ \|  |  |  | |  |_\  ___/ \___ \|   |  \/ __ \|    <\  ___/
//  |________/(______/__|  |__| |____/\_____>______>___|__(______/__|__\\_____>
//
// This file can be a nice home for your Battlesnake logic and helper functions.
//
// To get you started we've included code to prevent your Battlesnake from moving backwards.
// For more info see docs.battlesnake.com

use log::info;
use serde::Serialize;

use crate::game_objects::{Battlesnake, Board, Game};
use crate::heuristic_params::HeuristicParams;
use crate::responses::{InfoResponse, MoveResponse};
use crate::search;
use crate::simulation::SimBoard;

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

// info is called when you create your Battlesnake on play.battlesnake.com
// and controls your Battlesnake's appearance
// TIP: If you open your Battlesnake URL in a browser you should see this data
pub fn info() -> InfoResponse {
    info!("INFO");

    InfoResponse {
        apiversion: "1",
        author: "fusionstreak",
        color: "#f54a00",
        head: "pixel-round",
        tail: "mlh-gene",
        version: env!("CARGO_PKG_VERSION"),
    }
}

// start is called when your Battlesnake begins a game
pub fn start(game: &Game, _turn: i32, _board: &Board, _you: &Battlesnake) {
    info!("GAME START {}", game.id);
}

// end is called when your Battlesnake finishes a game
// Returns (won, is_draw) tuple
pub fn end(game: &Game, turn: i32, board: &Board, you: &Battlesnake) -> (bool, bool) {
    // Determine winner
    // Note: board.snakes only contains alive snakes (eliminated snakes are removed)
    let you_alive = board.snakes.iter().any(|s| s.id == you.id);
    let alive_count = board.snakes.len();

    let (won, is_draw) = match alive_count {
        0 => (false, true),      // All died simultaneously (draw)
        1 => (you_alive, false), // One survivor (win if it's you)
        _ => (false, false),     // Multiple survivors (shouldn't happen at game end)
    };

    let winner: Option<String> = if board.snakes.len() == 1 {
        Some(board.snakes[0].name.clone())
    } else {
        None
    };

    info!("GAME OVER {}, Turn {}, Winner: {:?}", game.id, turn, winner);

    (won, is_draw)
}

// move is called on every turn and returns your next move
// Valid moves are "up", "down", "left", or "right"
// See https://docs.battlesnake.com/api/example-move for available data
pub fn get_move(
    game: &Game,
    turn: i32,
    board: &Board,
    you: &Battlesnake,
    params: &HeuristicParams,
) -> (MoveResponse, MoveFeatures) {
    info!("TURN {turn}");

    // Build the compact simulation board
    let sim_board =
        SimBoard::from_game_state(board, &you.id, game.ruleset.settings.hazard_damage_per_turn);

    // Calculate time budget from game timeout and configured percentage
    let timeout_ms = u64::from(game.timeout);
    let time_budget_ms = timeout_ms * u64::from(params.search_time_pct) / 100;
    // Ensure at least 50ms for search
    let time_budget_ms = time_budget_ms.max(50);

    let start_time = std::time::Instant::now();

    // Run iterative-deepening minimax search
    let result = search::search(&sim_board, params, time_budget_ms);

    #[allow(clippy::cast_possible_truncation)]
    let search_time_ms = start_time.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;

    let chosen = result.best_move.as_str();

    info!(
        "MOVE {chosen} (depth={}, score={}, nodes={}, time={}ms)",
        result.depth_reached, result.score, result.nodes_evaluated, search_time_ms
    );

    // Pre-compute enemy length bounds for feature capture
    let (max_enemy_length, min_enemy_length) = board
        .snakes
        .iter()
        .filter(|s| s.id != you.id)
        .fold((0u32, u32::MAX), |(mx, mn), s| {
            (mx.max(s.length), mn.min(s.length))
        });
    let min_enemy_length = if min_enemy_length == u32::MAX {
        0
    } else {
        min_enemy_length
    };

    #[allow(clippy::cast_possible_wrap)]
    let features = MoveFeatures {
        turn,
        health: you.health,
        length: you.length,
        head_x: you.head.x,
        head_y: you.head.y,
        board_width: board.width,
        board_height: board.height,
        num_snakes: board.snakes.len(),
        num_food: board.food.len(),
        num_hazards: board.hazards.len(),
        hazard_damage_per_turn: game.ruleset.settings.hazard_damage_per_turn,
        max_enemy_length,
        min_enemy_length,
        length_advantage: you.length as i32 - max_enemy_length as i32,
        chosen_move: chosen,
        search_depth: result.depth_reached,
        eval_score: result.score,
        search_time_ms,
    };

    (
        MoveResponse {
            chosen_move: chosen,
        },
        features,
    )
}
