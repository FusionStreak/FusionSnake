use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fusion_core::{MoveFeatures, SnakeEngine};
use fusion_engine_standard::config_api::{StandardConfigApiDoc, configure_standard_config_routes};
use fusion_engine_standard::engine::StandardEngine;
use log::info;
use utoipa::OpenApi;

use crate::game_objects::{Battlesnake, Board, Game, GameType};
use crate::responses::MoveResponse;

type EngineMoveFuture<'a> = Pin<Box<dyn Future<Output = (MoveResponse, MoveFeatures)> + Send + 'a>>;

trait EngineRuntime: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports_game_type(&self, game_type: GameType) -> bool;
    fn get_move<'a>(
        &'a self,
        game: &'a Game,
        turn: i32,
        board: &'a Board,
        you: &'a Battlesnake,
    ) -> EngineMoveFuture<'a>;
    fn configure_config_routes(&self, cfg: &mut actix_web::web::ServiceConfig);
    fn merge_openapi(&self, openapi: &mut utoipa::openapi::OpenApi);
}

#[derive(Clone)]
struct StandardRuntime {
    engine: StandardEngine,
}

impl StandardRuntime {
    #[must_use]
    fn prime() -> Self {
        Self {
            engine: <StandardEngine as SnakeEngine>::prime(),
        }
    }
}

impl EngineRuntime for StandardRuntime {
    fn name(&self) -> &'static str {
        self.engine.name()
    }

    fn supports_game_type(&self, game_type: GameType) -> bool {
        matches!(game_type, GameType::Duel | GameType::Standard)
    }

    fn get_move<'a>(
        &'a self,
        game: &'a Game,
        turn: i32,
        board: &'a Board,
        you: &'a Battlesnake,
    ) -> EngineMoveFuture<'a> {
        Box::pin(async move {
            let decision = self.engine.get_move(game, turn, board, you).await;
            (
                MoveResponse {
                    chosen_move: decision.chosen_move,
                },
                decision.features,
            )
        })
    }

    fn configure_config_routes(&self, cfg: &mut actix_web::web::ServiceConfig) {
        configure_standard_config_routes(cfg, self.engine.clone());
    }

    fn merge_openapi(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.merge(StandardConfigApiDoc::openapi());
    }
}

#[derive(Clone)]
pub struct EngineRegistry {
    engines: Vec<Arc<dyn EngineRuntime>>,
}

impl EngineRegistry {
    #[must_use]
    pub fn prime() -> Self {
        let standard = Arc::new(StandardRuntime::prime()) as Arc<dyn EngineRuntime>;
        Self {
            engines: vec![standard],
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
        let default_engine = self
            .engines
            .first()
            .expect("EngineRegistry must contain at least one engine");
        let selected_engine = self
            .engines
            .iter()
            .find(|engine| engine.supports_game_type(game_type))
            .unwrap_or(default_engine);

        if !selected_engine.supports_game_type(game_type) {
            info!(
                "No dedicated engine registered for game_type={}, using {} engine",
                game_type.as_str(),
                selected_engine.name()
            );
        }

        selected_engine.get_move(game, turn, board, you).await
    }

    pub fn configure_config_routes(&self, cfg: &mut actix_web::web::ServiceConfig) {
        for engine in &self.engines {
            engine.configure_config_routes(cfg);
        }
    }

    pub fn merge_openapi(&self, openapi: &mut utoipa::openapi::OpenApi) {
        for engine in &self.engines {
            engine.merge_openapi(openapi);
        }
    }
}
