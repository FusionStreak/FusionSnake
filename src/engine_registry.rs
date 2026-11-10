use fusion_core::MoveFeatures;
use fusion_engine_standard::engine::StandardEngine;
use log::info;

use crate::game_objects::{Battlesnake, Board, Game, GameType};
use crate::responses::MoveResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Standard,
}

#[derive(Clone)]
pub struct EngineRegistry {
    standard: StandardEngine,
}

impl EngineRegistry {
    #[must_use]
    pub fn prime() -> Self {
        Self {
            standard: StandardEngine::prime(),
        }
    }

    pub async fn get_move(
        &self,
        game: &Game,
        turn: i32,
        board: &Board,
        you: &Battlesnake,
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
                let decision = self.standard.get_move(game, turn, board, you).await;
                (
                    MoveResponse {
                        chosen_move: decision.chosen_move,
                    },
                    decision.features,
                )
            }
        }
    }

    #[must_use]
    pub fn standard_engine(&self) -> StandardEngine {
        self.standard.clone()
    }
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
