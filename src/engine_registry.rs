use fusion_core::MoveFeatures;
use fusion_engine_standard::heuristic_params::HeuristicParams;
use log::info;

use crate::game_objects::{Battlesnake, Board, Game, GameType};
use crate::responses::MoveResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Standard,
}

#[derive(Clone)]
pub struct EngineRegistry {
    standard: fusion_engine_standard::engine::StandardEngine,
}

impl EngineRegistry {
    #[must_use]
    pub fn prime() -> Self {
        Self {
            standard: fusion_engine_standard::engine::StandardEngine::prime(),
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

    pub async fn get_params(&self, engine: EngineKind) -> HeuristicParams {
        match engine {
            EngineKind::Standard => self.standard.get_params().await,
        }
    }

    /// Persist and apply standard-engine heuristic params.
    ///
    /// # Errors
    /// Returns `Err` when validation or persistence fails.
    pub async fn set_params(
        &self,
        engine: EngineKind,
        new_params: HeuristicParams,
    ) -> Result<(), String> {
        match engine {
            EngineKind::Standard => self.standard.set_params(new_params).await,
        }
    }

    /// Reset standard-engine heuristic params to defaults and persist them.
    ///
    /// # Errors
    /// Returns `Err` when persistence fails.
    pub async fn reset_params(&self, engine: EngineKind) -> Result<(), String> {
        match engine {
            EngineKind::Standard => self.standard.reset_params().await,
        }
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

/// Resolve an engine kind for config endpoints.
///
/// `None` defaults to [`EngineKind::Standard`] for backward compatibility.
///
/// # Errors
/// Returns `Err` for unknown engine names.
pub fn resolve_config_engine(engine: Option<&str>) -> Result<EngineKind, String> {
    match engine {
        None => Ok(EngineKind::Standard),
        Some(raw) => {
            let normalized = raw.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "standard" => Ok(EngineKind::Standard),
                _ => Err(format!(
                    "Unsupported engine '{raw}'. Supported engines: standard"
                )),
            }
        }
    }
}
