//! Per-engine route mounting.
//!
//! Every engine is exposed as its own Battlesnake under `/<engine-name>`:
//!
//! ```text
//! GET  /<name>               snake info / appearance
//! POST /<name>/start|move|end
//! GET  /<name>/config        current engine params        (API key)
//! POST /<name>/config        update engine params         (API key)
//! POST /<name>/config/reset  reset engine params          (API key)
//! ```
//!
//! Handlers are generic over [`SnakeEngine`], so each engine gets statically
//! dispatched routes with its engine instance stored in scope-local app data.
//! Game bookkeeping (active-game tracking, training and stats logging) is
//! shared here so engines only implement move logic.

use actix_web::dev::Service;
use actix_web::web::{self, Data, Json, get, post};
use actix_web::{HttpRequest, HttpResponse};
use fusion_core::SnakeEngine;
use futures_util::future::Either;
use log::{error, info};

use crate::auth;
use crate::db;
use crate::game_objects::{GameSource, GameState, GameType};
use crate::logged_json::LoggedJson;
use crate::logic;
use crate::responses::{ErrorResponse, InfoResponse, MoveResponse};
use crate::stats::{self, ActiveGames, cleanup_stale_games};
use crate::training::TrainingLogger;

/// Mount the Battlesnake and config routes for `engine` under `/<engine.name()>`.
pub fn configure_engine<E>(cfg: &mut web::ServiceConfig, engine: E)
where
    E: SnakeEngine + Clone + Send + Sync + 'static,
{
    let name = engine.name();
    cfg.service(
        web::scope(&format!("/{name}"))
            .app_data(Data::new(engine))
            .route("", get().to(handle_info::<E>))
            .route("/", get().to(handle_info::<E>))
            .route("/start", post().to(handle_start::<E>))
            .route("/move", post().to(handle_move::<E>))
            .route("/end", post().to(handle_end::<E>))
            .service(
                web::scope("/config")
                    .wrap_fn(|req, service| {
                        if let Err(error) = auth::validate_api_key(req.request()) {
                            Either::Left(async move { Err(error) })
                        } else {
                            Either::Right(service.call(req))
                        }
                    })
                    .route("", get().to(handle_get_config::<E>))
                    .route("", post().to(handle_set_config::<E>))
                    .route("/reset", post().to(handle_reset_config::<E>)),
            ),
    );
}

// ---------------------------------------------------------------------------
// Battlesnake API
// ---------------------------------------------------------------------------

