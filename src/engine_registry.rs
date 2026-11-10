use log::info;

use crate::game_objects::{Battlesnake, Board, Game, GameType};
use crate::heuristic_params::HeuristicParams;
use crate::logic::{self, MoveFeatures};
use crate::responses::MoveResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Standard,
}

#[must_use]
pub fn select_engine(game_type: GameType) -> EngineKind {
    match game_type {
        GameType::Duel
        | GameType::Standard
        | GameType::BattleRoyal
        | GameType::Constrictor
        | GameType::Unknown => EngineKind::Standard,
    }
}

#[must_use]
pub fn get_move(
    game: &Game,
    turn: i32,
    board: &Board,
    you: &Battlesnake,
    params: &HeuristicParams,
) -> (MoveResponse, MoveFeatures) {
    let game_type = GameType::from_game_state(game, board);
    match select_engine(game_type) {
        EngineKind::Standard => {
            if game_type != GameType::Standard && game_type != GameType::Duel {
                info!(
                    "No dedicated engine registered for game_type={}, using standard engine",
                    game_type.as_str()
                );
            }
            logic::get_move(game, turn, board, you, params)
        }
    }
}
