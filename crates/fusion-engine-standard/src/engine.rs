use std::path::Path;

use fusion_core::{EngineMoveDecision, MoveFeatures, SnakeEngine};
use log::info;

use crate::game_objects::{Battlesnake, Board, Game};
use crate::heuristic_params::{
    HeuristicParams, SharedParams, create_shared_params, params_file_path,
};
use crate::search;
use crate::simulation::SimBoard;

#[derive(Clone)]
pub struct StandardEngine {
    params: SharedParams,
}

impl SnakeEngine for StandardEngine {
    type Params = HeuristicParams;

    fn prime() -> Self {
        Self {
            params: create_shared_params(),
        }
    }

    fn name(&self) -> &'static str {
        "standard"
    }

    async fn get_move(
        &self,
        game: &Game,
        turn: i32,
        board: &Board,
        you: &Battlesnake,
    ) -> EngineMoveDecision {
        let params = self.params.read().await.clone();
        get_move_with_params(game, turn, board, you, &params)
    }

    async fn get_params(&self) -> Self::Params {
        self.params.read().await.clone()
    }

    async fn set_params(&self, new_params: Self::Params) -> Result<(), String> {
        let errors = new_params.validate();
        if !errors.is_empty() {
            return Err(format!("Validation failed: {}", errors.join("; ")));
        }

        let path = params_file_path();
        new_params.save_to_file(Path::new(&path))?;

        let mut params = self.params.write().await;
        *params = new_params;
        Ok(())
    }

    async fn reset_params(&self) -> Result<(), String> {
        let defaults = HeuristicParams::default();
        let path = params_file_path();
        defaults.save_to_file(Path::new(&path))?;

        let mut params = self.params.write().await;
        *params = defaults;
        Ok(())
    }
}

#[must_use]
pub fn get_move_with_params(
    game: &Game,
    turn: i32,
    board: &Board,
    you: &Battlesnake,
    params: &HeuristicParams,
) -> EngineMoveDecision {
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

    EngineMoveDecision {
        chosen_move: chosen,
        features,
    }
}