async fn handle_info<E: SnakeEngine + 'static>(engine: Data<E>) -> HttpResponse {
    let appearance = engine.info();
    info!("INFO {}", engine.name());
    HttpResponse::Ok().json(InfoResponse {
        apiversion: "1",
        author: "fusionstreak",
        color: appearance.color,
        head: appearance.head,
        tail: appearance.tail,
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn handle_start<E: SnakeEngine + 'static>(
    req: HttpRequest,
    engine: Data<E>,
    game_state: LoggedJson<GameState>,
    active_games: Data<ActiveGames>,
) -> HttpResponse {
    logic::start(engine.name(), &game_state.game);

    let source = GameSource::from_user_agent(
        req.headers()
            .get(actix_web::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
    );
    let game_type = GameType::from_game_state(&game_state.game, &game_state.board);

    if let Ok(mut games) = active_games.lock() {
        games.insert(
            game_state.game.id.clone(),
            stats::ActiveGame {
                last_turn: 0,
                started_at: chrono::Utc::now(),
                starting_length: game_state.you.length,
                max_length_reached: game_state.you.length,
                source,
                game_type,
            },
        );

        // Cleanup stale games (older than 6 hours)
        drop(games); // Release the lock before cleanup
        cleanup_stale_games(&active_games, 6 * 60 * 60);
    }

    HttpResponse::Ok().finish()
}

async fn handle_move<E: SnakeEngine + 'static>(
    engine: Data<E>,
    game_state: LoggedJson<GameState>,
    active_games: Data<ActiveGames>,
    training: Data<TrainingLogger>,
) -> HttpResponse {
    let decision = engine
        .get_move(
            &game_state.game,
            game_state.turn,
            &game_state.board,
            &game_state.you,
        )
        .await;

    // Fire-and-forget: insert turn features into SQLite in the background
    training.log_turn(game_state.game.id.clone(), decision.features);

    if let Ok(mut games) = active_games.lock()
        && let Some(game) = games.get_mut(&game_state.game.id)
    {
        game.last_turn = game_state.turn.cast_unsigned();
        game.max_length_reached = game.max_length_reached.max(game_state.you.length);
    }

    HttpResponse::Ok().json(MoveResponse {
        chosen_move: decision.chosen_move,
    })
}

async fn handle_end<E: SnakeEngine + 'static>(
    engine: Data<E>,
    game_state: LoggedJson<GameState>,
    active_games: Data<ActiveGames>,
    training: Data<TrainingLogger>,
) -> HttpResponse {
    let (won, is_draw) = logic::end(
        engine.name(),
        &game_state.game,
        game_state.turn,
        &game_state.board,
        &game_state.you,
    );

    let tracked = if let Ok(mut games) = active_games.lock() {
        let tracked = games.remove(&game_state.game.id);
        if tracked.is_none() {
            log::warn!("Game {} not found in active games", game_state.game.id);
        }
        tracked
    } else {
        log::error!("Failed to acquire active games lock");
        None
    };

    let (turns, food_eaten, source, game_type, max_length_reached) = match tracked {
        Some(game) => (
            game.last_turn,
            game_state.you.length.saturating_sub(game.starting_length),
            game.source,
            game.game_type,
            game.max_length_reached,
        ),
        None => (
            game_state.turn.cast_unsigned(),
            0,
            GameSource::Unknown,
            GameType::from_game_state(&game_state.game, &game_state.board),
            game_state.you.length,
        ),
    };

    // Fire-and-forget: write outcome + aggregate stats to SQLite
    training.log_outcome(
        game_state.game.id.clone(),
        db::OutcomeRecord::new(
            won,
            is_draw,
            turns,
            food_eaten,
            source,
            game_type,
            max_length_reached,
        ),
    );
    training.log_game_stats(turns, food_eaten, won, is_draw);

    HttpResponse::Ok().finish()
}

// ---------------------------------------------------------------------------
// Engine params config
// ---------------------------------------------------------------------------

async fn handle_get_config<E: SnakeEngine + 'static>(engine: Data<E>) -> HttpResponse {
    HttpResponse::Ok().json(engine.get_params().await)
}

async fn handle_set_config<E: SnakeEngine + 'static>(
    engine: Data<E>,
    body: Json<E::Params>,
) -> HttpResponse {
    let name = engine.name();
    if let Err(e) = engine.set_params(body.into_inner()).await {
        let status = if e.starts_with("Validation failed:") {
            actix_web::http::StatusCode::BAD_REQUEST
        } else {
            error!("Failed to persist {name} engine params to disk: {e}");
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
        };
        return HttpResponse::build(status).json(ErrorResponse { error: e });
    }

    info!("{name} engine params updated via POST /{name}/config");
    HttpResponse::Ok().json(serde_json::json!({"status": "ok", "message": "Parameters updated"}))
}

async fn handle_reset_config<E: SnakeEngine + 'static>(engine: Data<E>) -> HttpResponse {
    let name = engine.name();
    if let Err(e) = engine.reset_params().await {
        error!("Failed to persist {name} engine defaults to disk: {e}");
        return HttpResponse::InternalServerError().json(ErrorResponse {
            error: format!("Failed to persist defaults to disk: {e}"),
        });
    }

    info!("{name} engine params reset to defaults via POST /{name}/config/reset");
    HttpResponse::Ok()
        .json(serde_json::json!({"status": "ok", "message": "Parameters reset to defaults"}))
}
